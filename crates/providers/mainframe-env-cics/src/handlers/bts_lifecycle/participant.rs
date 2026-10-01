//! Idempotent BTS publication and release at the CICS syncpoint boundary.

use super::*;
use crate::service::{CicsService, UowRecord};
use mainframe_env_host_api::CicsUnitOfWorkOutcome;

/// Complete a pending BTS participant during UOW reconciliation after an
/// uncertain syncpoint. Ownership is reconstructed from the durable records.
pub(in crate::service) fn settle_recorded_uow(
    service: &CicsService,
    record: &UowRecord,
) -> Result<(), HostProblem> {
    let Some(metadata) = record.metadata.as_ref() else {
        return Ok(());
    };
    let authority = BtsLifecycleStore::new(service.store.as_ref());
    let acquisition = authority.load_acquisition(&metadata.owner_run_unit)?;
    let context = authority.load_context_row(&metadata.owner_run_unit)?;
    let principal = match (acquisition.as_ref(), context.as_ref()) {
        (None, None) => return Ok(()),
        (Some(acquisition), Some(context))
            if acquisition.owner_principal != context.owner_principal =>
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        (Some(acquisition), _) => acquisition.owner_principal.as_str(),
        (_, Some(context)) => context.owner_principal.as_str(),
    };
    authority.finish_run_uow(
        &metadata.owner_run_unit,
        metadata
            .task_owner_execution
            .as_deref()
            .unwrap_or(&metadata.owner_execution),
        principal,
        record.outcome == CicsUnitOfWorkOutcome::Committed,
    )
}

impl<'a> BtsLifecycleStore<'a> {
    /// Settle pending child definitions and the one UOW acquisition before the
    /// owning CICS syncpoint is finalized. Reissue is safe after an uncertain
    /// provider write because each underlying transition uses CAS and tombstones.
    pub fn finish_run_uow(
        &self,
        run_unit: &str,
        owner_execution: &str,
        owner_principal: &str,
        commit: bool,
    ) -> Result<(), HostProblem> {
        validate_identifier(run_unit, 256)?;
        validate_identifier(owner_execution, 256)?;
        validate_identifier(owner_principal, 256)?;
        let context = self.load_context_row(run_unit)?;
        let acquisition = self.load_acquisition(run_unit)?;
        if let Some(context) = &context
            && (context.owner_execution != owner_execution
                || context.owner_principal != owner_principal)
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        if let Some(acquisition) = &acquisition
            && (acquisition.owner_execution != owner_execution
                || acquisition.owner_principal != owner_principal)
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        super::super::bts_container::settle_bts_container_uow(self.store, run_unit, commit)?;
        if let Some(context) = context {
            self.finish_child_uow(
                &context.process_type,
                &context.process_name,
                run_unit,
                commit,
            )?;
        }
        if let Some(acquisition) = acquisition
            && acquisition.is_held()
        {
            self.finish_uow(run_unit, owner_execution, owner_principal, commit)
                .map_err(|problem| match problem {
                    HostProblem::Condition {
                        name,
                        response: 108,
                        response2: 2,
                    } if commit && name == "PROCESSERR" => HostProblem::UnknownOutcome,
                    other => other,
                })?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_store::{MemoryStore, PostgresStateStore, SqliteStateStore};

    fn seed_frame_reconciliation_restart(store: &dyn ProviderStateStore) {
        let authority = BtsLifecycleStore::new(store);
        let root = BtsLifecycleStore::root_id("TYPE", "RESTART", "FRAME-UOW").unwrap();
        authority
            .define_process(
                BtsProcess::new(
                    "TYPE",
                    "RESTART",
                    &root,
                    "MAIN",
                    "BTS1",
                    "USER",
                    "FRAME-UOW",
                )
                .unwrap(),
                "FRAME-UOW",
                "ROOT",
                "USER",
            )
            .unwrap();
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "cics-uow".into(),
                    key: "frame-pending-syncpoint".into(),
                    version: 1,
                    payload: crate::service::encode_uow(&UowRecord {
                        finalized: false,
                        outcome: CicsUnitOfWorkOutcome::Committed,
                        transaction: "BTS1".into(),
                        metadata: Some(crate::retention::UowRetentionMetadata {
                            effect_key: "frame-pending-syncpoint".into(),
                            owner_execution: "CHILD".into(),
                            task_owner_execution: Some("ROOT".into()),
                            owner_run_unit: "FRAME-UOW".into(),
                            deadline_tick: 100,
                            terminal_tick: None,
                        }),
                    })
                    .unwrap(),
                },
                None,
            )
            .unwrap();
    }

    fn verify_frame_reconciliation_restart(store: std::sync::Arc<dyn ProviderStateStore>) {
        use mainframe_env_execution_api::{IdempotencyKey, InvocationLimits};
        let before = store
            .get_provider_state("cics-uow", "frame-pending-syncpoint")
            .unwrap()
            .unwrap();
        assert_eq!(&before.payload[..5], b"MECU3");
        let service = crate::service::tests::service(store.clone());
        let key = IdempotencyKey::new(&before.key, InvocationLimits::default()).unwrap();
        service
            .reconcile_unit_of_work(&key, CicsUnitOfWorkOutcome::Committed)
            .unwrap();
        let saved = store
            .get_provider_state("cics-uow", &before.key)
            .unwrap()
            .unwrap();
        let decoded = crate::service::decode_uow(&saved.payload).unwrap();
        assert!(decoded.finalized);
        assert_eq!(
            decoded.metadata,
            crate::service::decode_uow(&before.payload)
                .unwrap()
                .metadata
        );
        let authority = BtsLifecycleStore::new(store.as_ref());
        let acquisition = authority.load_acquisition("FRAME-UOW").unwrap().unwrap();
        assert_eq!(acquisition.owner_execution, "ROOT");
        assert!(!acquisition.is_held());
        assert!(
            authority
                .load_process("TYPE", "RESTART")
                .unwrap()
                .unwrap()
                .pending_uow
                .is_none()
        );
        drop(service);
        let service = crate::service::tests::service(store.clone());
        service
            .reconcile_unit_of_work(&key, CicsUnitOfWorkOutcome::Committed)
            .unwrap();
        assert_eq!(
            store.get_provider_state("cics-uow", &before.key).unwrap(),
            Some(saved)
        );
        assert_eq!(
            authority.load_acquisition("FRAME-UOW").unwrap(),
            Some(acquisition)
        );
    }

    #[test]
    fn sqlite_frame_syncpoint_reconciliation_survives_backend_reopen() {
        let directory = std::env::temp_dir().join(format!(
            "mainframe-env-frame-uow-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let url = format!("sqlite://{}?mode=rwc", directory.join("frame.db").display());
        {
            let store = SqliteStateStore::open(&url, 1024 * 1024, 64).unwrap();
            seed_frame_reconciliation_restart(&store);
        }
        verify_frame_reconciliation_restart(std::sync::Arc::new(
            SqliteStateStore::open(&url, 1024 * 1024, 64).unwrap(),
        ));
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    #[ignore = "requires isolated MAINFRAME_ENV_POSTGRES_TEST_URL pointing at PostgreSQL 18"]
    fn postgres_frame_syncpoint_reconciliation_survives_backend_reopen() {
        let url = std::env::var("MAINFRAME_ENV_POSTGRES_TEST_URL").unwrap();
        {
            let store = PostgresStateStore::open(&url, 1024 * 1024, 64).unwrap();
            seed_frame_reconciliation_restart(&store);
        }
        verify_frame_reconciliation_restart(std::sync::Arc::new(
            PostgresStateStore::open(&url, 1024 * 1024, 64).unwrap(),
        ));
    }

    #[test]
    fn frame_syncpoint_reconciliation_settles_root_bts_owner_and_rejects_forgery() {
        use crate::retention::UowRetentionMetadata;
        use crate::service::{decode_uow, encode_uow};
        use mainframe_env_execution_api::{IdempotencyKey, InvocationLimits};
        use mainframe_env_store_api::ProviderStateRecord;
        use std::sync::Arc;

        for (commit, forged) in [(true, false), (false, false), (true, true)] {
            let memory = Arc::new(MemoryStore::new(Default::default()));
            let service = crate::service::tests::service(memory.clone());
            let authority = BtsLifecycleStore::new(memory.as_ref());
            let root = BtsLifecycleStore::root_id("TYPE", "FRAME", "UOW1").unwrap();
            authority
                .define_process(
                    BtsProcess::new("TYPE", "FRAME", &root, "MAIN", "BTS1", "USER", "UOW1")
                        .unwrap(),
                    "UOW1",
                    "ROOT",
                    "USER",
                )
                .unwrap();
            let outcome = if commit {
                CicsUnitOfWorkOutcome::Committed
            } else {
                CicsUnitOfWorkOutcome::RolledBack
            };
            let record = UowRecord {
                finalized: false,
                outcome,
                transaction: "BTS1".into(),
                metadata: Some(UowRetentionMetadata {
                    effect_key: "child-syncpoint".into(),
                    owner_execution: "CHILD".into(),
                    task_owner_execution: Some(if forged { "FOREIGN" } else { "ROOT" }.into()),
                    owner_run_unit: "UOW1".into(),
                    deadline_tick: 100,
                    terminal_tick: None,
                }),
            };
            let row = ProviderStateRecord {
                namespace: "cics-uow".into(),
                key: "child-syncpoint".into(),
                version: 1,
                payload: encode_uow(&record).unwrap(),
            };
            memory.put_provider_state(row.clone(), None).unwrap();
            let before = authority.load_acquisition("UOW1").unwrap().unwrap();
            let key = IdempotencyKey::new(&row.key, InvocationLimits::default()).unwrap();
            if forged {
                assert_eq!(
                    service.reconcile_unit_of_work(&key, outcome),
                    Err(HostProblem::IdempotencyConflict)
                );
                assert_eq!(authority.load_acquisition("UOW1").unwrap().unwrap(), before);
                assert_eq!(
                    memory.get_provider_state("cics-uow", &row.key).unwrap(),
                    Some(row)
                );
                continue;
            }
            service.reconcile_unit_of_work(&key, outcome).unwrap();
            let settled = authority.load_acquisition("UOW1").unwrap().unwrap();
            assert_eq!(settled.owner_execution, "ROOT");
            assert!(!settled.is_held());
            let saved = memory
                .get_provider_state("cics-uow", &row.key)
                .unwrap()
                .unwrap();
            let decoded = decode_uow(&saved.payload).unwrap();
            assert!(decoded.finalized);
            let metadata = decoded.metadata.unwrap();
            assert_eq!(metadata.owner_execution, "CHILD");
            assert_eq!(metadata.task_owner_execution.as_deref(), Some("ROOT"));
            let process = authority.load_process("TYPE", "FRAME").unwrap();
            assert_eq!(process.is_some(), commit);
            if let Some(process) = process {
                assert!(process.pending_uow.is_none());
            }
            // Reconciliation is idempotent and never changes the root acquisition owner.
            service.reconcile_unit_of_work(&key, outcome).unwrap();
            assert_eq!(
                memory.get_provider_state("cics-uow", &row.key).unwrap(),
                Some(saved)
            );
            assert_eq!(
                authority.load_acquisition("UOW1").unwrap().unwrap(),
                settled
            );
        }
    }

    #[test]
    fn syncpoint_publishes_and_releases_or_rolls_back_pending_process() {
        let memory = MemoryStore::new(Default::default());
        let authority = BtsLifecycleStore::new(&memory);
        let root = BtsLifecycleStore::root_id("TYPE", "COMMIT", "UOW1").unwrap();
        authority
            .define_process(
                BtsProcess::new("TYPE", "COMMIT", &root, "MAIN", "BTS1", "USER", "UOW1").unwrap(),
                "UOW1",
                "EXEC1",
                "USER",
            )
            .unwrap();
        authority
            .finish_run_uow("UOW1", "EXEC1", "USER", true)
            .unwrap();
        authority
            .finish_run_uow("UOW1", "EXEC1", "USER", true)
            .unwrap();
        assert!(
            !authority
                .load_acquisition("UOW1")
                .unwrap()
                .unwrap()
                .is_held()
        );
        assert!(
            authority
                .load_process("TYPE", "COMMIT")
                .unwrap()
                .unwrap()
                .pending_uow
                .is_none()
        );

        let root = BtsLifecycleStore::root_id("TYPE", "ROLLBACK", "UOW2").unwrap();
        authority
            .define_process(
                BtsProcess::new("TYPE", "ROLLBACK", &root, "MAIN", "BTS1", "USER", "UOW2").unwrap(),
                "UOW2",
                "EXEC2",
                "USER",
            )
            .unwrap();
        authority
            .finish_run_uow("UOW2", "EXEC2", "USER", false)
            .unwrap();
        authority
            .finish_run_uow("UOW2", "EXEC2", "USER", false)
            .unwrap();
        assert!(
            authority
                .load_process("TYPE", "ROLLBACK")
                .unwrap()
                .is_none()
        );
        assert!(authority.load_activity_index(&root).unwrap().is_none());
    }

    #[test]
    fn syncpoint_rejects_wrong_owner_without_releasing_acquisition() {
        let memory = MemoryStore::new(Default::default());
        let authority = BtsLifecycleStore::new(&memory);
        let root = BtsLifecycleStore::root_id("TYPE", "OWNER", "UOW1").unwrap();
        authority
            .define_process(
                BtsProcess::new("TYPE", "OWNER", &root, "MAIN", "BTS1", "USER", "UOW1").unwrap(),
                "UOW1",
                "EXEC1",
                "USER",
            )
            .unwrap();
        assert_eq!(
            authority.finish_run_uow("UOW1", "OTHER", "USER", true),
            Err(HostProblem::IdempotencyConflict)
        );
        assert!(
            authority
                .load_acquisition("UOW1")
                .unwrap()
                .unwrap()
                .is_held()
        );
    }
}
