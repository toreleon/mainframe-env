//! Compiled finite OPEN/CLOSE; registry, topology and SAF remain host-owned.
use super::*;
use mainframe_env_host_api::MqHobj;
use mainframe_env_host_api::mq_object_route::{
    MqRouteCloseLifecycle, MqRouteCloseTarget, MqRouteLookup, MqRouteOpenAccess,
};
use mainframe_env_host_api::mq_raw_layout::{
    MqRawCapture, MqRawCharacterEncoding, MqRawFieldValue, MqRawInitialValue, MqRawLayoutKind,
    MqRawNumberEncoding, MqRawStructureEncoding,
};
use mainframe_env_host_api::mq_status::{MqCompletion, MqReviewedStatus};
use mainframe_env_host_api::mq_wire_options::{self, MqWireBindings, MqWireQueueManagerPlatform};

mod full_get;
mod full_put;
mod layout;

/// Exact decoded native target, not a topology assertion or registry permit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MqMqiNativePointTarget {
    /// The selected facet must independently establish a predefined normal local
    /// queue with this exact lookup and explicit OUTPUT or INPUT_SHARED access.
    Open {
        /// Decoded local queue identity; the actual catalog still owns resolution.
        lookup: MqRouteLookup,
        /// Explicit finite access intent; it does not grant permission to open.
        access: MqRouteOpenAccess,
    },
    /// A live ABI-resolved object, never a numeric handle reconstruction.
    Object(MqHobj),
    /// Exact predefined local PUT1 lookup. The provider still resolves and
    /// checks the original call/connection/current unit and actual catalog.
    PutOne {
        /// Exact application lookup; the selected catalog owns resolution and
        /// independently proves predefined normal-local applicability.
        lookup: MqRouteLookup,
    },
}

/// Privileged wrapper of the actual opaque same-service structure observation.
/// Retain its original root/store/frame/call/connection/current-unit identity.
/// Methods are bounded read-only observations, not dispatch or SAF callbacks;
/// do not hold a registry borrow across them or enter the selected service again.
pub trait MqMqiNativeStructure: Send + Sync {
    /// Configured structure ABI; body Encoding/CCSID never supplies this value.
    fn encoding(&self) -> MqRawStructureEncoding;
    /// Compare the retained observation with the actual live selected frame.
    fn recheck(&self) -> Result<(), HostProblem>;
    /// Bind the exact decoded target through the same opaque provider facet.
    /// Never accept equal foreign rows or caller-derived defaults as authority.
    fn point(
        &self,
        target: &MqMqiNativePointTarget,
    ) -> Result<Arc<dyn MqMqiNativePoint>, HostProblem>;
}

/// Exact tuple-bound wrapper of the selected point observation. Existing wire
/// queries must delegate to that SAME opaque observation, not copied booleans.
/// CLOSE rechecks its captured predefined queue after known object retirement;
/// it must not require/revive the closed object or resolve uncertainty as success.
pub trait MqMqiNativePoint: MqWireBindings + Send + Sync {
    /// Recheck original frame, connection, unit, stable route/profile and the
    /// final physical snapshot after any source/clock callback. No permission,
    /// allocation, queue transition, UOW decision or cleanup follows from this.
    fn recheck(&self) -> Result<(), HostProblem>;
    /// Delegate to the SAME opaque point's homogeneous descriptor version.
    /// Never infer it from caller MQMD or copy foreign configuration as proof.
    /// Existing embeddings refuse full PUT/GET until deliberately forwarding it.
    fn descriptor_version(&self) -> Result<i32, HostProblem> {
        Err(HostProblem::Unsupported)
    }
    /// Delegate to the lesser actual queue/QM body maxima of that SAME point.
    /// Product and per-call quotas are independent. This is not an allocation
    /// or permission grant, and existing embeddings remain fail-closed.
    fn max_message_bytes(&self) -> Result<usize, HostProblem> {
        Err(HostProblem::Unsupported)
    }
}

// The sole wire API requires a sized receiver. Forward every query, rather than
// copying opaque source facts into application-controlled fields.
struct Bindings<'a>(&'a dyn MqMqiNativePoint);
impl MqWireBindings for Bindings<'_> {
    fn queue_defaults_are_represented(
        &self,
        c: MqHconn,
        o: Option<MqHobj>,
        q: Option<&MqRouteLookup>,
    ) -> bool {
        self.0.queue_defaults_are_represented(c, o, q)
    }
    fn queue_manager_platform(&self) -> MqWireQueueManagerPlatform {
        self.0.queue_manager_platform()
    }
    fn admitted_unit(&self, c: MqHconn) -> Option<MqMqiUnitOfWork> {
        self.0.admitted_unit(c)
    }
    fn existing_cursor(&self, c: MqHconn, o: MqHobj) -> Option<u64> {
        self.0.existing_cursor(c, o)
    }
    fn milliseconds_to_ticks(&self, ms: u32) -> Option<u64> {
        self.0.milliseconds_to_ticks(ms)
    }
}

#[derive(Clone)]
pub(super) struct Capture {
    structure: Arc<dyn MqMqiNativeStructure>,
    point: Arc<dyn MqMqiNativePoint>,
    encoding: MqRawStructureEncoding,
    connection_alias: i32,
    connection: MqHconn,
    object_alias: Option<i32>,
    pub(super) arguments: Vec<connx::Storage>,
    members: Vec<connx::Storage>,
    put: Option<full_put::Capture>,
    get: Option<full_get::Capture>,
}
impl std::fmt::Debug for Capture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("NativePointCapture { volatile: true }")
    }
}
impl PartialEq for Capture {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.structure, &other.structure)
            && Arc::ptr_eq(&self.point, &other.point)
            && self.encoding == other.encoding
            && self.connection_alias == other.connection_alias
            && self.connection == other.connection
            && self.object_alias == other.object_alias
            && self.arguments == other.arguments
            && self.members == other.members
            && self.put == other.put
            && self.get == other.get
    }
}
impl Eq for Capture {}

fn observe<T>(f: impl FnOnce() -> Result<T, HostProblem>) -> Result<T, MachineProblem> {
    connx::contained(|| f().map_err(MachineProblem::Host))
}

impl ReferenceMachine {
    pub(super) fn prepare_point(
        &self,
        parameters: &[String],
        open: bool,
    ) -> Result<(MqMqiRequest, Targets), MachineProblem> {
        if parameters.len() != if open { 6 } else { 5 } {
            return Err(MachineProblem::InvalidOperation);
        }
        let state = self.mqi.as_ref().ok_or(MachineProblem::UnsupportedForm)?;
        let scope = state
            .scope
            .as_ref()
            .ok_or(MachineProblem::Host(HostProblem::Unsupported))?;
        let call = if open {
            MqMqiCall::Open
        } else {
            MqMqiCall::Close
        };
        self.mq_output_aliases(parameters)?;
        for (i, p) in parameters.iter().enumerate() {
            if !open || i != 1 {
                self.mq_long_target(p)?;
            }
        }
        let connection_alias = self.point_long(&parameters[0])?;
        let connection = scope
            .connection(connection_alias)
            .map_err(MachineProblem::Host)?;
        // This lookup precedes descriptor capture and decoding. Neither raw
        // byte contents nor options select the actual structure character ABI.
        let structure = observe(|| {
            state
                .frame
                .native_structure(&self.invocation, call, connection)
        })?;
        let encoding = connx::contained(|| Ok(structure.encoding()))?;
        if encoding.numbers != MqRawNumberEncoding::NormalBigEndian
            || encoding.characters != MqRawCharacterEncoding::AsciiCompatible
        {
            return Err(MachineProblem::Host(HostProblem::Unsupported));
        }
        observe(|| structure.recheck())?;
        connx::contained(|| state.current(&self.invocation))?;
        let arguments = parameters
            .iter()
            .map(|p| self.connx_storage(p))
            .collect::<Result<Vec<_>, _>>()?;
        if arguments
            .iter()
            .enumerate()
            .any(|(i, s)| (!open || i != 1) && (s.layout.native_binary || s.view.offset % 4 != 0))
        {
            return Err(MachineProblem::UnsupportedForm);
        }
        let (target, members) = if open {
            let (raw, members) = self.point_group(&arguments[1], MqRawLayoutKind::Od1, encoding)?;
            let object_type = raw
                .field("ObjectType")
                .map_err(|_| MachineProblem::UnsupportedForm)?;
            let expected = raw
                .layout()
                .fields
                .iter()
                .find(|f| f.name == "ObjectType")
                .ok_or(MachineProblem::UnsupportedForm)?;
            if !matches!((object_type, expected.initial), (MqRawFieldValue::Long(a), MqRawInitialValue::Long(b)) if a == b)
            {
                return Err(MachineProblem::Host(HostProblem::Unsupported));
            }
            let name = raw_name(&raw, "ObjectName")?;
            if !raw_chars(&raw, "ObjectQMgrName")?
                .iter()
                .all(|b| *b == b' ')
                || !raw_chars(&raw, "AlternateUserId")?
                    .iter()
                    .all(|b| *b == b' ')
            {
                return Err(MachineProblem::Host(HostProblem::Unsupported));
            }
            // DynamicQName is source-ignored for the independently verified
            // nonmodel target. Its complete bytes and suffix remain captured.
            let lookup = MqRouteLookup::Queue {
                name,
                manager: None,
                dynamic_pattern: None,
            };
            let value = self.point_long(&parameters[2])?;
            let access = if value == numeric("MQOO_OUTPUT")? {
                MqRouteOpenAccess::Output
            } else if value == numeric("MQOO_INPUT_SHARED")? {
                MqRouteOpenAccess::InputShared
            } else {
                return Err(MachineProblem::Host(HostProblem::Unsupported));
            };
            (MqMqiNativePointTarget::Open { lookup, access }, members)
        } else {
            let alias = self.point_long(&parameters[1])?;
            (
                MqMqiNativePointTarget::Object(
                    scope
                        .object(alias, connection)
                        .map_err(MachineProblem::Host)?,
                ),
                Vec::new(),
            )
        };
        let point = observe(|| structure.point(&target))?;
        let request = connx::contained(|| match &target {
            MqMqiNativePointTarget::Open { lookup, .. } => mq_wire_options::open(
                connection,
                lookup.clone(),
                i64::from(
                    mainframe_env_host_api::mq_raw_layout::mq_raw_layout(MqRawLayoutKind::Od1)
                        .version,
                ),
                i64::from(self.point_long(&parameters[2])?),
                &Bindings(point.as_ref()),
            )
            .map(MqMqiRequest::Open)
            .map_err(|_| MachineProblem::Host(HostProblem::Unsupported)),
            MqMqiNativePointTarget::Object(object) => {
                let options = self.point_long(&parameters[2])?;
                if options != numeric("MQCO_NONE")? {
                    return Err(MachineProblem::Host(HostProblem::Unsupported));
                }
                mq_wire_options::close(
                    connection,
                    MqRouteCloseTarget::Object {
                        handle: *object,
                        lifecycle: MqRouteCloseLifecycle::Predefined,
                    },
                    i64::from(options),
                    &Bindings(point.as_ref()),
                )
                .map(MqMqiRequest::Close)
                .map_err(|_| MachineProblem::Host(HostProblem::Unsupported))
            }
            MqMqiNativePointTarget::PutOne { .. } => Err(MachineProblem::UnsupportedForm),
        })?;
        let capture = Capture {
            structure,
            point,
            encoding,
            connection_alias,
            connection,
            object_alias: if open {
                None
            } else {
                Some(self.point_long(&parameters[1])?)
            },
            arguments,
            members,
            put: None,
            get: None,
        };
        self.recheck_point(&capture)?;
        Ok((
            request,
            Targets {
                call,
                connection: parameters[if open { 3 } else { 1 }].clone(),
                completion: parameters[parameters.len() - 2].clone(),
                reason: parameters[parameters.len() - 1].clone(),
                disconnected: None,
                unit: None,
                connx: None,
                connect: None,
                scope: None,
                reservation: None,
                scoped_arguments: Vec::new(),
                pending_lease: None,
                point: Some(capture),
            },
        ))
    }

    fn point_long(&self, name: &str) -> Result<i32, MachineProblem> {
        self.mq_long_target(name)?;
        let value = self.decimal(name)?;
        let value = i64::try_from(value.coefficient).map_err(|_| MachineProblem::DataException)?;
        mainframe_env_host_api::mq_raw_layout::mq_raw_cobol_long(value)
            .map_err(|_| MachineProblem::DataException)
    }

    fn recheck_point(&self, capture: &Capture) -> Result<(), MachineProblem> {
        observe(|| capture.structure.recheck())?;
        observe(|| capture.point.recheck())?;
        connx::contained(|| {
            self.mqi
                .as_ref()
                .ok_or(MachineProblem::UnsupportedForm)?
                .current(&self.invocation)
        })?;
        if connx::contained(|| Ok(capture.structure.encoding()))? != capture.encoding {
            return Err(MachineProblem::Host(HostProblem::UnknownOutcome));
        }
        observe(|| capture.structure.recheck())?;
        observe(|| capture.point.recheck())?;
        self.check_point_storage(capture)
    }

    fn check_point_storage(&self, capture: &Capture) -> Result<(), MachineProblem> {
        for s in capture.arguments.iter().chain(&capture.members) {
            if self.layout(&s.layout.name) != Some(&s.layout)
                || self.views.get(&s.layout.name) != Some(&s.view)
                || self.bases.get(s.view.base).and_then(|b| {
                    s.view
                        .offset
                        .checked_add(s.view.length)
                        .and_then(|end| b.get(s.view.offset..end))
                }) != Some(s.bytes.as_slice())
            {
                return Err(MachineProblem::Host(HostProblem::UnknownOutcome));
            }
        }
        Ok(())
    }

    pub(super) fn finish_point_mq(
        &mut self,
        targets: &Targets,
        outcome: MqMqiOutcome,
    ) -> Result<(), MachineProblem> {
        let capture = targets
            .point
            .as_ref()
            .ok_or(MachineProblem::UnexpectedHostResult)?;
        if capture.put.is_some() {
            return self.finish_full_put(targets, outcome);
        }
        if capture.get.is_some() {
            return self.finish_full_get(targets, outcome);
        }
        let (status, output) = match outcome {
            MqMqiOutcome::Completed {
                status: MqMqiStatus::OkNone,
                output,
            } => (
                MqReviewedStatus::from_symbols(targets.call, "MQCC_OK", "MQRC_NONE")
                    .map_err(|_| MachineProblem::UnexpectedHostResult)?,
                Some(output),
            ),
            MqMqiOutcome::ReviewedOutput { status, output }
                if status.completion() == MqCompletion::Ok
                    && status.reason_symbol() == "MQRC_NONE" =>
            {
                (status, Some(output))
            }
            MqMqiOutcome::ReviewedStatus { status }
                if status.completion() == MqCompletion::Failed =>
            {
                (status, None)
            }
            MqMqiOutcome::UnknownOutcome | MqMqiOutcome::DuplicatePossible => {
                return Err(MachineProblem::Host(HostProblem::UnknownOutcome));
            }
            _ => return Err(MachineProblem::UnexpectedHostResult),
        };
        let (object, closed) = match (targets.call, output) {
            (
                MqMqiCall::Open,
                Some(MqMqiOutput::Opened {
                    object,
                    dynamic: None,
                }),
            ) if !object.is_historical() => (Some(object), false),
            (MqMqiCall::Close, Some(MqMqiOutput::NoOutput)) => (None, true),
            (MqMqiCall::Open | MqMqiCall::Close, None) => (None, false),
            _ => return Err(MachineProblem::UnexpectedHostResult),
        };
        let scope = targets
            .scope
            .as_ref()
            .ok_or(MachineProblem::UnexpectedHostResult)?;
        let plan = scope
            .object_plan(
                capture.connection_alias,
                capture.connection,
                targets.reservation.as_ref(),
                object,
                capture.object_alias.filter(|_| closed),
            )
            .map_err(MachineProblem::Host)?;
        let (completion, reason) = status.wire_pair();
        let mut writes = Vec::new();
        writes
            .try_reserve_exact(3)
            .map_err(|_| MachineProblem::ResourceExhausted)?;
        for (target, value) in [
            (&targets.connection, plan.wire()),
            (&targets.completion, Some(completion)),
            (&targets.reason, Some(reason)),
        ] {
            let Some(value) = value else { continue };
            mainframe_env_host_api::mq_raw_layout::mq_raw_cobol_long(i64::from(value))
                .map_err(|_| MachineProblem::UnsupportedForm)?;
            self.mq_long_target(target)?;
            let storage = self.connx_storage(target)?;
            let bytes = encode_decimal(
                &storage.layout,
                Decimal {
                    coefficient: i128::from(value),
                    scale: 0,
                },
            )?;
            if bytes.len() != storage.view.length {
                return Err(MachineProblem::UnsupportedForm);
            }
            writes.push((storage.view, bytes));
        }
        self.recheck_point(capture)?;
        // Last physical/profile callback is complete. Only pure captured storage
        // comparison and the held alias guard precede infallible byte/slot copy.
        let mut guard = plan.guard(scope).map_err(MachineProblem::Host)?;
        for (view, bytes) in writes {
            self.bases[view.base][view.offset..view.offset + view.length].copy_from_slice(&bytes);
        }
        plan.commit(&mut guard);
        Ok(())
    }
}
fn numeric(symbol: &str) -> Result<i32, MachineProblem> {
    mq_wire_options::numeric_identities()
        .iter()
        .find(|(s, _)| *s == symbol)
        .map(|(_, v)| *v)
        .ok_or(MachineProblem::UnsupportedForm)
}
fn raw_chars<'a>(raw: &'a MqRawCapture, field: &str) -> Result<&'a [u8], MachineProblem> {
    match raw
        .field(field)
        .map_err(|_| MachineProblem::UnsupportedForm)?
    {
        MqRawFieldValue::Characters(value) => Ok(value),
        _ => Err(MachineProblem::UnsupportedForm),
    }
}
fn raw_name(raw: &MqRawCapture, field: &str) -> Result<MqRouteName, MachineProblem> {
    let bytes = raw_chars(raw, field)?;
    // Explicit fixed MQCHAR48 profile: blank padding, no guessed NUL alias.
    let name = std::str::from_utf8(bytes)
        .map_err(|_| MachineProblem::DataException)?
        .trim_end_matches(' ');
    MqRouteName::new(name).map_err(|_| MachineProblem::DataException)
}

#[cfg(test)]
mod tests;
