//! Fenced binding between a CICS run unit and one active BTS activity.

use super::*;
use crate::service::CicsService;
use mainframe_env_execution_api::RunUnitId;

const CONTEXT_NAMESPACE: &str = "cics-bts-activity-context-v1";
const CONTEXT_SCHEMA: &str = "mainframe-env.cics.bts-activity-context@1";
const MAX_CONTEXT_BYTES: usize = 1536;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BtsActivityContext {
    pub schema_version: String,
    pub run_unit: String,
    pub owner_execution: String,
    pub owner_principal: String,
    pub process_type: String,
    pub process_name: String,
    pub activity_id: String,
    pub activation_epoch: u64,
    pub owner_lease_epoch: u64,
    pub closed: bool,
    #[serde(skip)]
    pub row_version: u64,
}

impl BtsActivityContext {
    fn validate(&self) -> Result<(), HostProblem> {
        if self.schema_version != CONTEXT_SCHEMA
            || validate_identifier(&self.run_unit, 256).is_err()
            || validate_identifier(&self.owner_execution, 256).is_err()
            || validate_identifier(&self.owner_principal, 256).is_err()
            || validate_name(&self.process_type, 8, true).is_err()
            || validate_name(&self.process_name, 36, true).is_err()
            || validate_activity_id(&self.activity_id).is_err()
            || self.activation_epoch == 0
            || self.owner_lease_epoch == 0
        {
            return Err(HostProblem::InfrastructureFailure);
        }
        Ok(())
    }

    fn same_activation(&self, other: &Self) -> bool {
        self.schema_version == other.schema_version
            && self.run_unit == other.run_unit
            && self.owner_execution == other.owner_execution
            && self.owner_principal == other.owner_principal
            && self.process_type == other.process_type
            && self.process_name == other.process_name
            && self.activity_id == other.activity_id
            && self.activation_epoch == other.activation_epoch
    }
}

impl CicsService {
    /// Bind a registered run to the exact coordinator-fenced BTS activation.
    /// A lease takeover updates only the lease epoch on the same activation.
    pub fn bind_bts_activity_context(
        &self,
        run_unit: &RunUnitId,
        process_type: &str,
        process_name: &str,
        activity_id: &str,
        activation_epoch: u64,
        owner_lease_epoch: u64,
    ) -> Result<(), HostProblem> {
        let state = self.lock()?;
        let run = state.runs.get(run_unit).ok_or(HostProblem::Unauthorized)?;
        let context = BtsActivityContext {
            schema_version: CONTEXT_SCHEMA.into(),
            run_unit: run_unit.as_str().into(),
            owner_execution: run.invocation.execution_id.as_str().into(),
            owner_principal: run.invocation.principal.id().as_str().into(),
            process_type: process_type.into(),
            process_name: process_name.into(),
            activity_id: activity_id.into(),
            activation_epoch,
            owner_lease_epoch,
            closed: false,
            row_version: 0,
        };
        drop(state);
        BtsLifecycleStore::new(self.store.as_ref()).bind_context(context)
    }

    /// Fence a completed run unit while retaining its replay identity.
    pub fn close_bts_activity_context(&self, run_unit: &RunUnitId) -> Result<(), HostProblem> {
        if !self.lock()?.runs.contains_key(run_unit) {
            return Err(HostProblem::Unauthorized);
        }
        BtsLifecycleStore::new(self.store.as_ref()).close_context(run_unit.as_str())
    }
}

impl<'a> BtsLifecycleStore<'a> {
    /// Distinguish a never-bound run from a closed or stale BTS binding.
    pub fn has_context_row(&self, run_unit: &str) -> Result<bool, HostProblem> {
        Ok(self.load_context_row(run_unit)?.is_some())
    }

    pub fn bind_context(&self, context: BtsActivityContext) -> Result<(), HostProblem> {
        context.validate()?;
        if context.closed || context.row_version != 0 {
            return Err(HostProblem::Malformed);
        }
        self.validate_active_context(&context)?;
        for _ in 0..MAX_CAS_ATTEMPTS {
            let prior = self.load_context_row(&context.run_unit)?;
            match prior {
                None => {
                    match self
                        .store
                        .put_provider_state(context_record(&context, 1)?, None)
                    {
                        Ok(()) => return Ok(()),
                        Err(StoreError::Conflict | StoreError::AlreadyExists) => continue,
                        Err(error) => return Err(store_error(error)),
                    }
                }
                Some(prior) => {
                    if prior.closed
                        || !prior.same_activation(&context)
                        || prior.owner_lease_epoch > context.owner_lease_epoch
                    {
                        return Err(HostProblem::IdempotencyConflict);
                    }
                    if prior.owner_lease_epoch == context.owner_lease_epoch {
                        return Ok(());
                    }
                    let version = prior
                        .row_version
                        .checked_add(1)
                        .ok_or(HostProblem::ResourceExhausted)?;
                    match self.store.put_provider_state(
                        context_record(&context, version)?,
                        Some(prior.row_version),
                    ) {
                        Ok(()) => return Ok(()),
                        Err(StoreError::Conflict) => continue,
                        Err(error) => return Err(store_error(error)),
                    }
                }
            }
        }
        Err(HostProblem::UnknownOutcome)
    }

    pub fn active_context(
        &self,
        run_unit: &str,
        owner_execution: &str,
        owner_principal: &str,
    ) -> Result<Option<BtsActivityContext>, HostProblem> {
        let Some(context) = self.load_context_row(run_unit)? else {
            return Ok(None);
        };
        if context.closed {
            return Ok(None);
        }
        if context.owner_execution != owner_execution || context.owner_principal != owner_principal
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        self.validate_active_context(&context)?;
        Ok(Some(context))
    }

    pub fn close_context(&self, run_unit: &str) -> Result<(), HostProblem> {
        for _ in 0..MAX_CAS_ATTEMPTS {
            let Some(mut context) = self.load_context_row(run_unit)? else {
                return Ok(());
            };
            if context.closed {
                return Ok(());
            }
            context.closed = true;
            let version = context
                .row_version
                .checked_add(1)
                .ok_or(HostProblem::ResourceExhausted)?;
            match self.store.put_provider_state(
                context_record(&context, version)?,
                Some(context.row_version),
            ) {
                Ok(()) => return Ok(()),
                Err(StoreError::Conflict) => continue,
                Err(error) => return Err(store_error(error)),
            }
        }
        Err(HostProblem::UnknownOutcome)
    }

    pub(super) fn load_context_row(
        &self,
        run_unit: &str,
    ) -> Result<Option<BtsActivityContext>, HostProblem> {
        validate_identifier(run_unit, 256)?;
        let Some(row) = self
            .store
            .get_provider_state(CONTEXT_NAMESPACE, run_unit)
            .map_err(store_error)?
        else {
            return Ok(None);
        };
        if row.version == 0 || row.payload.len() > MAX_CONTEXT_BYTES {
            return Err(HostProblem::InfrastructureFailure);
        }
        let mut context: BtsActivityContext =
            serde_json::from_slice(&row.payload).map_err(|_| HostProblem::InfrastructureFailure)?;
        context.row_version = row.version;
        context.validate()?;
        if context.run_unit != run_unit {
            return Err(HostProblem::InfrastructureFailure);
        }
        Ok(Some(context))
    }

    fn validate_active_context(&self, context: &BtsActivityContext) -> Result<(), HostProblem> {
        let process = self
            .load_process(&context.process_type, &context.process_name)?
            .ok_or(HostProblem::InfrastructureFailure)?;
        let activity = process
            .activities
            .get(&context.activity_id)
            .ok_or(HostProblem::InfrastructureFailure)?;
        if activity.mode != BtsMode::Active
            || activity.activation_epoch != context.activation_epoch
            || activity
                .checkpoint
                .as_ref()
                .is_none_or(|checkpoint| checkpoint.owner_lease_epoch != context.owner_lease_epoch)
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        Ok(())
    }
}

fn context_record(
    context: &BtsActivityContext,
    version: u64,
) -> Result<ProviderStateRecord, HostProblem> {
    let payload = serde_json::to_vec(context).map_err(|_| HostProblem::ResourceExhausted)?;
    if payload.len() > MAX_CONTEXT_BYTES || version == 0 {
        return Err(HostProblem::ResourceExhausted);
    }
    Ok(ProviderStateRecord {
        namespace: CONTEXT_NAMESPACE.into(),
        key: context.run_unit.clone(),
        version,
        payload,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_store::{MemoryStore, SqliteStateStore};
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_SQLITE: AtomicU64 = AtomicU64::new(1);

    #[test]
    fn active_context_survives_reopen_and_fences_older_lease() {
        let memory = MemoryStore::new(Default::default());
        let authority = BtsLifecycleStore::new(&memory);
        let root = BtsLifecycleStore::root_id("TYPE", "ORDER", "UOW1").unwrap();
        authority
            .define_process(
                BtsProcess::new("TYPE", "ORDER", &root, "MAIN", "BTS1", "USER", "UOW1").unwrap(),
                "UOW1",
                "EXEC1",
                "USER",
            )
            .unwrap();
        authority.finish_uow("UOW1", "EXEC1", "USER", true).unwrap();
        authority
            .mutate_process(
                "TYPE",
                "ORDER",
                crate::service::handlers::bts_lifecycle::BtsReplayContext {
                    run_unit: "UOW2",
                    owner_execution: "EXEC2",
                    owner_principal: "USER",
                    replay_key: "start",
                    request_digest: [1; 32],
                },
                |p| {
                    p.start(&root, None, false)?;
                    p.checkpoint(&root, 1, 7, "checkpoint-1")?;
                    Ok(BtsReply::normal())
                },
            )
            .unwrap();
        let context = BtsActivityContext {
            schema_version: CONTEXT_SCHEMA.into(),
            run_unit: "UOW2".into(),
            owner_execution: "EXEC2".into(),
            owner_principal: "USER".into(),
            process_type: "TYPE".into(),
            process_name: "ORDER".into(),
            activity_id: root.clone(),
            activation_epoch: 1,
            owner_lease_epoch: 7,
            closed: false,
            row_version: 0,
        };
        authority.bind_context(context.clone()).unwrap();
        let reopened = BtsLifecycleStore::new(&memory);
        assert_eq!(
            reopened
                .active_context("UOW2", "EXEC2", "USER")
                .unwrap()
                .unwrap()
                .activity_id,
            root
        );
        let mut stale = context.clone();
        stale.owner_lease_epoch = 6;
        assert!(reopened.bind_context(stale).is_err());
        reopened
            .mutate_process(
                "TYPE",
                "ORDER",
                crate::service::handlers::bts_lifecycle::BtsReplayContext {
                    run_unit: "UOW2",
                    owner_execution: "EXEC2",
                    owner_principal: "USER",
                    replay_key: "promote",
                    request_digest: [2; 32],
                },
                |p| {
                    p.checkpoint(&root, 1, 8, "checkpoint-2")?;
                    Ok(BtsReply::normal())
                },
            )
            .unwrap();
        assert!(reopened.active_context("UOW2", "EXEC2", "USER").is_err());
        let mut promoted = context.clone();
        promoted.owner_lease_epoch = 8;
        reopened.bind_context(promoted).unwrap();
        assert_eq!(
            reopened
                .active_context("UOW2", "EXEC2", "USER")
                .unwrap()
                .unwrap()
                .row_version,
            2
        );
        reopened.close_context("UOW2").unwrap();
        assert!(
            reopened
                .active_context("UOW2", "EXEC2", "USER")
                .unwrap()
                .is_none()
        );
        assert!(reopened.bind_context(context).is_err());
    }

    #[test]
    fn sqlite_reopen_retains_exact_activity_context() {
        let directory = std::env::temp_dir().join(format!(
            "mainframe-env-bts-context-{}-{}",
            std::process::id(),
            NEXT_SQLITE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let url = format!("sqlite://{}?mode=rwc", directory.join("state.db").display());
        let root = BtsLifecycleStore::root_id("TYPE", "ORDER", "UOW1").unwrap();
        {
            let sqlite = SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap();
            let authority = BtsLifecycleStore::new(&sqlite);
            authority
                .define_process(
                    BtsProcess::new("TYPE", "ORDER", &root, "MAIN", "BTS1", "USER", "UOW1")
                        .unwrap(),
                    "UOW1",
                    "EXEC1",
                    "USER",
                )
                .unwrap();
            authority.finish_uow("UOW1", "EXEC1", "USER", true).unwrap();
            authority
                .mutate_process(
                    "TYPE",
                    "ORDER",
                    crate::service::handlers::bts_lifecycle::BtsReplayContext {
                        run_unit: "UOW2",
                        owner_execution: "EXEC2",
                        owner_principal: "USER",
                        replay_key: "start",
                        request_digest: [1; 32],
                    },
                    |p| {
                        p.start(&root, None, false)?;
                        p.checkpoint(&root, 1, 7, "checkpoint-1")?;
                        Ok(BtsReply::normal())
                    },
                )
                .unwrap();
            authority
                .bind_context(BtsActivityContext {
                    schema_version: CONTEXT_SCHEMA.into(),
                    run_unit: "UOW2".into(),
                    owner_execution: "EXEC2".into(),
                    owner_principal: "USER".into(),
                    process_type: "TYPE".into(),
                    process_name: "ORDER".into(),
                    activity_id: root.clone(),
                    activation_epoch: 1,
                    owner_lease_epoch: 7,
                    closed: false,
                    row_version: 0,
                })
                .unwrap();
        }
        {
            let sqlite = SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap();
            let reopened = BtsLifecycleStore::new(&sqlite);
            assert_eq!(
                reopened
                    .active_context("UOW2", "EXEC2", "USER")
                    .unwrap()
                    .unwrap()
                    .activity_id,
                root
            );
        }
        std::fs::remove_dir_all(directory).unwrap();
    }
}
