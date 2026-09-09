use mainframe_env_execution_api::{BoundedPayload, Invocation, InvocationLimits};
use mainframe_env_host_api::HostProblem;

#[must_use]
pub const fn compatible_system_services() -> &'static [&'static str] {
    &["CEEDAYS", "COBDATFT", "MVSWAIT", "CEE3ABD"]
}

pub(crate) fn bind_compatible_runtime_services(
    invocation: &mut Invocation,
) -> Result<(), HostProblem> {
    let limits = InvocationLimits::default();
    for name in compatible_system_services() {
        let key = format!("cobol.runtime-service.{name}");
        let value = format!("le:{name}:1");
        if let Some(existing) = invocation.bindings.get(&key) {
            if existing.schema() != "mainframe-env.runtime-service-selector@1"
                || existing.bytes() != value.as_bytes()
            {
                return Err(HostProblem::Malformed);
            }
            continue;
        }
        if invocation.bindings.len() >= limits.max_bindings {
            return Err(HostProblem::ResourceExhausted);
        }
        invocation.bindings.insert(
            key,
            BoundedPayload::new(
                "mainframe-env.runtime-service-selector@1",
                value.into_bytes(),
                limits,
            )
            .map_err(|_| HostProblem::ResourceExhausted)?,
        );
    }
    Ok(())
}

pub(super) fn with_compatible_runtime_services(
    mut invocation: Invocation,
) -> Result<Invocation, HostProblem> {
    bind_compatible_runtime_services(&mut invocation)?;
    Ok(invocation)
}
