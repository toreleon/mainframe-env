//! Explicit host-installed MQI adapter, not invocation-binding attestation.
//! Source rows 0008/0012, ibm-mq-9.4-mqi-2026-08-31.

use super::*;
use mainframe_env_host_api::mq_mqi::{
    MqMqiCall, MqMqiConnect, MqMqiContext, MqMqiLimits, MqMqiOptions, MqMqiOutcome, MqMqiOutput,
    MqMqiRequest, MqMqiRequestEnvelope, MqMqiStatus,
};
use mainframe_env_host_api::mq_object_route::MqRouteName;
use mainframe_env_host_api::{
    MQ_MAX_HANDLE_SLOTS, MqHandleSharing, MqHconn, MqHostEnvironment, MqMqiHostRequest,
    MqMqiHostResult, MqSyncpointOwner,
};
use std::sync::Arc;

/// A trusted embedding's already-admitted program frame. Implementations must
/// independently mint/check lifecycle ownership under the selected MQ authority,
/// not derive an owner from request assertions or equal binding bytes. This port
/// carries no SAF, registry, UOW or recovery permission. Production setup is
/// responsible for supplying the same frame to its selected provider route.
pub trait MqMqiProgramFrame: Send + Sync {
    fn profile(&self, invocation: &Invocation) -> Result<MqMqiProgramProfile, HostProblem>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MqMqiProgramProfile {
    pub context: MqMqiContext,
    pub limits: MqMqiLimits,
}

pub(super) struct State {
    frame: Arc<dyn MqMqiProgramFrame>,
    profile: MqMqiProgramProfile,
    // ABI aliases only. The MQ registry alone checks token lifetime/access.
    connections: BTreeMap<i32, MqHconn>,
    next_connection: i32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Targets {
    call: MqMqiCall,
    connection: String,
    completion: String,
    reason: String,
    disconnected: Option<i32>,
}

pub(super) fn is_call(program: &str) -> bool {
    let program = normalize(program.trim_matches(['\'', '"']));
    MqMqiCall::ALL.iter().any(|call| call.label() == program)
}

impl ReferenceMachine {
    /// Bind before driving or restoring. Serialized Invocation bindings cannot
    /// install this frame. The earlier public/legacy profile remains unchanged.
    pub fn bind_mqi_program_frame(
        &mut self,
        frame: Arc<dyn MqMqiProgramFrame>,
    ) -> Result<(), MachineProblem> {
        if self.mqi.is_some() || self.executed_steps != 0 || self.effect_sequence != 0 {
            return Err(MachineProblem::InvalidOperation);
        }
        let profile = frame
            .profile(&self.invocation)
            .map_err(MachineProblem::Host)?;
        profile_valid(profile)?;
        self.mqi = Some(State {
            frame,
            profile,
            connections: BTreeMap::new(),
            next_connection: 1,
        });
        Ok(())
    }

    pub(super) fn typed_mq_effect(&mut self, args: &[String]) -> Result<Step, MachineProblem> {
        let call = args.first().ok_or(MachineProblem::InvalidOperation)?;
        let call = normalize(call.trim_matches(['\'', '"']));
        let state = self.mqi.as_ref().ok_or(MachineProblem::UnsupportedForm)?;
        let profile = state.current(&self.invocation)?;
        let using = position(args, "USING").ok_or(MachineProblem::InvalidOperation)?;
        // The initial source signatures require reference arguments. Do not
        // silently erase BY VALUE/CONTENT or output/exception clauses.
        let mut parameters = Vec::new();
        let mut arguments = args[using + 1..].iter().peekable();
        while let Some(argument) = arguments.next() {
            match normalize(argument).as_str() {
                "," | "END-CALL" => {}
                "BY" => {
                    if arguments.next().map(|arg| normalize(arg)).as_deref() != Some("REFERENCE")
                        || arguments.peek().is_none()
                    {
                        return Err(MachineProblem::UnsupportedForm);
                    }
                }
                "VALUE" | "CONTENT" | "REFERENCE" => {
                    return Err(MachineProblem::UnsupportedForm);
                }
                _ => parameters.push(normalize(argument)),
            }
        }
        let (request, targets) = match call.as_str() {
            "MQCONN" => {
                if parameters.len() != 4 {
                    return Err(MachineProblem::InvalidOperation);
                }
                self.mq_long_target(&parameters[1])?;
                self.mq_long_target(&parameters[2])?;
                self.mq_long_target(&parameters[3])?;
                self.mq_output_aliases(&parameters[1..])?;
                if state.connections.len() >= MQ_MAX_HANDLE_SLOTS
                    || state.next_connection == i32::MAX
                {
                    return Err(MachineProblem::ResourceExhausted);
                }
                let name_layout = self
                    .layout(&parameters[0])
                    .ok_or(MachineProblem::UnknownStorage)?;
                if name_layout.category != LayoutCategory::Alphanumeric || name_layout.length != 48
                {
                    return Err(MachineProblem::UnsupportedForm);
                }
                let raw = self.read(&parameters[0])?;
                let significant = raw.split(|&byte| byte == 0).next().unwrap_or(&raw);
                let name =
                    std::str::from_utf8(significant).map_err(|_| MachineProblem::DataException)?;
                let name = name.trim_end_matches(' ');
                let manager = (!name.is_empty())
                    .then(|| MqRouteName::new(name).map_err(|_| MachineProblem::DataException))
                    .transpose()?;
                (
                    MqMqiRequest::Connect(MqMqiConnect {
                        manager,
                        sharing: MqHandleSharing::NonShared,
                        options: MqMqiOptions::ContractDefault,
                    }),
                    Targets {
                        call: MqMqiCall::Connect,
                        connection: parameters[1].clone(),
                        completion: parameters[2].clone(),
                        reason: parameters[3].clone(),
                        disconnected: None,
                    },
                )
            }
            "MQDISC" => {
                if parameters.len() != 3 {
                    return Err(MachineProblem::InvalidOperation);
                }
                for parameter in &parameters {
                    self.mq_long_target(parameter)?;
                }
                self.mq_output_aliases(&parameters)?;
                let value = self.decimal(&parameters[0])?;
                let wire =
                    i32::try_from(value.coefficient).map_err(|_| MachineProblem::DataException)?;
                let connection = state
                    .connections
                    .get(&wire)
                    .copied()
                    .ok_or(MachineProblem::Host(HostProblem::Malformed))?;
                (
                    MqMqiRequest::Disconnect { connection },
                    Targets {
                        call: MqMqiCall::Disconnect,
                        connection: parameters[0].clone(),
                        completion: parameters[1].clone(),
                        reason: parameters[2].clone(),
                        disconnected: Some(wire),
                    },
                )
            }
            _ => return Err(MachineProblem::UnsupportedForm),
        };
        self.effect(
            HostRequest::MqMqi(MqMqiHostRequest {
                envelope: MqMqiRequestEnvelope {
                    context: profile.context,
                    limits: profile.limits,
                    request,
                },
                mutation: self.mutation()?,
            }),
            PendingKind::MqMqi(targets),
        )
    }

    fn mq_long_target(&self, target: &str) -> Result<(), MachineProblem> {
        let layout = self.layout(target).ok_or(MachineProblem::UnknownStorage)?;
        if layout.category != LayoutCategory::Binary
            || !layout.signed
            || layout.scale != 0
            || layout.length != 4
            || layout.occurs != 1
            || layout.digits != 9
        {
            return Err(MachineProblem::UnsupportedForm);
        }
        // Resolve the actual view before dispatch. No result-time discovery of
        // absent/short output storage is permitted.
        if self.read(target)?.len() != 4 {
            return Err(MachineProblem::UnsupportedForm);
        }
        Ok(())
    }

    fn mq_output_aliases(&self, targets: &[String]) -> Result<(), MachineProblem> {
        let views = targets
            .iter()
            .map(|target| {
                let layout = self.layout(target).ok_or(MachineProblem::UnknownStorage)?;
                self.views
                    .get(&layout.name)
                    .ok_or(MachineProblem::UnknownStorage)
            })
            .collect::<Result<Vec<_>, _>>()?;
        for (i, left) in views.iter().enumerate() {
            for right in &views[i + 1..] {
                if left.base == right.base
                    && left.offset < right.offset.saturating_add(right.length)
                    && right.offset < left.offset.saturating_add(left.length)
                {
                    return Err(MachineProblem::UnsupportedForm);
                }
            }
        }
        Ok(())
    }

    pub(super) fn finish_typed_mq(
        &mut self,
        targets: Targets,
        value: MqMqiHostResult,
    ) -> Result<(), MachineProblem> {
        let state = self
            .mqi
            .as_ref()
            .ok_or(MachineProblem::UnexpectedHostResult)?;
        let profile = state.current(&self.invocation)?;
        if value.result.call != targets.call || value.limits != profile.limits {
            return Err(MachineProblem::UnexpectedHostResult);
        }
        value
            .validate(HostLimits::default())
            .map_err(MachineProblem::Host)?;
        // Writeback may normalize a validated OK/NONE observation locally;
        // the journal retains the full original result and its canonical tag.
        let outcome = match value.result.outcome {
            MqMqiOutcome::ReviewedOutput { status, output }
                if status.completion() == mainframe_env_host_api::mq_status::MqCompletion::Ok
                    && status.reason_symbol() == "MQRC_NONE" =>
            {
                MqMqiOutcome::Completed {
                    status: MqMqiStatus::OkNone,
                    output,
                }
            }
            other => other,
        };
        let (completion, reason, connection, completed) = match outcome {
            MqMqiOutcome::Completed {
                status: MqMqiStatus::OkNone,
                output,
            } => {
                let connection = match (targets.call, output) {
                    (
                        MqMqiCall::Connect,
                        MqMqiOutput::Connected(connection @ MqHconn::Issued(_)),
                    ) if !connection.is_historical() => Some(connection),
                    (MqMqiCall::Disconnect, MqMqiOutput::NoOutput) => None,
                    _ => return Err(MachineProblem::UnexpectedHostResult),
                };
                let status = mainframe_env_host_api::mq_status::MqReviewedStatus::from_symbols(
                    targets.call,
                    "MQCC_OK",
                    "MQRC_NONE",
                )
                .map_err(|_| MachineProblem::UnexpectedHostResult)?;
                let (completion, reason) = status.wire_pair();
                (completion, reason, connection, true)
            }
            MqMqiOutcome::ReviewedStatus { status } => {
                let (completion, reason) = status.wire_pair();
                // A status-only observation cannot stand in for a successful
                // connect or disconnect and cannot retire ABI aliases.
                if status.completion() != mainframe_env_host_api::mq_status::MqCompletion::Failed {
                    return Err(MachineProblem::UnexpectedHostResult);
                }
                (completion, reason, None, false)
            }
            MqMqiOutcome::UnknownOutcome | MqMqiOutcome::DuplicatePossible => {
                return Err(MachineProblem::Host(HostProblem::UnknownOutcome));
            }
            _ => return Err(MachineProblem::Host(HostProblem::Unsupported)),
        };
        let wire = if let Some(connection) = connection {
            let state = self
                .mqi
                .as_ref()
                .ok_or(MachineProblem::UnexpectedHostResult)?;
            if let Some((&wire, _)) = state
                .connections
                .iter()
                .find(|(_, old)| **old == connection)
            {
                Some(wire)
            } else {
                if state.connections.len() >= MQ_MAX_HANDLE_SLOTS
                    || state.next_connection == i32::MAX
                {
                    return Err(MachineProblem::Host(HostProblem::UnknownOutcome));
                }
                Some(state.next_connection)
            }
        } else {
            None
        };
        // Preflight *all* writebacks before changing any application storage.
        for target in [&targets.connection, &targets.completion, &targets.reason] {
            self.mq_long_target(target)?;
        }
        if let Some(wire) = wire {
            self.write_decimal(
                &targets.connection,
                Decimal {
                    coefficient: i128::from(wire),
                    scale: 0,
                },
            )?;
        }
        self.write_decimal(
            &targets.completion,
            Decimal {
                coefficient: i128::from(completion),
                scale: 0,
            },
        )?;
        self.write_decimal(
            &targets.reason,
            Decimal {
                coefficient: i128::from(reason),
                scale: 0,
            },
        )?;
        let state = self
            .mqi
            .as_mut()
            .ok_or(MachineProblem::UnexpectedHostResult)?;
        if let (Some(connection), Some(wire)) = (connection, wire) {
            if state.connections.insert(wire, connection).is_none() {
                state.next_connection += 1;
            }
        }
        if completed && let Some(wire) = targets.disconnected {
            state.connections.remove(&wire);
            // z/OS leaves the Hconn output undefined. Retaining its bytes does
            // not retain authority: its alias is now absent and never reused.
        }
        Ok(())
    }
}

impl State {
    fn current(&self, invocation: &Invocation) -> Result<MqMqiProgramProfile, MachineProblem> {
        let profile = self
            .frame
            .profile(invocation)
            .map_err(MachineProblem::Host)?;
        profile_valid(profile)?;
        if profile != self.profile {
            return Err(MachineProblem::Host(HostProblem::IdempotencyConflict));
        }
        Ok(profile)
    }
}

fn profile_valid(profile: MqMqiProgramProfile) -> Result<(), MachineProblem> {
    let context = profile.context;
    if context.owner.environment != MqHostEnvironment::ZosBatch
        || context.syncpoint_owner != MqSyncpointOwner::QueueManager
    {
        return Err(MachineProblem::UnsupportedForm);
    }
    MqMqiRequestEnvelope {
        context,
        limits: profile.limits,
        request: MqMqiRequest::Connect(MqMqiConnect {
            manager: None,
            sharing: MqHandleSharing::NonShared,
            options: MqMqiOptions::ContractDefault,
        }),
    }
    .validate()
    .map_err(|_| MachineProblem::InvalidOperation)
}

#[cfg(test)]
mod tests;
