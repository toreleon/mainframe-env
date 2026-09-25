use super::*;

pub(super) fn retention_wall_tick() -> Result<u64, HostProblem> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| HostProblem::InfrastructureFailure)?
        .as_millis();
    let tick = u64::try_from(millis).map_err(|_| HostProblem::ResourceExhausted)?;
    (tick != 0)
        .then_some(tick)
        .ok_or(HostProblem::InfrastructureFailure)
}
