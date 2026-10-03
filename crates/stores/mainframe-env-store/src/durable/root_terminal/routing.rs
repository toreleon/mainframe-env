//! Narrow native trait hooks and unchanged shared audit-key framing.
use super::*;

pub(crate) fn audit_storage_key(execution_id: &ExecutionId, suffix: &str) -> String {
    format!(
        "{:03}:{}:{}",
        execution_id.as_str().len(),
        execution_id,
        suffix
    )
}

macro_rules! root_journal_methods {
    (SqliteStateStore) => {
        fn mutate_root_provider_states(
            &self,
            request: mainframe_env_store_api::RootProviderPublication,
        ) -> Result<(), StoreError> {
            self.root_mutate_provider(request)
        }
        fn fence_root_driver(
            &self,
            claim: &mainframe_env_store_api::RootDriverClaim,
            execution: &ExecutionRecord,
            tick: u64,
        ) -> Result<ProviderStateRecord, StoreError> {
            self.root_fence(claim, execution, tick)
        }
        fn admit_root_driver(
            &self,
            admission: mainframe_env_store_api::RootDriverAdmission,
        ) -> Result<mainframe_env_store_api::RootDriverClaim, StoreError> {
            self.root_admit(admission)
        }
        fn admit_root_child(
            &self,
            admission: mainframe_env_store_api::RootChildAdmission,
        ) -> Result<(), StoreError> {
            self.root_admit_child(admission)
        }
        fn register_root_provider_row(
            &self,
            admission: mainframe_env_store_api::RootProviderRowAdmission,
        ) -> Result<(), StoreError> {
            self.root_register_row(admission)
        }
        fn close_root_driver(
            &self,
            claim: &mainframe_env_store_api::RootDriverClaim,
            execution: &ExecutionRecord,
            tick: u64,
        ) -> Result<mainframe_env_store_api::RootClosureSnapshot, StoreError> {
            self.root_close(claim, execution, tick)
        }
        fn commit_root_terminal_step(
            &self,
            request: mainframe_env_store_api::RootTerminalPublication,
        ) -> Result<mainframe_env_store_api::RootTerminalCommit, StoreError> {
            self.root_commit(request)
        }
    };
    (PostgresStateStore) => {};
}
macro_rules! root_audit_methods {
    (SqliteStateStore) => {
        fn audit_subject_records(
            &self,
            execution: &ExecutionId,
            max: usize,
        ) -> Result<Vec<mainframe_env_execution_api::AuditSubjectRecord>, StoreError> {
            self.root_audit_subjects(execution, max)
        }
    };
    (PostgresStateStore) => {};
}
pub(crate) use root_audit_methods;
pub(crate) use root_journal_methods;
