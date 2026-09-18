use super::super::{CicsService, Reader, Run, field};
use crate::generated::{CICS_AID_NAMES, CICS_CONDITION_NAMES};
use mainframe_env_execution_api::InvocationLimits;
use mainframe_env_host_api::HostProblem;
use std::collections::{BTreeMap, BTreeSet};

pub(super) const MAX_HANDLE_STACK_DEPTH: usize = 64;

pub(in crate::service) fn session_schema_version(schema: &[u8]) -> Option<u8> {
    match schema {
        b"MECS1" => Some(1),
        b"MECS2" => Some(2),
        b"MECS3" => Some(3),
        b"MECS4" => Some(4),
        b"MECS5" => Some(5),
        b"MECS6" => Some(6),
        b"MECS7" => Some(7),
        b"MECS8" => Some(8),
        b"MECS9" => Some(9),
        b"MECSA" => Some(10),
        _ => None,
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::service) enum AbendExit {
    Label(String),
    Program(String),
}

impl AbendExit {
    #[cfg(test)]
    pub(in crate::service) fn target(&self) -> &str {
        match self {
            Self::Label(target) | Self::Program(target) => target,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::service) struct AbendRecord {
    pub(in crate::service) code: Vec<u8>,
    pub(in crate::service) original_code: Vec<u8>,
    pub(in crate::service) dump_requested: bool,
    pub(in crate::service) program: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::service) struct HandleFrame {
    pub(in crate::service) handlers: BTreeMap<String, String>,
    pub(in crate::service) aid_handlers: BTreeMap<String, String>,
    pub(in crate::service) ignored_conditions: BTreeSet<String>,
    pub(in crate::service) abend_handler: Option<AbendExit>,
    pub(in crate::service) cancelled_abend_handler: Option<AbendExit>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::service) struct HandleState {
    pub(in crate::service) handlers: BTreeMap<String, String>,
    pub(in crate::service) aid_handlers: BTreeMap<String, String>,
    pub(in crate::service) ignored_conditions: BTreeSet<String>,
    pub(in crate::service) abend_handler: Option<AbendExit>,
    pub(in crate::service) cancelled_abend_handler: Option<AbendExit>,
    pub(in crate::service) stack: Vec<HandleFrame>,
    pub(in crate::service) latest_abend: Option<AbendRecord>,
}

impl HandleState {
    pub(in crate::service) fn from_run(run: &Run) -> Self {
        Self {
            handlers: run.handlers.clone(),
            aid_handlers: run.aid_handlers.clone(),
            ignored_conditions: run.ignored_conditions.clone(),
            abend_handler: run.abend_handler.clone(),
            cancelled_abend_handler: run.cancelled_abend_handler.clone(),
            stack: run.handle_stack.clone(),
            latest_abend: run.latest_abend.clone(),
        }
    }

    fn apply(self, run: &mut Run) {
        run.handlers = self.handlers;
        run.aid_handlers = self.aid_handlers;
        run.ignored_conditions = self.ignored_conditions;
        run.abend_handler = self.abend_handler;
        run.cancelled_abend_handler = self.cancelled_abend_handler;
        run.handle_stack = self.stack;
        run.latest_abend = self.latest_abend;
    }
}

pub(super) fn persist_handle_state(
    service: &CicsService,
    run: &mut Run,
    previous: HandleState,
) -> Result<(), HostProblem> {
    let result: Result<(), HostProblem> = (|| {
        let next_handle_state = HandleState::from_run(run);
        let mut state = service.lock()?;
        let current = state
            .sessions
            .get(&run.session)
            .cloned()
            .ok_or(HostProblem::NotFound)?;
        let mut next = current.clone();
        next.version = next
            .version
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        next.handle_state = next_handle_state;
        service.persist_session(&run.session, &next, Some(current.version))?;
        state.sessions.insert(run.session.clone(), next);
        Ok(())
    })();
    if result.is_err() {
        previous.apply(run);
    }
    result
}

pub(super) fn encode_handle_state(
    out: &mut Vec<u8>,
    state: &HandleState,
) -> Result<(), HostProblem> {
    encode_handle_specifications(
        out,
        &state.handlers,
        &state.aid_handlers,
        &state.ignored_conditions,
        state.abend_handler.as_ref(),
        state.cancelled_abend_handler.as_ref(),
    )?;
    if state.stack.len() > MAX_HANDLE_STACK_DEPTH {
        return Err(HostProblem::InfrastructureFailure);
    }
    out.extend_from_slice(
        &u32::try_from(state.stack.len())
            .map_err(|_| HostProblem::ResourceExhausted)?
            .to_be_bytes(),
    );
    for frame in &state.stack {
        encode_handle_specifications(
            out,
            &frame.handlers,
            &frame.aid_handlers,
            &frame.ignored_conditions,
            frame.abend_handler.as_ref(),
            frame.cancelled_abend_handler.as_ref(),
        )?;
    }
    encode_abend_record(out, state.latest_abend.as_ref())?;
    Ok(())
}

fn encode_abend_record(out: &mut Vec<u8>, record: Option<&AbendRecord>) -> Result<(), HostProblem> {
    let Some(record) = record else {
        out.push(0);
        return Ok(());
    };
    if !valid_abend_code(&record.code)
        || !valid_abend_code(&record.original_code)
        || record
            .program
            .as_deref()
            .is_some_and(|program| !valid_program_name(program))
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    out.push(1);
    field(out, &record.code)?;
    out.push(u8::from(record.dump_requested));
    field(out, record.program.as_deref().unwrap_or("").as_bytes())?;
    field(out, &record.original_code)?;
    Ok(())
}

fn encode_handle_specifications(
    out: &mut Vec<u8>,
    handlers: &BTreeMap<String, String>,
    aid_handlers: &BTreeMap<String, String>,
    ignored: &BTreeSet<String>,
    abend: Option<&AbendExit>,
    cancelled_abend: Option<&AbendExit>,
) -> Result<(), HostProblem> {
    encode_named_labels(out, handlers, CICS_CONDITION_NAMES, false)?;
    encode_named_labels(out, aid_handlers, CICS_AID_NAMES, true)?;
    if ignored.len() > CICS_CONDITION_NAMES.len() {
        return Err(HostProblem::InfrastructureFailure);
    }
    out.extend_from_slice(
        &u32::try_from(ignored.len())
            .map_err(|_| HostProblem::ResourceExhausted)?
            .to_be_bytes(),
    );
    for name in ignored {
        if CICS_CONDITION_NAMES.binary_search(&name.as_str()).is_err() {
            return Err(HostProblem::InfrastructureFailure);
        }
        field(out, name.as_bytes())?;
    }
    for exit in [abend, cancelled_abend] {
        encode_abend_exit(out, exit)?;
    }
    Ok(())
}

fn encode_named_labels(
    out: &mut Vec<u8>,
    values: &BTreeMap<String, String>,
    allowed: &[&str],
    allow_empty: bool,
) -> Result<(), HostProblem> {
    if values.len() > allowed.len() {
        return Err(HostProblem::InfrastructureFailure);
    }
    out.extend_from_slice(
        &u32::try_from(values.len())
            .map_err(|_| HostProblem::ResourceExhausted)?
            .to_be_bytes(),
    );
    for (name, label) in values {
        if allowed.binary_search(&name.as_str()).is_err()
            || !label.is_empty() && !valid_condition_label(label)
            || !allow_empty && label.is_empty()
        {
            return Err(HostProblem::InfrastructureFailure);
        }
        field(out, name.as_bytes())?;
        field(out, label.as_bytes())?;
    }
    Ok(())
}

fn encode_abend_exit(out: &mut Vec<u8>, exit: Option<&AbendExit>) -> Result<(), HostProblem> {
    match exit {
        None => out.push(0),
        Some(AbendExit::Label(label)) if valid_condition_label(label) => {
            out.push(1);
            field(out, label.as_bytes())?;
        }
        Some(AbendExit::Program(program)) if valid_program_name(program) => {
            out.push(2);
            field(out, program.as_bytes())?;
        }
        Some(_) => return Err(HostProblem::InfrastructureFailure),
    }
    Ok(())
}

pub(in crate::service) fn decode_handle_state(
    reader: &mut Reader<'_>,
    legacy_abend_labels: bool,
    abend_record_version: u8,
) -> Result<HandleState, HostProblem> {
    let (handlers, aid_handlers, ignored_conditions, abend_handler, cancelled_abend_handler) =
        decode_handle_specifications(reader, legacy_abend_labels)?;
    let depth = usize::try_from(u32::from_be_bytes(
        reader
            .take(4)?
            .try_into()
            .map_err(|_| HostProblem::InfrastructureFailure)?,
    ))
    .map_err(|_| HostProblem::ResourceExhausted)?;
    if depth > MAX_HANDLE_STACK_DEPTH {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut stack = Vec::with_capacity(depth);
    for _ in 0..depth {
        let (handlers, aid_handlers, ignored_conditions, abend_handler, cancelled_abend_handler) =
            decode_handle_specifications(reader, legacy_abend_labels)?;
        stack.push(HandleFrame {
            handlers,
            aid_handlers,
            ignored_conditions,
            abend_handler,
            cancelled_abend_handler,
        });
    }
    let latest_abend = if abend_record_version > 0 {
        decode_abend_record(reader, abend_record_version)?
    } else {
        None
    };
    Ok(HandleState {
        handlers,
        aid_handlers,
        ignored_conditions,
        abend_handler,
        cancelled_abend_handler,
        stack,
        latest_abend,
    })
}

pub(in crate::service) fn decode_session_handle_state(
    reader: &mut Reader<'_>,
    schema: u8,
) -> Result<HandleState, HostProblem> {
    match schema {
        6 => decode_handle_state(reader, true, 0),
        7 => decode_handle_state(reader, false, 0),
        8 => decode_handle_state(reader, false, 1),
        9 | 10 => decode_handle_state(reader, false, 2),
        _ => Ok(HandleState::default()),
    }
}

pub(in crate::service) fn decode_session_tail(
    reader: &mut Reader<'_>,
    schema: u8,
    payload: Option<Vec<u8>>,
) -> Result<(HandleState, super::TerminalInput), HostProblem> {
    let handle_state = decode_session_handle_state(reader, schema)?;
    let message_length = if schema >= 10 {
        u32::from_be_bytes(
            reader
                .take(4)?
                .try_into()
                .map_err(|_| HostProblem::InfrastructureFailure)?,
        )
    } else {
        u32::try_from(payload.as_ref().map_or(0, Vec::len))
            .map_err(|_| HostProblem::InfrastructureFailure)?
    };
    if message_length > 32_767
        || payload
            .as_ref()
            .is_some_and(|value| usize::try_from(message_length).ok() != Some(value.len()))
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok((
        handle_state,
        super::TerminalInput {
            payload,
            message_length,
        },
    ))
}

fn decode_abend_record(
    reader: &mut Reader<'_>,
    record_version: u8,
) -> Result<Option<AbendRecord>, HostProblem> {
    match reader.take(1)?[0] {
        0 => Ok(None),
        1 => {
            let code = reader.field(4)?;
            let dump_requested = match reader.take(1)?[0] {
                0 => false,
                1 => true,
                _ => return Err(HostProblem::InfrastructureFailure),
            };
            let program = String::from_utf8(reader.field(8)?)
                .map_err(|_| HostProblem::InfrastructureFailure)?;
            let original_code = if record_version >= 2 {
                reader.field(4)?
            } else {
                code.clone()
            };
            if !valid_abend_code(&code)
                || !valid_abend_code(&original_code)
                || !program.is_empty() && !valid_program_name(&program)
            {
                return Err(HostProblem::InfrastructureFailure);
            }
            Ok(Some(AbendRecord {
                code,
                original_code,
                dump_requested,
                program: (!program.is_empty()).then_some(program),
            }))
        }
        _ => Err(HostProblem::InfrastructureFailure),
    }
}

type DecodedHandleSpecifications = (
    BTreeMap<String, String>,
    BTreeMap<String, String>,
    BTreeSet<String>,
    Option<AbendExit>,
    Option<AbendExit>,
);

fn decode_handle_specifications(
    reader: &mut Reader<'_>,
    legacy_abend_labels: bool,
) -> Result<DecodedHandleSpecifications, HostProblem> {
    let handlers = decode_named_labels(reader, CICS_CONDITION_NAMES, false)?;
    let aid_handlers = decode_named_labels(reader, CICS_AID_NAMES, true)?;
    let ignored_count = usize::try_from(u32::from_be_bytes(
        reader
            .take(4)?
            .try_into()
            .map_err(|_| HostProblem::InfrastructureFailure)?,
    ))
    .map_err(|_| HostProblem::ResourceExhausted)?;
    if ignored_count > CICS_CONDITION_NAMES.len() {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut ignored = BTreeSet::new();
    for _ in 0..ignored_count {
        let name =
            String::from_utf8(reader.field(32)?).map_err(|_| HostProblem::InfrastructureFailure)?;
        if CICS_CONDITION_NAMES.binary_search(&name.as_str()).is_err() || !ignored.insert(name) {
            return Err(HostProblem::InfrastructureFailure);
        }
    }
    let (abend, cancelled_abend) = if legacy_abend_labels {
        (
            decode_legacy_abend_label(reader)?,
            decode_legacy_abend_label(reader)?,
        )
    } else {
        (decode_abend_exit(reader)?, decode_abend_exit(reader)?)
    };
    Ok((handlers, aid_handlers, ignored, abend, cancelled_abend))
}

fn decode_legacy_abend_label(reader: &mut Reader<'_>) -> Result<Option<AbendExit>, HostProblem> {
    let value = String::from_utf8(reader.field(InvocationLimits::default().max_identity_bytes)?)
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    if !value.is_empty() && !valid_condition_label(&value) {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok((!value.is_empty()).then_some(AbendExit::Label(value)))
}

fn decode_abend_exit(reader: &mut Reader<'_>) -> Result<Option<AbendExit>, HostProblem> {
    let kind = reader.take(1)?[0];
    if kind == 0 {
        return Ok(None);
    }
    let target = String::from_utf8(reader.field(InvocationLimits::default().max_identity_bytes)?)
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    match kind {
        1 if valid_condition_label(&target) => Ok(Some(AbendExit::Label(target))),
        2 if valid_program_name(&target) => Ok(Some(AbendExit::Program(target))),
        _ => Err(HostProblem::InfrastructureFailure),
    }
}

fn decode_named_labels(
    reader: &mut Reader<'_>,
    allowed: &[&str],
    allow_empty: bool,
) -> Result<BTreeMap<String, String>, HostProblem> {
    let count = usize::try_from(u32::from_be_bytes(
        reader
            .take(4)?
            .try_into()
            .map_err(|_| HostProblem::InfrastructureFailure)?,
    ))
    .map_err(|_| HostProblem::ResourceExhausted)?;
    if count > allowed.len() {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut values = BTreeMap::new();
    for _ in 0..count {
        let name =
            String::from_utf8(reader.field(32)?).map_err(|_| HostProblem::InfrastructureFailure)?;
        let label =
            String::from_utf8(reader.field(InvocationLimits::default().max_identity_bytes)?)
                .map_err(|_| HostProblem::InfrastructureFailure)?;
        if allowed.binary_search(&name.as_str()).is_err()
            || !label.is_empty() && !valid_condition_label(&label)
            || !allow_empty && label.is_empty()
            || values.insert(name, label).is_some()
        {
            return Err(HostProblem::InfrastructureFailure);
        }
    }
    Ok(values)
}

fn valid_condition_label(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= InvocationLimits::default().max_identity_bytes
        && value
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'-')
}

fn valid_program_name(value: &str) -> bool {
    matches!(value.len(), 1..=8) && value.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

fn valid_abend_code(value: &[u8]) -> bool {
    matches!(value.len(), 1..=4)
        && !value[0].eq_ignore_ascii_case(&b'A')
        && value.iter().all(|byte| byte.is_ascii_graphic())
}
