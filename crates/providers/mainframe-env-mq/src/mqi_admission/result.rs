//! Result shape/copy-capacity preflight, not handle/UOW/state permission.

use super::*;
use mainframe_env_host_api::{
    EffectResult, HostResult, canonical_result_digest, canonical_result_size,
};

/// Borrowed existing effect result and original. Outcome identities remain unchanged.
/// The service must still translate uncertainty through the shared effect authority.
#[derive(Debug)]
pub(crate) struct MqMqiResultPreflight<'a> {
    pub(crate) original: &'a EffectRequest,
    pub(crate) reply: &'a EffectResult,
    pub(crate) host_result_bytes: usize,
    pub(crate) host_result_digest: [u8; 32],
}

impl MqMqiAdmitted<'_> {
    pub(crate) fn preflight_result<'a>(
        &'a self,
        reply: &'a EffectResult,
        now_tick: u64,
    ) -> Result<MqMqiResultPreflight<'a>, HostProblem> {
        // Preserve the shared effect service's explicit-unknown precedence;
        // corrupt result metadata cannot convert uncertainty to known rejection.
        if matches!(&reply.outcome, Err(HostProblem::UnknownOutcome)) {
            return Err(HostProblem::UnknownOutcome);
        }
        self.recheck_controls(now_tick)?;
        let typed = match &reply.outcome {
            Ok(HostResult::MqMqi(value)) => {
                if value.result.call != self.envelope.request.call()
                    || value.limits != self.envelope.limits
                {
                    return Err(HostProblem::Malformed);
                }
                Some(value)
            }
            Ok(_) => return Err(HostProblem::Malformed),
            Err(_) => None, // Existing typed host errors retain their own validation/outcome.
        };
        let host_result_bytes = canonical_result_size(
            &reply.outcome,
            self.provider
                .max_result_bytes
                .min(self.envelope.limits.canonical_bytes)
                .min(MAX_CANONICAL_EFFECT_BYTES),
        )?;
        reply.validate(self.effect.sequence, self.host_limits)?;
        if let Some(value) = typed {
            copied_capacities(&self.envelope.request, &value.result.outcome)?;
            value
                .result
                .validate_reviewed_output_for(&self.envelope.request)
                .map_err(|_| HostProblem::Malformed)?;
        }
        self.recheck_controls(now_tick)?;
        Ok(MqMqiResultPreflight {
            original: self.effect,
            reply,
            host_result_bytes,
            host_result_digest: canonical_result_digest(&reply.outcome)?,
        })
    }
}

fn bound(copied: usize, capacity: usize) -> Result<(), HostProblem> {
    if copied > capacity {
        Err(HostProblem::ResourceExhausted)
    } else {
        Ok(())
    }
}

fn copied_capacities(request: &MqMqiRequest, outcome: &MqMqiOutcome) -> Result<(), HostProblem> {
    let output = match outcome {
        MqMqiOutcome::Completed { output, .. }
        | MqMqiOutcome::StatusPending { output }
        | MqMqiOutcome::ReviewedOutput { output, .. } => output,
        MqMqiOutcome::ReviewedStatus { .. }
        | MqMqiOutcome::Pending(_)
        | MqMqiOutcome::UnknownOutcome
        | MqMqiOutcome::DuplicatePossible => return Ok(()),
        MqMqiOutcome::CallbackReturned { .. } => return Err(HostProblem::Malformed),
    };
    use MqMqiOutput as O;
    use MqMqiRequest as R;
    match (request, output) {
        (R::QualifiedFullGet(request), O::QualifiedFullGot(value)) => {
            if let Some(message) = &value.message {
                bound(message.body.len(), request.buffer_capacity)?;
            }
            Ok(()) // Sole host validator binds complete MD, lengths and QName definedness/profile.
        }
        (R::FullPut { .. } | R::FullPutOne { .. }, O::Produced(_)) => Ok(()),
        (R::Rfh2(request), O::Rfh2Observation(value)) => {
            if let (
                MqMqiRfh2Request::HandleToBuffer {
                    buffer_capacity, ..
                },
                MqRfh2BufferObservation::WrittenPrefix(bytes),
            ) = (request, &value.buffer)
            {
                bound(bytes.len(), *buffer_capacity)?;
            }
            Ok(()) // Shared original binding validates descriptor/length/profile.
        }
        (R::FullGet(request), O::FullGot { message, .. }) => {
            if let Some(message) = message {
                bound(message.body.len(), request.buffer_capacity)?;
            }
            // Sole host full-message validator binds MD version/characters,
            // DataLength, required/copied bytes and original mode/truncation.
            Ok(())
        }
        (
            R::Property(MqPropertyRequest::Inquire {
                name_capacity,
                value_capacity,
                ..
            }),
            O::PropertyObservation(MqPropertyObservation::Inquired(value)),
        ) => {
            bound(value.returned_name.len(), *name_capacity)?;
            bound(value.copied_value.len(), *value_capacity)
        }
        (R::Property(MqPropertyRequest::Create { .. }), O::MessageHandle(_))
        | (R::Property(_), O::PropertyObservation(_)) => Ok(()),
        (
            R::Get(request),
            O::Got {
                disposition,
                message,
                ..
            },
        ) => {
            if let Some(message) = message {
                bound(message.body.len(), request.get.buffer_capacity)?;
            }
            // Existing source-bound truncation authority distinguishes required
            // length from copied bytes and checks the original mode/options.
            // A required length may exceed capacity; it is not a copied payload.
            disposition
                .validate(&request.get)
                .map_err(|_| HostProblem::Malformed)
        }
        (
            R::Inquire(request),
            O::Attributes {
                integers,
                characters,
            },
        ) => {
            bound(integers.len(), request.integer_capacity)?;
            bound(characters.len(), request.character_capacity)
        }
        (R::InquireProperty(request), O::Property(value)) => {
            bound(value.name.len(), request.name_capacity)?;
            bound(value.value.len(), request.value_capacity)
        }
        (R::BufferToHandle(request) | R::HandleToBuffer(request), O::Buffer { bytes, .. }) => {
            // DataLength shape remains the shared validator's authority; this
            // checks only copied bytes. Missing required-length payload forms
            // remain explicit status/pending forms, not inferred wire equivalence.
            bound(bytes.len(), request.capacity)
        }
        // Every payload already passed the exhaustive shared call/output validator.
        // These calls have no additional copied-buffer capacity in their request.
        (R::Connect(_) | R::ConnectExtended(_), O::Connected(_))
        | (R::Open(_), O::Opened { .. })
        | (R::CreateMessageHandle { .. }, O::MessageHandle(_))
        | (R::Subscribe(_), O::Subscribed { .. })
        | (R::Put { .. } | R::PutOne { .. }, O::Put { .. } | O::Distribution(_))
        | (R::Back { .. } | R::Begin { .. } | R::Commit { .. }, O::UnitOfWork { .. })
        | (R::SubscriptionRequest { .. }, O::PublicationsRequested { .. })
        | (
            R::Callback { .. }
            | R::Close(_)
            | R::Control { .. }
            | R::Disconnect { .. }
            | R::DeleteMessageHandle { .. }
            | R::DeleteProperty { .. }
            | R::Set(_)
            | R::SetProperty { .. },
            O::NoOutput,
        ) => Ok(()),
        // The direct syncpoint forbidden-context status has no payload.
        (R::Back { .. } | R::Begin { .. } | R::Commit { .. }, O::NoOutput)
            if matches!(
                outcome,
                MqMqiOutcome::Completed {
                    status: MqMqiStatus::FailedEnvironment,
                    ..
                } | MqMqiOutcome::ReviewedOutput { .. }
            ) =>
        {
            Ok(())
        }
        _ => Err(HostProblem::Malformed),
    }
}
