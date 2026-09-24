//! Durable definitions and device-specific effects for CIC-905 ISSUE forms.
//!
//! This record owns only physical terminal/3740/3650 facilities. APPC, MRO,
//! and LU6.1 conversation ownership belongs to the shared protocol ledger.

use mainframe_env_store_api::{
    ProviderStateMutation, ProviderStateRecord, ProviderStateStore, ProviderStateWrite, StoreError,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

use super::{CicsService, Run, store_error};
use mainframe_env_host_api::{
    AccessIntent, CicsDisposition, CicsOperation, CicsRequest, CicsResponse, HostProblem,
    HostRequest, canonical_request_digest,
};

pub const ISSUE_DEVICE_NAMESPACE: &str = "cics-issue-device-v1";
const MAX_ROW_BYTES: usize = 64 * 1024;
const MAX_NAMES: usize = 64;
const MAX_PASS_BYTES: usize = 255;
const MAX_PRINT_BYTES: usize = 32_767;
const RECEIPT_NAMESPACE: &str = "cics-issue-device-receipt-v1";
const MAX_RECEIPTS_PER_SCAN: usize = 4096;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum IssueDeviceKind {
    Display3270,
    Printer3270,
    Entry3740,
    Interpreter3650,
    Lu61,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct IssueDeviceDefinition {
    pub terminal: String,
    pub kind: IssueDeviceKind,
    /// The physical control-unit identity required for 3270 buffer copying.
    pub control_unit: Option<String>,
    /// Printer terminals in source-defined preference order.
    pub printers: Vec<String>,
    /// Installed 3650 application names.
    pub programs: Vec<String>,
    /// Installed Communications Server application names allowed for PASS.
    pub applications: Vec<String>,
    /// TYPETERM DISCREQ or RELREQ capability.
    pub disconnect_allowed: bool,
    /// Communications Server AUTH=PASS capability.
    pub pass_allowed: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct IssueDeviceState {
    pub endfile: bool,
    pub endoutput: bool,
    pub eods: bool,
    pub loaded_program: Option<String>,
    pub loaded_converse: bool,
    pub pass_target: Option<String>,
    pub pass_data: Vec<u8>,
    pub pass_logmode: Option<String>,
    pub pass_noquiesce: bool,
    pub disconnected: bool,
    pub print_count: u32,
    pub last_print: Vec<u8>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct IssueDeviceRecord {
    schema_version: u16,
    pub version: u64,
    pub definition: IssueDeviceDefinition,
    pub state: IssueDeviceState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IssueDeviceProblem {
    Malformed,
    WrongDevice,
    NotConfigured,
    Disconnected,
    Length,
    Capacity,
}

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

fn valid_name(name: &str, maximum: usize) -> bool {
    !name.is_empty()
        && name.len() <= maximum
        && name.bytes().all(|byte| {
            byte.is_ascii_uppercase() || byte.is_ascii_digit() || b"$#@".contains(&byte)
        })
}

fn valid_names(names: &[String], maximum: usize) -> bool {
    names.len() <= MAX_NAMES
        && names.iter().all(|name| valid_name(name, maximum))
        && names.iter().collect::<BTreeSet<_>>().len() == names.len()
}

impl IssueDeviceDefinition {
    pub fn validate(&self) -> Result<(), IssueDeviceProblem> {
        if !valid_name(&self.terminal, 4)
            || self
                .control_unit
                .as_ref()
                .is_some_and(|name| !valid_name(name, 8))
            || !valid_names(&self.printers, 4)
            || !valid_names(&self.programs, 8)
            || !valid_names(&self.applications, 8)
            || matches!(
                self.kind,
                IssueDeviceKind::Display3270 | IssueDeviceKind::Printer3270
            ) && self.control_unit.is_none()
            || self.kind != IssueDeviceKind::Display3270 && !self.printers.is_empty()
            || self.kind != IssueDeviceKind::Interpreter3650 && !self.programs.is_empty()
            || self.pass_allowed && self.applications.is_empty()
        {
            return Err(IssueDeviceProblem::Malformed);
        }
        Ok(())
    }
}

impl IssueDeviceRecord {
    pub fn new(definition: IssueDeviceDefinition) -> Result<Self, IssueDeviceProblem> {
        let record = Self {
            schema_version: 1,
            version: 1,
            definition,
            state: IssueDeviceState::default(),
        };
        record.validate()?;
        Ok(record)
    }

    pub fn load(
        store: &dyn ProviderStateStore,
        terminal: &str,
    ) -> Result<Option<Self>, StoreError> {
        let Some(row) = store.get_provider_state(ISSUE_DEVICE_NAMESPACE, terminal)? else {
            return Ok(None);
        };
        if row.version == 0 || row.payload.len() > MAX_ROW_BYTES {
            return Err(StoreError::IncompatibleVersion);
        }
        let record: Self =
            serde_json::from_slice(&row.payload).map_err(|_| StoreError::IncompatibleVersion)?;
        if record.version != row.version
            || record.definition.terminal != terminal
            || record.validate().is_err()
            || record
                .encode()
                .map_err(|_| StoreError::IncompatibleVersion)?
                != row.payload
        {
            return Err(StoreError::IncompatibleVersion);
        }
        Ok(Some(record))
    }

    pub fn persist(
        &self,
        next: &mut Self,
        store: &dyn ProviderStateStore,
    ) -> Result<bool, StoreError> {
        if self.definition != next.definition {
            return Err(StoreError::IncompatibleVersion);
        }
        next.version = self
            .version
            .checked_add(1)
            .ok_or(StoreError::CapacityExceeded)?;
        let payload = next.encode().map_err(|_| StoreError::CapacityExceeded)?;
        match store.put_provider_state(
            ProviderStateRecord {
                namespace: ISSUE_DEVICE_NAMESPACE.into(),
                key: self.definition.terminal.clone(),
                version: next.version,
                payload,
            },
            Some(self.version),
        ) {
            Ok(()) => Ok(true),
            Err(StoreError::Conflict) => Ok(false),
            Err(error) => Err(error),
        }
    }

    /// Prepare an exact-version state mutation for an atomic device/receipt
    /// commit. The caller owns authorization and the source response mapping.
    pub fn mutation(&self, next: &mut Self) -> Result<ProviderStateMutation, StoreError> {
        if self.definition != next.definition {
            return Err(StoreError::IncompatibleVersion);
        }
        next.version = self
            .version
            .checked_add(1)
            .ok_or(StoreError::CapacityExceeded)?;
        let payload = next.encode().map_err(|_| StoreError::CapacityExceeded)?;
        Ok(ProviderStateMutation::Put(ProviderStateWrite {
            record: ProviderStateRecord {
                namespace: ISSUE_DEVICE_NAMESPACE.into(),
                key: self.definition.terminal.clone(),
                version: next.version,
                payload,
            },
            expected_version: Some(self.version),
        }))
    }

    pub fn install(&self, store: &dyn ProviderStateStore) -> Result<(), StoreError> {
        if self.version != 1 {
            return Err(StoreError::IncompatibleVersion);
        }
        let payload = self.encode().map_err(|_| StoreError::CapacityExceeded)?;
        store.put_provider_state(
            ProviderStateRecord {
                namespace: ISSUE_DEVICE_NAMESPACE.into(),
                key: self.definition.terminal.clone(),
                version: self.version,
                payload,
            },
            None,
        )
    }

    pub fn validate(&self) -> Result<(), IssueDeviceProblem> {
        self.definition.validate()?;
        if self.schema_version != 1
            || self.version == 0
            || self.state.pass_data.len() > MAX_PASS_BYTES
            || self.state.last_print.len() > MAX_PRINT_BYTES
            || self.state.loaded_program.as_ref().is_some_and(|name| {
                !self.definition.programs.contains(name)
                    || self.definition.kind != IssueDeviceKind::Interpreter3650
            })
            || self.state.loaded_converse && self.state.loaded_program.is_none()
            || self.state.pass_target.as_ref().is_some_and(|name| {
                !self.definition.pass_allowed || !self.definition.applications.contains(name)
            })
            || self.state.pass_target.is_none()
                && (!self.state.pass_data.is_empty()
                    || self.state.pass_logmode.is_some()
                    || self.state.pass_noquiesce)
            || self
                .state
                .pass_logmode
                .as_ref()
                .is_some_and(|name| !valid_name(name, 8))
            || (self.state.endfile || self.state.endoutput)
                && self.definition.kind != IssueDeviceKind::Entry3740
            || self.state.eods && self.definition.kind != IssueDeviceKind::Interpreter3650
            || self.state.print_count > 0 && self.definition.kind != IssueDeviceKind::Display3270
        {
            return Err(IssueDeviceProblem::Malformed);
        }
        Ok(())
    }

    fn encode(&self) -> Result<Vec<u8>, IssueDeviceProblem> {
        self.validate()?;
        let payload = serde_json::to_vec(self).map_err(|_| IssueDeviceProblem::Malformed)?;
        if payload.len() > MAX_ROW_BYTES {
            return Err(IssueDeviceProblem::Capacity);
        }
        Ok(payload)
    }

    fn active(&self) -> Result<(), IssueDeviceProblem> {
        if self.state.disconnected {
            Err(IssueDeviceProblem::Disconnected)
        } else {
            Ok(())
        }
    }

    pub fn mark_endfile(&mut self, also_endoutput: bool) -> Result<(), IssueDeviceProblem> {
        self.active()?;
        if self.definition.kind != IssueDeviceKind::Entry3740 {
            return Err(IssueDeviceProblem::WrongDevice);
        }
        self.state.endfile = true;
        self.state.endoutput |= also_endoutput;
        Ok(())
    }

    pub fn mark_endoutput(&mut self, also_endfile: bool) -> Result<(), IssueDeviceProblem> {
        self.active()?;
        if self.definition.kind != IssueDeviceKind::Entry3740 {
            return Err(IssueDeviceProblem::WrongDevice);
        }
        self.state.endoutput = true;
        self.state.endfile |= also_endfile;
        Ok(())
    }

    pub fn mark_eods(&mut self) -> Result<(), IssueDeviceProblem> {
        self.active()?;
        if self.definition.kind != IssueDeviceKind::Interpreter3650 {
            return Err(IssueDeviceProblem::WrongDevice);
        }
        self.state.eods = true;
        Ok(())
    }

    pub fn load_program(
        &mut self,
        program: &str,
        converse: bool,
    ) -> Result<(), IssueDeviceProblem> {
        self.active()?;
        if self.definition.kind != IssueDeviceKind::Interpreter3650 {
            return Err(IssueDeviceProblem::WrongDevice);
        }
        if !self.definition.programs.iter().any(|name| name == program) {
            return Err(IssueDeviceProblem::NotConfigured);
        }
        self.state.loaded_program = Some(program.into());
        self.state.loaded_converse = converse;
        Ok(())
    }

    pub fn prepare_pass(
        &mut self,
        application: &str,
        data: &[u8],
        logmode: Option<&str>,
        noquiesce: bool,
    ) -> Result<(), IssueDeviceProblem> {
        self.active()?;
        if !self.definition.pass_allowed || !self.definition.disconnect_allowed {
            return Err(IssueDeviceProblem::NotConfigured);
        }
        if !self
            .definition
            .applications
            .iter()
            .any(|name| name == application)
        {
            return Err(IssueDeviceProblem::NotConfigured);
        }
        if data.len() > MAX_PASS_BYTES || logmode.is_some_and(|name| !valid_name(name, 8)) {
            return Err(IssueDeviceProblem::Length);
        }
        self.state.pass_target = Some(application.into());
        self.state.pass_data = data.to_vec();
        self.state.pass_logmode = logmode.map(str::to_owned);
        self.state.pass_noquiesce = noquiesce;
        Ok(())
    }

    pub fn disconnect(&mut self) -> Result<(), IssueDeviceProblem> {
        self.active()?;
        if !self.definition.disconnect_allowed {
            return Err(IssueDeviceProblem::NotConfigured);
        }
        self.state.disconnected = true;
        Ok(())
    }

    pub fn record_print(&mut self, bytes: &[u8]) -> Result<(), IssueDeviceProblem> {
        self.active()?;
        if self.definition.kind != IssueDeviceKind::Display3270
            || self.definition.printers.is_empty()
        {
            return Err(IssueDeviceProblem::NotConfigured);
        }
        if bytes.len() > MAX_PRINT_BYTES {
            return Err(IssueDeviceProblem::Length);
        }
        self.state.print_count = self
            .state
            .print_count
            .checked_add(1)
            .ok_or(IssueDeviceProblem::Capacity)?;
        self.state.last_print = bytes.to_vec();
        Ok(())
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
    let (terminal, connected) = {
        let state = service.lock()?;
        let session = state
            .sessions
            .get(&run.session)
            .ok_or(HostProblem::NotFound)?;
        (session.input.terminal_id.clone(), session.connected)
    };
    let terminal = terminal.ok_or_else(not_allocated)?;
    if !connected {
        return Err(not_allocated());
    }
    if matches!(
        request.operation,
        CicsOperation::IssueEndfile | CicsOperation::IssueEndoutput | CicsOperation::IssueEods
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
    let state_write = current.mutation(&mut next).map_err(store_error)?;
    let receipt_write = ProviderStateMutation::Put(ProviderStateWrite {
        record: ProviderStateRecord {
            namespace: RECEIPT_NAMESPACE.into(),
            key: effect_key.into(),
            version: 1,
            payload: receipt.bytes()?,
        },
        expected_version: None,
    });
    match service
        .store
        .mutate_provider_states_atomic(vec![state_write, receipt_write])
    {
        Ok(()) => receipt_response(service, run, &receipt),
        Err(StoreError::Conflict | StoreError::AlreadyExists) => {
            let saved = load_receipt(service, effect_key, run, mutation.sequence, digest)?
                .ok_or(HostProblem::IdempotencyConflict)?;
            receipt_response(service, run, &saved)
        }
        Err(error) => Err(super::super::mutation_problem(store_error(error))),
    }
}

fn validate_request(request: &CicsRequest) -> Result<(), HostProblem> {
    if !matches!(
        request.operation,
        CicsOperation::IssueEndfile
            | CicsOperation::IssueEndoutput
            | CicsOperation::IssueEods
            | CicsOperation::IssueLoad
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
            _ => false,
        };
        if !valid {
            return Err(HostProblem::Malformed);
        }
    }
    if request.operation == CicsOperation::IssueLoad && !request.arguments.contains_key("PROGRAM") {
        return Err(HostProblem::Malformed);
    }
    Ok(())
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
        (_, IssueDeviceProblem::Malformed | IssueDeviceProblem::Capacity) => {
            HostProblem::InfrastructureFailure
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_store::{MemoryStore, SqliteStateStore};

    fn definition(terminal: &str, kind: IssueDeviceKind) -> IssueDeviceDefinition {
        IssueDeviceDefinition {
            terminal: terminal.into(),
            kind,
            control_unit: None,
            printers: vec![],
            programs: vec![],
            applications: vec![],
            disconnect_allowed: true,
            pass_allowed: false,
        }
    }

    #[test]
    fn end_markers_are_device_specific_and_stale_cas_cannot_replace_them() {
        let store = MemoryStore::new(Default::default());
        let initial =
            IssueDeviceRecord::new(definition("T001", IssueDeviceKind::Entry3740)).unwrap();
        initial.install(&store).unwrap();
        let current = IssueDeviceRecord::load(&store, "T001").unwrap().unwrap();
        let mut next = current.clone();
        next.mark_endfile(true).unwrap();
        assert!(current.persist(&mut next, &store).unwrap());
        assert!(next.state.endfile && next.state.endoutput);
        let mut stale = current.clone();
        stale.mark_endoutput(false).unwrap();
        assert!(!current.persist(&mut stale, &store).unwrap());
        assert_eq!(
            IssueDeviceRecord::load(&store, "T001")
                .unwrap()
                .unwrap()
                .state,
            next.state
        );

        let mut wrong =
            IssueDeviceRecord::new(definition("T002", IssueDeviceKind::Entry3740)).unwrap();
        assert_eq!(wrong.mark_eods(), Err(IssueDeviceProblem::WrongDevice));
        assert!(!wrong.state.eods);
    }

    #[test]
    fn sqlite_reopen_preserves_load_and_pass_with_exact_bounds() {
        let root = std::env::temp_dir().join(format!(
            "mainframe-env-cics-issue-device-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let url = format!("sqlite://{}?mode=rwc", root.join("state.db").display());
        let first = SqliteStateStore::open(&url, MAX_ROW_BYTES, 65_536).unwrap();
        let mut definition = definition("T003", IssueDeviceKind::Interpreter3650);
        definition.programs.push("PROG1".into());
        definition.applications.push("APPL1".into());
        definition.pass_allowed = true;
        let initial = IssueDeviceRecord::new(definition).unwrap();
        initial.install(&first).unwrap();
        let mut next = initial.clone();
        next.load_program("PROG1", true).unwrap();
        next.mark_eods().unwrap();
        assert_eq!(
            next.prepare_pass("APPL1", &[1; MAX_PASS_BYTES + 1], None, false),
            Err(IssueDeviceProblem::Length)
        );
        assert!(next.state.pass_target.is_none());
        next.prepare_pass("APPL1", &[2; MAX_PASS_BYTES], Some("MODE1"), true)
            .unwrap();
        assert!(initial.persist(&mut next, &first).unwrap());
        drop(first);
        let reopened = SqliteStateStore::open(&url, MAX_ROW_BYTES, 65_536).unwrap();
        let record = IssueDeviceRecord::load(&reopened, "T003").unwrap().unwrap();
        assert_eq!(record.state.loaded_program.as_deref(), Some("PROG1"));
        assert!(record.state.loaded_converse && record.state.eods);
        assert_eq!(record.state.pass_data, vec![2; MAX_PASS_BYTES]);
        assert_eq!(record.state.pass_logmode.as_deref(), Some("MODE1"));
        drop(reopened);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn printer_requires_a_configured_peer_and_keeps_bounded_image() {
        let mut definition = definition("T004", IssueDeviceKind::Display3270);
        definition.control_unit = Some("CU1".into());
        let mut device = IssueDeviceRecord::new(definition.clone()).unwrap();
        assert_eq!(
            device.record_print(b"SCREEN"),
            Err(IssueDeviceProblem::NotConfigured)
        );
        definition.printers.push("P001".into());
        device = IssueDeviceRecord::new(definition).unwrap();
        device.record_print(b"SCREEN").unwrap();
        assert_eq!(device.state.print_count, 1);
        assert_eq!(device.state.last_print, b"SCREEN");
        assert_eq!(
            device.record_print(&vec![0; MAX_PRINT_BYTES + 1]),
            Err(IssueDeviceProblem::Length)
        );
        assert_eq!(device.state.print_count, 1);
    }

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
