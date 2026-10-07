//! Explicit host-installed MQI adapter, not invocation-binding attestation.
//! Source rows 0001/0007/0008/0012, ibm-mq-9.4-mqi-2026-08-31.

use super::*;
use mainframe_env_host_api::mq_mqi::{
    MqMqiCall, MqMqiConnect, MqMqiContext, MqMqiLimits, MqMqiOptions, MqMqiOutcome, MqMqiOutput,
    MqMqiRequest, MqMqiRequestEnvelope, MqMqiStatus, MqMqiUnitOfWork,
};
use mainframe_env_host_api::mq_object_route::MqRouteName;
use mainframe_env_host_api::{
    MQ_MAX_HANDLE_SLOTS, MqHandleSharing, MqHconn, MqHostEnvironment, MqMqiHostRequest,
    MqMqiHostResult, MqSyncpointOwner,
};
use std::sync::Arc;

mod abi_scope;
mod connection_writeback;
mod connx;
mod point;
pub use abi_scope::MqMqiAbiScope;
pub use connx::MqMqiConnxProfile;
pub use point::{MqMqiNativePoint, MqMqiNativePointTarget, MqMqiNativeStructure};

/// A trusted embedding's already-admitted program frame. Implementations must
/// independently mint/check lifecycle ownership under the selected MQ authority,
/// not derive an owner from request assertions or equal binding bytes. This port
/// carries no SAF, registry, UOW or recovery permission. Production setup is
/// responsible for supplying the same frame to its selected provider route.
pub trait MqMqiProgramFrame: Send + Sync {
    fn profile(&self, invocation: &Invocation) -> Result<MqMqiProgramProfile, HostProblem>;

    /// The SAME independently admitted task-root ABI allocation, shared by its
    /// SAME TASK frames. Equality of context/bindings is not a sharing proof.
    /// Old embeddings retain their per-machine connection-only ABI; native
    /// point/property consumers must require the deliberate shared scope.
    /// The returned table supplies no registry, original-effect or SAF authority.
    fn abi_scope(
        &self,
        _invocation: &Invocation,
    ) -> Result<Option<Arc<MqMqiAbiScope>>, HostProblem> {
        Ok(None)
    }

    /// Independently selected structure ABI and ordinary connection profile.
    /// No invocation binding or application Options value attests this input.
    /// Existing embeddings refuse CONNX until deliberately forwarding this port.
    fn connx_profile(&self, _invocation: &Invocation) -> Result<MqMqiConnxProfile, HostProblem> {
        Err(HostProblem::Unsupported)
    }

    /// Capture the actual selected frame's structure ABI BEFORE reading raw
    /// application descriptors. Implementations wrap the same opaque provider
    /// observation and preserve its original root/store/frame/current-unit
    /// identity. Bindings and numeric descriptor fields cannot select this port.
    /// Old embeddings refuse native point calls until deliberately configured.
    fn native_structure(
        &self,
        _invocation: &Invocation,
        _call: MqMqiCall,
        _connection: MqHconn,
    ) -> Result<Arc<dyn MqMqiNativeStructure>, HostProblem> {
        Err(HostProblem::Unsupported)
    }

    /// Read-only current local-UOW assertion for an actual live connection.
    /// The selected provider must still check original intent, logical owner,
    /// registry/control incarnation and durable UOW CAS when it executes the
    /// effect. This port neither authorizes a decision nor accepts a wire ID.
    /// Old embeddings fail closed until they deliberately supply that lookup.
    fn local_unit(
        &self,
        _invocation: &Invocation,
        _connection: MqHconn,
    ) -> Result<MqMqiUnitOfWork, HostProblem> {
        Err(HostProblem::Unsupported)
    }
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
    connx_profile: Option<MqMqiConnxProfile>,
    scope: Option<Arc<MqMqiAbiScope>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Targets {
    call: MqMqiCall,
    connection: String,
    completion: String,
    reason: String,
    disconnected: Option<i32>,
    unit: Option<u64>,
    connx: Option<connx::Capture>,
    connect: Option<connection_writeback::Capture>,
    scope: Option<Arc<MqMqiAbiScope>>,
    reservation: Option<abi_scope::Reservation>,
    scoped_arguments: Vec<connx::Storage>,
    pending_lease: Option<abi_scope::PendingLease>,
    point: Option<point::Capture>,
}

pub(super) fn is_call(program: &str) -> bool {
    let program = normalize(program.trim_matches(['\'', '"']));
    MqMqiCall::ALL.iter().any(|call| call.label() == program)
}

pub(super) fn validate_reply(
    pending: &Pending,
    result: &EffectResult,
) -> Result<(), MachineProblem> {
    if let PendingKind::MqMqi(targets) = &pending.kind
        && matches!(result.outcome, Err(HostProblem::UnknownOutcome))
        && let Some(scope) = &targets.scope
    {
        scope.fence();
    }
    result
        .validate(pending.sequence, HostLimits::default())
        .map_err(|problem| {
            MachineProblem::Host(if matches!(&pending.kind, PendingKind::MqMqi(_)) {
                if let PendingKind::MqMqi(targets) = &pending.kind
                    && let Some(scope) = &targets.scope
                {
                    scope.fence();
                }
                // The typed call has already been dispatched. An unusable
                // envelope cannot establish that its owned work did not occur.
                HostProblem::UnknownOutcome
            } else {
                problem
            })
        })
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
        let scope = connx::contained(|| {
            frame
                .abi_scope(&self.invocation)
                .map_err(MachineProblem::Host)
        })?;
        if let Some(scope) = &scope {
            scope
                .require_context(profile.context)
                .map_err(MachineProblem::Host)?;
        }
        self.mqi = Some(State {
            frame,
            profile,
            connections: BTreeMap::new(),
            next_connection: 1,
            connx_profile: None,
            scope,
        });
        Ok(())
    }

    pub(super) fn typed_mq_effect(&mut self, args: &[String]) -> Result<Step, MachineProblem> {
        let call = args.first().ok_or(MachineProblem::InvalidOperation)?;
        let call = normalize(call.trim_matches(['\'', '"']));
        let state = self.mqi.as_ref().ok_or(MachineProblem::UnsupportedForm)?;
        let profile = if matches!(
            call.as_str(),
            "MQCONN" | "MQCONNX" | "MQPUT" | "MQPUT1" | "MQGET"
        ) {
            connx::contained(|| state.current(&self.invocation))?
        } else {
            state.current(&self.invocation)?
        };
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
        let (request, mut targets) = match call.as_str() {
            "MQCONN" => {
                if parameters.len() != 4 {
                    return Err(MachineProblem::InvalidOperation);
                }
                self.mq_long_target(&parameters[1])?;
                self.mq_long_target(&parameters[2])?;
                self.mq_long_target(&parameters[3])?;
                self.mq_output_aliases(&parameters)?;
                let capture = self.capture_connect(&parameters)?;
                if state.scope.is_none()
                    && (state.connections.len() >= MQ_MAX_HANDLE_SLOTS
                        || state.next_connection == i32::MAX)
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
                        unit: None,
                        connx: None,
                        connect: Some(capture),
                        scope: None,
                        reservation: None,
                        scoped_arguments: Vec::new(),
                        pending_lease: None,
                        point: None,
                    },
                )
            }
            "MQCONNX" => self.prepare_connx(&parameters)?,
            "MQOPEN" | "MQCLOSE" => self.prepare_point(&parameters, call == "MQOPEN")?,
            "MQPUT" | "MQPUT1" => self.prepare_full_put(&parameters, call == "MQPUT1")?,
            "MQGET" => self.prepare_full_get(&parameters)?,
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
                let connection = state.connection(wire)?;
                (
                    MqMqiRequest::Disconnect { connection },
                    Targets {
                        call: MqMqiCall::Disconnect,
                        connection: parameters[0].clone(),
                        completion: parameters[1].clone(),
                        reason: parameters[2].clone(),
                        disconnected: Some(wire),
                        unit: None,
                        connx: None,
                        connect: None,
                        scope: None,
                        reservation: None,
                        scoped_arguments: Vec::new(),
                        pending_lease: None,
                        point: None,
                    },
                )
            }
            "MQCMIT" | "MQBACK" => {
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
                let connection = state.connection(wire)?;
                let unit = state
                    .frame
                    .local_unit(&self.invocation, connection)
                    .map_err(MachineProblem::Host)?;
                let MqMqiUnitOfWork::Local { unit } = unit else {
                    return Err(MachineProblem::Host(HostProblem::Unsupported));
                };
                if unit == 0 {
                    return Err(MachineProblem::Host(HostProblem::Malformed));
                }
                // Lookup callbacks cannot change the frame behind the original
                // pending effect. Recheck before allocating its sequence/key.
                state.current(&self.invocation)?;
                let request = if call == "MQCMIT" {
                    MqMqiRequest::Commit { connection, unit }
                } else {
                    MqMqiRequest::Back { connection, unit }
                };
                (
                    request,
                    Targets {
                        call: if call == "MQCMIT" {
                            MqMqiCall::Commit
                        } else {
                            MqMqiCall::Back
                        },
                        connection: parameters[0].clone(),
                        completion: parameters[1].clone(),
                        reason: parameters[2].clone(),
                        disconnected: None,
                        unit: Some(unit),
                        connx: None,
                        connect: None,
                        scope: None,
                        reservation: None,
                        scoped_arguments: Vec::new(),
                        pending_lease: None,
                        point: None,
                    },
                )
            }
            _ => return Err(MachineProblem::UnsupportedForm),
        };
        if let Some(capture) = &targets.connx {
            self.recheck_connx(capture)?;
        }
        if let Some(capture) = &targets.connect {
            self.recheck_connect(capture)?;
        }
        targets.scope = state.scope.clone();
        if let Some(scope) = &targets.scope {
            targets.pending_lease = Some(abi_scope::PendingLease::new(scope.clone()));
            targets.scoped_arguments = if let Some(point) = &targets.point {
                point.arguments.clone()
            } else {
                parameters
                    .iter()
                    .map(|p| self.connx_storage(p))
                    .collect::<Result<Vec<_>, _>>()?
            };
            if targets.scoped_arguments.iter().any(|s| {
                s.layout.category == LayoutCategory::Binary
                    && (s.layout.native_binary || s.view.offset % 4 != 0)
            }) {
                return Err(MachineProblem::UnsupportedForm);
            }
            if matches!(
                targets.call,
                MqMqiCall::Connect | MqMqiCall::ConnectExtended | MqMqiCall::Open
            ) {
                targets.reservation = Some(scope.reserve().map_err(MachineProblem::Host)?);
            }
        }
        let connx_profile = targets.connx.as_ref().map(|capture| capture.profile);
        let step = self.effect(
            HostRequest::MqMqi(MqMqiHostRequest {
                envelope: MqMqiRequestEnvelope {
                    context: profile.context,
                    limits: profile.limits,
                    request,
                },
                mutation: self.mutation()?,
            }),
            PendingKind::MqMqi(targets),
        )?;
        if let Some(Pending {
            kind: PendingKind::MqMqi(targets),
            ..
        }) = &self.pending
            && let Some(lease) = &targets.pending_lease
        {
            lease.arm();
        }
        if let Some(profile) = connx_profile {
            self.mqi
                .as_mut()
                .ok_or(MachineProblem::InvalidOperation)?
                .connx_profile = Some(profile);
        }
        Ok(step)
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
        let mut scope_completion = abi_scope::CompletionGuard::new(targets.scope.clone());
        let state = self
            .mqi
            .as_ref()
            .ok_or(MachineProblem::UnexpectedHostResult)?;
        let profile = if let Some(capture) = &targets.connx {
            self.recheck_connx(capture)?;
            connx::contained(|| state.current(&self.invocation))?
        } else if let Some(capture) = &targets.connect {
            self.recheck_connect(capture)?;
            connx::contained(|| state.current(&self.invocation))?
        } else if targets.point.is_some() {
            connx::contained(|| state.current(&self.invocation))?
        } else {
            state.current(&self.invocation)?
        };
        if value.result.call != targets.call || value.limits != profile.limits {
            return Err(MachineProblem::UnexpectedHostResult);
        }
        value
            .validate(HostLimits::default())
            .map_err(MachineProblem::Host)?;
        if targets.point.is_some() {
            self.finish_point_mq(&targets, value.result.outcome)?;
            if let Some(lease) = &targets.pending_lease {
                lease.known();
            }
            scope_completion.known();
            return Ok(());
        }
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
            MqMqiOutcome::ReviewedOutput {
                status,
                output: MqMqiOutput::Connected(connection @ MqHconn::Issued(_)),
            } if matches!(
                targets.call,
                MqMqiCall::Connect | MqMqiCall::ConnectExtended
            ) && status.completion()
                == mainframe_env_host_api::mq_status::MqCompletion::Warning
                && status.reason_symbol() == "MQRC_ALREADY_CONNECTED"
                && !connection.is_historical() =>
            {
                // The provider attests the prior live connection. This machine
                // records only an ABI alias, even on a child's first observation.
                // Preserve the original reviewed warning; never map it to OK.
                let (completion, reason) = status.wire_pair();
                (completion, reason, Some(connection), true)
            }
            MqMqiOutcome::Completed {
                status: MqMqiStatus::OkNone,
                output,
            } => {
                let connection = match (targets.call, output) {
                    (
                        MqMqiCall::Connect | MqMqiCall::ConnectExtended,
                        MqMqiOutput::Connected(connection @ MqHconn::Issued(_)),
                    ) if !connection.is_historical() => Some(connection),
                    (MqMqiCall::Disconnect, MqMqiOutput::NoOutput) => None,
                    (MqMqiCall::Commit | MqMqiCall::Back, MqMqiOutput::UnitOfWork { unit })
                        if targets.unit == Some(unit) =>
                    {
                        None
                    }
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
                // Syncpoint calls have only CompCode/Reason wire outputs. An
                // exact reviewed warning/failure is not a local UOW decision:
                // the real provider/recovery authority retains that ownership.
                // CONNECT/DISC still require usable successful output proof.
                if !matches!(targets.call, MqMqiCall::Commit | MqMqiCall::Back)
                    && status.completion()
                        != mainframe_env_host_api::mq_status::MqCompletion::Failed
                {
                    return Err(MachineProblem::UnexpectedHostResult);
                }
                (completion, reason, None, false)
            }
            MqMqiOutcome::Completed {
                status: MqMqiStatus::FailedEnvironment,
                output: MqMqiOutput::NoOutput,
            } if matches!(targets.call, MqMqiCall::Commit | MqMqiCall::Back) => {
                let status = mainframe_env_host_api::mq_status::MqReviewedStatus::from_symbols(
                    targets.call,
                    "MQCC_FAILED",
                    "MQRC_ENVIRONMENT_ERROR",
                )
                .map_err(|_| MachineProblem::UnexpectedHostResult)?;
                let (completion, reason) = status.wire_pair();
                (completion, reason, None, false)
            }
            MqMqiOutcome::UnknownOutcome | MqMqiOutcome::DuplicatePossible => {
                return Err(MachineProblem::Host(HostProblem::UnknownOutcome));
            }
            _ => return Err(MachineProblem::Host(HostProblem::Unsupported)),
        };
        if targets.scope.is_some() {
            self.finish_scoped_mq(&targets, connection, completion, reason, completed)?;
            if let Some(lease) = &targets.pending_lease {
                lease.known();
            }
            scope_completion.known();
            return Ok(());
        }
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
        // Connection calls use captured compiled views and a bounded atomic
        // batch. DISC/CMIT/BACK retain their established writeback behavior.
        if let Some(capture) = &targets.connx {
            self.write_connx(capture, &targets, wire, completion, reason)?;
        } else if let Some(capture) = &targets.connect {
            self.write_connect(capture, &targets, wire, completion, reason)?;
        } else {
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
        }
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
    fn connection(&self, wire: i32) -> Result<MqHconn, MachineProblem> {
        if let Some(scope) = &self.scope {
            scope.connection(wire).map_err(MachineProblem::Host)
        } else {
            self.connections
                .get(&wire)
                .copied()
                .ok_or(MachineProblem::Host(HostProblem::Malformed))
        }
    }
    fn current(&self, invocation: &Invocation) -> Result<MqMqiProgramProfile, MachineProblem> {
        let lookup = || self.frame.profile(invocation).map_err(MachineProblem::Host);
        let profile = if self.scope.is_some() {
            connx::contained(lookup)?
        } else {
            lookup()?
        };
        profile_valid(profile)?;
        if profile != self.profile {
            return Err(MachineProblem::Host(HostProblem::IdempotencyConflict));
        }
        let scope = connx::contained(|| {
            self.frame
                .abi_scope(invocation)
                .map_err(MachineProblem::Host)
        })?;
        if scope != self.scope {
            return Err(MachineProblem::Host(HostProblem::IdempotencyConflict));
        }
        if let Some(scope) = &scope {
            scope
                .require_context(profile.context)
                .map_err(MachineProblem::Host)?;
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
