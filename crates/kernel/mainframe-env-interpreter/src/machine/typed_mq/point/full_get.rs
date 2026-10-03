//! Nine-reference finite GET; complete observed values, no queue/permission engine.
use super::*;
use mainframe_env_host_api::mq_md_value::MqMdValue;
use mainframe_env_host_api::mq_mqi::{MqMqiRequestEnvelope, MqMqiResult};
use mainframe_env_host_api::mq_raw_layout::{MqRawPlatform, MqRawWritebackContext, mq_raw_layout};
use mainframe_env_host_api::mq_wire_options::MqWireFullGet;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Capture {
    envelope: MqMqiRequestEnvelope,
    md: MqRawCapture,
    gmo: MqRawCapture,
    descriptor_version: i32,
    max_message_bytes: usize,
    object: MqHobj,
}

// Deliberate first unformatted, no-selector profile. The actual selected owner
// still admits handles/access/unit/SAF and checks the queued payload. This does
// not turn diagnostic MD observations into general call or conversion legality.
fn finite_descriptor(value: &MqMdValue, input: bool) -> Result<(), MachineProblem> {
    let f = value.fields();
    if f.format != [b' '; 8]
        || (input && (f.msg_id != [0; 24] || f.correl_id != [0; 24]))
        || (!input && (f.coded_char_set_id <= 0 || !(0..=255).contains(&f.backout_count)))
    {
        return Err(MachineProblem::Host(HostProblem::Unsupported));
    }
    if let MqMdValue::V2 { extension, .. } = value {
        let initial = |name: &str, value| {
            mq_raw_layout(MqRawLayoutKind::Md2)
                .fields
                .iter()
                .any(|f| f.name == name && f.initial == MqRawInitialValue::Long(value))
        };
        if extension.group_id != [0; 24]
            || !initial("MsgFlags", extension.msg_flags)
            || !initial("MsgSeqNumber", extension.msg_seq_number)
            || !initial("Offset", extension.offset)
            || !initial("OriginalLength", extension.original_length)
        {
            return Err(MachineProblem::Host(HostProblem::Unsupported));
        }
    }
    Ok(())
}
fn pic_fields(raw: &MqRawCapture) -> Result<(), MachineProblem> {
    for f in raw.layout().fields {
        if let MqRawFieldValue::Long(value) | MqRawFieldValue::Alias(value) = raw
            .field(f.name)
            .map_err(|_| MachineProblem::UnsupportedForm)?
        {
            mainframe_env_host_api::mq_raw_layout::mq_raw_cobol_long(i64::from(value))
                .map_err(|_| MachineProblem::DataException)?;
        }
    }
    Ok(())
}

impl ReferenceMachine {
    pub(in crate::machine::typed_mq) fn prepare_full_get(
        &self,
        parameters: &[String],
    ) -> Result<(MqMqiRequest, Targets), MachineProblem> {
        if parameters.len() != 9 {
            return Err(MachineProblem::InvalidOperation);
        }
        let state = self.mqi.as_ref().ok_or(MachineProblem::UnsupportedForm)?;
        let scope = state
            .scope
            .as_ref()
            .ok_or(MachineProblem::Host(HostProblem::Unsupported))?;
        self.mq_output_aliases(parameters)?;
        let connection_alias = self.point_long(&parameters[0])?;
        let connection = scope
            .connection(connection_alias)
            .map_err(MachineProblem::Host)?;
        // Structure ABI must be selected by the genuine frame BEFORE raw MD/GMO.
        let structure = observe(|| {
            state
                .frame
                .native_structure(&self.invocation, MqMqiCall::Get, connection)
        })?;
        let encoding = connx::contained(|| Ok(structure.encoding()))?;
        if encoding.numbers != MqRawNumberEncoding::NormalBigEndian
            || encoding.characters != MqRawCharacterEncoding::AsciiCompatible
        {
            return Err(MachineProblem::Host(HostProblem::Unsupported));
        }
        observe(|| structure.recheck())?;
        connx::contained(|| state.current(&self.invocation))?;
        let object_alias = self.point_long(&parameters[1])?;
        let object = scope
            .object(object_alias, connection)
            .map_err(MachineProblem::Host)?;
        let point = observe(|| structure.point(&MqMqiNativePointTarget::Object(object)))?;
        let version = observe(|| point.descriptor_version())?;
        let maximum = observe(|| point.max_message_bytes())?;
        let kind = match version {
            1 => MqRawLayoutKind::Md1,
            2 => MqRawLayoutKind::Md2,
            _ => return Err(MachineProblem::Host(HostProblem::Unsupported)),
        };
        if maximum == 0 {
            return Err(MachineProblem::Host(HostProblem::Unsupported));
        }
        let mut arguments = Vec::new();
        arguments
            .try_reserve_exact(9)
            .map_err(|_| MachineProblem::ResourceExhausted)?;
        for (i, p) in parameters.iter().enumerate() {
            if i == 5 {
                continue;
            }
            let s = self.connx_storage(p)?;
            if [0, 1, 4, 6, 7, 8].contains(&i) {
                self.point_long(p)?;
                if s.layout.native_binary || s.view.offset % 4 != 0 {
                    return Err(MachineProblem::UnsupportedForm);
                }
            }
            arguments.push(s);
        }
        let (md, mut members) = self.point_group(&arguments[2], kind, encoding)?;
        let (gmo, gmo_members) =
            self.point_group(&arguments[3], MqRawLayoutKind::Gmo1, encoding)?;
        members.extend(gmo_members);
        for group in &arguments[2..4] {
            self.point_suffix_members(group, &mut members)?;
        }
        pic_fields(&md)?;
        pic_fields(&gmo)?;
        let descriptor = md
            .to_full_md_value()
            .map_err(|_| MachineProblem::UnsupportedForm)?;
        finite_descriptor(&descriptor, true)?;
        let limit = maximum.min(state.profile.limits.message.body_bytes);
        let body = self.fixed_mq_storage(&parameters[5], limit)?;
        if body.layout.category != LayoutCategory::Alphanumeric {
            return Err(MachineProblem::UnsupportedForm);
        }
        let length = self.point_long(&parameters[4])?;
        if length < 0 {
            return Err(MachineProblem::DataException);
        }
        if length as usize > body.bytes.len() || length as usize > limit {
            return Err(MachineProblem::ResourceExhausted);
        }
        arguments.insert(5, body);
        let long = |name| match gmo.field(name) {
            Ok(MqRawFieldValue::Long(value)) => Ok(i64::from(value)),
            _ => Err(MachineProblem::UnsupportedForm),
        };
        let get = connx::contained(|| {
            mq_wire_options::get_full(
                MqWireFullGet {
                    connection,
                    object,
                    descriptor,
                    gmo_version: i64::from(gmo.layout().version),
                    options: long("Options")?,
                    wait_milliseconds: long("WaitInterval")?,
                    buffer_capacity: i64::from(length),
                },
                &Bindings(point.as_ref()),
                state.profile.limits.message,
            )
            .map_err(|_| MachineProblem::Host(HostProblem::Unsupported))
        })?;
        let request = MqMqiRequest::QualifiedFullGet(get);
        let envelope = MqMqiRequestEnvelope {
            context: state.profile.context,
            limits: state.profile.limits,
            request: request.clone(),
        };
        envelope
            .validate()
            .map_err(|_| MachineProblem::UnsupportedForm)?;
        let capture = super::Capture {
            structure,
            point,
            encoding,
            connection_alias,
            connection,
            object_alias: Some(object_alias),
            arguments,
            members,
            put: None,
            get: Some(Capture {
                envelope,
                md,
                gmo,
                descriptor_version: version,
                max_message_bytes: maximum,
                object,
            }),
        };
        self.recheck_full_get(&capture)?;
        Ok((
            request,
            Targets {
                call: MqMqiCall::Get,
                connection: parameters[0].clone(),
                completion: parameters[7].clone(),
                reason: parameters[8].clone(),
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

    fn recheck_full_get(&self, capture: &super::Capture) -> Result<(), MachineProblem> {
        let get = capture
            .get
            .as_ref()
            .ok_or(MachineProblem::UnexpectedHostResult)?;
        if observe(|| capture.point.descriptor_version())? != get.descriptor_version
            || observe(|| capture.point.max_message_bytes())? != get.max_message_bytes
        {
            return Err(MachineProblem::Host(HostProblem::UnknownOutcome));
        }
        // Every getter/callback is BEFORE the last physical/profile observations.
        self.recheck_point(capture)
    }

    pub(super) fn finish_full_get(
        &mut self,
        targets: &Targets,
        outcome: MqMqiOutcome,
    ) -> Result<(), MachineProblem> {
        let capture = targets
            .point
            .as_ref()
            .ok_or(MachineProblem::UnexpectedHostResult)?;
        let get = capture
            .get
            .as_ref()
            .ok_or(MachineProblem::UnexpectedHostResult)?;
        let result = MqMqiResult {
            call: targets.call,
            outcome,
        };
        result
            .validate(get.envelope.limits)
            .and_then(|()| result.validate_reviewed_output_for(&get.envelope.request))
            .map_err(|_| MachineProblem::UnexpectedHostResult)?;
        let (status, value) = match &result.outcome {
            MqMqiOutcome::ReviewedOutput {
                status,
                output: MqMqiOutput::QualifiedFullGot(value),
            } => (*status, value),
            MqMqiOutcome::Completed {
                status: MqMqiStatus::OkNone,
                output: MqMqiOutput::QualifiedFullGot(value),
            } => (
                MqReviewedStatus::from_symbols(MqMqiCall::Get, "MQCC_OK", "MQRC_NONE")
                    .map_err(|_| MachineProblem::UnexpectedHostResult)?,
                value,
            ),
            MqMqiOutcome::UnknownOutcome | MqMqiOutcome::DuplicatePossible => {
                return Err(MachineProblem::Host(HostProblem::UnknownOutcome));
            }
            _ => return Err(MachineProblem::UnexpectedHostResult),
        };
        if value.cursor.is_some()
            || value
                .data_length
                .is_some_and(|n| n < 0 || n as usize > get.max_message_bytes)
        {
            return Err(MachineProblem::UnexpectedHostResult);
        }
        let mut md = [0; connx::MAX_CNO_CAPACITY];
        let mut gmo = [0; connx::MAX_CNO_CAPACITY];
        let md_size = capture.arguments[2].bytes.len();
        let gmo_size = capture.arguments[3].bytes.len();
        md[..md_size].copy_from_slice(&capture.arguments[2].bytes);
        gmo[..gmo_size].copy_from_slice(&capture.arguments[3].bytes);
        let context = MqRawWritebackContext {
            call: MqMqiCall::Get,
            platform: MqRawPlatform::Zos,
            single_queue: true,
            dynamic_model_open: false,
        };
        get.gmo
            .stage_qualified_full_get_gmo(
                context,
                &get.envelope,
                &result,
                &capture.arguments[3].bytes,
                &mut gmo[..gmo_size],
            )
            .map_err(|_| MachineProblem::UnexpectedHostResult)?;
        if let Some(message) = &value.message {
            finite_descriptor(&message.descriptor, false)?;
            if !message.properties.is_empty() {
                return Err(MachineProblem::UnexpectedHostResult);
            }
            get.md
                .writeback_full_get_md(context, &message.descriptor, &mut md[..md_size])
                .map_err(|_| MachineProblem::UnexpectedHostResult)?;
            // Writer represents signed32; this actual compiled PIC S9(9) ABI
            // additionally refuses returned numeric observations outside PIC range.
            let raw = MqRawCapture::capture(get.md.layout().kind, &md[..md_size], capture.encoding)
                .map_err(|_| MachineProblem::UnexpectedHostResult)?;
            pic_fields(&raw)?;
        }
        let (cc, rc) = status.wire_pair();
        let mut scalars = [[0; 4]; 3];
        scalars[0].copy_from_slice(&capture.arguments[6].bytes);
        for (i, value) in [value.data_length, Some(cc), Some(rc)]
            .into_iter()
            .enumerate()
        {
            let Some(value) = value else {
                continue;
            };
            mainframe_env_host_api::mq_raw_layout::mq_raw_cobol_long(i64::from(value))
                .map_err(|_| MachineProblem::UnexpectedHostResult)?;
            let bytes = encode_decimal(
                &capture.arguments[6 + i].layout,
                Decimal {
                    coefficient: i128::from(value),
                    scale: 0,
                },
            )?;
            scalars[i].copy_from_slice(&bytes);
        }
        let scope = targets
            .scope
            .as_ref()
            .ok_or(MachineProblem::UnexpectedHostResult)?;
        let plan = scope
            .use_plan(
                capture.connection_alias,
                capture.connection,
                capture.object_alias.map(|a| (a, get.object)),
            )
            .map_err(MachineProblem::Host)?;
        self.recheck_full_get(capture)?;
        if self.invocation.cancellation_requested() {
            return Err(MachineProblem::Host(HostProblem::UnknownOutcome));
        }
        let _guard = plan.guard(scope).map_err(MachineProblem::Host)?;
        // Every slice/range/length, scalar encoding and fallible writer has been
        // checked. No callback, allocation, lookup or alias mutation follows.
        for (s, bytes) in [
            (&capture.arguments[2], &md[..md_size]),
            (&capture.arguments[3], &gmo[..gmo_size]),
        ] {
            self.bases[s.view.base][s.view.offset..s.view.offset + s.view.length]
                .copy_from_slice(bytes);
        }
        if let Some(message) = &value.message {
            let s = &capture.arguments[5];
            self.bases[s.view.base][s.view.offset..s.view.offset + message.body.len()]
                .copy_from_slice(&message.body);
        }
        for (s, bytes) in capture.arguments[6..9].iter().zip(scalars) {
            self.bases[s.view.base][s.view.offset..s.view.offset + s.view.length]
                .copy_from_slice(&bytes);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
