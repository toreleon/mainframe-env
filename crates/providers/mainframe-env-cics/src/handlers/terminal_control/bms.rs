//! Durable full-BMS device controls, logical messages, and completed pages.

use super::super::super::{CicsService, Run, Session, bounded, mutation_problem};
use super::super::{field, store_error};
use super::partition_set::{read_array, read_field, take, take_bytes};
use mainframe_env_host_api::{
    CicsDisposition, CicsOperation, CicsRequest, CicsResponse, HostProblem, HostRequest,
    canonical_request_digest,
};
use mainframe_env_store_api::{ProviderStateMutation, ProviderStateRecord, ProviderStateWrite};

mod control;
mod page;

const STATE_NAMESPACE: &str = "cics-terminal-bms-v1";
const RECEIPT_NAMESPACE: &str = "cics-terminal-bms-receipt-v1";
const STATE_MAGIC: &[u8; 8] = b"MECBMS01";
const RECEIPT_MAGIC: &[u8; 8] = b"MECBR001";

/// Public, bounded view of terminal controls and pending BMS output.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CicsBmsControlSnapshot {
    /// Zero-based cursor position in the current terminal geometry.
    pub cursor: u16,
    /// Whether the terminal keyboard has been unlocked by a control operation.
    pub keyboard_unlocked: bool,
    /// Number of delivered alarm controls.
    pub alarm_count: u32,
    /// Number of delivered form-feed controls.
    pub formfeed_count: u32,
    /// Number of delivered print controls.
    pub print_count: u32,
    /// Active printer line width.
    pub print_width: u16,
    /// Whether alternate screen mode was selected.
    pub alternate_screen: bool,
    /// Partition whose cursor was last activated.
    pub active_partition: Option<String>,
    /// Partition selected for terminal output.
    pub output_partition: Option<String>,
    /// Last magnetic-stripe-reader control, when supplied.
    pub msr_control: Option<[u8; 4]>,
    /// True while a full-BMS logical message is being built.
    pub pending_logical_message: bool,
    /// Completed pages retained for operator paging.
    pub queued_pages: usize,
    /// Last completed bounded page image.
    pub last_page: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ControlFrame {
    pub flags: u16,
    pub cursor: Option<u16>,
    pub outpartn: Option<String>,
    pub actpartn: Option<String>,
    pub msr: Option<[u8; 4]>,
}

pub(super) const ERASE: u16 = 1 << 0;
pub(super) const ERASEAUP: u16 = 1 << 1;
pub(super) const FRSET: u16 = 1 << 2;
pub(super) const FREEKB: u16 = 1 << 3;
pub(super) const ALARM: u16 = 1 << 4;
pub(super) const PRINT: u16 = 1 << 5;
pub(super) const FORMFEED: u16 = 1 << 6;
pub(super) const DEFAULT: u16 = 1 << 7;
pub(super) const ALTERNATE: u16 = 1 << 8;
pub(super) const HONEOM: u16 = 1 << 9;
pub(super) const L40: u16 = 1 << 10;
pub(super) const L64: u16 = 1 << 11;
pub(super) const L80: u16 = 1 << 12;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct LogicalMessage {
    pub mode: u8,
    pub reqid: String,
    pub frames: Vec<ControlFrame>,
    pub payload: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct BmsState {
    pub version: u64,
    pub cursor: u16,
    pub keyboard_unlocked: bool,
    pub alarm_count: u32,
    pub formfeed_count: u32,
    pub print_count: u32,
    pub print_width: u16,
    pub alternate_screen: bool,
    pub active_partition: Option<String>,
    pub output_partition: Option<String>,
    pub msr_control: Option<[u8; 4]>,
    pub last_page: Vec<u8>,
    pub queued_pages: Vec<Vec<u8>>,
    pub message: Option<LogicalMessage>,
}

impl Default for BmsState {
    fn default() -> Self {
        Self {
            version: 0,
            cursor: 0,
            keyboard_unlocked: false,
            alarm_count: 0,
            formfeed_count: 0,
            print_count: 0,
            print_width: 80,
            alternate_screen: false,
            active_partition: None,
            output_partition: None,
            msr_control: None,
            last_page: Vec::new(),
            queued_pages: Vec::new(),
            message: None,
        }
    }
}

impl CicsService {
    /// Read the durable BMS control state for a terminal session.
    pub fn terminal_control_snapshot(
        &self,
        session: &mainframe_env_host_api::SessionId,
    ) -> Result<CicsBmsControlSnapshot, HostProblem> {
        if !self.lock()?.sessions.contains_key(session.as_str()) {
            return Err(HostProblem::NotFound);
        }
        let state = read_state(self, session.as_str())?;
        Ok(CicsBmsControlSnapshot {
            cursor: state.cursor,
            keyboard_unlocked: state.keyboard_unlocked,
            alarm_count: state.alarm_count,
            formfeed_count: state.formfeed_count,
            print_count: state.print_count,
            print_width: state.print_width,
            alternate_screen: state.alternate_screen,
            active_partition: state.active_partition,
            output_partition: state.output_partition,
            msr_control: state.msr_control,
            pending_logical_message: state.message.is_some(),
            queued_pages: state.queued_pages.len(),
            last_page: state.last_page,
        })
    }
}

pub(super) fn invoke_control(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    control::invoke(service, run, request)
}

pub(super) fn invoke_page(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    page::invoke(service, run, request)
}

pub(super) fn purge(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    page::purge(service, run, request)
}

pub(super) fn message_active(service: &CicsService, session: &str) -> Result<bool, HostProblem> {
    Ok(read_state(service, session)?.message.is_some())
}

pub(in crate::service) fn release_task(
    service: &CicsService,
    run: &Run,
) -> Result<(), HostProblem> {
    let mut state = read_state(service, &run.session)?;
    if state.message.take().is_some() {
        service
            .store
            .mutate_provider_states_atomic(vec![state_write(&run.session, &state, service)?])
            .map_err(store_error)
            .map_err(mutation_problem)?;
    }
    Ok(())
}

pub(super) fn read_state(service: &CicsService, session: &str) -> Result<BmsState, HostProblem> {
    service
        .store
        .get_provider_state(STATE_NAMESPACE, session)
        .map_err(store_error)?
        .map(|row| decode_state(&row, service))
        .transpose()
        .map(|state| state.unwrap_or_default())
}

pub(super) fn state_write(
    session: &str,
    state: &BmsState,
    service: &CicsService,
) -> Result<ProviderStateMutation, HostProblem> {
    let next_version = state
        .version
        .checked_add(1)
        .ok_or(HostProblem::ResourceExhausted)?;
    Ok(ProviderStateMutation::Put(ProviderStateWrite {
        record: ProviderStateRecord {
            namespace: STATE_NAMESPACE.into(),
            key: session.into(),
            version: next_version,
            payload: encode_state(state, service)?,
        },
        expected_version: (state.version != 0).then_some(state.version),
    }))
}

pub(super) fn session_write(
    session: &str,
    current: &Session,
    next: &Session,
) -> Result<ProviderStateMutation, HostProblem> {
    Ok(ProviderStateMutation::Put(ProviderStateWrite {
        record: ProviderStateRecord {
            namespace: "cics-session".into(),
            key: session.into(),
            version: next.version,
            payload: super::super::encode_session(next)?,
        },
        expected_version: Some(current.version),
    }))
}

pub(super) fn digest(request: &CicsRequest) -> Result<[u8; 32], HostProblem> {
    canonical_request_digest(&HostRequest::Cics(request.clone()))
        .map_err(|_| HostProblem::ResourceExhausted)
}

#[derive(Clone, Debug)]
pub(super) struct BmsReceipt {
    pub owner: String,
    pub digest: [u8; 32],
    pub disposition: CicsDisposition,
    pub condition: String,
    pub response: i32,
    pub response2: i32,
    pub payload: Vec<u8>,
    pub set: Option<Vec<u8>>,
    pub next_transaction: Option<String>,
}

pub(super) fn receipt_for_request(
    service: &CicsService,
    run: &Run,
    request: &CicsRequest,
) -> Result<Option<CicsResponse>, HostProblem> {
    let key = request
        .mutation
        .as_ref()
        .ok_or(HostProblem::MissingIdempotency)?
        .idempotency_key
        .as_str();
    let Some(row) = service
        .store
        .get_provider_state(RECEIPT_NAMESPACE, key)
        .map_err(store_error)?
    else {
        return Ok(None);
    };
    let receipt = decode_receipt(&row, service)?;
    if receipt.owner != run.invocation.run_unit_id.as_str() || receipt.digest != digest(request)? {
        return Err(HostProblem::IdempotencyConflict);
    }
    Ok(Some(receipt_response(service, run, &receipt)?))
}

pub(super) fn receipt_write(
    request: &CicsRequest,
    receipt: &BmsReceipt,
    service: &CicsService,
) -> Result<ProviderStateMutation, HostProblem> {
    let key = request
        .mutation
        .as_ref()
        .ok_or(HostProblem::MissingIdempotency)?
        .idempotency_key
        .as_str();
    Ok(ProviderStateMutation::Put(ProviderStateWrite {
        record: ProviderStateRecord {
            namespace: RECEIPT_NAMESPACE.into(),
            key: key.into(),
            version: 1,
            payload: encode_receipt(receipt, service)?,
        },
        expected_version: None,
    }))
}

pub(super) fn receipt_response(
    service: &CicsService,
    run: &Run,
    receipt: &BmsReceipt,
) -> Result<CicsResponse, HostProblem> {
    let mut response = service.response(
        run,
        receipt.disposition,
        &receipt.condition,
        receipt.response,
        receipt.response2,
        None,
        receipt.next_transaction.clone(),
        receipt.payload.clone(),
    )?;
    if let Some(set) = &receipt.set {
        response.outputs.insert("SET".into(), bounded(set.clone())?);
    }
    Ok(response)
}

pub(super) fn commit(
    service: &CicsService,
    run: &Run,
    request: &CicsRequest,
    current_session: Option<&Session>,
    next_session: Option<&Session>,
    state: &BmsState,
    receipt: &BmsReceipt,
) -> Result<CicsResponse, HostProblem> {
    let mut writes = Vec::new();
    if let (Some(current), Some(next)) = (current_session, next_session) {
        writes.push(session_write(&run.session, current, next)?);
    }
    writes.push(state_write(&run.session, state, service)?);
    if request.operation == CicsOperation::SendControl
        && let Some(mutation) = super::partition_set::mark_control_send(service, run)?
    {
        writes.push(mutation);
    }
    writes.push(receipt_write(request, receipt, service)?);
    match service.store.mutate_provider_states_atomic(writes) {
        Ok(()) => {
            if let Some(next) = next_session {
                service
                    .lock()
                    .map_err(mutation_problem)?
                    .sessions
                    .insert(run.session.clone(), next.clone());
            }
            receipt_response(service, run, receipt)
        }
        Err(
            mainframe_env_store_api::StoreError::Conflict
            | mainframe_env_store_api::StoreError::AlreadyExists,
        ) => receipt_for_request(service, run, request)?.ok_or(HostProblem::IdempotencyConflict),
        Err(error) => Err(mutation_problem(store_error(error))),
    }
}

fn encode_state(state: &BmsState, service: &CicsService) -> Result<Vec<u8>, HostProblem> {
    let queued_bytes = state.queued_pages.iter().try_fold(0usize, |size, page| {
        size.checked_add(page.len())
            .ok_or(HostProblem::ResourceExhausted)
    })?;
    if state.last_page.len() > service.limits.max_screen_bytes
        || state.queued_pages.len() > service.limits.max_queue_records
        || queued_bytes > service.limits.max_queue_bytes
    {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut out = STATE_MAGIC.to_vec();
    out.extend_from_slice(&state.cursor.to_be_bytes());
    out.push(u8::from(state.keyboard_unlocked));
    out.extend_from_slice(&state.alarm_count.to_be_bytes());
    out.extend_from_slice(&state.formfeed_count.to_be_bytes());
    out.extend_from_slice(&state.print_count.to_be_bytes());
    out.extend_from_slice(&state.print_width.to_be_bytes());
    out.push(u8::from(state.alternate_screen));
    field(
        &mut out,
        state.active_partition.as_deref().unwrap_or("").as_bytes(),
    )?;
    field(
        &mut out,
        state.output_partition.as_deref().unwrap_or("").as_bytes(),
    )?;
    field(
        &mut out,
        state
            .msr_control
            .as_ref()
            .map_or(&[][..], |value| &value[..]),
    )?;
    field(&mut out, &state.last_page)?;
    out.extend_from_slice(
        &u16::try_from(state.queued_pages.len())
            .map_err(|_| HostProblem::ResourceExhausted)?
            .to_be_bytes(),
    );
    for page in &state.queued_pages {
        field(&mut out, page)?;
    }
    out.push(u8::from(state.message.is_some()));
    if let Some(message) = &state.message {
        if message.frames.len() > service.limits.max_fields
            || message.payload.len() > service.limits.max_screen_bytes
        {
            return Err(HostProblem::ResourceExhausted);
        }
        out.push(message.mode);
        field(&mut out, message.reqid.as_bytes())?;
        out.extend_from_slice(
            &u16::try_from(message.frames.len())
                .map_err(|_| HostProblem::ResourceExhausted)?
                .to_be_bytes(),
        );
        for frame in &message.frames {
            out.extend_from_slice(&frame.flags.to_be_bytes());
            out.extend_from_slice(&frame.cursor.unwrap_or(u16::MAX).to_be_bytes());
            field(&mut out, frame.outpartn.as_deref().unwrap_or("").as_bytes())?;
            field(&mut out, frame.actpartn.as_deref().unwrap_or("").as_bytes())?;
            field(&mut out, frame.msr.as_ref().map_or(&[][..], |msr| &msr[..]))?;
        }
        field(&mut out, &message.payload)?;
    }
    Ok(out)
}

fn decode_state(row: &ProviderStateRecord, service: &CicsService) -> Result<BmsState, HostProblem> {
    if row.version == 0 {
        return Err(HostProblem::InfrastructureFailure);
    }
    let mut input = row.payload.as_slice();
    take(&mut input, STATE_MAGIC.len(), STATE_MAGIC)?;
    let cursor = u16::from_be_bytes(read_array(&mut input)?);
    let keyboard_unlocked = read_bool(&mut input)?;
    let alarm_count = u32::from_be_bytes(read_array(&mut input)?);
    let formfeed_count = u32::from_be_bytes(read_array(&mut input)?);
    let print_count = u32::from_be_bytes(read_array(&mut input)?);
    let print_width = u16::from_be_bytes(read_array(&mut input)?);
    if !matches!(print_width, 40 | 64 | 80) {
        return Err(HostProblem::InfrastructureFailure);
    }
    let alternate_screen = read_bool(&mut input)?;
    let active_partition = read_optional_name(&mut input, 2)?;
    let output_partition = read_optional_name(&mut input, 2)?;
    let msr_control = match read_field(&mut input, 4)? {
        [] => None,
        bytes if bytes.len() == 4 => Some(
            bytes
                .try_into()
                .map_err(|_| HostProblem::InfrastructureFailure)?,
        ),
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    let last_page = read_field(&mut input, service.limits.max_screen_bytes)?.to_vec();
    let queued_count = usize::from(u16::from_be_bytes(read_array(&mut input)?));
    if queued_count > service.limits.max_queue_records {
        return Err(HostProblem::InfrastructureFailure);
    }
    let mut queued_pages = Vec::with_capacity(queued_count);
    let mut queued_bytes = 0usize;
    for _ in 0..queued_count {
        let page = read_field(&mut input, service.limits.max_screen_bytes)?.to_vec();
        queued_bytes = queued_bytes
            .checked_add(page.len())
            .ok_or(HostProblem::InfrastructureFailure)?;
        if queued_bytes > service.limits.max_queue_bytes {
            return Err(HostProblem::InfrastructureFailure);
        }
        queued_pages.push(page);
    }
    let message = if read_bool(&mut input)? {
        let mode = take_bytes(&mut input, 1)?[0];
        if mode > 2 {
            return Err(HostProblem::InfrastructureFailure);
        }
        let reqid = String::from_utf8(read_field(&mut input, 2)?.to_vec())
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        if reqid.len() != 2 {
            return Err(HostProblem::InfrastructureFailure);
        }
        let count = usize::from(u16::from_be_bytes(read_array(&mut input)?));
        if count == 0 || count > service.limits.max_fields {
            return Err(HostProblem::InfrastructureFailure);
        }
        let mut frames = Vec::with_capacity(count);
        for _ in 0..count {
            let flags = u16::from_be_bytes(read_array(&mut input)?);
            if flags
                & !(ERASE
                    | ERASEAUP
                    | FRSET
                    | FREEKB
                    | ALARM
                    | PRINT
                    | FORMFEED
                    | DEFAULT
                    | ALTERNATE
                    | HONEOM
                    | L40
                    | L64
                    | L80)
                != 0
            {
                return Err(HostProblem::InfrastructureFailure);
            }
            let cursor = match u16::from_be_bytes(read_array(&mut input)?) {
                u16::MAX => None,
                cursor => Some(cursor),
            };
            let outpartn = read_optional_name(&mut input, 2)?;
            let actpartn = read_optional_name(&mut input, 2)?;
            let msr = match read_field(&mut input, 4)? {
                [] => None,
                bytes if bytes.len() == 4 => Some(
                    bytes
                        .try_into()
                        .map_err(|_| HostProblem::InfrastructureFailure)?,
                ),
                _ => return Err(HostProblem::InfrastructureFailure),
            };
            frames.push(ControlFrame {
                flags,
                cursor,
                outpartn,
                actpartn,
                msr,
            });
        }
        let payload = read_field(&mut input, service.limits.max_screen_bytes)?.to_vec();
        Some(LogicalMessage {
            mode,
            reqid,
            frames,
            payload,
        })
    } else {
        None
    };
    if !input.is_empty() {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(BmsState {
        version: row.version,
        cursor,
        keyboard_unlocked,
        alarm_count,
        formfeed_count,
        print_count,
        print_width,
        alternate_screen,
        active_partition,
        output_partition,
        msr_control,
        last_page,
        queued_pages,
        message,
    })
}

fn encode_receipt(receipt: &BmsReceipt, service: &CicsService) -> Result<Vec<u8>, HostProblem> {
    if receipt.payload.len() > service.limits.max_screen_bytes
        || receipt
            .set
            .as_ref()
            .is_some_and(|set| set.len() > service.limits.max_screen_bytes)
    {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut out = RECEIPT_MAGIC.to_vec();
    field(&mut out, receipt.owner.as_bytes())?;
    out.extend_from_slice(&receipt.digest);
    out.push(match receipt.disposition {
        CicsDisposition::Complete => 0,
        CicsDisposition::Returned => 1,
        _ => return Err(HostProblem::Malformed),
    });
    field(&mut out, receipt.condition.as_bytes())?;
    out.extend_from_slice(&receipt.response.to_be_bytes());
    out.extend_from_slice(&receipt.response2.to_be_bytes());
    field(&mut out, &receipt.payload)?;
    out.push(u8::from(receipt.set.is_some()));
    if let Some(set) = &receipt.set {
        field(&mut out, set)?;
    }
    field(
        &mut out,
        receipt.next_transaction.as_deref().unwrap_or("").as_bytes(),
    )?;
    Ok(out)
}

fn decode_receipt(
    row: &ProviderStateRecord,
    service: &CicsService,
) -> Result<BmsReceipt, HostProblem> {
    if row.version != 1 {
        return Err(HostProblem::InfrastructureFailure);
    }
    let mut input = row.payload.as_slice();
    take(&mut input, RECEIPT_MAGIC.len(), RECEIPT_MAGIC)?;
    let owner = String::from_utf8(read_field(&mut input, 256)?.to_vec())
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    let digest = read_array::<32>(&mut input)?;
    let disposition = match take_bytes(&mut input, 1)?[0] {
        0 => CicsDisposition::Complete,
        1 => CicsDisposition::Returned,
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    let condition = String::from_utf8(read_field(&mut input, 32)?.to_vec())
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    let response = i32::from_be_bytes(read_array(&mut input)?);
    let response2 = i32::from_be_bytes(read_array(&mut input)?);
    let payload = read_field(&mut input, service.limits.max_screen_bytes)?.to_vec();
    let set = if read_bool(&mut input)? {
        Some(read_field(&mut input, service.limits.max_screen_bytes)?.to_vec())
    } else {
        None
    };
    let next_transaction = String::from_utf8(read_field(&mut input, 4)?.to_vec())
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    if !input.is_empty() {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(BmsReceipt {
        owner,
        digest,
        disposition,
        condition,
        response,
        response2,
        payload,
        set,
        next_transaction: (!next_transaction.is_empty()).then_some(next_transaction),
    })
}

fn read_optional_name(input: &mut &[u8], max: usize) -> Result<Option<String>, HostProblem> {
    let bytes = read_field(input, max)?;
    if bytes.is_empty() {
        return Ok(None);
    }
    super::partition_set::normalized_name(bytes, max)
        .map(Some)
        .map_err(|_| HostProblem::InfrastructureFailure)
}

fn read_bool(input: &mut &[u8]) -> Result<bool, HostProblem> {
    match take_bytes(input, 1)?[0] {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(HostProblem::InfrastructureFailure),
    }
}

pub(super) fn base_receipt(run: &Run, request: &CicsRequest) -> Result<BmsReceipt, HostProblem> {
    Ok(BmsReceipt {
        owner: run.invocation.run_unit_id.as_str().into(),
        digest: digest(request)?,
        disposition: CicsDisposition::Complete,
        condition: "NORMAL".into(),
        response: 0,
        response2: 0,
        payload: Vec::new(),
        set: None,
        next_transaction: None,
    })
}
