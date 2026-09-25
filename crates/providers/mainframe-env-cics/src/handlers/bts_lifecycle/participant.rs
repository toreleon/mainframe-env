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
        &metadata.owner_execution,
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
        if let Some(context) = self.load_context_row(run_unit)? {
            if context.owner_execution != owner_execution
                || context.owner_principal != owner_principal
            {
                return Err(HostProblem::IdempotencyConflict);
            }
            self.finish_child_uow(
                &context.process_type,
                &context.process_name,
                run_unit,
                commit,
            )?;
        }
        if let Some(acquisition) = self.load_acquisition(run_unit)? {
            if acquisition.owner_execution != owner_execution
                || acquisition.owner_principal != owner_principal
            {
                return Err(HostProblem::IdempotencyConflict);
            }
            if acquisition.is_held() {
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
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_store::MemoryStore;

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
