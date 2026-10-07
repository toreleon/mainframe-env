//! Minimal trait routing to backend-owned physical methods. PostgreSQL refuses.
macro_rules! checked_read_methods {
    () => {
        fn publish_provider_read_audited(
            &self,
            request: mainframe_env_store_api::CheckedProviderReadPublication,
        ) -> Result<(), mainframe_env_store_api::StoreError> {
            self.publish_checked_read(request)
        }
        fn assert_provider_replay(
            &self,
            request: mainframe_env_store_api::ProviderReplayAssertion,
        ) -> Result<(), mainframe_env_store_api::StoreError> {
            self.assert_checked_replay(request)
        }
        fn publish_provider_states_audited(
            &self,
            request: mainframe_env_store_api::AuditedProviderPublication,
        ) -> Result<(), mainframe_env_store_api::StoreError> {
            self.publish_audited(request)
        }
    };
}
pub(crate) use checked_read_methods;
