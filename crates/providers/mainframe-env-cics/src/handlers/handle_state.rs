use super::super::{CicsService, Reader, Run, field};
use crate::generated::{CICS_AID_NAMES, CICS_CONDITION_NAMES};
use mainframe_env_execution_api::InvocationLimits;
use mainframe_env_host_api::HostProblem;
use std::collections::{BTreeMap, BTreeSet};

pub(super) const MAX_HANDLE_STACK_DEPTH: usize = 64;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::service) struct HandleFrame {
    pub(in crate::service) handlers: BTreeMap<String, String>,
    pub(in crate::service) aid_handlers: BTreeMap<String, String>,
    pub(in crate::service) ignored_conditions: BTreeSet<String>,
    pub(in crate::service) abend_handler: Option<String>,
    pub(in crate::service) cancelled_abend_handler: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::service) struct HandleState {
    pub(in crate::service) handlers: BTreeMap<String, String>,
    pub(in crate::service) aid_handlers: BTreeMap<String, String>,
    pub(in crate::service) ignored_conditions: BTreeSet<String>,
    pub(in crate::service) abend_handler: Option<String>,
    pub(in crate::service) cancelled_abend_handler: Option<String>,
    pub(in crate::service) stack: Vec<HandleFrame>,
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
        }
    }

    fn apply(self, run: &mut Run) {
        run.handlers = self.handlers;
        run.aid_handlers = self.aid_handlers;
        run.ignored_conditions = self.ignored_conditions;
        run.abend_handler = self.abend_handler;
        run.cancelled_abend_handler = self.cancelled_abend_handler;
        run.handle_stack = self.stack;
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
        state.abend_handler.as_deref(),
        state.cancelled_abend_handler.as_deref(),
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
            frame.abend_handler.as_deref(),
            frame.cancelled_abend_handler.as_deref(),
        )?;
    }
    Ok(())
}

fn encode_handle_specifications(
    out: &mut Vec<u8>,
    handlers: &BTreeMap<String, String>,
    aid_handlers: &BTreeMap<String, String>,
    ignored: &BTreeSet<String>,
    abend: Option<&str>,
    cancelled_abend: Option<&str>,
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
    for label in [abend, cancelled_abend] {
        if label.is_some_and(|label| label.is_empty() || !valid_condition_label(label)) {
            return Err(HostProblem::InfrastructureFailure);
        }
        field(out, label.unwrap_or("").as_bytes())?;
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
            || !valid_condition_label(label)
            || !allow_empty && label.is_empty()
        {
            return Err(HostProblem::InfrastructureFailure);
        }
        field(out, name.as_bytes())?;
        field(out, label.as_bytes())?;
    }
    Ok(())
}

pub(in crate::service) fn decode_handle_state(
    reader: &mut Reader<'_>,
) -> Result<HandleState, HostProblem> {
    let (handlers, aid_handlers, ignored_conditions, abend_handler, cancelled_abend_handler) =
        decode_handle_specifications(reader)?;
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
            decode_handle_specifications(reader)?;
        stack.push(HandleFrame {
            handlers,
            aid_handlers,
            ignored_conditions,
            abend_handler,
            cancelled_abend_handler,
        });
    }
    Ok(HandleState {
        handlers,
        aid_handlers,
        ignored_conditions,
        abend_handler,
        cancelled_abend_handler,
        stack,
    })
}

type DecodedHandleSpecifications = (
    BTreeMap<String, String>,
    BTreeMap<String, String>,
    BTreeSet<String>,
    Option<String>,
    Option<String>,
);

fn decode_handle_specifications(
    reader: &mut Reader<'_>,
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
    let mut label = || -> Result<Option<String>, HostProblem> {
        let value =
            String::from_utf8(reader.field(InvocationLimits::default().max_identity_bytes)?)
                .map_err(|_| HostProblem::InfrastructureFailure)?;
        if !value.is_empty() && !valid_condition_label(&value) {
            return Err(HostProblem::InfrastructureFailure);
        }
        Ok((!value.is_empty()).then_some(value))
    };
    Ok((handlers, aid_handlers, ignored, label()?, label()?))
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
            || !valid_condition_label(&label)
            || !allow_empty && label.is_empty()
            || values.insert(name, label).is_some()
        {
            return Err(HostProblem::InfrastructureFailure);
        }
    }
    Ok(values)
}

fn valid_condition_label(value: &str) -> bool {
    value.len() <= InvocationLimits::default().max_identity_bytes
        && value
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'-')
}
