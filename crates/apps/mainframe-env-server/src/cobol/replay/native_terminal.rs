//! Pure original CALL closure through the existing strict receipt parser.
use super::*;
use mainframe_env_host_api::{HostResult, canonical_result_digest};
use mainframe_env_store_api::{
    EffectDigestFormat, EffectState, RootClosureSnapshot, TerminalRowDependency,
};

pub(in crate::cobol) fn validate_native_calls(
    closure: &RootClosureSnapshot,
) -> Result<(), HostProblem> {
    // The caller already validated the complete scoped count/byte budget.
    // Narrow further before JSON parsing, whose reply arrays may allocate.
    let ceiling = InvocationLimits::default()
        .max_payload_bytes
        .checked_mul(4)
        .and_then(|n| n.checked_add(16 * 1024))
        .ok_or(HostProblem::ResourceExhausted)?;
    for dependency in &closure.provider_dependencies {
        let TerminalRowDependency::Exact(row) = dependency else {
            continue;
        };
        if row.namespace != CALL_REPLAY_NAMESPACE {
            continue;
        }
        if row.payload.len() > ceiling {
            return Err(HostProblem::ResourceExhausted);
        }
        let DecodedReceipt::Current(receipt) =
            decode_receipt(row).map_err(|_| HostProblem::UnknownOutcome)?
        else {
            // Unattributed legacy history cannot attest native root closure.
            return Err(HostProblem::Unsupported);
        };
        if receipt.owner_run_unit != closure.claim.admission().execution.run_unit_id.as_str() {
            continue; // Captured unrelated attributed history remains unchanged.
        }
        let actor = closure
            .actors
            .iter()
            .find(|actor| {
                actor
                    .call
                    .as_ref()
                    .is_some_and(|call| call.namespace == row.namespace && call.key == row.key)
            })
            .ok_or(HostProblem::UnknownOutcome)?;
        let call = actor.call.as_ref().ok_or(HostProblem::UnknownOutcome)?;
        let parent = closure
            .actors
            .iter()
            .find(|parent| Some(&parent.execution.execution_id) == actor.parent.as_ref())
            .ok_or(HostProblem::UnknownOutcome)?;
        if receipt.child_execution != actor.execution.execution_id.as_str()
            || receipt.owner_execution != parent.execution.execution_id.as_str()
            || receipt.owner_principal != parent.execution.principal.as_str()
            || receipt.transfer.is_some()
            || receipt.target.is_some()
            || receipt
                .completion_tick
                .is_none_or(|tick| tick == 0 || tick > closure.observed_tick)
        {
            return Err(HostProblem::UnknownOutcome);
        }
        let effect = parent
            .effects
            .iter()
            .find(|effect| effect.key == call.effect_key)
            .ok_or(HostProblem::UnknownOutcome)?;
        if effect.state != EffectState::Completed
            || effect.digest_format != EffectDigestFormat::CanonicalHostV1
            || effect.request_digest != call.request_digest
        {
            return Err(HostProblem::UnknownOutcome);
        }
        let reply = receipt.reply.ok_or(HostProblem::UnknownOutcome)?;
        let result = Ok(HostResult::Program(
            BoundedPayload::new(reply.schema, reply.bytes, InvocationLimits::default())
                .map_err(|_| HostProblem::UnknownOutcome)?,
        ));
        if effect.result_digest != Some(canonical_result_digest(&result)?) {
            return Err(HostProblem::UnknownOutcome);
        }
    }
    // A missing original CALL is not made acceptable by a completed child row.
    for actor in &closure.actors {
        if let Some(call) = &actor.call {
            if !closure.provider_dependencies.iter().any(|dependency| matches!(dependency,
                TerminalRowDependency::Exact(row) if row.namespace == call.namespace && row.key == call.key)) {
                return Err(HostProblem::UnknownOutcome);
            }
        } else if actor.parent.is_some() {
            return Err(HostProblem::UnknownOutcome);
        }
    }
    Ok(())
}
