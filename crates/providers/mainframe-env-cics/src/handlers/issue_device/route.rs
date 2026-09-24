//! Atomic provider route and bounded replay for physical ISSUE device flows.

use super::super::{CicsService, Run, store_error};
use super::*;
use mainframe_env_host_api::{
    AccessIntent, CicsDisposition, CicsOperation, CicsRequest, CicsResponse, CicsUnitOfWorkOutcome,
    HostProblem, HostRequest, canonical_request_digest,
};
use std::sync::atomic::Ordering;

const RECEIPT_NAMESPACE: &str = "cics-issue-device-receipt-v1";
const MAX_RECEIPTS_PER_SCAN: usize = 4096;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct IssueDeviceReceipt {
    schema_version: u16,
    effect_key: String,
    owner_execution: String,
    owner_run_unit: String,
    owner_principal: String,
    owner_epoch: u64,
    mutation_sequence: u64,
    request_digest: [u8; 32],
    deadline_tick: u64,
    retain_until_tick: u64,
    condition: String,
    response: i32,
    response2: i32,
}

impl IssueDeviceReceipt {
    fn validate(&self) -> Result<(), HostProblem> {
        if self.schema_version != 1
            || self.effect_key.is_empty()
            || self.effect_key.len() > 256
            || self.owner_execution.is_empty()
            || self.owner_execution.len() > 128
            || self.owner_run_unit.is_empty()
            || self.owner_run_unit.len() > 128
            || self.owner_principal.is_empty()
            || self.owner_principal.len() > 128
            || self.owner_epoch == 0
            || self.mutation_sequence == 0
            || self.deadline_tick == 0
            || self.retain_until_tick < self.deadline_tick
            || self.condition != "NORMAL"
            || self.response != 0
            || self.response2 != 0
        {
            return Err(HostProblem::InfrastructureFailure);
        }
        Ok(())
    }

    fn bytes(&self) -> Result<Vec<u8>, HostProblem> {
        self.validate()?;
        let bytes = serde_json::to_vec(self).map_err(|_| HostProblem::InfrastructureFailure)?;
        if bytes.len() > 1024 {
            return Err(HostProblem::ResourceExhausted);
        }
        Ok(bytes)
    }
}

/// Execute the 3740/3650 ISSUE state commands against one installed physical
/// device, atomically retaining the state transition and exact replay result.
pub(in crate::service) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    validate_request(request)?;
    let current_session = {
        let state = service.lock()?;
        state
            .sessions
            .get(&run.session)
            .cloned()
            .ok_or(HostProblem::NotFound)?
    };
    let terminal = current_session
        .input
        .terminal_id
        .as_deref()
        .ok_or_else(not_allocated)?;
    if matches!(
        request.operation,
        CicsOperation::IssueEndfile
            | CicsOperation::IssueEndoutput
            | CicsOperation::IssueEods
            | CicsOperation::IssuePrint
    ) {
        reject_dpl_principal(run)?;
    }
    let mutation = request
        .mutation
        .as_ref()
        .ok_or(HostProblem::MissingIdempotency)?;
    let effect_key = mutation.idempotency_key.as_str();
    let digest = canonical_request_digest(&HostRequest::Cics(request.clone()))
        .map_err(|_| HostProblem::ResourceExhausted)?;
    if let Some(receipt) = load_receipt(service, effect_key, run, mutation.sequence, digest)? {
        return receipt_response(service, run, &receipt);
    }
    check_request_live(service, run)?;
    let already_disconnected = !current_session.connected
        && matches!(
            request.operation,
            CicsOperation::IssueDisconnect | CicsOperation::IssueReset
        )
        && !request.arguments.contains_key("SESSION");
    if !current_session.connected && !already_disconnected {
        return Err(not_allocated());
    }
    if service
        .store
        .list_provider_state(RECEIPT_NAMESPACE, MAX_RECEIPTS_PER_SCAN + 1)
        .map_err(store_error)?
        .len()
        >= MAX_RECEIPTS_PER_SCAN
    {
        return Err(HostProblem::ResourceExhausted);
    }
    let current = IssueDeviceRecord::load(service.store.as_ref(), &terminal)
        .map_err(store_error)?
        .ok_or_else(not_allocated)?;
    service.authorize(
        run,
        "FACILITY",
        &format!("CICS.ISSUE.DEVICE.{terminal}"),
        AccessIntent::Update,
    )?;
    let mut next = current.clone();
    let mut disconnected_session = None;
    let mut printer_write = None;
    match request.operation {
        CicsOperation::IssueEndfile => next
            .mark_endfile(request.arguments.contains_key("OPTION.ENDOUTPUT"))
            .map_err(|problem| device_condition(request.operation, problem))?,
        CicsOperation::IssueEndoutput => next
            .mark_endoutput(request.arguments.contains_key("OPTION.ENDFILE"))
            .map_err(|problem| device_condition(request.operation, problem))?,
        CicsOperation::IssueEods => next
            .mark_eods()
            .map_err(|problem| device_condition(request.operation, problem))?,
        CicsOperation::IssueLoad => {
            let program = request
                .arguments
                .get("PROGRAM")
                .ok_or(HostProblem::Malformed)?;
            let name = std::str::from_utf8(program.bytes())
                .map_err(|_| condition("NONVAL", 9, 0))?
                .trim_end()
                .to_ascii_uppercase();
            if !valid_name(&name, 8) {
                return Err(condition("NONVAL", 9, 0));
            }
            next.load_program(&name, request.arguments.contains_key("OPTION.CONVERSE"))
                .map_err(|problem| device_condition(request.operation, problem))?;
        }
        CicsOperation::IssuePass => {
            let application = text_argument(request, "LUNAME")?
                .ok_or(HostProblem::Malformed)?
                .trim_end()
                .to_ascii_uppercase();
            if !valid_name(&application, 8) {
                return Err(condition("INVREQ", 16, 0));
            }
            let from = request.arguments.get("FROM").map(|value| value.bytes());
            let length = text_argument(request, "LENGTH")?
                .map(|value| {
                    value
                        .parse::<i64>()
                        .map_err(|_| condition("LENGERR", 22, 0))
                })
                .transpose()?
                .unwrap_or(0);
            let length = usize::try_from(length).map_err(|_| condition("LENGERR", 22, 0))?;
            if length > MAX_PASS_BYTES || from.is_some_and(|data| length > data.len()) {
                return Err(condition("LENGERR", 22, 0));
            }
            let data = from.map_or(&[][..], |bytes| &bytes[..length]);
            let logmode =
                text_argument(request, "LOGMODE")?.map(|value| value.trim_end().to_owned());
            next.prepare_pass(
                &application,
                run.invocation.run_unit_id.as_str(),
                data,
                logmode.as_deref(),
                request.arguments.contains_key("OPTION.LOGONLOGMODE"),
                request.arguments.contains_key("OPTION.NOQUIESCE"),
            )
            .map_err(|problem| device_condition(request.operation, problem))?;
        }
        CicsOperation::IssueDisconnect | CicsOperation::IssueReset => {
            if request.arguments.contains_key("SESSION") {
                return Err(HostProblem::Unsupported);
            }
            if already_disconnected {
                if !current.state.disconnected {
                    return Err(HostProblem::UnknownOutcome);
                }
            } else {
                next.disconnect()
                    .map_err(|problem| device_condition(request.operation, problem))?;
                let mut updated = current_session.clone();
                updated.version = updated
                    .version
                    .checked_add(1)
                    .ok_or(HostProblem::ResourceExhausted)?;
                updated.connected = false;
                disconnected_session = Some(updated);
            }
        }
        CicsOperation::IssuePrint => {
            if !matches!(
                current.definition.kind,
                IssueDeviceKind::Display3270 | IssueDeviceKind::Interpreter3650
            ) {
                return Err(condition("INVREQ", 16, 0));
            }
            if current_session.screen.len() > MAX_PRINT_BYTES {
                return Err(HostProblem::ResourceExhausted);
            }
            let mut selected = None;
            for id in &current.definition.printers {
                let Some(printer) =
                    IssueDeviceRecord::load(service.store.as_ref(), id).map_err(store_error)?
                else {
                    continue;
                };
                if printer.printer_available() {
                    selected = Some((id, printer));
                    break;
                }
            }
            let (printer_id, printer) = selected.ok_or_else(|| condition("TERMERR", 81, 0))?;
            service.authorize(
                run,
                "FACILITY",
                &format!("CICS.ISSUE.DEVICE.{printer_id}"),
                AccessIntent::Update,
            )?;
            next.record_print(printer_id, &current_session.screen)
                .map_err(|problem| device_condition(request.operation, problem))?;
            let mut reserved = printer.clone();
            reserved
                .accept_print(&current_session.screen)
                .map_err(|problem| device_condition(request.operation, problem))?;
            printer_write = Some(printer.mutation(&mut reserved).map_err(store_error)?);
        }
        _ => return Err(HostProblem::Unsupported),
    }
    let receipt = IssueDeviceReceipt {
        schema_version: 1,
        effect_key: effect_key.into(),
        owner_execution: run.invocation.execution_id.as_str().into(),
        owner_run_unit: run.invocation.run_unit_id.as_str().into(),
        owner_principal: run.invocation.principal.id().as_str().into(),
        owner_epoch: u64::from(run.invocation.attempt),
        mutation_sequence: mutation.sequence,
        request_digest: digest,
        deadline_tick: run.invocation.deadline_tick,
        retain_until_tick: run.invocation.deadline_tick,
        condition: "NORMAL".into(),
        response: 0,
        response2: 0,
    };
    let receipt_write = ProviderStateMutation::Put(ProviderStateWrite {
        record: ProviderStateRecord {
            namespace: RECEIPT_NAMESPACE.into(),
            key: effect_key.into(),
            version: 1,
            payload: receipt.bytes()?,
        },
        expected_version: None,
    });
    let mut writes = vec![receipt_write];
    if !already_disconnected {
        writes.push(current.mutation(&mut next).map_err(store_error)?);
    }
    if let Some(printer_write) = printer_write {
        writes.push(printer_write);
    }
    if let Some(updated) = &disconnected_session {
        writes.push(ProviderStateMutation::Put(ProviderStateWrite {
            record: ProviderStateRecord {
                namespace: "cics-session".into(),
                key: run.session.clone(),
                version: updated.version,
                payload: super::super::encode_session(updated)?,
            },
            expected_version: Some(current_session.version),
        }));
    }
    match service.store.mutate_provider_states_atomic(writes) {
        Ok(()) => {
            if let Some(updated) = disconnected_session {
                service
                    .lock()
                    .map_err(|_| HostProblem::UnknownOutcome)?
                    .sessions
                    .insert(run.session.clone(), updated);
            }
            if run.invocation.cancellation_requested()
                || request_expired(service, run)?
                || service
                    .replay_unknown_after_persist
                    .swap(false, Ordering::SeqCst)
            {
                return Err(HostProblem::UnknownOutcome);
            }
            receipt_response(service, run, &receipt)
        }
        Err(StoreError::Conflict | StoreError::AlreadyExists) => {
            let saved = load_receipt(service, effect_key, run, mutation.sequence, digest)?
                .ok_or(HostProblem::UnknownOutcome)?;
            receipt_response(service, run, &saved)
        }
        Err(error) => Err(super::super::super::mutation_problem(store_error(error))),
    }
}

fn check_request_live(service: &CicsService, run: &Run) -> Result<(), HostProblem> {
    if run.invocation.cancellation_requested() {
        return Err(HostProblem::Cancelled);
    }
    if request_expired(service, run)? {
        return Err(HostProblem::TimedOut);
    }
    Ok(())
}

fn request_expired(service: &CicsService, run: &Run) -> Result<bool, HostProblem> {
    service
        .replay_clock
        .as_ref()
        .map(|clock| {
            clock
                .now_tick()
                .map(|tick| tick >= run.invocation.deadline_tick)
        })
        .transpose()
        .map(|expired| expired.unwrap_or(false))
}

fn validate_request(request: &CicsRequest) -> Result<(), HostProblem> {
    if !matches!(
        request.operation,
        CicsOperation::IssueEndfile
            | CicsOperation::IssueEndoutput
            | CicsOperation::IssueEods
            | CicsOperation::IssueLoad
            | CicsOperation::IssuePass
            | CicsOperation::IssueDisconnect
            | CicsOperation::IssueReset
            | CicsOperation::IssuePrint
    ) || request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
    {
        return Err(HostProblem::Malformed);
    }
    for (name, value) in &request.arguments {
        let valid = match name.as_str() {
            "RESP" | "RESP2" => value.schema() == "mainframe-env.cics.argument@1",
            "PROGRAM" if request.operation == CicsOperation::IssueLoad => matches!(
                value.schema(),
                "mainframe-env.cics.literal@1" | "mainframe-env.cics.storage-value@1"
            ),
            "LUNAME" | "LOGMODE" if request.operation == CicsOperation::IssuePass => matches!(
                value.schema(),
                "mainframe-env.cics.literal@1" | "mainframe-env.cics.storage-value@1"
            ),
            "FROM" if request.operation == CicsOperation::IssuePass => {
                value.schema() == "mainframe-env.cics.storage-value@1"
            }
            "LENGTH" if request.operation == CicsOperation::IssuePass => {
                value.schema() == "mainframe-env.cics.decimal@1"
            }
            "SESSION" if request.operation == CicsOperation::IssueDisconnect => matches!(
                value.schema(),
                "mainframe-env.cics.literal@1" | "mainframe-env.cics.storage-value@1"
            ),
            "OPTION.NOHANDLE" => {
                value.schema() == "mainframe-env.cics.option@1" && value.bytes().is_empty()
            }
            "OPTION.ENDOUTPUT" if request.operation == CicsOperation::IssueEndfile => {
                value.schema() == "mainframe-env.cics.option@1" && value.bytes().is_empty()
            }
            "OPTION.ENDFILE" if request.operation == CicsOperation::IssueEndoutput => {
                value.schema() == "mainframe-env.cics.option@1" && value.bytes().is_empty()
            }
            "OPTION.CONVERSE" if request.operation == CicsOperation::IssueLoad => {
                value.schema() == "mainframe-env.cics.option@1" && value.bytes().is_empty()
            }
            "OPTION.LOGONLOGMODE" | "OPTION.NOQUIESCE"
                if request.operation == CicsOperation::IssuePass =>
            {
                value.schema() == "mainframe-env.cics.option@1" && value.bytes().is_empty()
            }
            _ => false,
        };
        if !valid {
            return Err(HostProblem::Malformed);
        }
    }
    if request.operation == CicsOperation::IssueLoad && !request.arguments.contains_key("PROGRAM") {
        return Err(HostProblem::Malformed);
    }
    if request.operation == CicsOperation::IssuePass
        && (!request.arguments.contains_key("LUNAME")
            || request.arguments.contains_key("FROM") != request.arguments.contains_key("LENGTH")
            || request.arguments.contains_key("LOGMODE")
                && request.arguments.contains_key("OPTION.LOGONLOGMODE"))
    {
        return Err(HostProblem::Malformed);
    }
    Ok(())
}

fn text_argument(request: &CicsRequest, name: &str) -> Result<Option<String>, HostProblem> {
    request
        .arguments
        .get(name)
        .map(|value| String::from_utf8(value.bytes().to_vec()).map_err(|_| HostProblem::Malformed))
        .transpose()
}

fn reject_dpl_principal(run: &Run) -> Result<(), HostProblem> {
    let Some(context) = run.invocation.bindings.get("cics.execution-context") else {
        return Ok(());
    };
    if context.schema() != "mainframe-env.cics.execution-context@1" {
        return Err(HostProblem::Malformed);
    }
    match context.bytes() {
        b"local" => Ok(()),
        b"dpl-synconreturn" | b"dpl-without-synconreturn" | b"dpl-executionset-subset" => {
            Err(condition("INVREQ", 16, 200))
        }
        _ => Err(HostProblem::Malformed),
    }
}

fn load_receipt(
    service: &CicsService,
    key: &str,
    run: &Run,
    sequence: u64,
    digest: [u8; 32],
) -> Result<Option<IssueDeviceReceipt>, HostProblem> {
    let Some(row) = service
        .store
        .get_provider_state(RECEIPT_NAMESPACE, key)
        .map_err(store_error)?
    else {
        return Ok(None);
    };
    if row.version != 1 || row.payload.len() > 1024 {
        return Err(HostProblem::InfrastructureFailure);
    }
    let receipt: IssueDeviceReceipt =
        serde_json::from_slice(&row.payload).map_err(|_| HostProblem::InfrastructureFailure)?;
    if receipt.schema_version != 1
        || receipt.effect_key != key
        || receipt.owner_execution != run.invocation.execution_id.as_str()
        || receipt.owner_run_unit != run.invocation.run_unit_id.as_str()
        || receipt.owner_principal != run.invocation.principal.id().as_str()
        || receipt.owner_epoch != u64::from(run.invocation.attempt)
        || receipt.mutation_sequence != sequence
        || receipt.request_digest != digest
        || receipt.bytes()? != row.payload
    {
        return Err(HostProblem::IdempotencyConflict);
    }
    Ok(Some(receipt))
}

/// Reclaim only receipts past an operator-supplied safe watermark and absent
/// from retained checkpoint/effect references. A full scan is an explicit
/// saturation error rather than an unsafe partial deletion.
pub fn prune_issue_device_receipts(
    store: &dyn ProviderStateStore,
    safe_watermark_tick: u64,
    protected_keys: &BTreeSet<String>,
) -> Result<usize, StoreError> {
    if safe_watermark_tick == 0 || protected_keys.len() > MAX_RECEIPTS_PER_SCAN {
        return Err(StoreError::InvalidTransition);
    }
    let rows = store.list_provider_state(RECEIPT_NAMESPACE, MAX_RECEIPTS_PER_SCAN + 1)?;
    if rows.len() > MAX_RECEIPTS_PER_SCAN {
        return Err(StoreError::CapacityExceeded);
    }
    let mut removed = 0;
    for row in rows {
        if row.version != 1 || row.payload.len() > 1024 {
            return Err(StoreError::IncompatibleVersion);
        }
        let receipt: IssueDeviceReceipt =
            serde_json::from_slice(&row.payload).map_err(|_| StoreError::IncompatibleVersion)?;
        if receipt.effect_key != row.key
            || receipt
                .bytes()
                .map_err(|_| StoreError::IncompatibleVersion)?
                != row.payload
        {
            return Err(StoreError::IncompatibleVersion);
        }
        if receipt.retain_until_tick <= safe_watermark_tick && !protected_keys.contains(&row.key) {
            store.delete_provider_state(RECEIPT_NAMESPACE, &row.key, row.version)?;
            removed += 1;
        }
    }
    Ok(removed)
}

fn receipt_response(
    service: &CicsService,
    run: &Run,
    receipt: &IssueDeviceReceipt,
) -> Result<CicsResponse, HostProblem> {
    service.response(
        run,
        CicsDisposition::Complete,
        &receipt.condition,
        receipt.response,
        receipt.response2,
        None,
        None,
        Vec::new(),
    )
}

fn not_allocated() -> HostProblem {
    condition("NOTALLOC", 61, 0)
}

fn condition(name: &str, response: i32, response2: i32) -> HostProblem {
    HostProblem::Condition {
        name: name.into(),
        response,
        response2,
    }
}

fn device_condition(operation: CicsOperation, problem: IssueDeviceProblem) -> HostProblem {
    match (operation, problem) {
        (CicsOperation::IssueLoad, IssueDeviceProblem::NotConfigured) => condition("NONVAL", 9, 0),
        (CicsOperation::IssueLoad, IssueDeviceProblem::Disconnected) => condition("NOSTART", 10, 0),
        (_, IssueDeviceProblem::WrongDevice | IssueDeviceProblem::NotConfigured) => {
            condition("INVREQ", 16, 0)
        }
        (_, IssueDeviceProblem::Disconnected) => condition("TERMERR", 81, 0),
        (_, IssueDeviceProblem::Length) => condition("LENGERR", 22, 0),
        (_, IssueDeviceProblem::StaleOwner) => condition("NOTALLOC", 61, 0),
        (_, IssueDeviceProblem::Malformed | IssueDeviceProblem::Capacity) => {
            HostProblem::InfrastructureFailure
        }
    }
}

/// Resolve a staged PASS only at a known task end. The device disposition and
/// terminal connectivity share one durable CAS so a crash cannot deliver the
/// PASS while leaving the source session usable.
pub(in crate::service) fn finish_task(
    service: &CicsService,
    run: &Run,
    outcome: CicsUnitOfWorkOutcome,
) -> Result<(), HostProblem> {
    let current_session = service
        .lock()?
        .sessions
        .get(&run.session)
        .cloned()
        .ok_or(HostProblem::NotFound)?;
    let Some(terminal) = current_session.input.terminal_id.as_deref() else {
        return Ok(());
    };
    let Some(current) =
        IssueDeviceRecord::load(service.store.as_ref(), terminal).map_err(store_error)?
    else {
        return Ok(());
    };
    if current.state.pass_target.is_none() {
        return Ok(());
    }
    let owner = run.invocation.run_unit_id.as_str();
    let mut next = current.clone();
    let changed = match outcome {
        CicsUnitOfWorkOutcome::Committed => next.complete_pass(owner),
        CicsUnitOfWorkOutcome::RolledBack => next.cancel_pass(owner),
    }
    .map_err(|problem| device_condition(CicsOperation::IssuePass, problem))?;
    if !changed {
        if outcome == CicsUnitOfWorkOutcome::Committed
            && (!current.state.pass_delivered || current_session.connected)
        {
            return Err(HostProblem::UnknownOutcome);
        }
        return Ok(());
    }
    let mut writes = vec![current.mutation(&mut next).map_err(store_error)?];
    let mut disconnected_session = None;
    if outcome == CicsUnitOfWorkOutcome::Committed {
        let mut updated = current_session.clone();
        updated.version = updated
            .version
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        updated.connected = false;
        writes.push(ProviderStateMutation::Put(ProviderStateWrite {
            record: ProviderStateRecord {
                namespace: "cics-session".into(),
                key: run.session.clone(),
                version: updated.version,
                payload: super::super::encode_session(&updated)?,
            },
            expected_version: Some(current_session.version),
        }));
        disconnected_session = Some(updated);
    }
    match service.store.mutate_provider_states_atomic(writes) {
        Ok(()) => {
            if let Some(updated) = disconnected_session {
                service
                    .lock()?
                    .sessions
                    .insert(run.session.clone(), updated);
            }
            Ok(())
        }
        Err(StoreError::Conflict | StoreError::AlreadyExists) => Err(HostProblem::UnknownOutcome),
        Err(error) => Err(super::super::super::mutation_problem(store_error(error))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_store::MemoryStore;

    #[test]
    fn replay_receipt_retains_exact_owner_until_safe_watermark() {
        let store = MemoryStore::new(Default::default());
        let receipt = IssueDeviceReceipt {
            schema_version: 1,
            effect_key: "effect-1".into(),
            owner_execution: "execution".into(),
            owner_run_unit: "run".into(),
            owner_principal: "IBMUSER".into(),
            owner_epoch: 1,
            mutation_sequence: 1,
            request_digest: [7; 32],
            deadline_tick: 100,
            retain_until_tick: 120,
            condition: "NORMAL".into(),
            response: 0,
            response2: 0,
        };
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: RECEIPT_NAMESPACE.into(),
                    key: receipt.effect_key.clone(),
                    version: 1,
                    payload: receipt.bytes().unwrap(),
                },
                None,
            )
            .unwrap();
        assert_eq!(
            prune_issue_device_receipts(&store, 119, &BTreeSet::new()),
            Ok(0)
        );
        assert_eq!(
            prune_issue_device_receipts(&store, 120, &BTreeSet::from(["effect-1".into()])),
            Ok(0)
        );
        assert_eq!(
            prune_issue_device_receipts(&store, 120, &BTreeSet::new()),
            Ok(1)
        );
        assert!(
            store
                .get_provider_state(RECEIPT_NAMESPACE, "effect-1")
                .unwrap()
                .is_none()
        );
    }
}
