//! Atomic provider-retention dependency validation in the existing postgres transaction owner.
use super::*;

pub(super) async fn validate_postgres_provider_dependency(
    transaction: &mut Transaction<'_, Postgres>,
    candidate: &ProviderRetentionRow,
) -> Result<(), StoreError> {
    match &candidate.dependency {
        ProviderRetentionDependency::None => {
            return if candidate.owner_execution.is_none() && candidate.owner_run_unit.is_none() {
                Ok(())
            } else {
                Err(StoreError::IncompatibleVersion)
            };
        }
        ProviderRetentionDependency::DirectProduct => {
            validate_postgres_owner_dependency(transaction, candidate, false).await?;
            return Ok(());
        }
        ProviderRetentionDependency::ProviderGraph {
            required_rows,
            required_executions,
        } => {
            for required in required_rows {
                let current = sqlx::query(
                    "SELECT version,payload FROM provider_state WHERE namespace=$1 AND key=$2",
                )
                .bind(&required.namespace)
                .bind(&required.key)
                .fetch_optional(&mut **transaction)
                .await
                .map_err(infrastructure)?
                .ok_or(StoreError::Conflict)?;
                if u64::try_from(current.try_get::<i64, _>(0).map_err(infrastructure)?)
                    .map_err(|_| StoreError::IncompatibleVersion)?
                    != required.version
                    || current.try_get::<Vec<u8>, _>(1).map_err(infrastructure)? != required.payload
                {
                    return Err(StoreError::Conflict);
                }
            }
            for required in required_executions {
                validate_postgres_execution_dependency(transaction, required, None).await?;
            }
            validate_postgres_owner_dependency(
                transaction,
                candidate,
                candidate.owner_execution.is_some(),
            )
            .await?;
            return Ok(());
        }
        _ => {}
    }
    let (owner, run) = validate_postgres_owner_dependency(transaction, candidate, true)
        .await?
        .ok_or(StoreError::Conflict)?;
    match &candidate.dependency {
        ProviderRetentionDependency::CoreEffect {
            key,
            request_digest,
            result_digest,
        } => {
            let effect = sqlx::query(
                "SELECT payload FROM provider_state WHERE namespace='durable-effect' AND key=$1",
            )
            .bind(key.as_str())
            .fetch_optional(&mut **transaction)
            .await
            .map_err(infrastructure)?
            .ok_or(StoreError::Conflict)?;
            let payload: Vec<u8> = effect.try_get(0).map_err(infrastructure)?;
            let effect = crate::durable::decode_effect(key, &payload)?;
            if effect.execution_id != owner
                || effect.run_unit_id != run
                || effect.state != mainframe_env_store_api::EffectState::Completed
                || candidate.row.namespace == "cics-container-replay-v1"
                    && effect
                        .intent
                        .capability
                        .as_ref()
                        .map(|value| value.as_str())
                        != Some("host.cics.execute")
                || effect.digest_format
                    != mainframe_env_store_api::EffectDigestFormat::CanonicalHostV1
                || effect.request_digest != *request_digest
                || effect.result_digest != Some(*result_digest)
                || effect
                    .resolved_tick
                    .is_none_or(|tick| tick == 0 || candidate.retention_tick < tick)
            {
                return Err(StoreError::Conflict);
            }
        }
        ProviderRetentionDependency::CicsNested {
            provenance,
            absent,
            required_executions,
        } => {
            for required in required_executions {
                validate_postgres_execution_dependency(transaction, required, Some(&run)).await?;
            }
            let current = sqlx::query(
                "SELECT version,payload FROM provider_state WHERE namespace=$1 AND key=$2",
            )
            .bind(&provenance.namespace)
            .bind(&provenance.key)
            .fetch_optional(&mut **transaction)
            .await
            .map_err(infrastructure)?
            .ok_or(StoreError::Conflict)?;
            if u64::try_from(current.try_get::<i64, _>(0).map_err(infrastructure)?)
                .map_err(|_| StoreError::IncompatibleVersion)?
                != provenance.version
                || current.try_get::<Vec<u8>, _>(1).map_err(infrastructure)? != provenance.payload
            {
                return Err(StoreError::Conflict);
            }
            for identity in absent {
                if sqlx::query_scalar::<_, i64>(
                    "SELECT COUNT(*) FROM provider_state WHERE namespace=$1 AND key=$2",
                )
                .bind(&identity.namespace)
                .bind(&identity.key)
                .fetch_one(&mut **transaction)
                .await
                .map_err(infrastructure)?
                    != 0
                {
                    return Err(StoreError::Conflict);
                }
            }
            let effects = sqlx::query(
                "SELECT key,payload FROM provider_state WHERE namespace='durable-effect'",
            )
            .fetch_all(&mut **transaction)
            .await
            .map_err(infrastructure)?;
            let mut terminal_origin = false;
            for effect in effects {
                let key: String = effect.try_get(0).map_err(infrastructure)?;
                let key = mainframe_env_execution_api::IdempotencyKey::new(
                    key,
                    mainframe_env_execution_api::InvocationLimits::default(),
                )
                .map_err(|_| StoreError::IncompatibleVersion)?;
                let payload: Vec<u8> = effect.try_get(1).map_err(infrastructure)?;
                let effect = crate::durable::decode_effect(&key, &payload)?;
                if effect.execution_id == owner && effect.run_unit_id == run {
                    if effect.key.as_str() != provenance.key {
                        continue;
                    }
                    if matches!(
                        effect.state,
                        mainframe_env_store_api::EffectState::Intent
                            | mainframe_env_store_api::EffectState::UnknownOutcome
                    ) {
                        return Err(StoreError::Conflict);
                    }
                    terminal_origin |= effect.state
                        == mainframe_env_store_api::EffectState::Completed
                        && effect.digest_format
                            == mainframe_env_store_api::EffectDigestFormat::CanonicalHostV1
                        && effect.intent.capability.as_ref().map(|item| item.as_str())
                            == Some("host.cics.execute")
                        && effect
                            .resolved_tick
                            .is_some_and(|tick| tick != 0 && candidate.retention_tick >= tick);
                }
            }
            let undo: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM provider_state WHERE namespace='cics-uow-undo' AND key=$1",
            )
            .bind(run.as_str())
            .fetch_one(&mut **transaction)
            .await
            .map_err(infrastructure)?;
            if !terminal_origin || undo != 0 {
                return Err(StoreError::Conflict);
            }
        }
        ProviderRetentionDependency::ProviderGraph { .. }
        | ProviderRetentionDependency::DirectProduct
        | ProviderRetentionDependency::None => unreachable!(),
    }
    Ok(())
}

async fn validate_postgres_execution_dependency(
    transaction: &mut Transaction<'_, Postgres>,
    owner: &mainframe_env_execution_api::ExecutionId,
    expected_run: Option<&mainframe_env_execution_api::RunUnitId>,
) -> Result<(), StoreError> {
    let row = sqlx::query(
        "SELECT version,payload FROM provider_state WHERE namespace='durable-execution' AND key=$1",
    )
    .bind(owner.as_str())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(infrastructure)?
    .ok_or(StoreError::Conflict)?;
    let version = u64::try_from(row.try_get::<i64, _>(0).map_err(infrastructure)?)
        .map_err(|_| StoreError::IncompatibleVersion)?;
    let execution = crate::durable::decode_execution(
        &row.try_get::<Vec<u8>, _>(1).map_err(infrastructure)?,
        version,
    )?;
    if !execution.state.terminal()
        || expected_run.is_some_and(|run| execution.run_unit_id != *run)
        || sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM provider_state WHERE namespace='durable-checkpoint' AND key=$1",
        )
        .bind(owner.as_str())
        .fetch_one(&mut **transaction)
        .await
        .map_err(infrastructure)?
            != 0
    {
        return Err(StoreError::Conflict);
    }
    for row in
        sqlx::query("SELECT key,payload FROM provider_state WHERE namespace='durable-effect'")
            .fetch_all(&mut **transaction)
            .await
            .map_err(infrastructure)?
    {
        let key = mainframe_env_execution_api::IdempotencyKey::new(
            row.try_get::<String, _>(0).map_err(infrastructure)?,
            mainframe_env_execution_api::InvocationLimits::default(),
        )
        .map_err(|_| StoreError::IncompatibleVersion)?;
        let effect = crate::durable::decode_effect(
            &key,
            &row.try_get::<Vec<u8>, _>(1).map_err(infrastructure)?,
        )?;
        if effect.execution_id == *owner
            && matches!(
                effect.state,
                mainframe_env_store_api::EffectState::Intent
                    | mainframe_env_store_api::EffectState::UnknownOutcome
            )
        {
            return Err(StoreError::Conflict);
        }
    }
    Ok(())
}

async fn validate_postgres_owner_dependency(
    transaction: &mut Transaction<'_, Postgres>,
    candidate: &ProviderRetentionRow,
    required: bool,
) -> Result<
    Option<(
        mainframe_env_execution_api::ExecutionId,
        mainframe_env_execution_api::RunUnitId,
    )>,
    StoreError,
> {
    let Some(owner) = candidate.owner_execution.as_ref() else {
        return if required || candidate.owner_run_unit.is_some() {
            Err(StoreError::IncompatibleVersion)
        } else {
            Ok(None)
        };
    };
    let run = candidate
        .owner_run_unit
        .as_ref()
        .ok_or(StoreError::IncompatibleVersion)?;
    let execution = sqlx::query(
        "SELECT version,payload FROM provider_state WHERE namespace='durable-execution' AND key=$1",
    )
    .bind(owner.as_str())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(infrastructure)?;
    let Some(execution) = execution else {
        return if required {
            Err(StoreError::Conflict)
        } else {
            Ok(None)
        };
    };
    let execution_version = u64::try_from(execution.try_get::<i64, _>(0).map_err(infrastructure)?)
        .map_err(|_| StoreError::IncompatibleVersion)?;
    let execution_payload: Vec<u8> = execution.try_get(1).map_err(infrastructure)?;
    let execution = crate::durable::decode_execution(&execution_payload, execution_version)?;
    if !execution.state.terminal()
        || execution.run_unit_id != *run
        || sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM provider_state WHERE namespace='durable-checkpoint' AND key=$1",
        )
        .bind(owner.as_str())
        .fetch_one(&mut **transaction)
        .await
        .map_err(infrastructure)?
            != 0
    {
        return Err(StoreError::Conflict);
    }
    let effects =
        sqlx::query("SELECT key,payload FROM provider_state WHERE namespace='durable-effect'")
            .fetch_all(&mut **transaction)
            .await
            .map_err(infrastructure)?;
    for effect in effects {
        let key: String = effect.try_get(0).map_err(infrastructure)?;
        let key = mainframe_env_execution_api::IdempotencyKey::new(
            key,
            mainframe_env_execution_api::InvocationLimits::default(),
        )
        .map_err(|_| StoreError::IncompatibleVersion)?;
        let payload: Vec<u8> = effect.try_get(1).map_err(infrastructure)?;
        let effect = crate::durable::decode_effect(&key, &payload)?;
        if effect.execution_id == *owner
            && effect.run_unit_id == *run
            && matches!(
                effect.state,
                mainframe_env_store_api::EffectState::Intent
                    | mainframe_env_store_api::EffectState::UnknownOutcome
            )
        {
            return Err(StoreError::Conflict);
        }
    }
    Ok(Some((owner.clone(), run.clone())))
}
