use super::codec::{
    CATALOG_KEY, CATALOG_NAMESPACE, PACKAGE_NAMESPACE, REPLAY_NAMESPACE, list, mutate, put, read,
};
use super::contracts::{TmCall, TmDefinitionSet};
use super::model::WorkPayload;
use super::model::{CatalogRow, PackageDefinitionsRow, ReplayRow, TmInstallReceipt};
use super::service::{TmService, WORK_PAYLOAD_SCHEMA};
use super::support::{find_transaction, invocation_digest, work_error, work_generation};
use mainframe_env_execution_api::Invocation;
use mainframe_env_host_api::HostProblem;
use mainframe_env_store_api::WorkRecord;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TmPackageBinding {
    pub application: String,
    pub generation: u64,
    pub package_identity: String,
}

impl TmService {
    /// Select a verified package generation under the existing TM catalog CAS authority.
    /// Retained definitions keep prior queued work and conversations bound across rollback.
    pub fn publish_package_definitions(
        &self,
        application: &str,
        generation: u64,
        package_identity: &str,
        definitions: Option<&TmDefinitionSet>,
    ) -> Result<TmInstallReceipt, HostProblem> {
        if application.is_empty()
            || application.len() > 128
            || !application
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
            || generation == 0
            || !package_identity.starts_with("sha256:")
            || package_identity.len() != 71
            || !package_identity[7..]
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(HostProblem::Malformed);
        }
        if let Some(definitions) = definitions {
            definitions.validate(self.limits)?;
        }
        let application = application.to_ascii_uppercase();
        let binding = format!("{application}:{generation}");
        let retained = read::<PackageDefinitionsRow>(
            self.store.as_ref(),
            PACKAGE_NAMESPACE,
            &binding,
            self.limits.max_state_bytes,
        )?;
        let desired = definitions.map(|definitions| PackageDefinitionsRow {
            application: application.clone(),
            generation,
            package_identity: package_identity.into(),
            definitions: definitions.clone(),
        });
        if retained
            .as_ref()
            .is_some_and(|(_, row)| Some(row) != desired.as_ref())
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        let current = self.catalog()?;
        if desired.is_none()
            && current.as_ref().is_some_and(|(_, row)| {
                row.active && row.application.as_deref() != Some(application.as_str())
            })
        {
            return Ok(TmInstallReceipt {
                transactions: 0,
                identity: package_identity.into(),
                replayed: true,
            });
        }
        if let Some((_, row)) = &current
            && (row.application.as_deref().is_none()
                || (row.active && row.application.as_deref() != Some(application.as_str())))
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        if current.is_none() && desired.is_none() {
            return Ok(TmInstallReceipt {
                transactions: 0,
                identity: package_identity.into(),
                replayed: true,
            });
        }
        let selected_binding = desired.as_ref().map(|_| binding.clone());
        let selected = current.as_ref().is_some_and(|(_, row)| {
            row.application.as_deref() == Some(application.as_str())
                && row.package_binding == selected_binding
                && row.active == desired.is_some()
                && desired
                    .as_ref()
                    .is_none_or(|target| row.definitions == target.definitions)
        });
        if selected && (desired.is_none() || retained.is_some()) {
            return Ok(TmInstallReceipt {
                transactions: definitions.map_or(0, |value| value.transactions.len()),
                identity: package_identity.into(),
                replayed: true,
            });
        }
        let mut mutations = Vec::new();
        if let Some(desired) = &desired
            && retained.is_none()
        {
            let existing = list::<PackageDefinitionsRow>(
                self.store.as_ref(),
                PACKAGE_NAMESPACE,
                1_024,
                self.limits.max_state_bytes,
            )?;
            if existing
                .iter()
                .filter(|(_, _, row)| row.application == application)
                .count()
                >= 64
            {
                return Err(HostProblem::ResourceExhausted);
            }
            mutations.push(put(
                PACKAGE_NAMESPACE,
                &binding,
                desired,
                None,
                self.limits.max_state_bytes,
            )?);
        }
        let next_sequence = current.as_ref().map_or(1, |(_, row)| row.next_sequence);
        let catalog = CatalogRow {
            definitions: desired
                .as_ref()
                .map(|row| row.definitions.clone())
                .or_else(|| current.as_ref().map(|(_, row)| row.definitions.clone()))
                .ok_or(HostProblem::InfrastructureFailure)?,
            next_sequence,
            package_binding: selected_binding,
            application: Some(application),
            active: desired.is_some(),
        };
        mutations.push(put(
            CATALOG_NAMESPACE,
            CATALOG_KEY,
            &catalog,
            current.as_ref().map(|(version, _)| *version),
            self.limits.max_state_bytes,
        )?);
        mutate(self.store.as_ref(), mutations)?;
        Ok(TmInstallReceipt {
            transactions: definitions.map_or(0, |value| value.transactions.len()),
            identity: package_identity.into(),
            replayed: false,
        })
    }

    pub fn selected_package_matches(
        &self,
        application: &str,
        generation: u64,
        package_identity: &str,
    ) -> Result<bool, HostProblem> {
        let binding = format!("{}:{generation}", application.to_ascii_uppercase());
        let Some((_, catalog)) = self.catalog()? else {
            return Ok(false);
        };
        if !catalog.active || catalog.package_binding.as_deref() != Some(&binding) {
            return Ok(false);
        }
        let Some((_, retained)) = read::<PackageDefinitionsRow>(
            self.store.as_ref(),
            PACKAGE_NAMESPACE,
            &binding,
            self.limits.max_state_bytes,
        )?
        else {
            return Err(HostProblem::InfrastructureFailure);
        };
        Ok(retained.package_identity == package_identity
            && retained.definitions == catalog.definitions)
    }

    pub fn retained_package_matches(
        &self,
        application: &str,
        generation: u64,
        package_identity: &str,
        definitions: &TmDefinitionSet,
    ) -> Result<bool, HostProblem> {
        let binding = format!("{}:{generation}", application.to_ascii_uppercase());
        let Some((_, retained)) = read::<PackageDefinitionsRow>(
            self.store.as_ref(),
            PACKAGE_NAMESPACE,
            &binding,
            self.limits.max_state_bytes,
        )?
        else {
            return Ok(false);
        };
        Ok(retained.application == application.to_ascii_uppercase()
            && retained.generation == generation
            && retained.package_identity == package_identity
            && retained.definitions == *definitions)
    }

    /// Claim previously admitted work under its retained signed generation.
    pub fn claim_retained(
        &self,
        application: &str,
        generation: u64,
        package_identity: &str,
        transaction: &str,
        worker: &str,
        now_tick: u64,
        lease_ticks: u64,
    ) -> Result<Option<WorkRecord>, HostProblem> {
        let binding = format!("{}:{generation}", application.to_ascii_uppercase());
        let (_, retained) = read::<PackageDefinitionsRow>(
            self.store.as_ref(),
            PACKAGE_NAMESPACE,
            &binding,
            self.limits.max_state_bytes,
        )?
        .ok_or(HostProblem::NotFound)?;
        if retained.package_identity != package_identity
            || retained.application != application.to_ascii_uppercase()
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        let transaction = find_transaction(
            &CatalogRow {
                definitions: retained.definitions,
                next_sequence: 1,
                package_binding: Some(binding.clone()),
                application: Some(retained.application),
                active: true,
            },
            transaction,
        )?
        .clone();
        let now = self
            .store
            .advance_logical_clock(now_tick)
            .map_err(super::codec::store_error)?;
        self.work_store
            .claim(
                worker,
                Some(&work_generation(&transaction, Some(&binding))?),
                now,
                lease_ticks,
            )
            .map_err(work_error)
    }

    pub fn package_for_message(&self, message_id: &str) -> Result<TmPackageBinding, HostProblem> {
        let (_, message) = self.message(message_id)?.ok_or(HostProblem::NotFound)?;
        let binding = message.package_binding.ok_or(HostProblem::NotFound)?;
        self.package_binding(&binding)
    }

    pub fn package_for_run(&self, run_unit: &str) -> Result<TmPackageBinding, HostProblem> {
        let (_, session) = self.session(run_unit)?.ok_or(HostProblem::NotFound)?;
        let binding = session.package_binding.ok_or(HostProblem::NotFound)?;
        self.package_binding(&binding)
    }

    pub fn package_for_call(
        &self,
        invocation: &Invocation,
        call: &TmCall,
    ) -> Result<TmPackageBinding, HostProblem> {
        if self.session(invocation.run_unit_id.as_str())?.is_some() {
            return self.package_for_run(invocation.run_unit_id.as_str());
        }
        let (_, candidate) = read::<ReplayRow>(
            self.store.as_ref(),
            REPLAY_NAMESPACE,
            invocation.idempotency_key.as_str(),
            self.limits.max_state_bytes,
        )?
        .ok_or(HostProblem::NotFound)?;
        let work = candidate.work.ok_or(HostProblem::NotFound)?;
        let record = self
            .work_store
            .get_work(&work.work_id)
            .map_err(work_error)?
            .ok_or(HostProblem::UnknownOutcome)?;
        let payload: WorkPayload = serde_json::from_slice(&record.payload)
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let (_, message) = self
            .message(&payload.message_id)?
            .ok_or(HostProblem::UnknownOutcome)?;
        let digest = invocation_digest(
            "mainframe-env.ims-tm-call@1",
            invocation,
            &(message.package_binding.as_deref(), call),
        )?;
        self.replay(invocation, digest)?
            .ok_or(HostProblem::UnknownOutcome)?;
        self.package_for_work(&record)
    }

    pub fn package_for_work(&self, work: &WorkRecord) -> Result<TmPackageBinding, HostProblem> {
        let payload: WorkPayload =
            serde_json::from_slice(&work.payload).map_err(|_| HostProblem::Malformed)?;
        if payload.schema_version != WORK_PAYLOAD_SCHEMA {
            return Err(HostProblem::Malformed);
        }
        let (_, message) = self
            .message(&payload.message_id)?
            .ok_or(HostProblem::NotFound)?;
        if message.work_id != work.work_id || message.message.transaction != payload.transaction {
            return Err(HostProblem::IdempotencyConflict);
        }
        let binding = message.package_binding.ok_or(HostProblem::NotFound)?;
        self.package_binding(&binding)
    }

    fn package_binding(&self, key: &str) -> Result<TmPackageBinding, HostProblem> {
        let (_, row) = read::<PackageDefinitionsRow>(
            self.store.as_ref(),
            PACKAGE_NAMESPACE,
            key,
            self.limits.max_state_bytes,
        )?
        .ok_or(HostProblem::InfrastructureFailure)?;
        if key != format!("{}:{}", row.application, row.generation) {
            return Err(HostProblem::InfrastructureFailure);
        }
        Ok(TmPackageBinding {
            application: row.application,
            generation: row.generation,
            package_identity: row.package_identity,
        })
    }
}
