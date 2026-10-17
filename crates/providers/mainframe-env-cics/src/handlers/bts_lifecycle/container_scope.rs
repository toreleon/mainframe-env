//! Read-only projection of a UOW-held acquisition for sibling BTS containers.

use super::*;

/// Access to process containers implied by the acquired activity's position.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BtsProcessContainerAccess {
    /// A root acquisition (including DEFINE PROCESS) permits reads and writes.
    ReadWrite,
    /// An acquired descendant permits reads of its process containers only.
    ReadOnly,
}

/// Identity and epoch from the sole durable process/activity authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BtsAcquiredProcessContainerScope {
    pub process_type: String,
    pub process_name: String,
    pub acquired_activity_id: String,
    pub root_activity_id: String,
    pub acquisition_epoch: u64,
    pub access: BtsProcessContainerAccess,
}

impl BtsAcquiredProcessContainerScope {
    /// GET CONTAINER (BTS) ACQPROCESS requires an acquired root in this UOW.
    pub fn permits_acqprocess(&self) -> bool {
        self.access == BtsProcessContainerAccess::ReadWrite
    }
}

impl BtsLifecycleStore<'_> {
    /// Resolve the current UOW's acquired process container scope.
    ///
    /// An acquired descendant allows a process-container *read* even though
    /// GET CONTAINER ACQPROCESS names only an acquired root. This projection
    /// supplies no command selector; the sibling lane must resolve its
    /// descendant selector from pinned sources before exposing a route. The
    /// caller performs typed SAF/audit before container access.
    pub fn acquired_process_container_scope(
        &self,
        run_unit: &str,
        owner_execution: &str,
        owner_principal: &str,
    ) -> Result<Option<BtsAcquiredProcessContainerScope>, HostProblem> {
        validate_identifier(owner_execution, 256)?;
        validate_identifier(owner_principal, 256)?;
        let Some(acquisition) = self
            .load_acquisition(run_unit)?
            .filter(BtsAcquisition::is_held)
        else {
            return Ok(None);
        };
        if acquisition.owner_execution != owner_execution
            || acquisition.owner_principal != owner_principal
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        if acquisition
            .effect
            .as_ref()
            .is_some_and(|effect| effect.deferred_duplicate)
        {
            // NOCHECK has not established a new process tree. Never borrow
            // containers from the existing, conflicting process row.
            return Err(HostProblem::NotFound);
        }
        let process_type = acquisition
            .process_type
            .as_deref()
            .ok_or(HostProblem::InfrastructureFailure)?;
        let process_name = acquisition
            .process_name
            .as_deref()
            .ok_or(HostProblem::InfrastructureFailure)?;
        let activity_id = acquisition
            .activity_id
            .as_deref()
            .ok_or(HostProblem::InfrastructureFailure)?;
        let process = self
            .load_process(process_type, process_name)?
            .ok_or(HostProblem::InfrastructureFailure)?;
        let activity = process
            .activities
            .get(activity_id)
            .ok_or(HostProblem::InfrastructureFailure)?;
        let index = self
            .load_activity_index(activity_id)?
            .ok_or(HostProblem::InfrastructureFailure)?;
        if !process.visible_to(run_unit)
            || activity.acquired_by.as_deref() != Some(run_unit)
            || activity
                .pending_uow
                .as_deref()
                .is_some_and(|owner| owner != run_unit)
            || index.process_type != process_type
            || index.process_name != process_name
            || index
                .pending_uow
                .as_deref()
                .is_some_and(|owner| owner != run_unit)
            || index.parent_id != activity.parent_id
        {
            return Err(HostProblem::InfrastructureFailure);
        }
        Ok(Some(BtsAcquiredProcessContainerScope {
            process_type: process_type.into(),
            process_name: process_name.into(),
            acquired_activity_id: activity_id.into(),
            root_activity_id: process.root_id.clone(),
            acquisition_epoch: acquisition.epoch,
            access: if activity_id == process.root_id {
                BtsProcessContainerAccess::ReadWrite
            } else {
                BtsProcessContainerAccess::ReadOnly
            },
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_store::{MemoryStore, SqliteStateStore};
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_SQLITE: AtomicU64 = AtomicU64::new(1);

    fn held_descendant(store: &dyn ProviderStateStore) -> String {
        let authority = BtsLifecycleStore::new(store);
        let root = BtsLifecycleStore::root_id("TYPE", "ORDER", "UOW1").unwrap();
        authority
            .define_process(
                BtsProcess::new("TYPE", "ORDER", &root, "MAIN", "BTS1", "USER", "UOW1").unwrap(),
                "UOW1",
                "EXEC1",
                "USER",
            )
            .unwrap();
        let defined = authority
            .acquired_process_container_scope("UOW1", "EXEC1", "USER")
            .unwrap()
            .unwrap();
        assert_eq!(defined.access, BtsProcessContainerAccess::ReadWrite);
        assert!(defined.permits_acqprocess());
        assert_eq!(defined.acquisition_epoch, 1);
        assert_eq!(
            authority.acquired_process_container_scope("UOW1", "OTHER", "USER"),
            Err(HostProblem::IdempotencyConflict)
        );
        authority.finish_uow("UOW1", "EXEC1", "USER", true).unwrap();
        assert_eq!(
            authority
                .acquired_process_container_scope("UOW1", "EXEC1", "USER")
                .unwrap(),
            None
        );
        authority
            .acquire("UOW2", "EXEC2", "USER", "TYPE", "ORDER", &root)
            .unwrap();
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
                |process| {
                    process.start(&root, None, true)?;
                    Ok(BtsReply::normal())
                },
            )
            .unwrap();
        let child = authority
            .define_child(
                "TYPE",
                "ORDER",
                &root,
                &super::super::children::BtsChildDefinition {
                    name: "CHILD".into(),
                    completion_event: "DONE".into(),
                    program: "WORKER".into(),
                    transid: "BTS2".into(),
                    userid: "USER".into(),
                },
                crate::service::handlers::bts_lifecycle::BtsReplayContext {
                    run_unit: "UOW2",
                    owner_execution: "EXEC2",
                    owner_principal: "USER",
                    replay_key: "define-child",
                    request_digest: [2; 32],
                },
            )
            .unwrap();
        authority.finish_uow("UOW2", "EXEC2", "USER", true).unwrap();
        authority
            .acquire("UOW3", "EXEC3", "USER", "TYPE", "ORDER", &child)
            .unwrap();
        child
    }

    fn check_descendant(store: &dyn ProviderStateStore, child: &str) {
        let authority = BtsLifecycleStore::new(store);
        let scope = authority
            .acquired_process_container_scope("UOW3", "EXEC3", "USER")
            .unwrap()
            .unwrap();
        assert_eq!(scope.process_type, "TYPE");
        assert_eq!(scope.process_name, "ORDER");
        assert_eq!(scope.acquired_activity_id, child);
        assert_ne!(scope.root_activity_id, child);
        assert_eq!(scope.access, BtsProcessContainerAccess::ReadOnly);
        assert!(!scope.permits_acqprocess());
        assert_eq!(
            authority.acquired_process_container_scope("UOW3", "EXEC3", "OTHER"),
            Err(HostProblem::IdempotencyConflict)
        );
        authority.finish_uow("UOW3", "EXEC3", "USER", true).unwrap();
        assert_eq!(
            authority
                .acquired_process_container_scope("UOW3", "EXEC3", "USER")
                .unwrap(),
            None
        );
    }

    #[test]
    fn descendant_exposes_process_read_scope_without_acqprocess() {
        let memory = MemoryStore::new(Default::default());
        let child = held_descendant(&memory);
        check_descendant(&memory, &child);
    }

    #[test]
    fn descendant_scope_and_owner_survive_sqlite_reopen() {
        let directory = std::env::temp_dir().join(format!(
            "mainframe-env-bts-container-scope-{}-{}",
            std::process::id(),
            NEXT_SQLITE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let url = format!("sqlite://{}?mode=rwc", directory.join("state.db").display());
        let child = {
            let sqlite = SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap();
            held_descendant(&sqlite)
        };
        {
            let sqlite = SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap();
            check_descendant(&sqlite, &child);
        }
        std::fs::remove_dir_all(directory).unwrap();
    }
}
