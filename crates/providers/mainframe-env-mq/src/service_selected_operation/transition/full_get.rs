//! Finite original selected MQGET. No numeric GMO, PUT policy or new authority.
//! MQ9.4 original row0015 q101830_; supplemental q096715_/q097395_/q097390_.
use super::super::producer;
use super::*;
use crate::delivery::full_message::QueueProfile;
use mainframe_env_host_api::mq_md_value::{MqMdCharacterEncoding, MqMdValue};
use mainframe_env_host_api::mq_raw_layout::{MqRawInitialValue, MqRawLayoutKind, mq_raw_layout};
use mainframe_env_host_api::{MqGetContract, MqMessageIdentifiers, MqMessageMatch, MqWait};

fn descriptor_profile(descriptor: &MqMdValue) -> Result<(), HostProblem> {
    // First profile is opaque, unformatted body only. No MQMDE/RFH2/header
    // conversion or property-mode inference. Fixed bytes are never decoded as
    // UTF-8 or by BODY Encoding/CCSID. This is a restriction, not normalization.
    let blank = match descriptor.characters() {
        MqMdCharacterEncoding::AsciiCompatible => b' ',
        MqMdCharacterEncoding::OwnedCp037 => 0x40,
    };
    if descriptor.fields().format != [blank; 8] {
        return Err(HostProblem::Unsupported);
    }
    if let MqMdValue::V2 { extension, .. } = descriptor {
        // q097395_: MQGI_NONE is binary zero and MQMF_NONE=0; ungrouped
        // messages have sequence1/offset0. OriginalLength uses the existing
        // reviewed generated raw declaration, never a guessed scalar sentinel.
        let initial = |name: &str, value: i32| {
            mq_raw_layout(MqRawLayoutKind::Md2)
                .fields
                .iter()
                .any(|field| field.name == name && field.initial == MqRawInitialValue::Long(value))
        };
        if extension.group_id != [0; 24]
            || extension.msg_flags != 0
            || !initial("MsgSeqNumber", extension.msg_seq_number)
            || !initial("Offset", extension.offset)
            || !initial("OriginalLength", extension.original_length)
        {
            return Err(HostProblem::Unsupported);
        }
    }
    Ok(())
}

fn controls(get: &MqMqiFullGet) -> Result<MqGetContract, HostProblem> {
    if get.options != MqMqiOptions::ContractDefault
        || get.wait != MqWait::NoWait
        || get.mode != MqGetMode::Remove
        || get.message_handle.is_some()
        || matches!(get.unit, MqMqiUnitOfWork::ExternalPending { .. })
        || !matches!(get.connection, MqHconn::Issued(_))
    {
        return Err(HostProblem::Unsupported);
    }
    descriptor_profile(&get.descriptor)?;
    let fields = get.descriptor.fields();
    // q097395_ 1389–1482 and q096715_ 1269–1381: default matching uses
    // BOTH MsgId and CorrelId; binary-zero MQMI_NONE/MQCI_NONE are wildcards.
    // Group/sequence/offset matching and queue index policy are not inferred.
    Ok(MqGetContract {
        selection: MqMessageMatch {
            identifiers: MqMessageIdentifiers {
                message_id: (fields.msg_id != [0; 24]).then(|| fields.msg_id.to_vec()),
                correlation_id: (fields.correl_id != [0; 24]).then(|| fields.correl_id.to_vec()),
                group_id: None,
            },
        },
        mode: get.mode,
        wait: get.wait,
        truncation: get.truncation,
        buffer_capacity: get.buffer_capacity,
    })
}

pub(super) fn prepare(
    state: &rich_state::RichStoredState,
    runtime: &mut SelectedRuntime,
    invocation: &Invocation,
    logical: &LogicalBatchOwner,
    owner: MqHandleOwner,
    get: &MqMqiFullGet,
    now: u64,
    authorizer: &dyn EnterpriseAuthorizer,
    next: &mut Candidate,
    qualified: bool,
    service: &MqService,
    frame: FrameLease,
    admitted: &crate::mqi_admission::MqMqiAdmitted<'_>,
) -> Result<(), HostProblem> {
    let request = controls(get)?;
    let binding = require_object(
        runtime,
        owner,
        get.connection,
        get.object,
        MqRouteOpenAccess::InputShared,
    )?;
    let unit = resolve_unit(next, logical, get.connection, get.unit)?;
    authorize_path(authorizer, invocation, &binding.path, AccessIntent::Read)?;
    if qualified {
        // Exact held predefined local route and actual native structure profile,
        // never OD text, a caller encoding claim or an alias/model fallback.
        let attrs = state
            .catalog
            .native_attributes()
            .ok_or(HostProblem::Unsupported)?;
        if attrs.characters.md() != get.descriptor.characters()
            || binding.path != [binding.queue.as_str()]
            || !attrs.queues.iter().any(|q| q.name == binding.queue)
            || !state.catalog.definitions().any(|d| matches!(d,
                crate::MqObjectDefinition::LocalQueue { name, usage: crate::MqLocalQueueUsage::Normal, .. }
                    if name == &binding.queue)) {
            return Err(HostProblem::Unsupported);
        }
        producer::recheck(
            service,
            frame,
            invocation,
            admitted,
            &runtime.directory,
            now,
        )?;
    }
    // One existing kernel staging clone; no side inventory or public DTO
    // narrowing. Validate the exact first matching payload before installing
    // any candidate clock/UOW changes, never skip an unsupported selected head.
    let mut staged = next.delivery.clone();
    let (disposition, message, cursor) = staged
        .get_full(
            &state.catalog,
            &binding.queue,
            QueueProfile::Complete {
                version: get.descriptor.version(),
                characters: get.descriptor.characters(),
            },
            &request,
            unit,
        )
        .map_err(delivery_error)?;
    if let Some(message) = &message {
        descriptor_profile(&message.descriptor)?;
        // q097395_1498–1508: the stored GET output counter is bounded on
        // z/OS. The input MD counter is output-only and MUST NOT select policy.
        // Structured property transport is NOT native GMO/RFH2 admission.
        // q097395_946–962: successful GET cannot return Q_MGR/INHERIT
        // sentinels. Complete storage preserves diagnostic observations, but
        // this unformatted profile requires an explicit positive body CCSID.
        // Refuse rather than infer a queue default or perform conversion.
        if message.descriptor.fields().coded_char_set_id <= 0
            || !(0..=255).contains(&message.descriptor.fields().backout_count)
            || !message.properties.is_empty()
        {
            return Err(HostProblem::Unsupported);
        }
    }
    staged.advance_tick(now).map_err(delivery_error)?;
    let data_length = match disposition {
        MqGetDisposition::Message(MqTruncationDisposition::Complete { length }) => Some(length),
        MqGetDisposition::Message(
            MqTruncationDisposition::RejectedRetained { required, .. }
            | MqTruncationDisposition::AcceptedRemoved { required, .. },
        ) => Some(required),
        MqGetDisposition::NoMessage => None,
        _ => return Err(HostProblem::Unsupported),
    }
    .map(|length| i32::try_from(length).map_err(|_| HostProblem::ResourceExhausted))
    .transpose()?;
    let (completion, reason) = match disposition {
        MqGetDisposition::Message(MqTruncationDisposition::Complete { .. }) => {
            ("MQCC_OK", "MQRC_NONE")
        }
        MqGetDisposition::Message(MqTruncationDisposition::AcceptedRemoved { .. }) => {
            ("MQCC_WARNING", "MQRC_TRUNCATED_MSG_ACCEPTED")
        }
        MqGetDisposition::Message(MqTruncationDisposition::RejectedRetained { .. }) => {
            ("MQCC_WARNING", "MQRC_TRUNCATED_MSG_FAILED")
        }
        MqGetDisposition::NoMessage => ("MQCC_FAILED", "MQRC_NO_MSG_AVAILABLE"),
        _ => return Err(HostProblem::Unsupported),
    };
    let status = MqReviewedStatus::from_symbols(MqMqiCall::Get, completion, reason)
        .map_err(|_| HostProblem::Unsupported)?;
    if let Some(unit) = unit
        && matches!(
            disposition,
            MqGetDisposition::Message(
                MqTruncationDisposition::Complete { .. }
                    | MqTruncationDisposition::AcceptedRemoved { .. }
            )
        )
        && matches!(staged.unit_outcome(unit), MqDeliveryOutcome::Pending)
    {
        next.units
            .get_mut(&unit)
            .ok_or(HostProblem::Malformed)?
            .touch_queue(binding.queue.as_str())?;
    }
    let output = if qualified {
        // q096715_1260–1268: local queue of retrieved message. Rejected
        // truncation preserves the explicitly defined MD/prefix/length, but
        // lacks an explicit QName applicability fact in the pinned call page.
        // Preserve absence/caller GMO bytes, never manufacture blanks/success.
        let resolved_queue = if matches!(
            disposition,
            MqGetDisposition::Message(
                MqTruncationDisposition::Complete { .. }
                    | MqTruncationDisposition::AcceptedRemoved { .. }
            )
        ) {
            Some(producer::fixed(
                service,
                binding.queue.as_str(),
                get.descriptor.characters(),
            )?)
        } else {
            None
        };
        producer::recheck(
            service,
            frame,
            invocation,
            admitted,
            &runtime.directory,
            service
                .replay_clock
                .as_ref()
                .ok_or(HostProblem::Unsupported)?
                .now_tick()?,
        )?;
        MqMqiOutput::QualifiedFullGot(MqMqiQualifiedGot {
            characters: get.descriptor.characters(),
            disposition,
            message,
            cursor,
            data_length,
            resolved_queue,
        })
    } else {
        MqMqiOutput::FullGot {
            disposition,
            message,
            cursor,
            data_length,
        }
    };
    next.delivery = staged;
    next.reviewed_status = Some(status);
    next.output = output;
    Ok(())
}

pub(super) fn require_replay(
    state: &rich_state::RichStoredState,
    runtime: &mut SelectedRuntime,
    logical: &LogicalBatchOwner,
    owner: MqHandleOwner,
    get: &MqMqiFullGet,
    invocation: &Invocation,
    authorizer: &dyn EnterpriseAuthorizer,
    qualified: bool,
) -> Result<(), HostProblem> {
    controls(get).map_err(|_| HostProblem::UnknownOutcome)?;
    let object = require_object(
        runtime,
        owner,
        get.connection,
        get.object,
        MqRouteOpenAccess::InputShared,
    )
    .map_err(|_| HostProblem::UnknownOutcome)?;
    if qualified {
        let attrs = state
            .catalog
            .native_attributes()
            .ok_or(HostProblem::UnknownOutcome)?;
        if attrs.characters.md() != get.descriptor.characters()
            || object.path != [object.queue.as_str()]
            || !attrs.queues.iter().any(|q| q.name == object.queue)
            || state
                .delivery
                .full_queue_profile(&object.queue)
                .map_err(|_| HostProblem::UnknownOutcome)?
                != (QueueProfile::Complete {
                    version: get.descriptor.version(),
                    characters: get.descriptor.characters(),
                })
        {
            return Err(HostProblem::UnknownOutcome);
        }
    }
    let binding = runtime
        .connections
        .iter()
        .find(|b| b.connection == get.connection)
        .ok_or(HostProblem::UnknownOutcome)?;
    state
        .ownership
        .units
        .get(&binding.unit)
        .ok_or(HostProblem::UnknownOutcome)?
        .require_owner(logical, &binding.key, &runtime.control, binding.unit)
        .map_err(|_| HostProblem::UnknownOutcome)?;
    if matches!(get.unit, MqMqiUnitOfWork::Local { unit } if unit != binding.unit) {
        return Err(HostProblem::UnknownOutcome);
    }
    authorize_path(authorizer, invocation, &object.path, AccessIntent::Read)?;
    Ok(())
}
