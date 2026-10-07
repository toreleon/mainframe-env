//! Privileged source port, tied to one frozen selected service. It is not a
//! caller option or a claim that a native LE runtime has been installed.
use super::*;
use mainframe_env_host_api::mq_mqi::{MqMqiCall, MqRfh2Profile};
use std::cell::Cell;

/// Privileged batch LE/DLL source at genuine original MQCONN/MQCONNX preparation.
/// An embedding must read its actual configured LE CODESET, not request/binding,
/// MQMD, process locale guesses or equal provider rows. Implementations are
/// private trust-boundary inputs; this port alone proves no installed acceptance.
/// Capture runs under the selected service gate: it must be bounded/nonblocking
/// and must not wait for another thread to enter that same service. Synchronous
/// reentry fails closed; callbacks cannot use it as a nested dispatch facility.
pub trait MqBatchLeDllCodesetSource: Send + Sync {
    /// Capture once for a new original connection. Returning1208 admits only the
    /// named UTF8 profile; other values fail closed before registry allocation.
    /// Replay and prior-connection warning reuse never invoke this callback.
    fn capture_codeset(
        &self,
        invocation: &Invocation,
        original: &MqMqiEffectOccurrence<'_>,
    ) -> Result<i32, HostProblem>;
}

thread_local! { static CAPTURING: Cell<bool> = const { Cell::new(false) }; }
pub(crate) fn capturing() -> bool {
    CAPTURING.with(Cell::get)
}
struct CaptureGuard;
impl Drop for CaptureGuard {
    fn drop(&mut self) {
        CAPTURING.with(|v| v.set(false));
    }
}

impl MqService {
    pub(crate) fn capture_rfh2_source(
        &self,
        invocation: &Invocation,
        original: &MqMqiEffectOccurrence<'_>,
    ) -> Result<Option<MqRfh2Profile>, HostProblem> {
        let Some(source) = &self.rfh2_source else {
            return Ok(None);
        };
        if !matches!(
            original.envelope().request.call(),
            MqMqiCall::Connect | MqMqiCall::ConnectExtended
        ) || capturing()
        {
            return Err(HostProblem::Unsupported);
        }
        CAPTURING.with(|v| v.set(true));
        let _guard = CaptureGuard;
        let value = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            source.capture_codeset(invocation, original)
        }))
        .map_err(|_| HostProblem::InfrastructureFailure)??;
        if value != mainframe_env_host_api::mq_mqi::property::mq_property_profile_ccsid() {
            return Err(HostProblem::Unsupported);
        }
        Ok(Some(MqRfh2Profile::ZosBatchUtf8NativeV1))
    }
}

impl MqTrustedBatchRuntime {
    /// Privileged immutable setup for batch LE/DLL RFH2 generation. The same
    /// newly opened service/store retains this source; old open remains closed
    /// to RFH2. This does not initialize rich rows or install native LE support.
    #[allow(clippy::too_many_arguments)]
    pub fn open_with_rfh2_codeset_source(
        store: Arc<dyn PlatformStore>,
        limits: MqLimits,
        generation: u64,
        fence: u64,
        authorizer: Arc<dyn EnterpriseAuthorizer>,
        clock: Arc<dyn MqReplayClock>,
        provider: CapabilityDescriptor,
        host_limits: HostLimits,
        mqi_limits: MqMqiLimits,
        source: Arc<dyn MqBatchLeDllCodesetSource>,
    ) -> Result<Self, HostProblem> {
        let mut runtime = Self::open(
            store,
            limits,
            generation,
            fence,
            authorizer,
            clock,
            provider,
            host_limits,
            mqi_limits,
        )?;
        let inner = Arc::get_mut(&mut runtime.inner).ok_or(HostProblem::InfrastructureFailure)?;
        let service = Arc::get_mut(&mut inner.service).ok_or(HostProblem::InfrastructureFailure)?;
        service.rfh2_source = Some(source);
        Ok(runtime)
    }
}
