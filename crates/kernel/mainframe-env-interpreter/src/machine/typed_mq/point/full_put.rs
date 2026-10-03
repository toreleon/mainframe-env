//! Full compiled PUT/PUT1; encoded provider observations, never context synthesis.
use super::*;
use mainframe_env_host_api::mq_mqi::{MqFullMessage, MqMqiResult};
use mainframe_env_host_api::mq_raw_layout::{
    MqRawObservation, MqRawObservedField, MqRawPlatform, MqRawWritebackContext, mq_raw_layout,
};
use mainframe_env_host_api::mq_wire_options::MqWireFullPut;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Capture {
    request: MqMqiRequest,
    md: MqRawCapture,
    pmo: MqRawCapture,
    descriptor_version: i32,
    max_message_bytes: usize,
    object: Option<MqHobj>,
}

impl ReferenceMachine {
    pub(in crate::machine::typed_mq) fn prepare_full_put(
        &self,
        parameters: &[String],
        one: bool,
    ) -> Result<(MqMqiRequest, Targets), MachineProblem> {
        if parameters.len() != 8 {
            return Err(MachineProblem::InvalidOperation);
        }
        let state = self.mqi.as_ref().ok_or(MachineProblem::UnsupportedForm)?;
        let scope = state
            .scope
            .as_ref()
            .ok_or(MachineProblem::Host(HostProblem::Unsupported))?;
        let call = if one {
            MqMqiCall::PutOne
        } else {
            MqMqiCall::Put
        };
        self.mq_output_aliases(parameters)?;
        let connection_alias = self.point_long(&parameters[0])?;
        let connection = scope
            .connection(connection_alias)
            .map_err(MachineProblem::Host)?;
        // Independent structure ABI is captured before reading any descriptor.
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
        let mut arguments = Vec::new();
        arguments
            .try_reserve_exact(8)
            .map_err(|_| MachineProblem::ResourceExhausted)?;
        // Body capacity has a separate profile bound; never widen the CNO cap.
        for (i, p) in parameters.iter().enumerate() {
            if i == 5 {
                continue;
            }
            let s = self.connx_storage(p)?;
            if [0, 4, 6, 7].contains(&i) || (!one && i == 1) {
                self.point_long(p)?;
                if s.layout.native_binary || s.view.offset % 4 != 0 {
                    return Err(MachineProblem::UnsupportedForm);
                }
            }
            arguments.push(s);
        }
        let (target, mut members, object_alias, object) = if one {
            let (od, members) = self.point_group(&arguments[1], MqRawLayoutKind::Od1, encoding)?;
            let expected = od
                .layout()
                .fields
                .iter()
                .find(|f| f.name == "ObjectType")
                .ok_or(MachineProblem::UnsupportedForm)?;
            if !matches!((od.field("ObjectType"), expected.initial),
                (Ok(MqRawFieldValue::Long(a)), MqRawInitialValue::Long(b)) if a == b)
                || !raw_chars(&od, "ObjectQMgrName")?.iter().all(|b| *b == b' ')
                || !raw_chars(&od, "AlternateUserId")?
                    .iter()
                    .all(|b| *b == b' ')
            {
                return Err(MachineProblem::Host(HostProblem::Unsupported));
            }
            (
                MqMqiNativePointTarget::PutOne {
                    lookup: MqRouteLookup::Queue {
                        name: raw_name(&od, "ObjectName")?,
                        manager: None,
                        dynamic_pattern: None,
                    },
                },
                members,
                None,
                None,
            )
        } else {
            let alias = self.point_long(&parameters[1])?;
            let object = scope
                .object(alias, connection)
                .map_err(MachineProblem::Host)?;
            (
                MqMqiNativePointTarget::Object(object),
                Vec::new(),
                Some(alias),
                Some(object),
            )
        };
        let point = observe(|| structure.point(&target))?;
        let version = observe(|| point.descriptor_version())?;
        let maximum = observe(|| point.max_message_bytes())?;
        if maximum == 0 {
            return Err(MachineProblem::Host(HostProblem::Unsupported));
        }
        let kind = match version {
            1 => MqRawLayoutKind::Md1,
            2 => MqRawLayoutKind::Md2,
            _ => return Err(MachineProblem::Host(HostProblem::Unsupported)),
        };
        let (md, md_members) = self.point_group(&arguments[2], kind, encoding)?;
        let (pmo, pmo_members) =
            self.point_group(&arguments[3], MqRawLayoutKind::Pmo1, encoding)?;
        members.extend(md_members);
        members.extend(pmo_members);
        // Freeze declared suffix views too, not just their containing bytes.
        // No raw suffix supplies semantics; changed member metadata still makes
        // the dispatched application's output storage unusable.
        for group in &arguments[1..4] {
            if group.layout.category == LayoutCategory::Group {
                self.point_suffix_members(group, &mut members)?;
            }
        }
        for raw in [&md, &pmo] {
            for field in raw.layout().fields {
                if let MqRawFieldValue::Long(value) | MqRawFieldValue::Alias(value) = raw
                    .field(field.name)
                    .map_err(|_| MachineProblem::UnsupportedForm)?
                {
                    mainframe_env_host_api::mq_raw_layout::mq_raw_cobol_long(i64::from(value))
                        .map_err(|_| MachineProblem::DataException)?;
                }
            }
        }
        let descriptor = md
            .to_full_md_value()
            .map_err(|_| MachineProblem::UnsupportedForm)?;
        // A zero MsgId requests unsupported generation. CorrelId is supplied
        // input (including binary zero) without NEW_CORREL_ID; the sole options
        // constructor below rejects that generation flag independently.
        if descriptor.fields().msg_id == [0; 24] {
            return Err(MachineProblem::Host(HostProblem::Unsupported));
        }
        let limit = maximum.min(state.profile.limits.message.body_bytes);
        let body = self.fixed_mq_storage(&parameters[5], limit)?;
        if body.layout.category != LayoutCategory::Alphanumeric {
            return Err(MachineProblem::UnsupportedForm);
        }
        let length = usize::try_from(self.point_long(&parameters[4])?)
            .map_err(|_| MachineProblem::DataException)?;
        if length > body.bytes.len() || length > limit {
            return Err(MachineProblem::ResourceExhausted);
        }
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(length)
            .map_err(|_| MachineProblem::ResourceExhausted)?;
        bytes.extend_from_slice(&body.bytes[..length]);
        arguments.insert(5, body);
        let options = match pmo
            .field("Options")
            .map_err(|_| MachineProblem::UnsupportedForm)?
        {
            MqRawFieldValue::Long(value) => value,
            _ => return Err(MachineProblem::UnsupportedForm),
        };
        let put = connx::contained(|| {
            mq_wire_options::put_full_for_target(
                connection,
                object,
                match &target {
                    MqMqiNativePointTarget::PutOne { lookup } => Some(lookup),
                    _ => None,
                },
                MqWireFullPut {
                    message: MqFullMessage {
                        descriptor,
                        body: bytes,
                        properties: Vec::new(),
                    },
                    pmo_version: i64::from(mq_raw_layout(MqRawLayoutKind::Pmo1).version),
                    options: i64::from(options),
                },
                &Bindings(point.as_ref()),
                state.profile.limits.message,
            )
            .map_err(|_| MachineProblem::Host(HostProblem::Unsupported))
        })?;
        let request = match target {
            MqMqiNativePointTarget::Object(object) => MqMqiRequest::FullPut {
                connection,
                object,
                put,
            },
            MqMqiNativePointTarget::PutOne { lookup } => MqMqiRequest::FullPutOne {
                connection,
                lookup,
                alternate_user: None,
                put,
            },
            _ => return Err(MachineProblem::UnsupportedForm),
        };
        let capture = super::Capture {
            structure,
            point,
            encoding,
            connection_alias,
            connection,
            object_alias,
            arguments,
            members,
            put: Some(Capture {
                request: request.clone(),
                md,
                pmo,
                descriptor_version: version,
                max_message_bytes: maximum,
                object,
            }),
            get: None,
        };
        self.recheck_full_put(&capture)?;
        Ok((
            request,
            Targets {
                call,
                connection: parameters[0].clone(),
                completion: parameters[6].clone(),
                reason: parameters[7].clone(),
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

    fn recheck_full_put(&self, capture: &super::Capture) -> Result<(), MachineProblem> {
        let put = capture
            .put
            .as_ref()
            .ok_or(MachineProblem::UnexpectedHostResult)?;
        if observe(|| capture.point.descriptor_version())? != put.descriptor_version
            || observe(|| capture.point.max_message_bytes())? != put.max_message_bytes
        {
            return Err(MachineProblem::Host(HostProblem::UnknownOutcome));
        }
        // Immutable opaque getters precede final live physical/profile callbacks.
        self.recheck_point(capture)
    }

    pub(super) fn finish_full_put(
        &mut self,
        targets: &Targets,
        outcome: MqMqiOutcome,
    ) -> Result<(), MachineProblem> {
        let capture = targets
            .point
            .as_ref()
            .ok_or(MachineProblem::UnexpectedHostResult)?;
        let put = capture
            .put
            .as_ref()
            .ok_or(MachineProblem::UnexpectedHostResult)?;
        let result = MqMqiResult {
            call: targets.call,
            outcome,
        };
        result
            .validate_reviewed_output_for(&put.request)
            .map_err(|_| MachineProblem::UnexpectedHostResult)?;
        let (status, output) = match result.outcome {
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
        // Fixed output scratch, bounded by unchanged structure capture ceiling.
        // No allocation/encoding/profile callback after the final alias lock.
        let mut md = [0; connx::MAX_CNO_CAPACITY];
        let mut pmo = [0; connx::MAX_CNO_CAPACITY];
        let md_size = capture.arguments[2].bytes.len();
        let pmo_size = capture.arguments[3].bytes.len();
        md[..md_size].copy_from_slice(&capture.arguments[2].bytes);
        pmo[..pmo_size].copy_from_slice(&capture.arguments[3].bytes);
        let produced = match output {
            Some(MqMqiOutput::Produced(produced)) => {
                let context = MqRawWritebackContext {
                    call: targets.call,
                    platform: MqRawPlatform::Zos,
                    single_queue: true,
                    dynamic_model_open: false,
                };
                put.md
                    .writeback_put_context_md(context, &produced.descriptor, &mut md[..md_size])
                    .map_err(|_| MachineProblem::UnexpectedHostResult)?;
                put.pmo
                    .writeback(
                        context,
                        &[
                            MqRawObservedField {
                                field: "ResolvedQName",
                                observation: MqRawObservation::Observed(
                                    MqRawFieldValue::Characters(&produced.resolved_queue),
                                ),
                            },
                            MqRawObservedField {
                                field: "ResolvedQMgrName",
                                observation: MqRawObservation::Observed(
                                    MqRawFieldValue::Characters(&produced.resolved_manager),
                                ),
                            },
                        ],
                        &mut pmo[..pmo_size],
                    )
                    .map_err(|_| MachineProblem::UnexpectedHostResult)?;
                true
            }
            None => false,
            _ => return Err(MachineProblem::UnexpectedHostResult),
        };
        let (cc, rc) = status.wire_pair();
        let mut scalars = [[0; 4]; 2];
        for (index, value) in [cc, rc].into_iter().enumerate() {
            mainframe_env_host_api::mq_raw_layout::mq_raw_cobol_long(i64::from(value))
                .map_err(|_| MachineProblem::UnexpectedHostResult)?;
            let s = &capture.arguments[6 + index];
            let bytes = encode_decimal(
                &s.layout,
                Decimal {
                    coefficient: i128::from(value),
                    scale: 0,
                },
            )?;
            scalars[index].copy_from_slice(&bytes);
        }
        let scope = targets
            .scope
            .as_ref()
            .ok_or(MachineProblem::UnexpectedHostResult)?;
        let plan = scope
            .use_plan(
                capture.connection_alias,
                capture.connection,
                capture.object_alias.zip(put.object),
            )
            .map_err(MachineProblem::Host)?;
        self.recheck_full_put(capture)?;
        // Live cancellation is an existing atomic probe, not another host
        // callback. A request observed after the final physical check still
        // forbids application writeback; the completion guard fences the root.
        if self.invocation.cancellation_requested() {
            return Err(MachineProblem::Host(HostProblem::UnknownOutcome));
        }
        let _guard = plan.guard(scope).map_err(MachineProblem::Host)?;
        if produced {
            for (s, bytes) in [
                (&capture.arguments[2], &md[..md_size]),
                (&capture.arguments[3], &pmo[..pmo_size]),
            ] {
                self.bases[s.view.base][s.view.offset..s.view.offset + s.view.length]
                    .copy_from_slice(bytes);
            }
        }
        for (s, bytes) in capture.arguments[6..8].iter().zip(scalars) {
            self.bases[s.view.base][s.view.offset..s.view.offset + s.view.length]
                .copy_from_slice(&bytes);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
