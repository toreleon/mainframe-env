use crate::FixedValue;
use mainframe_env_diagnostics::{
    DiagnosticCode, DiagnosticLimits, ExecutionProblem, FailureCategory, Phase,
};
use mainframe_env_encoding::CodePage;
use mainframe_env_execution_api::{
    Abend, BoundedPayload, Completion, Condition, IdempotencyKey, Invocation, InvocationLimits,
    Machine, MachineDrive, MachineResume, Quantum, Selector, Suspension, Transfer,
};
use mainframe_env_host_api::{
    CicsConditionPolicy, CicsDisposition, CicsOperation, CicsRequest, CicsResponse, DatasetName,
    DatasetRequest, Db2HostVariable, Db2Operation, Db2Request, EffectRequest, EffectResult,
    HostLimits, HostProblem, HostRequest, HostResult, ImsOperation, ImsQualifier, ImsRequest,
    MqOperation, MqRequest, Mutation, ProgramName, ProgramRequest, TerminalRequest,
};
use mainframe_env_ir::{
    Attribute, CodecLimits, Module, Operation, OperationIdentity, StorageId, decode_binary,
};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

const NAMESPACE: &str = "mainframe.core.cobol";
pub const SUPPORTED_LAYOUT_CATEGORIES: &[&str] = &[
    "alphabetic",
    "alphanumeric",
    "alphanumeric_edited",
    "binary",
    "condition",
    "dbcs",
    "function_pointer",
    "group",
    "index",
    "national",
    "national_edited",
    "national_group",
    "numeric_display",
    "numeric_edited",
    "object_reference",
    "packed_decimal",
    "pointer",
    "pointer_32",
    "procedure_pointer",
    "rename",
    "utf8",
    "utf8_group",
];

#[derive(Clone, Debug, Eq, PartialEq)]
struct StorageView {
    base: usize,
    offset: usize,
    length: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LayoutCategory {
    Alphabetic,
    Alphanumeric,
    AlphanumericEdited,
    Dbcs,
    National,
    NationalEdited,
    Utf8,
    NumericDisplay,
    NumericEdited,
    PackedDecimal,
    Binary,
    FloatShort,
    FloatLong,
    Index,
    Pointer,
    Pointer32,
    ProcedurePointer,
    FunctionPointer,
    ObjectReference,
    Group,
    NationalGroup,
    Utf8Group,
    Condition,
    Rename,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct LayoutMetadata {
    name: String,
    simple_name: String,
    category: LayoutCategory,
    picture: String,
    digits: usize,
    scale: u32,
    signed: bool,
    sign_separate: bool,
    justified_right: bool,
    linkage: bool,
    offset: usize,
    length: usize,
    element_length: usize,
    occurs: usize,
    parent: Option<String>,
    condition_values: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ResolvedReference {
    layout: LayoutMetadata,
    offset: usize,
    length: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct FileMetadata {
    assignment: String,
    record_name: Option<String>,
    organization: String,
    access_mode: String,
    record_key: Option<String>,
    alternate_record_keys: Vec<String>,
    relative_key: Option<String>,
    file_status: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Decimal {
    coefficient: i128,
    scale: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum CobolValue {
    Bytes(Vec<u8>),
    Decimal(Decimal),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct ConditionStatus {
    arithmetic_size_error: bool,
    accept_exception: bool,
    call_exception: bool,
    string_overflow: bool,
    unstring_overflow: bool,
    json_exception: bool,
    xml_exception: bool,
}

impl ConditionStatus {
    fn bits(self) -> u8 {
        u8::from(self.arithmetic_size_error)
            | u8::from(self.accept_exception) << 1
            | u8::from(self.call_exception) << 2
            | u8::from(self.string_overflow) << 3
            | u8::from(self.unstring_overflow) << 4
            | u8::from(self.json_exception) << 5
            | u8::from(self.xml_exception) << 6
    }

    fn from_bits(bits: u8) -> Option<Self> {
        if bits & 0x80 != 0 {
            return None;
        }
        Some(Self {
            arithmetic_size_error: bits & 1 != 0,
            accept_exception: bits & (1 << 1) != 0,
            call_exception: bits & (1 << 2) != 0,
            string_overflow: bits & (1 << 3) != 0,
            unstring_overflow: bits & (1 << 4) != 0,
            json_exception: bits & (1 << 5) != 0,
            xml_exception: bits & (1 << 6) != 0,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PendingKind {
    Accept {
        target: String,
        handles_exception: bool,
    },
    DatasetRead {
        target: Option<String>,
        status: Option<String>,
        ccsid: Option<u16>,
    },
    DatasetStatus {
        status: Option<String>,
        cursor: Option<DatasetCursorAction>,
    },
    ProgramCall {
        targets: Vec<String>,
        handles_exception: bool,
    },
    Db2 {
        targets: Vec<String>,
    },
    Ims {
        target: Option<String>,
    },
    Mq {
        handle: Option<String>,
        descriptor: Option<String>,
        buffer: Option<String>,
        data_length: Option<String>,
        completion_code: Option<String>,
        reason_code: Option<String>,
    },
    Cics {
        operation: CicsOperation,
        argument_summary: String,
        into: Option<String>,
        outputs: BTreeMap<String, String>,
        response: Option<String>,
        response2: Option<String>,
        no_handle: bool,
    },
    Ignore,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum DatasetCursorAction {
    Start(String),
    End(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Pending {
    sequence: u64,
    kind: PendingKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MachineSnapshot {
    pub schema_version: u32,
    pub program_counter: usize,
    pub effect_sequence: u64,
    pub executed_steps: u64,
    pub output: Vec<u8>,
    pub base_storage: Vec<Vec<u8>>,
    pub perform_stack: Vec<usize>,
    pub altered_targets: BTreeMap<String, String>,
    pub loop_reentry: BTreeSet<usize>,
    pub loop_counts: BTreeMap<usize, i128>,
    pub last_file_status: String,
    pub dataset_cursors: BTreeMap<String, String>,
    pub condition_statuses: u8,
}

pub struct ReferenceMachine {
    invocation: Invocation,
    operations: Vec<Operation>,
    bases: Vec<Vec<u8>>,
    views: BTreeMap<String, StorageView>,
    views_by_id: BTreeMap<StorageId, StorageView>,
    entry_initials: BTreeMap<StorageId, Vec<u8>>,
    implicit: BTreeMap<String, CobolValue>,
    layouts: BTreeMap<String, LayoutMetadata>,
    simple_layouts: BTreeMap<String, Vec<String>>,
    files: BTreeMap<String, FileMetadata>,
    labels: BTreeMap<String, usize>,
    control_nodes: BTreeMap<usize, usize>,
    loop_reentry: BTreeSet<usize>,
    loop_counts: BTreeMap<usize, i128>,
    altered: BTreeMap<String, String>,
    last_file_status: String,
    condition_status: ConditionStatus,
    dataset_cursors: BTreeMap<String, String>,
    sql_cursors: BTreeMap<String, Vec<String>>,
    pc: usize,
    output: Vec<u8>,
    effect_sequence: u64,
    executed_steps: u64,
    pending: Option<Pending>,
    perform_stack: Vec<usize>,
    deferred_drive: Option<MachineDrive<EffectRequest>>,
}

impl ReferenceMachine {
    pub fn from_binary(
        binary: &[u8],
        invocation: Invocation,
        codec_limits: CodecLimits,
    ) -> Result<Self, MachineProblem> {
        let module = decode_binary(binary, codec_limits)
            .map_err(|problem| MachineProblem::InvalidArtifact(problem.to_string()))?;
        validate_module(&module)?;
        let (bases, views, views_by_id) = storage(&module, invocation.limits.max_storage_bytes)?;
        let mut entry_initials = BTreeMap::new();
        let entry_commarea_len = invocation
            .bindings
            .get("cics.commarea")
            .map(|value| value.bytes().len());
        let entry_aid = invocation
            .bindings
            .get("cics.aid")
            .and_then(|value| value.bytes().first().copied());
        let entry_transaction = invocation
            .bindings
            .get("cics.transaction")
            .map(|value| value.bytes().to_vec());
        let mut implicit = BTreeMap::from([
            (
                "EIBRESP".into(),
                CobolValue::Decimal(Decimal {
                    coefficient: 0,
                    scale: 0,
                }),
            ),
            (
                "EIBRESP2".into(),
                CobolValue::Decimal(Decimal {
                    coefficient: 0,
                    scale: 0,
                }),
            ),
            (
                "EIBCALEN".into(),
                CobolValue::Decimal(Decimal {
                    coefficient: i128::try_from(entry_commarea_len.unwrap_or(0))
                        .map_err(|_| MachineProblem::InvalidOperation)?,
                    scale: 0,
                }),
            ),
            (
                "EIBAID".into(),
                CobolValue::Bytes(vec![entry_aid.unwrap_or(0)]),
            ),
            (
                "EIBTRNID".into(),
                CobolValue::Bytes(entry_transaction.clone().unwrap_or_default()),
            ),
            (
                "RETURN-CODE".into(),
                CobolValue::Decimal(Decimal {
                    coefficient: 0,
                    scale: 0,
                }),
            ),
            (
                "SQLCODE".into(),
                CobolValue::Decimal(Decimal {
                    coefficient: 0,
                    scale: 0,
                }),
            ),
            ("SQLSTATE".into(), CobolValue::Bytes(b"00000".to_vec())),
            ("DIBSTAT".into(), CobolValue::Bytes(b"  ".to_vec())),
        ]);
        if let Some(commarea) = invocation.bindings.get("cics.commarea")
            && let Some(storage) = module
                .storage()
                .iter()
                .find(|storage| storage.name.eq_ignore_ascii_case("DFHCOMMAREA"))
        {
            let mut value = vec![b' '; storage.size as usize];
            let copied = value.len().min(commarea.bytes().len());
            value[..copied].copy_from_slice(&commarea.bytes()[..copied]);
            entry_initials.insert(storage.id, value);
        }
        let operations: Vec<_> = module
            .regions()
            .iter()
            .flat_map(|region| &region.blocks)
            .flat_map(|block| &block.operations)
            .cloned()
            .collect();
        let (layouts, simple_layouts) = layout_metadata(&operations)?;
        if let Some(call) = invocation.bindings.get("cobol.call.arguments") {
            let values = decode_call_arguments(call)?;
            let mut linkage = layouts
                .values()
                .filter(|layout| layout.linkage && layout.parent.is_none() && layout.length > 0)
                .collect::<Vec<_>>();
            linkage.sort_by_key(|layout| layout.offset);
            if linkage.len() < values.len() {
                return Err(MachineProblem::InvalidOperation);
            }
            for (layout, value) in linkage.into_iter().zip(values) {
                let storage = module
                    .storage()
                    .iter()
                    .find(|storage| storage.name.eq_ignore_ascii_case(&layout.name))
                    .ok_or(MachineProblem::UnknownStorage)?;
                entry_initials.insert(
                    storage.id,
                    FixedValue::fit(&value, layout.length, false)
                        .bytes()
                        .to_vec(),
                );
            }
        }
        let files = file_metadata(&operations)?;
        let labels = operations
            .iter()
            .enumerate()
            .filter_map(|(index, operation)| {
                (operation.identity.name() == "label")
                    .then(|| {
                        arguments(operation)
                            .into_iter()
                            .next()
                            .map(|name| (normalize(&name), index))
                    })
                    .flatten()
            })
            .collect();
        let control_nodes = operations
            .iter()
            .enumerate()
            .filter_map(|(index, operation)| {
                operation
                    .attributes
                    .get("control_node")
                    .and_then(|attribute| match attribute {
                        Attribute::Integer(node) => usize::try_from(*node).ok(),
                        _ => None,
                    })
                    .map(|node| (node, index))
            })
            .collect();
        let mut machine = Self {
            invocation,
            operations,
            bases,
            views,
            views_by_id,
            entry_initials,
            implicit: std::mem::take(&mut implicit),
            layouts,
            simple_layouts,
            files,
            labels,
            control_nodes,
            loop_reentry: BTreeSet::new(),
            loop_counts: BTreeMap::new(),
            altered: BTreeMap::new(),
            last_file_status: "00".into(),
            condition_status: ConditionStatus::default(),
            dataset_cursors: BTreeMap::new(),
            sql_cursors: BTreeMap::new(),
            pc: 0,
            output: Vec::new(),
            effect_sequence: 0,
            executed_steps: 0,
            pending: None,
            perform_stack: Vec::new(),
            deferred_drive: None,
        };
        if let Some(entry_commarea_len) = entry_commarea_len
            && machine.layout("EIBCALEN").is_some()
        {
            machine.write_decimal(
                "EIBCALEN",
                Decimal {
                    coefficient: i128::try_from(entry_commarea_len)
                        .map_err(|_| MachineProblem::InvalidOperation)?,
                    scale: 0,
                },
            )?;
        }
        if let Some(aid) = entry_aid
            && machine.layout("EIBAID").is_some()
        {
            machine.write("EIBAID", &[aid])?;
        }
        if let Some(transaction) = entry_transaction
            && machine.layout("EIBTRNID").is_some()
        {
            machine.write("EIBTRNID", &transaction)?;
        }
        Ok(machine)
    }

    #[must_use]
    pub fn snapshot(&self) -> MachineSnapshot {
        MachineSnapshot {
            schema_version: 7,
            program_counter: self.pc,
            effect_sequence: self.effect_sequence,
            executed_steps: self.executed_steps,
            output: self.output.clone(),
            base_storage: self.bases.clone(),
            perform_stack: self.perform_stack.clone(),
            altered_targets: self.altered.clone(),
            loop_reentry: self.loop_reentry.clone(),
            loop_counts: self.loop_counts.clone(),
            last_file_status: self.last_file_status.clone(),
            dataset_cursors: self.dataset_cursors.clone(),
            condition_statuses: self.condition_status.bits(),
        }
    }

    pub fn restore(&mut self, snapshot: MachineSnapshot) -> Result<(), MachineProblem> {
        if !matches!(snapshot.schema_version, 1..=7)
            || snapshot.program_counter > self.operations.len()
            || snapshot.base_storage.iter().map(Vec::len).sum::<usize>()
                > self.invocation.limits.max_storage_bytes as usize
            || snapshot.last_file_status.len() != 2
        {
            return Err(MachineProblem::IncompatibleSnapshot);
        }
        self.pc = snapshot.program_counter;
        self.effect_sequence = snapshot.effect_sequence;
        self.executed_steps = if snapshot.schema_version < 6 {
            0
        } else {
            snapshot.executed_steps
        };
        self.output = snapshot.output;
        self.bases = snapshot.base_storage;
        self.perform_stack = if snapshot.schema_version < 3 {
            snapshot
                .perform_stack
                .into_iter()
                .map(|return_pc| return_pc.saturating_sub(1))
                .collect()
        } else {
            snapshot.perform_stack
        };
        self.altered = snapshot.altered_targets;
        self.loop_reentry = snapshot.loop_reentry;
        self.loop_counts = snapshot.loop_counts;
        self.last_file_status = if snapshot.schema_version < 4 {
            "00".into()
        } else {
            snapshot.last_file_status
        };
        self.dataset_cursors = if snapshot.schema_version < 5 {
            BTreeMap::new()
        } else {
            snapshot.dataset_cursors
        };
        self.condition_status = if snapshot.schema_version < 7 {
            ConditionStatus::default()
        } else {
            ConditionStatus::from_bits(snapshot.condition_statuses)
                .ok_or(MachineProblem::IncompatibleSnapshot)?
        };
        self.pending = None;
        self.deferred_drive = None;
        Ok(())
    }

    pub fn restore_checkpoint(&mut self, payload: &BoundedPayload) -> Result<(), MachineProblem> {
        if !matches!(
            payload.schema(),
            "mainframe-env.reference-machine-checkpoint@1"
                | "mainframe-env.reference-machine-checkpoint@2"
                | "mainframe-env.reference-machine-checkpoint@3"
                | "mainframe-env.reference-machine-checkpoint@4"
                | "mainframe-env.reference-machine-checkpoint@5"
                | "mainframe-env.reference-machine-checkpoint@6"
                | "mainframe-env.reference-machine-checkpoint@7"
        ) {
            return Err(MachineProblem::IncompatibleSnapshot);
        }
        let snapshot = decode_snapshot(
            payload.bytes(),
            self.invocation.limits.max_storage_bytes as usize,
            self.invocation.limits.max_output_bytes as usize,
            self.invocation.limits.max_frames as usize,
        )?;
        self.restore(snapshot)?;
        self.reapply_entry_context()
    }

    #[must_use]
    pub fn output(&self) -> &[u8] {
        &self.output
    }
    #[must_use]
    pub fn dataset_cursors(&self) -> &BTreeMap<String, String> {
        &self.dataset_cursors
    }
    pub fn install_dataset_cursors(
        &mut self,
        cursors: BTreeMap<String, String>,
    ) -> Result<(), MachineProblem> {
        if cursors.len() > self.invocation.limits.max_frames as usize
            || cursors.iter().any(|(dataset, cursor)| {
                DatasetName::new(dataset, 128).is_err() || cursor.is_empty() || cursor.len() > 128
            })
        {
            return Err(MachineProblem::ResourceExhausted);
        }
        self.dataset_cursors = cursors;
        Ok(())
    }
    #[must_use]
    pub fn variable(&self, name: &str) -> Option<FixedValue> {
        self.read(name).ok().map(FixedValue::new)
    }

    pub fn linkage_values(&self) -> Result<Vec<Vec<u8>>, MachineProblem> {
        let mut linkage = self
            .layouts
            .values()
            .filter(|layout| layout.linkage && layout.parent.is_none() && layout.length > 0)
            .collect::<Vec<_>>();
        linkage.sort_by_key(|layout| layout.offset);
        linkage
            .into_iter()
            .map(|layout| self.read(&layout.name))
            .collect()
    }

    #[must_use]
    pub fn position_summary(&self) -> String {
        self.operations.get(self.pc).map_or_else(
            || format!("pc={} completed", self.pc),
            |operation| {
                format!(
                    "pc={} {} {:?} role={:?} scope={:?} text={:?}",
                    self.pc,
                    operation.identity.name(),
                    arguments(operation),
                    optional_text_attribute(operation, "control_role"),
                    optional_text_attribute(operation, "control_scope"),
                    optional_text_attribute(operation, "control_text")
                )
            },
        )
    }

    fn resume_host(&mut self, result: EffectResult) -> Result<(), MachineProblem> {
        let pending = self
            .pending
            .take()
            .ok_or(MachineProblem::UnexpectedResume)?;
        result
            .validate(pending.sequence, HostLimits::default())
            .map_err(MachineProblem::Host)?;
        if let Err(HostProblem::Condition { name, response, .. }) = &result.outcome
            && let Some(status_target) = match &pending.kind {
                PendingKind::DatasetRead { status, .. }
                | PendingKind::DatasetStatus { status, .. } => Some(status.clone()),
                _ => None,
            }
        {
            let status = dataset_file_status(name, *response);
            self.last_file_status.clone_from(&status);
            if let Some(target) = status_target {
                self.write(&target, status.as_bytes())?;
            }
            return Ok(());
        }
        if result.outcome.is_err() {
            match &pending.kind {
                PendingKind::Accept {
                    handles_exception: true,
                    ..
                } => {
                    self.condition_status.accept_exception = true;
                    return Ok(());
                }
                PendingKind::ProgramCall {
                    handles_exception: true,
                    ..
                } => {
                    self.condition_status.call_exception = true;
                    return Ok(());
                }
                _ => {}
            }
        }
        let outcome = result
            .outcome
            .map_err(|problem| match (&pending.kind, problem) {
                (
                    PendingKind::Cics {
                        operation,
                        argument_summary,
                        ..
                    },
                    problem,
                ) => MachineProblem::InvalidArtifact(format!(
                    "CICS {operation:?} {argument_summary} host call failed: {problem:?}"
                )),
                (_, problem) => MachineProblem::Host(problem),
            })?;
        match (pending.kind, outcome) {
            (PendingKind::Accept { target, .. }, HostResult::Terminal(payload)) => {
                self.condition_status.accept_exception = false;
                self.write(&target, payload.bytes())?
            }
            (
                PendingKind::DatasetRead {
                    target: Some(target),
                    status,
                    ccsid,
                },
                HostResult::Dataset(mainframe_env_host_api::DatasetResult::Records {
                    records, ..
                }),
            ) => {
                self.last_file_status = if records.is_empty() {
                    "10".into()
                } else {
                    "00".into()
                };
                if let Some(record) = records.first() {
                    self.write(&target, &decode_dataset_record(ccsid, record)?)?;
                }
                if let Some(status) = status {
                    self.write(&status, if records.is_empty() { b"10" } else { b"00" })?;
                }
            }
            (
                PendingKind::DatasetRead {
                    target,
                    status,
                    ccsid,
                },
                HostResult::Dataset(mainframe_env_host_api::DatasetResult::Browse {
                    record, ..
                }),
            ) => {
                self.last_file_status = if record.is_some() {
                    "00".into()
                } else {
                    "10".into()
                };
                if let Some((target, record)) = target.zip(record.as_ref()) {
                    self.write(&target, &decode_dataset_record(ccsid, record)?)?;
                }
                if let Some(status) = status {
                    self.write(&status, if record.is_some() { b"00" } else { b"10" })?;
                }
            }
            (
                PendingKind::DatasetStatus {
                    status,
                    cursor: Some(DatasetCursorAction::Start(dataset)),
                },
                HostResult::Dataset(mainframe_env_host_api::DatasetResult::Browse {
                    cursor, ..
                }),
            ) => {
                self.dataset_cursors.insert(dataset, cursor);
                self.last_file_status = "00".into();
                if let Some(status) = status {
                    self.write(&status, b"00")?;
                }
            }
            (
                PendingKind::DatasetStatus {
                    status,
                    cursor: Some(DatasetCursorAction::End(dataset)),
                },
                HostResult::Dataset(mainframe_env_host_api::DatasetResult::Browse { .. }),
            ) => {
                self.dataset_cursors.remove(&dataset);
                self.last_file_status = "00".into();
                if let Some(status) = status {
                    self.write(&status, b"00")?;
                }
            }
            (
                PendingKind::DatasetRead { status, .. } | PendingKind::DatasetStatus { status, .. },
                HostResult::Dataset(mainframe_env_host_api::DatasetResult::Condition {
                    status: condition_status,
                    ..
                }),
            ) => {
                self.last_file_status.clone_from(&condition_status);
                if let Some(status) = status {
                    self.write(&status, condition_status.as_bytes())?;
                }
            }
            (PendingKind::DatasetRead { status, .. }, HostResult::Dataset(_))
            | (
                PendingKind::DatasetStatus {
                    status,
                    cursor: None,
                },
                HostResult::Dataset(_),
            ) => {
                self.last_file_status = "00".into();
                if let Some(status) = status {
                    self.write(&status, b"00")?;
                }
            }
            (
                PendingKind::DatasetStatus {
                    cursor: Some(_), ..
                },
                HostResult::Dataset(_),
            ) => {
                return Err(MachineProblem::UnexpectedHostResult);
            }
            (
                PendingKind::ProgramCall {
                    targets,
                    handles_exception,
                },
                HostResult::Program(payload),
            ) => {
                if payload.schema() == "mainframe-env.program.abend@1" {
                    if handles_exception {
                        self.condition_status.call_exception = true;
                        return Ok(());
                    }
                    let code = String::from_utf8(payload.bytes().to_vec())
                        .map_err(|_| MachineProblem::UnexpectedHostResult)?;
                    self.deferred_drive = Some(MachineDrive::Abend(Abend {
                        code,
                        reason: Some("compatible CEE3ABD service".into()),
                    }));
                    return Ok(());
                }
                self.condition_status.call_exception = false;
                let values = decode_call_values(&payload)?;
                if values.len() != targets.len() {
                    return Err(MachineProblem::UnexpectedHostResult);
                }
                for (target, value) in targets.iter().zip(values) {
                    self.write(target, &value)?;
                }
            }
            (PendingKind::Db2 { targets }, HostResult::Db2(result)) => {
                self.write_decimal(
                    "SQLCODE",
                    Decimal {
                        coefficient: i128::from(result.sqlcode),
                        scale: 0,
                    },
                )?;
                self.write("SQLSTATE", result.sqlstate.as_bytes())?;
                if self.layout("SQLERRML").is_some() {
                    self.write_decimal(
                        "SQLERRML",
                        Decimal {
                            coefficient: i128::try_from(result.message.len())
                                .map_err(|_| MachineProblem::ResourceExhausted)?,
                            scale: 0,
                        },
                    )?;
                }
                if self.layout("SQLERRMC").is_some() {
                    self.write("SQLERRMC", result.message.as_bytes())?;
                }
                if let Some(row) = result.rows.first() {
                    if row.columns.len() != targets.len() {
                        return Err(MachineProblem::UnexpectedHostResult);
                    }
                    for (target, value) in targets.iter().zip(&row.columns) {
                        self.write_value(target, &CobolValue::Bytes(value.clone()))?;
                    }
                }
            }
            (PendingKind::Ims { target }, HostResult::Ims(result)) => {
                self.write("DIBSTAT", result.status.as_bytes())?;
                if let Some((target, segment)) = target.zip(result.segments.first()) {
                    self.write(&target, &segment.data)?;
                }
            }
            (
                PendingKind::Mq {
                    handle,
                    descriptor,
                    buffer,
                    data_length,
                    completion_code,
                    reason_code,
                },
                HostResult::Mq(result),
            ) => {
                if let Some((target, handle)) = handle.zip(result.handle) {
                    self.write_decimal(
                        &target,
                        Decimal {
                            coefficient: i128::from(handle),
                            scale: 0,
                        },
                    )?;
                }
                if let Some(target) = buffer {
                    self.write(&target, &result.message)?;
                }
                if let Some(target) = data_length {
                    self.write_decimal(
                        &target,
                        Decimal {
                            coefficient: i128::try_from(result.message.len())
                                .map_err(|_| MachineProblem::ResourceExhausted)?,
                            scale: 0,
                        },
                    )?;
                }
                if let Some(target) = completion_code {
                    self.write_decimal(
                        &target,
                        Decimal {
                            coefficient: i128::from(result.completion_code),
                            scale: 0,
                        },
                    )?;
                }
                if let Some(target) = reason_code {
                    self.write_decimal(
                        &target,
                        Decimal {
                            coefficient: i128::from(result.reason_code),
                            scale: 0,
                        },
                    )?;
                }
                if let Some(descriptor) = descriptor {
                    if let Some((target, value)) = self
                        .mq_descriptor_field(&descriptor, "MQMD-MSGID")
                        .zip(result.message_id.as_ref())
                    {
                        self.write(&target, value)?;
                    }
                    if let Some((target, value)) = self
                        .mq_descriptor_field(&descriptor, "MQMD-CORRELID")
                        .zip(result.correlation_id.as_ref())
                    {
                        self.write(&target, value)?;
                    }
                }
            }
            (
                PendingKind::Cics {
                    operation,
                    argument_summary: _,
                    into,
                    outputs,
                    response: response_target,
                    response2: response2_target,
                    no_handle,
                },
                HostResult::Cics(response),
            ) => {
                let responded = response_target.is_some() || no_handle;
                if let Some(target) = response_target {
                    self.write_cics_value(
                        &target,
                        &CobolValue::Decimal(Decimal {
                            coefficient: i128::from(response.response),
                            scale: 0,
                        }),
                    )?;
                }
                if let Some(target) = response2_target {
                    self.write_cics_value(
                        &target,
                        &CobolValue::Decimal(Decimal {
                            coefficient: i128::from(response.response2),
                            scale: 0,
                        }),
                    )?;
                }
                if let Some(target) = into
                    && matches!(
                        response.payload.schema(),
                        "mainframe-env.cics.into@1" | "mainframe-env.cics.payload@1"
                    )
                {
                    self.write_cics_value(
                        &target,
                        &CobolValue::Bytes(response.payload.bytes().to_vec()),
                    )?;
                }
                for (name, value) in &response.outputs {
                    if let Some(field) = name.strip_prefix("BMS.") {
                        if let Some(field) = field.strip_suffix(".LENGTH") {
                            let target = format!("{field}L");
                            if self.layout(&target).is_some() {
                                let coefficient = String::from_utf8_lossy(value.bytes())
                                    .parse::<i128>()
                                    .map_err(|_| MachineProblem::UnexpectedHostResult)?;
                                self.write_decimal(
                                    &target,
                                    Decimal {
                                        coefficient,
                                        scale: 0,
                                    },
                                )?;
                            }
                        } else {
                            let target = format!("{field}I");
                            if self.layout(&target).is_some() {
                                self.write(&target, value.bytes())?;
                            }
                        }
                        continue;
                    }
                    let Some(target) = outputs.get(name) else {
                        continue;
                    };
                    if value.schema() == "mainframe-env.cics.decimal@1" {
                        let coefficient = String::from_utf8_lossy(value.bytes())
                            .parse::<i128>()
                            .map_err(|_| MachineProblem::UnexpectedHostResult)?;
                        self.write_cics_value(
                            target,
                            &CobolValue::Decimal(Decimal {
                                coefficient,
                                scale: 0,
                            }),
                        )?;
                    } else {
                        self.write_cics_value(target, &CobolValue::Bytes(value.bytes().to_vec()))?;
                    }
                }
                self.write_cics_context(operation, &response)?;
                self.deferred_drive = match response.disposition {
                    CicsDisposition::Complete => (response.response != 0 && !responded).then_some(
                        MachineDrive::Condition(Condition {
                            name: response.condition,
                            response: response.response,
                            response2: response.response2,
                            handled: false,
                        }),
                    ),
                    CicsDisposition::Suspended => {
                        self.pc = self.pc.saturating_sub(1);
                        Some(MachineDrive::Suspended(Suspension {
                            kind: "cics-terminal".into(),
                            resume_token: format!(
                                "{}:{}",
                                self.invocation.run_unit_id, self.effect_sequence
                            ),
                            state_bytes: response.payload.bytes().len() as u64,
                        }))
                    }
                    CicsDisposition::Transfer => {
                        let target = response
                            .target
                            .ok_or(MachineProblem::UnexpectedHostResult)?;
                        Some(MachineDrive::Transfer(Transfer {
                            selector: Selector::new(target, InvocationLimits::default())
                                .map_err(|_| MachineProblem::UnexpectedHostResult)?,
                            payload: response.payload,
                            replace_frame: true,
                        }))
                    }
                    CicsDisposition::Handler => {
                        let target = response
                            .target
                            .ok_or(MachineProblem::UnexpectedHostResult)?;
                        self.pc = self
                            .labels
                            .get(&normalize(&target))
                            .copied()
                            .ok_or(MachineProblem::UnexpectedHostResult)?;
                        None
                    }
                    CicsDisposition::Returned => Some(MachineDrive::Completed(self.complete()?)),
                    CicsDisposition::Abended => Some(MachineDrive::Abend(Abend {
                        code: response.condition,
                        reason: Some(format!(
                            "EIBRESP={} EIBRESP2={}",
                            response.response, response.response2
                        )),
                    })),
                };
            }
            (
                PendingKind::DatasetRead { .. }
                | PendingKind::DatasetStatus { .. }
                | PendingKind::ProgramCall { .. }
                | PendingKind::Db2 { .. }
                | PendingKind::Ims { .. }
                | PendingKind::Mq { .. }
                | PendingKind::Cics { .. }
                | PendingKind::Ignore,
                _,
            ) => {}
            _ => return Err(MachineProblem::UnexpectedHostResult),
        }
        Ok(())
    }

    fn execute(&mut self, operation: &Operation) -> Result<Step, MachineProblem> {
        let name = operation.identity.name();
        let args = arguments(operation);
        if let Some(step) = self.execute_control(operation, name, &args)? {
            return Ok(step);
        }
        match name {
            "define" | "file" => {}
            "init" => {
                let reference = operation
                    .storage
                    .first()
                    .ok_or(MachineProblem::InvalidOperation)?;
                let bytes = self
                    .entry_initials
                    .get(&reference.storage)
                    .cloned()
                    .unwrap_or(bytes_attribute(operation, "initial")?.to_vec());
                self.write_storage(reference.storage, &bytes)?;
                self.reapply_entry_context()?;
            }
            "display" => {
                let mut line = Vec::new();
                for token in &args {
                    if token.eq_ignore_ascii_case("WITH")
                        || token.eq_ignore_ascii_case("NO")
                        || token.eq_ignore_ascii_case("ADVANCING")
                    {
                        continue;
                    }
                    line.extend(self.resolve(token)?);
                }
                let no_advancing = args.windows(3).any(|window| {
                    window
                        .iter()
                        .map(|item| item.as_str())
                        .eq(["WITH", "NO", "ADVANCING"])
                });
                if !no_advancing {
                    line.push(b'\n');
                }
                self.append_output(&line)?;
            }
            "move" => self.move_op(&args)?,
            "add" | "subtract" | "multiply" | "divide" | "compute" => {
                match self.arithmetic(name, &args) {
                    Ok(()) => self.condition_status.arithmetic_size_error = false,
                    Err(MachineProblem::SizeError) => {
                        self.condition_status.arithmetic_size_error = true;
                        if !self.has_condition_handler(operation, "ON SIZE ERROR") {
                            return Err(MachineProblem::SizeError);
                        }
                    }
                    Err(problem) => return Err(problem),
                }
            }
            "initialize" => self.initialize_op(&args)?,
            "set" => self.set_op(&args)?,
            "allocate" => {
                if let Some(target) = args.last() {
                    self.write(target, b"1")?;
                }
            }
            "free" => {
                if let Some(target) = args.first() {
                    let length = self.read(target)?.len();
                    self.write(target, &vec![0; length])?;
                }
            }
            "string" => {
                self.condition_status.string_overflow = self.string_op(&args)?;
            }
            "unstring" => {
                self.condition_status.unstring_overflow = self.unstring_op(&args)?;
            }
            "inspect" => self.inspect_op(&args)?,
            "json_generate" | "json_parse" | "xml_generate" | "xml_parse" => {
                let json = name.starts_with("json_");
                let result = if name.ends_with("_generate") {
                    self.generate(&args, json)
                } else {
                    self.parse_generated(&args, json)
                };
                let failed = match result {
                    Ok(()) => false,
                    Err(_problem) if self.has_condition_handler(operation, "ON EXCEPTION") => true,
                    Err(problem) => return Err(problem),
                };
                if json {
                    self.condition_status.json_exception = failed;
                } else {
                    self.condition_status.xml_exception = failed;
                }
            }
            "if" => self.if_op(&args)?,
            "evaluate" => self.evaluate_op(&args)?,
            "search" => self.search_op(&args)?,
            "go_to" => {
                return Ok(Step::Jump(
                    self.label(args.last().ok_or(MachineProblem::InvalidOperation)?)?,
                ));
            }
            "alter" => {
                if args.len() < 5 {
                    return Err(MachineProblem::InvalidOperation);
                }
                self.altered
                    .insert(normalize(&args[0]), normalize(args.last().unwrap()));
            }
            "perform" => {
                let target = args.first().ok_or(MachineProblem::InvalidOperation)?;
                self.perform_stack.push(self.pc);
                return Ok(Step::Jump(self.label(target)?));
            }
            "exit" => {
                // A paragraph EXIT is a no-op; the active PERFORM frame returns
                // at the exact paragraph endpoint in the machine driver.
            }
            "entry" | "label" | "continue" => {}
            "accept" => return self.accept_effect(operation, &args),
            "call" | "cancel" => return self.program_effect(operation, name, &args),
            "open" | "close" | "read" | "rewrite" | "write" => {
                return self.dataset_effect(name, &args);
            }
            "exec_cics" => return self.cics_effect(&args),
            "exec_sql" => return self.sql_effect(&args),
            "exec_dli" => return self.ims_effect(&args),
            "stop_run" | "go_back" | "halt" => return Ok(Step::Complete),
            _ => return Err(MachineProblem::InvalidOperation),
        }
        if optional_integer_attribute(operation, "edge_loop").is_some() {
            return self.perform_control_end(operation);
        }
        if optional_integer_attribute(operation, "edge_fallthrough").is_some() {
            let target = self.control_target(operation, "edge_fallthrough")?;
            if target != self.pc.saturating_add(1) {
                return Ok(Step::Jump(target));
            }
        }
        Ok(Step::Next)
    }

    fn execute_control(
        &mut self,
        operation: &Operation,
        name: &str,
        args: &[String],
    ) -> Result<Option<Step>, MachineProblem> {
        let Some(role) = optional_text_attribute(operation, "control_role") else {
            return Ok(None);
        };
        let scope = optional_text_attribute(operation, "control_scope").unwrap_or("");
        match role {
            "block_start" if scope == "if" => {
                if self.eval_condition(args)? {
                    Ok(Some(Step::Next))
                } else {
                    Ok(Some(Step::Jump(
                        self.control_target(operation, "edge_false")?,
                    )))
                }
            }
            "block_start" if matches!(scope, "evaluate" | "search") => Ok(Some(Step::Next)),
            "block_start" if scope == "perform" => {
                self.perform_control_start(operation, args).map(Some)
            }
            "branch" => {
                if self.control_branch(operation)? {
                    if optional_integer_attribute(operation, "edge_branch_true").is_some() {
                        Ok(Some(Step::Jump(
                            self.control_target(operation, "edge_branch_true")?,
                        )))
                    } else {
                        Ok(Some(Step::Next))
                    }
                } else {
                    Ok(Some(Step::Jump(
                        self.control_target(operation, "edge_branch_false")?,
                    )))
                }
            }
            "block_end" => {
                if optional_integer_attribute(operation, "edge_loop").is_some() {
                    self.perform_control_end(operation).map(Some)
                } else if optional_integer_attribute(operation, "edge_fallthrough").is_some() {
                    let target = self.control_target(operation, "edge_fallthrough")?;
                    Ok(Some(if target == self.pc.saturating_add(1) {
                        Step::Next
                    } else {
                        Step::Jump(target)
                    }))
                } else {
                    Ok(Some(Step::Next))
                }
            }
            "terminator" => {
                if self.at_perform_endpoint() {
                    Ok(Some(Step::Next))
                } else if optional_integer_attribute(operation, "edge_fallthrough").is_some() {
                    let target = self.control_target(operation, "edge_fallthrough")?;
                    Ok(Some(if target == self.pc.saturating_add(1) {
                        Step::Next
                    } else {
                        Step::Jump(target)
                    }))
                } else {
                    Ok(Some(Step::Next))
                }
            }
            "transfer" if name == "go_to" => Ok(Some(Step::Jump(
                self.label(args.last().ok_or(MachineProblem::InvalidOperation)?)?,
            ))),
            "transfer" if name == "next_sentence" => Ok(Some(Step::Jump(
                self.control_target(operation, "edge_transfer")?,
            ))),
            _ if name == "control" => Ok(Some(Step::Next)),
            _ => Ok(None),
        }
    }

    fn control_target(&self, operation: &Operation, edge: &str) -> Result<usize, MachineProblem> {
        let node = optional_integer_attribute(operation, edge)
            .and_then(|node| usize::try_from(node).ok())
            .ok_or(MachineProblem::InvalidOperation)?;
        self.control_nodes
            .get(&node)
            .copied()
            .ok_or(MachineProblem::InvalidOperation)
    }

    fn control_branch(&self, operation: &Operation) -> Result<bool, MachineProblem> {
        let text = text_attribute(operation, "control_text")?.trim();
        if text.eq_ignore_ascii_case("ELSE") || text.eq_ignore_ascii_case("WHEN OTHER") {
            return Ok(true);
        }
        let tokens = control_tokens(text);
        let condition = tokens
            .strip_prefix(&["WHEN".to_string()])
            .unwrap_or(tokens.as_slice());
        let parent = optional_integer_attribute(operation, "control_parent")
            .and_then(|node| usize::try_from(node).ok())
            .and_then(|node| self.control_nodes.get(&node))
            .and_then(|pc| self.operations.get(*pc));
        let parent_name = parent.map(|operation| operation.identity.name());
        let status_branch = || {
            let positive = match (text.to_ascii_uppercase().as_str(), parent_name) {
                ("AT END", _) => Some(self.last_file_status == "10"),
                ("INVALID KEY", _) => Some(self.last_file_status != "00"),
                ("ON SIZE ERROR", Some("add" | "compute" | "divide" | "multiply" | "subtract")) => {
                    Some(self.condition_status.arithmetic_size_error)
                }
                ("ON EXCEPTION", Some("accept")) => Some(self.condition_status.accept_exception),
                ("ON EXCEPTION", Some("call" | "invoke")) => {
                    Some(self.condition_status.call_exception)
                }
                ("ON EXCEPTION", Some("json_generate" | "json_parse")) => {
                    Some(self.condition_status.json_exception)
                }
                ("ON EXCEPTION", Some("xml_generate" | "xml_parse")) => {
                    Some(self.condition_status.xml_exception)
                }
                ("OVERFLOW" | "ON OVERFLOW", Some("string")) => {
                    Some(self.condition_status.string_overflow)
                }
                ("OVERFLOW" | "ON OVERFLOW", Some("unstring")) => {
                    Some(self.condition_status.unstring_overflow)
                }
                _ => None,
            };
            positive.or_else(|| match (text.to_ascii_uppercase().as_str(), parent_name) {
                ("NOT AT END", _) => Some(self.last_file_status != "10"),
                ("NOT INVALID KEY", _) => Some(self.last_file_status == "00"),
                (
                    "NOT ON SIZE ERROR",
                    Some("add" | "compute" | "divide" | "multiply" | "subtract"),
                ) => Some(!self.condition_status.arithmetic_size_error),
                ("NOT ON EXCEPTION", Some("accept")) => {
                    Some(!self.condition_status.accept_exception)
                }
                ("NOT ON EXCEPTION", Some("call" | "invoke")) => {
                    Some(!self.condition_status.call_exception)
                }
                ("NOT ON EXCEPTION", Some("json_generate" | "json_parse")) => {
                    Some(!self.condition_status.json_exception)
                }
                ("NOT ON EXCEPTION", Some("xml_generate" | "xml_parse")) => {
                    Some(!self.condition_status.xml_exception)
                }
                ("NOT ON OVERFLOW", Some("string")) => Some(!self.condition_status.string_overflow),
                ("NOT ON OVERFLOW", Some("unstring")) => {
                    Some(!self.condition_status.unstring_overflow)
                }
                _ => None,
            })
        };
        let Some(parent) = parent else {
            return Ok(status_branch().unwrap_or(false));
        };
        let scope = optional_text_attribute(parent, "control_scope").unwrap_or("");
        if scope == "evaluate" {
            let subject = arguments(parent);
            if subject.first().is_some_and(|token| token == "TRUE") {
                self.eval_condition(condition)
            } else {
                let left = self.eval_value(&subject).map_err(|problem| {
                    MachineProblem::InvalidArtifact(format!(
                        "evaluate subject {subject:?}: {problem:?}"
                    ))
                })?;
                let right = self.eval_value(condition).map_err(|problem| {
                    MachineProblem::InvalidArtifact(format!(
                        "evaluate condition {condition:?}: {problem:?}"
                    ))
                })?;
                Ok(match (left, right) {
                    (CobolValue::Decimal(left), CobolValue::Decimal(right)) => {
                        let (left, right) = decimal_aligned(left, right)?;
                        left.coefficient == right.coefficient
                    }
                    (CobolValue::Bytes(left), CobolValue::Bytes(right)) => left == right,
                    _ => false,
                })
            }
        } else if scope == "search" {
            self.eval_condition(condition)
        } else {
            Ok(status_branch().unwrap_or(true))
        }
    }

    fn has_condition_handler(&self, operation: &Operation, phrase: &str) -> bool {
        let Some(owner) = optional_integer_attribute(operation, "control_node") else {
            return false;
        };
        self.operations.iter().any(|candidate| {
            optional_integer_attribute(candidate, "control_parent") == Some(owner)
                && optional_text_attribute(candidate, "control_role") == Some("branch")
                && optional_text_attribute(candidate, "control_text")
                    .is_some_and(|text| text.eq_ignore_ascii_case(phrase))
        })
    }

    fn perform_control_start(
        &mut self,
        operation: &Operation,
        args: &[String],
    ) -> Result<Step, MachineProblem> {
        let node = usize::try_from(
            optional_integer_attribute(operation, "control_node")
                .ok_or(MachineProblem::InvalidOperation)?,
        )
        .map_err(|_| MachineProblem::InvalidOperation)?;
        let reentry = self.loop_reentry.remove(&node);
        if let Some(varying) = position(args, "VARYING") {
            let variable = args
                .get(varying + 1)
                .ok_or(MachineProblem::InvalidOperation)?;
            if !reentry {
                let from = position(args, "FROM").ok_or(MachineProblem::InvalidOperation)? + 1;
                let end = position(args, "BY")
                    .or_else(|| position(args, "UNTIL"))
                    .unwrap_or(args.len());
                if from >= end {
                    return Err(MachineProblem::InvalidOperation);
                }
                let value = self.eval_value(&args[from..end])?;
                self.write_value(variable, &value)?;
            }
        } else if let Some(times) = position(args, "TIMES") {
            if !reentry {
                let count = self.decimal(
                    args.get(times.saturating_sub(1))
                        .ok_or(MachineProblem::InvalidOperation)?,
                )?;
                if count.scale != 0 || count.coefficient < 0 {
                    return Err(MachineProblem::DataException);
                }
                self.loop_counts.insert(node, count.coefficient);
            }
            if self.loop_counts.get(&node).copied().unwrap_or(0) <= 0 {
                self.loop_counts.remove(&node);
                return Ok(Step::Jump(self.control_target(operation, "edge_false")?));
            }
        }
        if let Some(until) = position(args, "UNTIL") {
            let test_after = args.windows(2).any(|pair| pair == ["TEST", "AFTER"]);
            if (reentry || !test_after) && self.eval_condition(&args[until + 1..])? {
                self.loop_counts.remove(&node);
                return Ok(Step::Jump(self.control_target(operation, "edge_false")?));
            }
        }
        Ok(Step::Next)
    }

    fn perform_control_end(&mut self, operation: &Operation) -> Result<Step, MachineProblem> {
        let parent = usize::try_from(
            optional_integer_attribute(operation, "edge_loop")
                .ok_or(MachineProblem::InvalidOperation)?,
        )
        .map_err(|_| MachineProblem::InvalidOperation)?;
        let start_pc = self
            .control_nodes
            .get(&parent)
            .copied()
            .ok_or(MachineProblem::InvalidOperation)?;
        let start = self
            .operations
            .get(start_pc)
            .ok_or(MachineProblem::InvalidOperation)?;
        let args = arguments(start);
        if let Some(varying) = position(&args, "VARYING") {
            let variable = args
                .get(varying + 1)
                .ok_or(MachineProblem::InvalidOperation)?;
            let by = position(&args, "BY")
                .and_then(|index| args.get(index + 1))
                .ok_or(MachineProblem::InvalidOperation)?;
            let value = decimal_add(self.decimal(variable)?, self.decimal(by)?)?;
            self.write_decimal(variable, value)?;
        }
        if let Some(count) = self.loop_counts.get_mut(&parent) {
            *count = count.saturating_sub(1);
        }
        self.loop_reentry.insert(parent);
        Ok(Step::Jump(self.control_target(operation, "edge_loop")?))
    }

    fn at_perform_endpoint(&self) -> bool {
        let Some(call_pc) = self.perform_stack.last().copied() else {
            return false;
        };
        self.perform_endpoint(call_pc) == Some(self.pc)
    }

    fn perform_endpoint(&self, call_pc: usize) -> Option<usize> {
        let call = self.operations.get(call_pc)?;
        let args = arguments(call);
        let target = position(&args, "THRU")
            .or_else(|| position(&args, "THROUGH"))
            .and_then(|index| args.get(index + 1))
            .or_else(|| args.first());
        let start = target.and_then(|target| self.labels.get(&normalize(target)).copied())?;
        Some(
            self.labels
                .values()
                .copied()
                .filter(|label| *label > start)
                .min()
                .unwrap_or_else(|| {
                    if self
                        .operations
                        .last()
                        .is_some_and(|operation| operation.identity.name() == "halt")
                    {
                        self.operations.len().saturating_sub(1)
                    } else {
                        self.operations.len()
                    }
                })
                .saturating_sub(1),
        )
    }

    fn perform_return_pc(&mut self, call_pc: usize) -> Result<usize, MachineProblem> {
        let operation = self
            .operations
            .get(call_pc)
            .cloned()
            .ok_or(MachineProblem::InvalidOperation)?;
        if optional_integer_attribute(&operation, "edge_loop").is_some() {
            return match self.perform_control_end(&operation)? {
                Step::Jump(target) => Ok(target),
                _ => Err(MachineProblem::InvalidOperation),
            };
        }
        Ok(self
            .control_target(&operation, "edge_fallthrough")
            .unwrap_or(call_pc.saturating_add(1)))
    }

    fn finish_perform(&mut self) -> Result<usize, MachineProblem> {
        let mut completed_pc = self
            .perform_stack
            .pop()
            .ok_or(MachineProblem::InvalidOperation)?;
        let mut return_pc = self.perform_return_pc(completed_pc)?;
        while let Some(outer_call_pc) = self.perform_stack.last().copied() {
            if self.perform_endpoint(outer_call_pc) != Some(completed_pc) {
                break;
            }
            self.perform_stack.pop();
            completed_pc = outer_call_pc;
            return_pc = self.perform_return_pc(completed_pc)?;
        }
        Ok(return_pc)
    }

    fn accept_effect(
        &mut self,
        operation: &Operation,
        args: &[String],
    ) -> Result<Step, MachineProblem> {
        let target = normalize(args.first().ok_or(MachineProblem::InvalidOperation)?);
        let request = HostRequest::Terminal(TerminalRequest::Read {
            session: mainframe_env_host_api::SessionId::new(
                self.invocation.run_unit_id.as_str(),
                128,
            )
            .map_err(|_| MachineProblem::InvalidOperation)?,
        });
        let handles_exception = self.has_condition_handler(operation, "ON EXCEPTION");
        self.condition_status.accept_exception = false;
        self.effect(
            request,
            PendingKind::Accept {
                target,
                handles_exception,
            },
        )
    }
    fn program_effect(
        &mut self,
        operation: &Operation,
        name: &str,
        args: &[String],
    ) -> Result<Step, MachineProblem> {
        if name == "call"
            && args.first().is_some_and(|program| {
                matches!(
                    normalize(program.trim_matches(['\'', '"'])).as_str(),
                    "MQOPEN" | "MQGET" | "MQPUT" | "MQPUT1" | "MQCLOSE"
                )
            })
        {
            return self.mq_effect(args);
        }
        let program = ProgramName::new(
            args.first()
                .ok_or(MachineProblem::InvalidOperation)?
                .trim_matches(['\'', '"']),
            128,
        )
        .map_err(|_| MachineProblem::InvalidOperation)?;
        if name == "cancel" {
            return self.effect(
                HostRequest::Program(ProgramRequest::Cancel { program }),
                PendingKind::Ignore,
            );
        }
        let targets = position(args, "USING").map_or_else(Vec::new, |using| {
            args[using + 1..]
                .iter()
                .filter(|argument| !matches!(argument.as_str(), "BY" | "REFERENCE" | "CONTENT"))
                .cloned()
                .collect()
        });
        let values = targets
            .iter()
            .map(|target| self.read(target))
            .collect::<Result<Vec<_>, _>>()?;
        let payload = encode_call_values(&targets, &values)?;
        let handles_exception = self.has_condition_handler(operation, "ON EXCEPTION");
        self.condition_status.call_exception = false;
        self.effect(
            HostRequest::Program(ProgramRequest::Call { program, payload }),
            PendingKind::ProgramCall {
                targets,
                handles_exception,
            },
        )
    }
    fn mq_effect(&mut self, args: &[String]) -> Result<Step, MachineProblem> {
        let operation = match normalize(
            args.first()
                .ok_or(MachineProblem::InvalidOperation)?
                .trim_matches(['\'', '"']),
        )
        .as_str()
        {
            "MQOPEN" => MqOperation::Open,
            "MQGET" => MqOperation::Get,
            "MQPUT" => MqOperation::Put,
            "MQPUT1" => MqOperation::PutOne,
            "MQCLOSE" => MqOperation::Close,
            _ => return Err(MachineProblem::UnsupportedForm),
        };
        let using = position(args, "USING").ok_or(MachineProblem::InvalidOperation)?;
        let parameters = args[using + 1..]
            .iter()
            .filter(|argument| {
                !matches!(
                    argument.as_str(),
                    "BY" | "REFERENCE" | "CONTENT" | "VALUE" | "END-CALL"
                )
            })
            .map(|argument| normalize(argument))
            .collect::<Vec<_>>();
        let parameter = |index: usize| {
            parameters
                .get(index)
                .cloned()
                .ok_or(MachineProblem::InvalidOperation)
        };
        let read_i32 = |machine: &Self, target: &str| -> Result<i32, MachineProblem> {
            let value = machine.decimal(target)?;
            if value.scale != 0 {
                return Err(MachineProblem::DataException);
            }
            i32::try_from(value.coefficient).map_err(|_| MachineProblem::DataException)
        };
        let read_handle = |machine: &Self, target: &str| -> Result<u32, MachineProblem> {
            let value = read_i32(machine, target)?;
            u32::try_from(value).map_err(|_| MachineProblem::DataException)
        };

        let mut request = MqRequest {
            operation,
            queue: None,
            handle: None,
            options: 0,
            message: Vec::new(),
            message_id: None,
            correlation_id: None,
            wait_ticks: 0,
            max_message_bytes: 1,
            mutation: Some(self.mutation()?),
        };
        let mut descriptor = None;
        let mut handle_target = None;
        let mut buffer = None;
        let mut data_length = None;
        let (completion_code, reason_code) = match operation {
            MqOperation::Open => {
                let object_descriptor = parameter(1)?;
                request.queue = Some(self.mq_queue_name(&object_descriptor)?);
                request.options = read_i32(self, &parameter(2)?)?;
                handle_target = Some(parameter(3)?);
                (Some(parameter(4)?), Some(parameter(5)?))
            }
            MqOperation::Get => {
                let target = parameter(1)?;
                request.handle = Some(read_handle(self, &target)?);
                let message_descriptor = parameter(2)?;
                let get_options = parameter(3)?;
                request.options = self
                    .mq_descriptor_decimal(&get_options, "MQGMO-OPTIONS")
                    .unwrap_or_else(|| read_i32(self, &get_options))?;
                request.wait_ticks = self
                    .mq_descriptor_decimal(&get_options, "MQGMO-WAITINTERVAL")
                    .transpose()?
                    .unwrap_or_default()
                    .max(0) as u64;
                let maximum = read_i32(self, &parameter(4)?)?.max(0);
                request.max_message_bytes =
                    u32::try_from(maximum).map_err(|_| MachineProblem::ResourceExhausted)?;
                request.message_id = self.mq_descriptor_bytes(&message_descriptor, "MQMD-MSGID")?;
                request.correlation_id =
                    self.mq_descriptor_bytes(&message_descriptor, "MQMD-CORRELID")?;
                descriptor = Some(message_descriptor);
                buffer = Some(parameter(5)?);
                data_length = Some(parameter(6)?);
                (Some(parameter(7)?), Some(parameter(8)?))
            }
            MqOperation::Put => {
                let target = parameter(1)?;
                request.handle = Some(read_handle(self, &target)?);
                let message_descriptor = parameter(2)?;
                let put_options = parameter(3)?;
                request.options = self
                    .mq_descriptor_decimal(&put_options, "MQPMO-OPTIONS")
                    .unwrap_or_else(|| read_i32(self, &put_options))?;
                let length = read_i32(self, &parameter(4)?)?.max(0) as usize;
                let target = parameter(5)?;
                let mut message = self.read(&target)?;
                message.truncate(length);
                request.max_message_bytes =
                    u32::try_from(length).map_err(|_| MachineProblem::ResourceExhausted)?;
                request.message = message;
                request.message_id = self.mq_descriptor_bytes(&message_descriptor, "MQMD-MSGID")?;
                request.correlation_id =
                    self.mq_descriptor_bytes(&message_descriptor, "MQMD-CORRELID")?;
                descriptor = Some(message_descriptor);
                (Some(parameter(6)?), Some(parameter(7)?))
            }
            MqOperation::PutOne => {
                let object_descriptor = parameter(1)?;
                request.queue = Some(self.mq_queue_name(&object_descriptor)?);
                let message_descriptor = parameter(2)?;
                let put_options = parameter(3)?;
                request.options = self
                    .mq_descriptor_decimal(&put_options, "MQPMO-OPTIONS")
                    .unwrap_or_else(|| read_i32(self, &put_options))?;
                let length = read_i32(self, &parameter(4)?)?.max(0) as usize;
                let target = parameter(5)?;
                let mut message = self.read(&target)?;
                message.truncate(length);
                request.max_message_bytes =
                    u32::try_from(length).map_err(|_| MachineProblem::ResourceExhausted)?;
                request.message = message;
                request.message_id = self.mq_descriptor_bytes(&message_descriptor, "MQMD-MSGID")?;
                request.correlation_id =
                    self.mq_descriptor_bytes(&message_descriptor, "MQMD-CORRELID")?;
                descriptor = Some(message_descriptor);
                (Some(parameter(6)?), Some(parameter(7)?))
            }
            MqOperation::Close => {
                let target = parameter(1)?;
                request.handle = Some(read_handle(self, &target)?);
                request.options = read_i32(self, &parameter(2)?)?;
                handle_target = Some(target);
                (Some(parameter(3)?), Some(parameter(4)?))
            }
            MqOperation::Commit | MqOperation::Rollback => {
                return Err(MachineProblem::UnsupportedForm);
            }
        };
        self.effect(
            HostRequest::Mq(request),
            PendingKind::Mq {
                handle: handle_target,
                descriptor,
                buffer,
                data_length,
                completion_code,
                reason_code,
            },
        )
    }
    fn dataset_effect(&mut self, name: &str, args: &[String]) -> Result<Step, MachineProblem> {
        let open_mode = (name == "open")
            .then(|| {
                args.iter()
                    .find(|argument| {
                        matches!(argument.as_str(), "INPUT" | "OUTPUT" | "I-O" | "EXTEND")
                    })
                    .map(String::as_str)
            })
            .flatten();
        let logical = if name == "open" {
            args.iter().find(|argument| {
                !matches!(
                    argument.as_str(),
                    "INPUT" | "OUTPUT" | "I-O" | "EXTEND" | "SHARING" | "WITH"
                )
            })
        } else {
            args.first()
        }
        .ok_or(MachineProblem::InvalidOperation)?
        .trim_matches(['\'', '"']);
        let logical_name = normalize(logical);
        let file = self.files.get(&logical_name).cloned().or_else(|| {
            self.files
                .values()
                .find(|file| file.record_name.as_deref() == Some(logical_name.as_str()))
                .cloned()
        });
        let explicit_key = (name == "read")
            .then(|| {
                position(args, "KEY").and_then(|index| {
                    args.get(
                        index
                            + usize::from(args.get(index + 1).is_some_and(|value| value == "IS"))
                            + 1,
                    )
                    .cloned()
                })
            })
            .flatten();
        let alternate_dd = explicit_key.as_ref().and_then(|key| {
            file.as_ref().and_then(|file| {
                file.alternate_record_keys
                    .iter()
                    .position(|candidate| normalize(candidate) == normalize(key))
                    .map(|index| alternate_dd_name(&file.assignment, index + 1))
            })
        });
        let binding_name = alternate_dd.unwrap_or_else(|| normalize(logical));
        let dataset_name = self
            .invocation
            .bindings
            .get(&format!("cobol.dd.{binding_name}"))
            .or_else(|| {
                file.as_ref().and_then(|file| {
                    self.invocation
                        .bindings
                        .get(&format!("cobol.dd.{}", normalize(&file.assignment)))
                })
            })
            .map(|payload| String::from_utf8_lossy(payload.bytes()).into_owned())
            .unwrap_or_else(|| {
                file.as_ref()
                    .map(|file| file.assignment.clone())
                    .unwrap_or_else(|| logical.to_string())
            });
        let dataset =
            DatasetName::new(&dataset_name, 128).map_err(|_| MachineProblem::InvalidOperation)?;
        let status = self
            .invocation
            .bindings
            .get(&format!("cobol.file-status.{}", normalize(logical)))
            .map(|payload| String::from_utf8_lossy(payload.bytes()).into_owned())
            .or_else(|| file.as_ref().and_then(|file| file.file_status.clone()));
        let ccsid = self
            .invocation
            .bindings
            .get(&format!("cobol.dd.{}.ccsid", normalize(logical)))
            .or_else(|| {
                file.as_ref().and_then(|file| {
                    self.invocation
                        .bindings
                        .get(&format!("cobol.dd.{}.ccsid", normalize(&file.assignment)))
                })
            })
            .map(|payload| {
                std::str::from_utf8(payload.bytes())
                    .map_err(|_| MachineProblem::InvalidOperation)?
                    .parse::<u16>()
                    .map_err(|_| MachineProblem::InvalidOperation)
            })
            .transpose()?;
        let default_key = file
            .as_ref()
            .filter(|file| file.access_mode == "RANDOM")
            .and_then(|file| file.record_key.as_ref().or(file.relative_key.as_ref()))
            .map(|key| self.resolve(key))
            .transpose()?
            .map(|key| encode_dataset_record(ccsid, &key))
            .transpose()?;
        let sequential = file
            .as_ref()
            .is_some_and(|file| file.access_mode == "SEQUENTIAL");
        let sequential_organization = file
            .as_ref()
            .is_some_and(|file| file.organization == "SEQUENTIAL");
        let current_cursor = self.dataset_cursors.get(&dataset_name).cloned();
        let (request, cursor_action) = match name {
            "read" if sequential && current_cursor.is_some() => (
                DatasetRequest::ReadNext {
                    dataset,
                    cursor: current_cursor.ok_or(MachineProblem::InvalidOperation)?,
                    reverse: false,
                },
                None,
            ),
            "read" => (
                DatasetRequest::Read {
                    dataset,
                    member: None,
                    key: explicit_key
                        .as_ref()
                        .map(|key| self.resolve(key))
                        .transpose()?
                        .map(|key| encode_dataset_record(ccsid, &key))
                        .transpose()?
                        .or(default_key),
                    max_records: 1,
                },
                None,
            ),
            "write" => {
                let record = position(args, "FROM")
                    .and_then(|index| args.get(index + 1))
                    .or_else(|| args.get(1))
                    .map(|value| self.resolve(value))
                    .transpose()?
                    .unwrap_or_default();
                let records = vec![encode_dataset_record(ccsid, &record)?];
                (
                    if sequential_organization {
                        DatasetRequest::Append {
                            dataset,
                            member: None,
                            records,
                            expected_version: None,
                            mutation: self.mutation()?,
                        }
                    } else {
                        DatasetRequest::Write {
                            dataset,
                            member: None,
                            records,
                            expected_version: None,
                            mutation: self.mutation()?,
                        }
                    },
                    None,
                )
            }
            "rewrite" => {
                let record = position(args, "FROM")
                    .and_then(|index| args.get(index + 1))
                    .or_else(|| args.first())
                    .map(|value| self.resolve(value))
                    .transpose()?
                    .unwrap_or_default();
                (
                    DatasetRequest::RewriteRecord {
                        dataset,
                        key: default_key.ok_or(MachineProblem::InvalidOperation)?,
                        record: encode_dataset_record(ccsid, &record)?,
                        expected_version: None,
                        mutation: self.mutation()?,
                    },
                    None,
                )
            }
            "open" if open_mode == Some("OUTPUT") => (
                DatasetRequest::Truncate {
                    dataset,
                    expected_version: None,
                    mutation: self.mutation()?,
                },
                None,
            ),
            "open" if sequential && matches!(open_mode, Some("INPUT" | "I-O")) => (
                DatasetRequest::StartBrowse {
                    dataset,
                    key: Vec::new(),
                },
                Some(DatasetCursorAction::Start(dataset_name.clone())),
            ),
            "close" if current_cursor.is_some() => (
                DatasetRequest::EndBrowse {
                    dataset,
                    cursor: current_cursor.ok_or(MachineProblem::InvalidOperation)?,
                },
                Some(DatasetCursorAction::End(dataset_name.clone())),
            ),
            _ => (DatasetRequest::Attributes { dataset }, None),
        };
        let target = (name == "read")
            .then(|| position(args, "INTO").and_then(|index| args.get(index + 1).cloned()))
            .flatten();
        let pending = if name == "read" {
            PendingKind::DatasetRead {
                target,
                status,
                ccsid,
            }
        } else {
            PendingKind::DatasetStatus {
                status,
                cursor: cursor_action,
            }
        };
        self.effect(HostRequest::Dataset(request), pending)
    }
    fn cics_effect(&mut self, args: &[String]) -> Result<Step, MachineProblem> {
        let operation = CicsOperation::from_tokens(args).ok_or(MachineProblem::UnsupportedForm)?;
        let mut arguments = cics_arguments(args)?;
        let into = cics_destination(&arguments, "INTO");
        let output_names: &[&str] = match operation {
            CicsOperation::Asktime => &["ABSTIME"],
            CicsOperation::Assign => &["APPLID", "SYSID", "TRANSID", "PRINCIPAL"],
            CicsOperation::FormatTime => &[
                "YYYYMMDD",
                "YYMMDD",
                "MMDDYY",
                "MMDDYYYY",
                "YYDDD",
                "TIME",
                "MILLISECONDS",
            ],
            CicsOperation::Link => &["COMMAREA"],
            CicsOperation::ReadNext | CicsOperation::ReadPrev => &["RIDFLD"],
            _ => &[],
        };
        let outputs = output_names
            .iter()
            .filter_map(|name| {
                cics_destination(&arguments, name).map(|target| ((*name).into(), target))
            })
            .collect::<BTreeMap<_, _>>();
        let response_target = cics_destination(&arguments, "RESP");
        let response2_target = cics_destination(&arguments, "RESP2");
        let absolute_time = cics_destination(&arguments, "ABSTIME");
        for key in [
            "FROM", "COMMAREA", "RIDFLD", "LENGTH", "QUEUE", "MAP", "MAPSET", "TRANSID", "PROGRAM",
            "DATASET", "FILE", "MEMBER", "VERSION",
        ] {
            if outputs.contains_key(key) {
                continue;
            }
            let Some(argument) = arguments.get(key) else {
                continue;
            };
            if argument.schema() == "mainframe-env.cics.literal@1" {
                continue;
            }
            let token = String::from_utf8_lossy(argument.bytes()).into_owned();
            let reference_tokens = token
                .split_whitespace()
                .map(str::to_string)
                .collect::<Vec<_>>();
            if let Ok(reference) = self.reference(&reference_tokens) {
                let value = self.read_reference(&reference)?;
                arguments.insert(
                    key.into(),
                    BoundedPayload::new(
                        "mainframe-env.cics.storage-value@1",
                        value,
                        InvocationLimits::default(),
                    )
                    .map_err(|_| MachineProblem::ResourceExhausted)?,
                );
            }
        }
        if operation == CicsOperation::FormatTime
            && let Some(target) = absolute_time
        {
            let tokens = target
                .split_whitespace()
                .map(str::to_string)
                .collect::<Vec<_>>();
            let reference = self.reference(&tokens)?;
            let bytes = self.read_reference(&reference)?;
            if !is_numeric(reference.layout.category) {
                return Err(MachineProblem::DataException);
            }
            let value = decode_decimal(&reference.layout, &bytes)?;
            if value.scale != 0 {
                return Err(MachineProblem::DataException);
            }
            arguments.insert(
                "ABSTIME".into(),
                BoundedPayload::new(
                    "mainframe-env.cics.decimal@1",
                    value.coefficient.to_string().into_bytes(),
                    InvocationLimits::default(),
                )
                .map_err(|_| MachineProblem::ResourceExhausted)?,
            );
        }
        let condition_policy = if args.iter().any(|arg| arg.eq_ignore_ascii_case("NOHANDLE")) {
            CicsConditionPolicy::NoHandle
        } else if let Some(response) = arguments.get("RESP") {
            CicsConditionPolicy::Respond {
                response_field: String::from_utf8_lossy(response.bytes()).into_owned(),
                response2_field: arguments
                    .get("RESP2")
                    .map(|value| String::from_utf8_lossy(value.bytes()).into_owned()),
            }
        } else {
            CicsConditionPolicy::Default
        };
        let mut mutation = operation
            .is_mutating()
            .then(|| self.mutation())
            .transpose()?;
        if let Some(mutation) = &mut mutation {
            mutation.transaction = Some(
                self.invocation
                    .bindings
                    .get("cics.transaction")
                    .map(|payload| String::from_utf8_lossy(payload.bytes()).into_owned())
                    .unwrap_or_else(|| "DEFAULT".into()),
            );
        }
        let argument_summary = arguments
            .iter()
            .map(|(name, value)| {
                if matches!(name.as_str(), "MAP" | "MAPSET" | "TRANSID" | "PROGRAM") {
                    format!(
                        "{name}={:?}/{}",
                        String::from_utf8_lossy(value.bytes()),
                        value.schema()
                    )
                } else {
                    format!("{name}=<{} bytes>/{}", value.bytes().len(), value.schema())
                }
            })
            .collect::<Vec<_>>()
            .join(",");
        self.effect(
            HostRequest::Cics(CicsRequest {
                operation,
                arguments,
                condition_policy,
                mutation,
            }),
            PendingKind::Cics {
                operation,
                argument_summary,
                into,
                outputs,
                response: response_target,
                response2: response2_target,
                no_handle: args.iter().any(|argument| argument == "NOHANDLE"),
            },
        )
    }

    fn write_cics_context(
        &mut self,
        operation: CicsOperation,
        response: &CicsResponse,
    ) -> Result<(), MachineProblem> {
        for (name, value) in [
            ("EIBRESP", i128::from(response.response)),
            ("EIBRESP2", i128::from(response.response2)),
        ] {
            self.write_decimal(
                name,
                Decimal {
                    coefficient: value,
                    scale: 0,
                },
            )?;
        }
        if operation == CicsOperation::ReceiveMap {
            self.write("EIBAID", &[response.aid])?;
        }
        self.write("EIBTRNID", response.transaction.as_bytes())?;
        Ok(())
    }

    fn ims_effect(&mut self, args: &[String]) -> Result<Step, MachineProblem> {
        let opcode = args
            .iter()
            .find(|token| !matches!(token.as_str(), "DLI" | "END-EXEC"))
            .ok_or(MachineProblem::InvalidOperation)?
            .to_ascii_uppercase();
        let operation = match opcode.as_str() {
            "SCHD" => ImsOperation::Schedule,
            "TERM" => ImsOperation::Terminate,
            "GU" => ImsOperation::GetUnique,
            "GN" => ImsOperation::GetNext,
            "GNP" => ImsOperation::GetNextParent,
            "ISRT" => ImsOperation::Insert,
            "REPL" => ImsOperation::Replace,
            "DLET" => ImsOperation::Delete,
            "CHKP" => ImsOperation::Checkpoint,
            _ => return Err(MachineProblem::UnsupportedForm),
        };
        let groups = ims_option_groups(args)?;
        let segments = groups
            .iter()
            .filter(|(name, _)| name == "SEGMENT")
            .filter_map(|(_, values)| values.first())
            .map(|name| normalize(name))
            .collect::<Vec<_>>();
        let operand_name = |keyword: &str| {
            groups
                .iter()
                .find(|(name, _)| name == keyword)
                .and_then(|(_, values)| {
                    values
                        .iter()
                        .find(|value| self.layout(value).is_some())
                        .or_else(|| values.first())
                })
                .map(|name| normalize(name))
        };
        let target = operand_name("INTO");
        let data = operand_name("FROM")
            .map(|name| self.read(&name))
            .transpose()?
            .unwrap_or_default();
        let psb = operand_name("PSB")
            .map(|name| {
                if self.layout(&name).is_some() {
                    self.read(&name)
                        .map(|value| String::from_utf8_lossy(&value).trim().to_ascii_uppercase())
                } else {
                    Ok(name)
                }
            })
            .transpose()?;
        let pcb = operand_name("PCB")
            .and_then(|name| self.decimal(&name).ok())
            .and_then(|value| u16::try_from(value.coefficient).ok())
            .filter(|value| *value > 0)
            .unwrap_or(1);
        let mut qualifiers = Vec::new();
        for (_, values) in groups.iter().filter(|(name, _)| name == "WHERE") {
            let Some(equal) = values.iter().position(|token| token == "=") else {
                return Err(MachineProblem::UnsupportedForm);
            };
            let field = values
                .get(equal.saturating_sub(1))
                .ok_or(MachineProblem::InvalidOperation)?;
            let value_name = values
                .get(equal + 1)
                .ok_or(MachineProblem::InvalidOperation)?;
            qualifiers.push(ImsQualifier {
                segment: segments.last().cloned().unwrap_or_else(|| "ROOT".into()),
                field: normalize(field),
                value: self.resolve(value_name)?,
            });
        }
        if qualifiers.is_empty()
            && let Some(using) = position(args, "USING")
        {
            for token in args.iter().skip(using + 2) {
                if self.layout(token).is_some() {
                    qualifiers.push(ImsQualifier {
                        segment: segments.last().cloned().unwrap_or_else(|| "ROOT".into()),
                        field: normalize(token),
                        value: self.read(token)?,
                    });
                }
            }
        }
        let checkpoint_id = operand_name("ID")
            .map(|name| self.resolve(&name))
            .transpose()?
            .map(|value| String::from_utf8_lossy(&value).trim().to_string())
            .filter(|value| !value.is_empty());
        let mutation = operation
            .is_mutating()
            .then(|| self.mutation())
            .transpose()?;
        self.effect(
            HostRequest::Ims(ImsRequest {
                operation,
                psb,
                pcb,
                segments,
                data,
                qualifiers,
                checkpoint_id,
                max_segments: 1,
                mutation,
            }),
            PendingKind::Ims { target },
        )
    }

    fn sql_effect(&mut self, args: &[String]) -> Result<Step, MachineProblem> {
        let opcode_index = args
            .iter()
            .position(|token| !matches!(token.as_str(), "SQL" | "END-EXEC"))
            .ok_or(MachineProblem::InvalidOperation)?;
        let opcode = args[opcode_index].to_ascii_uppercase();
        if opcode == "DECLARE" {
            let cursor = args
                .get(opcode_index + 1)
                .ok_or(MachineProblem::InvalidOperation)?;
            if args
                .get(opcode_index + 2)
                .is_some_and(|token| token == "CURSOR")
            {
                self.sql_cursors.insert(normalize(cursor), args.to_vec());
            }
            return Ok(Step::Next);
        }
        let mut statement_tokens = args.to_vec();
        let cursor = if matches!(opcode.as_str(), "OPEN" | "FETCH" | "CLOSE") {
            Some(
                args.get(opcode_index + 1)
                    .ok_or(MachineProblem::InvalidOperation)?
                    .to_ascii_uppercase(),
            )
        } else {
            None
        };
        if opcode == "OPEN" {
            statement_tokens = self
                .sql_cursors
                .get(&normalize(cursor.as_deref().unwrap_or_default()))
                .cloned()
                .ok_or(MachineProblem::InvalidOperation)?;
        }
        let into = position(&statement_tokens, "INTO");
        let from = position(&statement_tokens, "FROM");
        let mut inputs = BTreeMap::new();
        let mut targets = Vec::new();
        for (index, token) in statement_tokens.iter().enumerate() {
            if !token.starts_with(':') {
                continue;
            }
            for raw_name in token.trim_start_matches(':').split(':') {
                let name = normalize(raw_name);
                if self.layout(&name).is_none() && !self.implicit.contains_key(&name) {
                    continue;
                }
                let write = matches!(opcode.as_str(), "SELECT" | "FETCH")
                    && into.is_some_and(|into| index > into)
                    && from.is_none_or(|from| index < from);
                if write {
                    targets.push(name);
                } else {
                    inputs.insert(
                        name.clone(),
                        Db2HostVariable {
                            value: self.read(&name)?,
                            indicator: None,
                        },
                    );
                }
            }
        }
        let operation = match opcode.as_str() {
            "SELECT"
                if statement_tokens
                    .iter()
                    .any(|token| token.starts_with("COUNT")) =>
            {
                Db2Operation::Count
            }
            "SELECT" => Db2Operation::Select,
            "INSERT" => Db2Operation::Insert,
            "UPDATE" => Db2Operation::Update,
            "DELETE" => Db2Operation::Delete,
            "OPEN" => Db2Operation::OpenCursor,
            "FETCH" => Db2Operation::FetchCursor,
            "CLOSE" => Db2Operation::CloseCursor,
            "COMMIT" => Db2Operation::Commit,
            "ROLLBACK" => Db2Operation::Rollback,
            _ => return Err(MachineProblem::UnsupportedForm),
        };
        let mutation = operation
            .is_mutating()
            .then(|| self.mutation())
            .transpose()?;
        self.effect(
            HostRequest::Db2(Db2Request {
                operation,
                statement: statement_tokens
                    .iter()
                    .filter(|token| !matches!(token.as_str(), "SQL" | "END-EXEC"))
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(" "),
                cursor,
                inputs,
                outputs: targets.clone(),
                max_rows: 1,
                mutation,
            }),
            PendingKind::Db2 { targets },
        )
    }
    fn effect(&mut self, request: HostRequest, kind: PendingKind) -> Result<Step, MachineProblem> {
        self.effect_sequence = self
            .effect_sequence
            .checked_add(1)
            .ok_or(MachineProblem::ResourceExhausted)?;
        if self.effect_sequence > self.invocation.limits.max_effects {
            return Err(MachineProblem::ResourceExhausted);
        }
        let mutating = request.is_mutating();
        let key = mutating.then(|| self.effect_key()).transpose()?;
        let effect = EffectRequest {
            run_unit: self.invocation.run_unit_id.clone(),
            sequence: self.effect_sequence,
            deadline_tick: self.invocation.deadline_tick,
            idempotency_key: key,
            request,
        };
        effect
            .validate(HostLimits::default())
            .map_err(MachineProblem::Host)?;
        self.pending = Some(Pending {
            sequence: self.effect_sequence,
            kind,
        });
        Ok(Step::Effect(Box::new(effect)))
    }
    fn mutation(&self) -> Result<Mutation, MachineProblem> {
        Ok(Mutation {
            sequence: self.effect_sequence + 1,
            idempotency_key: self.next_effect_key()?,
            transaction: None,
        })
    }
    fn effect_key(&self) -> Result<IdempotencyKey, MachineProblem> {
        IdempotencyKey::new(
            format!(
                "{}:{}",
                self.invocation.idempotency_key.as_str(),
                self.effect_sequence
            ),
            InvocationLimits::default(),
        )
        .map_err(|_| MachineProblem::ResourceExhausted)
    }
    fn next_effect_key(&self) -> Result<IdempotencyKey, MachineProblem> {
        IdempotencyKey::new(
            format!(
                "{}:{}",
                self.invocation.idempotency_key.as_str(),
                self.effect_sequence + 1
            ),
            InvocationLimits::default(),
        )
        .map_err(|_| MachineProblem::ResourceExhausted)
    }

    fn move_op(&mut self, args: &[String]) -> Result<(), MachineProblem> {
        let to = position(args, "TO").ok_or(MachineProblem::InvalidOperation)?;
        if to == 0 || to + 1 >= args.len() {
            return Err(MachineProblem::InvalidOperation);
        }
        let value = self.eval_value(&args[..to])?;
        let figurative =
            (to == 1)
                .then(|| normalize(&args[0]))
                .and_then(|name| match name.as_str() {
                    "SPACE" | "SPACES" => Some(b' '),
                    "ZERO" | "ZEROS" | "ZEROES" => Some(b'0'),
                    "LOW-VALUE" | "LOW-VALUES" => Some(0),
                    "HIGH-VALUE" | "HIGH-VALUES" => Some(0xff),
                    _ => None,
                });
        let source_display = self
            .reference(&args[..to])
            .ok()
            .filter(|reference| {
                matches!(
                    reference.layout.category,
                    LayoutCategory::NumericDisplay | LayoutCategory::NumericEdited
                )
            })
            .and_then(|reference| self.read_reference(&reference).ok());
        let targets = &args[to + 1..];
        let control = targets
            .iter()
            .position(|target| matches!(target.as_str(), "ROUNDED" | "ON" | "NOT" | "END-MOVE"))
            .unwrap_or(targets.len());
        let mut at = 0usize;
        while at < control {
            if self.implicit.contains_key(&normalize(&targets[at])) {
                self.write_value(&targets[at], &value)?;
                at += 1;
                continue;
            }
            let end = (at + 1..=control)
                .rev()
                .find(|end| self.reference(&targets[at..*end]).is_ok())
                .ok_or(MachineProblem::UnknownStorage)?;
            let target = self.reference(&targets[at..end])?;
            let value = if figurative == Some(b'0') && is_numeric(target.layout.category) {
                CobolValue::Decimal(Decimal {
                    coefficient: 0,
                    scale: target.layout.scale,
                })
            } else if let Some(byte) = figurative {
                CobolValue::Bytes(vec![byte; target.length])
            } else if matches!(&value, CobolValue::Decimal(_))
                && !is_numeric(target.layout.category)
                && let Some(source_display) = &source_display
            {
                CobolValue::Bytes(source_display.clone())
            } else {
                value.clone()
            };
            self.write_reference_value(&targets[at..end], &value)?;
            at = end;
        }
        Ok(())
    }

    fn set_op(&mut self, args: &[String]) -> Result<(), MachineProblem> {
        if args.first().is_some_and(|argument| argument == "ADDRESS")
            && args.get(1).is_some_and(|argument| argument == "OF")
        {
            let to = position(args, "TO").ok_or(MachineProblem::InvalidOperation)?;
            if to != 3 || args.len() != 5 {
                return Err(MachineProblem::InvalidOperation);
            }
            self.reference(&args[2..3])?;
            let target = self.reference(&args[4..5])?;
            return self.write_reference(&target, &vec![0; target.length]);
        }
        if args.len() < 3 || args[1] != "TO" {
            return Err(MachineProblem::InvalidOperation);
        }
        if args[2] == "TRUE" {
            let condition = self
                .layout(&args[0])
                .filter(|layout| layout.category == LayoutCategory::Condition)
                .cloned()
                .ok_or(MachineProblem::InvalidOperation)?;
            let parent = condition.parent.ok_or(MachineProblem::InvalidOperation)?;
            let value = condition
                .condition_values
                .first()
                .ok_or(MachineProblem::InvalidOperation)?;
            let parent_layout = self
                .layouts
                .get(&parent)
                .cloned()
                .ok_or(MachineProblem::UnknownStorage)?;
            let normalized = normalize(value);
            let value = match normalized.as_str() {
                "SPACE" | "SPACES" => CobolValue::Bytes(vec![b' '; parent_layout.length]),
                "LOW-VALUE" | "LOW-VALUES" => CobolValue::Bytes(vec![0; parent_layout.length]),
                "HIGH-VALUE" | "HIGH-VALUES" => CobolValue::Bytes(vec![0xff; parent_layout.length]),
                "ZERO" | "ZEROS" | "ZEROES" if is_numeric(parent_layout.category) => {
                    CobolValue::Decimal(Decimal {
                        coefficient: 0,
                        scale: parent_layout.scale,
                    })
                }
                "ZERO" | "ZEROS" | "ZEROES" => CobolValue::Bytes(vec![b'0'; parent_layout.length]),
                _ if is_numeric(parent_layout.category) => {
                    CobolValue::Decimal(decimal_text(value).ok_or(MachineProblem::DataException)?)
                }
                _ => CobolValue::Bytes(value.trim_matches(['\'', '"']).as_bytes().to_vec()),
            };
            return self.write_reference_value(&[parent], &value);
        }
        let value = self.eval_value(&args[2..3])?;
        self.write_value(&args[0], &value)
    }

    fn arithmetic(&mut self, name: &str, args: &[String]) -> Result<(), MachineProblem> {
        let rounded = args.iter().any(|argument| argument == "ROUNDED");
        let (target, value) = match name {
            "compute" => {
                let target = args
                    .first()
                    .ok_or(MachineProblem::InvalidOperation)?
                    .clone();
                let equals = position(args, "=").ok_or(MachineProblem::InvalidOperation)?;
                (target, self.eval_expression(&args[equals + 1..])?)
            }
            "add" => {
                let to = position(args, "TO");
                let giving = position(args, "GIVING");
                let operand_end = to.or(giving).ok_or(MachineProblem::InvalidOperation)?;
                if operand_end == 0 {
                    return Err(MachineProblem::InvalidOperation);
                }
                let mut sum = Decimal {
                    coefficient: 0,
                    scale: 0,
                };
                for operand in &args[..operand_end] {
                    sum = decimal_add(sum, self.decimal(operand)?)?;
                }
                if let Some(to) = to {
                    let receiver = args
                        .get(to + 1)
                        .ok_or(MachineProblem::InvalidOperation)?
                        .clone();
                    sum = decimal_add(sum, self.decimal(&receiver)?)?;
                    let target = giving
                        .and_then(|giving| args.get(giving + 1))
                        .cloned()
                        .unwrap_or(receiver);
                    (target, sum)
                } else {
                    let target = giving
                        .and_then(|giving| args.get(giving + 1))
                        .cloned()
                        .ok_or(MachineProblem::InvalidOperation)?;
                    (target, sum)
                }
            }
            "subtract" => {
                let pos = position(args, "FROM").ok_or(MachineProblem::InvalidOperation)?;
                let target = args
                    .get(pos + 1)
                    .ok_or(MachineProblem::InvalidOperation)?
                    .clone();
                (
                    target.clone(),
                    decimal_subtract(self.decimal(&target)?, self.decimal(&args[0])?)?,
                )
            }
            "multiply" => {
                let pos = position(args, "BY").ok_or(MachineProblem::InvalidOperation)?;
                let target = args
                    .get(pos + 1)
                    .ok_or(MachineProblem::InvalidOperation)?
                    .clone();
                (
                    target.clone(),
                    decimal_multiply(self.decimal(&target)?, self.decimal(&args[0])?)?,
                )
            }
            "divide" => {
                let (target, dividend, divisor) = if let Some(pos) = position(args, "INTO") {
                    let target = args
                        .get(pos + 1)
                        .ok_or(MachineProblem::InvalidOperation)?
                        .clone();
                    (
                        target.clone(),
                        self.decimal(&target)?,
                        self.decimal(&args[0])?,
                    )
                } else {
                    let pos = position(args, "BY").ok_or(MachineProblem::InvalidOperation)?;
                    let giving =
                        position(args, "GIVING").ok_or(MachineProblem::InvalidOperation)?;
                    let target = args
                        .get(giving + 1)
                        .ok_or(MachineProblem::InvalidOperation)?
                        .clone();
                    (
                        target,
                        self.decimal(args.first().ok_or(MachineProblem::InvalidOperation)?)?,
                        self.decimal(args.get(pos + 1).ok_or(MachineProblem::InvalidOperation)?)?,
                    )
                };
                let scale = self.layout(&target).map_or(0, |layout| layout.scale);
                (target, decimal_divide(dividend, divisor, scale)?)
            }
            _ => return Err(MachineProblem::InvalidOperation),
        };
        self.write_decimal_mode(&target, value, rounded)
    }
    fn eval_expression(&self, args: &[String]) -> Result<Decimal, MachineProblem> {
        let mut position = 0usize;
        let value = self.expression_additive(args, &mut position)?;
        if position != args.len() {
            return Err(MachineProblem::InvalidOperation);
        }
        Ok(value)
    }

    fn expression_additive(
        &self,
        args: &[String],
        position: &mut usize,
    ) -> Result<Decimal, MachineProblem> {
        let mut value = self.expression_multiplicative(args, position)?;
        while let Some(operator) = args
            .get(*position)
            .filter(|token| matches!(token.as_str(), "+" | "-"))
        {
            *position += 1;
            let right = self.expression_multiplicative(args, position)?;
            value = if operator == "+" {
                decimal_add(value, right)?
            } else {
                decimal_subtract(value, right)?
            };
        }
        Ok(value)
    }

    fn expression_multiplicative(
        &self,
        args: &[String],
        position: &mut usize,
    ) -> Result<Decimal, MachineProblem> {
        let mut value = self.expression_factor(args, position)?;
        while let Some(operator) = args
            .get(*position)
            .filter(|token| matches!(token.as_str(), "*" | "/"))
        {
            *position += 1;
            let right = self.expression_factor(args, position)?;
            value = if operator == "*" {
                decimal_multiply(value, right)?
            } else {
                decimal_divide(value, right, value.scale.max(right.scale).saturating_add(9))?
            };
        }
        Ok(value)
    }

    fn expression_factor(
        &self,
        args: &[String],
        position: &mut usize,
    ) -> Result<Decimal, MachineProblem> {
        let token = args
            .get(*position)
            .ok_or(MachineProblem::InvalidOperation)?;
        if token == "(" {
            *position += 1;
            let value = self.expression_additive(args, position)?;
            if args.get(*position).is_none_or(|token| token != ")") {
                return Err(MachineProblem::InvalidOperation);
            }
            *position += 1;
            return Ok(value);
        }
        if token == "-" {
            *position += 1;
            let mut value = self.expression_factor(args, position)?;
            value.coefficient = value
                .coefficient
                .checked_neg()
                .ok_or(MachineProblem::SizeError)?;
            return Ok(value);
        }
        let start = *position;
        if token == "LENGTH" && args.get(start + 1).is_some_and(|token| token == "OF") {
            let reference_start = start + 2;
            let reference_name = args
                .get(reference_start)
                .ok_or(MachineProblem::InvalidOperation)?;
            *position = reference_start + 1;
            if args.get(*position).is_some_and(|token| token == "(") {
                *position = matching_close(args, *position)
                    .ok_or(MachineProblem::InvalidOperation)?
                    .saturating_add(1);
            }
            if reference_name.is_empty() {
                return Err(MachineProblem::InvalidOperation);
            }
        } else if token == "FUNCTION" {
            let open = args[start..]
                .iter()
                .position(|token| token == "(")
                .map(|offset| start + offset)
                .ok_or(MachineProblem::InvalidOperation)?;
            *position = matching_close(args, open)
                .ok_or(MachineProblem::InvalidOperation)?
                .saturating_add(1);
        } else if args.get(start + 1).is_some_and(|token| token == "(") {
            *position = matching_close(args, start + 1)
                .ok_or(MachineProblem::InvalidOperation)?
                .saturating_add(1);
        } else {
            *position += 1;
        }
        match self.eval_value(&args[start..*position])? {
            CobolValue::Decimal(value) => Ok(value),
            CobolValue::Bytes(bytes) => {
                decimal_text(&String::from_utf8_lossy(&bytes)).ok_or(MachineProblem::DataException)
            }
        }
    }
    fn if_op(&mut self, args: &[String]) -> Result<(), MachineProblem> {
        let action = args
            .iter()
            .position(|arg| matches!(arg.as_str(), "DISPLAY" | "CONTINUE"))
            .unwrap_or(args.len());
        let condition = self.eval_condition(&args[..action])?;
        if condition && action < args.len() && args[action] == "DISPLAY" {
            let value = self.resolve(
                args.get(action + 1)
                    .ok_or(MachineProblem::InvalidOperation)?,
            )?;
            self.append_output(&[value.as_slice(), b"\n"].concat())?;
        }
        Ok(())
    }
    fn evaluate_op(&mut self, args: &[String]) -> Result<(), MachineProblem> {
        let subject = args.first().ok_or(MachineProblem::InvalidOperation)?;
        let value = self.resolve(subject)?;
        let mut index = 1;
        while index + 2 < args.len() {
            if args[index] == "WHEN" && self.resolve(&args[index + 1])? == value {
                if args[index + 2] == "DISPLAY" {
                    let shown = self.resolve(
                        args.get(index + 3)
                            .ok_or(MachineProblem::InvalidOperation)?,
                    )?;
                    self.append_output(&[shown.as_slice(), b"\n"].concat())?;
                }
                break;
            }
            index += 1;
        }
        Ok(())
    }
    fn search_op(&mut self, args: &[String]) -> Result<(), MachineProblem> {
        if args.is_empty() {
            Err(MachineProblem::InvalidOperation)
        } else {
            Ok(())
        }
    }
    fn string_op(&mut self, args: &[String]) -> Result<bool, MachineProblem> {
        let into = position(args, "INTO").ok_or(MachineProblem::InvalidOperation)?;
        let mut value = Vec::new();
        let mut index = 0usize;
        while index < into {
            let mut source = self.resolve(&args[index])?;
            index += 1;
            if args.get(index).is_some_and(|token| token == "DELIMITED") {
                if args.get(index + 1).is_none_or(|token| token != "BY") {
                    return Err(MachineProblem::InvalidOperation);
                }
                let delimiter = args
                    .get(index + 2)
                    .ok_or(MachineProblem::InvalidOperation)?;
                if delimiter != "SIZE" {
                    let delimiter = self.resolve(delimiter)?;
                    if delimiter.is_empty() {
                        return Err(MachineProblem::InvalidOperation);
                    }
                    if let Some(position) = find_bytes(&source, &delimiter) {
                        source.truncate(position);
                    }
                }
                index += 3;
            }
            value.extend(source);
        }
        let target = args.get(into + 1).ok_or(MachineProblem::InvalidOperation)?;
        let overflow = value.len() > self.read(target)?.len();
        self.write(target, &value)?;
        Ok(overflow)
    }
    fn unstring_op(&mut self, args: &[String]) -> Result<bool, MachineProblem> {
        let into = position(args, "INTO").ok_or(MachineProblem::InvalidOperation)?;
        let source = self.resolve(args.first().ok_or(MachineProblem::InvalidOperation)?)?;
        let delimiter = position(args, "DELIMITED")
            .and_then(|index| args.get(index + 2))
            .map(|token| self.resolve(token))
            .transpose()?
            .unwrap_or_else(|| vec![b' ']);
        if delimiter.is_empty() {
            return Err(MachineProblem::InvalidOperation);
        }
        let fields = split_bytes(&source, &delimiter);
        let targets = args[into + 1..]
            .iter()
            .take_while(|target| {
                !matches!(
                    target.as_str(),
                    "WITH" | "POINTER" | "TALLYING" | "ON" | "NOT" | "END-UNSTRING"
                )
            })
            .collect::<Vec<_>>();
        let overflow = fields.len() > targets.len();
        for (target, value) in targets.into_iter().zip(fields) {
            self.write(target, value)?;
        }
        Ok(overflow)
    }
    fn inspect_op(&mut self, args: &[String]) -> Result<(), MachineProblem> {
        if args.is_empty() {
            return Err(MachineProblem::InvalidOperation);
        }
        let control = ["CONVERTING", "REPLACING", "TALLYING"]
            .into_iter()
            .filter_map(|keyword| position(args, keyword))
            .min()
            .unwrap_or(args.len());
        let reference = self.reference(&args[..control])?;
        let source = self.read_reference(&reference)?;
        if let Some(converting) = position(args, "CONVERTING") {
            if args.get(converting + 2).is_none_or(|token| token != "TO") {
                return Err(MachineProblem::InvalidOperation);
            }
            let from = self.resolve(
                args.get(converting + 1)
                    .ok_or(MachineProblem::InvalidOperation)?,
            )?;
            let to = self.resolve(
                args.get(converting + 3)
                    .ok_or(MachineProblem::InvalidOperation)?,
            )?;
            if from.is_empty() || from.len() != to.len() {
                return Err(MachineProblem::DataException);
            }
            let converted = source
                .iter()
                .map(|byte| {
                    from.iter()
                        .position(|candidate| candidate == byte)
                        .map_or(*byte, |index| to[index])
                })
                .collect::<Vec<_>>();
            self.write_reference(&reference, &converted)?;
        } else if let Some(replacing) = position(args, "REPLACING") {
            if args.get(replacing + 1).is_none_or(|token| token != "ALL")
                || args.get(replacing + 3).is_none_or(|token| token != "BY")
            {
                return Err(MachineProblem::UnsupportedForm);
            }
            let from = self.resolve(
                args.get(replacing + 2)
                    .ok_or(MachineProblem::InvalidOperation)?,
            )?;
            let to = self.resolve(
                args.get(replacing + 4)
                    .ok_or(MachineProblem::InvalidOperation)?,
            )?;
            let replaced = replace_bytes(&source, &from, &to)?;
            self.write_reference(&reference, &replaced)?;
        } else if let Some(tallying) = position(args, "TALLYING") {
            let target = args
                .get(tallying + 1)
                .ok_or(MachineProblem::InvalidOperation)?;
            let all = args
                .iter()
                .skip(tallying + 2)
                .position(|token| token == "ALL")
                .map(|offset| tallying + 2 + offset)
                .ok_or(MachineProblem::UnsupportedForm)?;
            let needle =
                self.resolve(args.get(all + 1).ok_or(MachineProblem::InvalidOperation)?)?;
            let count = count_bytes(&source, &needle)?;
            self.write_decimal(
                target,
                Decimal {
                    coefficient: count as i128,
                    scale: 0,
                },
            )?;
        }
        Ok(())
    }

    fn initialize_op(&mut self, args: &[String]) -> Result<(), MachineProblem> {
        let control = args
            .iter()
            .position(|target| matches!(target.as_str(), "REPLACING" | "WITH"))
            .unwrap_or(args.len());
        let mut at = 0usize;
        while at < control {
            let end = (at + 1..=control)
                .rev()
                .find(|end| self.reference(&args[at..*end]).is_ok())
                .ok_or(MachineProblem::UnknownStorage)?;
            let reference = self.reference(&args[at..end])?;
            let layout = reference.layout.clone();
            let targets = if is_group(layout.category) {
                self.layouts
                    .values()
                    .filter(|candidate| {
                        candidate.length > 0
                            && !is_group(candidate.category)
                            && candidate.offset >= layout.offset
                            && candidate.offset.saturating_add(candidate.length)
                                <= layout.offset.saturating_add(layout.length)
                    })
                    .cloned()
                    .collect::<Vec<_>>()
            } else {
                vec![layout]
            };
            for target in targets {
                if is_numeric(target.category) {
                    self.write_decimal(
                        &target.name,
                        Decimal {
                            coefficient: 0,
                            scale: target.scale,
                        },
                    )?;
                } else if is_pointer_like(target.category) {
                    self.write_raw(&target.name, &vec![0; target.length])?;
                } else {
                    self.write_raw(&target.name, &vec![b' '; target.length])?;
                }
            }
            at = end;
        }
        Ok(())
    }
    fn generate(&mut self, args: &[String], json: bool) -> Result<(), MachineProblem> {
        if args.len() < 3 {
            return Err(MachineProblem::InvalidOperation);
        }
        let target = &args[0];
        let from = position(args, "FROM")
            .and_then(|i| args.get(i + 1))
            .ok_or(MachineProblem::InvalidOperation)?;
        let value = String::from_utf8_lossy(&self.resolve(from)?)
            .trim()
            .to_string();
        let generated = if json {
            format!("{{\"{from}\":\"{value}\"}}")
        } else {
            format!("<{from}>{value}</{from}>")
        };
        self.write(target, generated.as_bytes())
    }

    fn parse_generated(&mut self, args: &[String], json: bool) -> Result<(), MachineProblem> {
        if args.len() < 3 {
            return Err(MachineProblem::InvalidOperation);
        }
        let source = String::from_utf8(self.resolve(&args[0])?)
            .map_err(|_| MachineProblem::DataException)?;
        let into = position(args, "INTO").ok_or(MachineProblem::UnsupportedForm)?;
        let target = args.get(into + 1).ok_or(MachineProblem::InvalidOperation)?;
        let value = if json {
            let colon = source.find(':').ok_or(MachineProblem::DataException)?;
            source[colon + 1..]
                .trim()
                .trim_end_matches('}')
                .trim()
                .trim_matches('"')
                .to_string()
        } else {
            let start = source.find('>').ok_or(MachineProblem::DataException)? + 1;
            let end = source[start..]
                .find('<')
                .ok_or(MachineProblem::DataException)?
                + start;
            source[start..end].to_string()
        };
        self.write(target, value.as_bytes())
    }

    fn eval_value(&self, tokens: &[String]) -> Result<CobolValue, MachineProblem> {
        if tokens.is_empty() {
            return Err(MachineProblem::InvalidOperation);
        }
        if tokens.len() == 4 && tokens[0] == "DFHRESP" && tokens[1] == "(" && tokens[3] == ")" {
            let coefficient = match tokens[2].as_str() {
                "NORMAL" => 0,
                "NOTFND" => 13,
                "DUPREC" => 14,
                "DUPKEY" => 15,
                "ENDFILE" => 20,
                _ => return Err(MachineProblem::UnsupportedForm),
            };
            return Ok(CobolValue::Decimal(Decimal {
                coefficient,
                scale: 0,
            }));
        }
        if tokens[0] == "FUNCTION" {
            return self.eval_function(tokens);
        }
        if tokens.first().is_some_and(|token| token == "LENGTH")
            && tokens.get(1).is_some_and(|token| token == "OF")
        {
            let reference = self.reference(&tokens[2..])?;
            return Ok(CobolValue::Decimal(Decimal {
                coefficient: i128::try_from(reference.length)
                    .map_err(|_| MachineProblem::ResourceExhausted)?,
                scale: 0,
            }));
        }
        if tokens.len() == 1
            && self.layout(&tokens[0]).is_none()
            && let Some(value) = self.implicit.get(&normalize(&tokens[0]))
        {
            return Ok(value.clone());
        }
        match self.reference(tokens) {
            Ok(reference) => {
                let bytes = self.read_reference(&reference)?;
                if is_numeric(reference.layout.category)
                    && reference.length == reference.layout.length
                {
                    return match decode_decimal(&reference.layout, &bytes) {
                        Ok(value) => Ok(CobolValue::Decimal(value)),
                        Err(MachineProblem::DataException)
                            if matches!(
                                reference.layout.category,
                                LayoutCategory::NumericDisplay | LayoutCategory::NumericEdited
                            ) =>
                        {
                            Ok(CobolValue::Bytes(bytes))
                        }
                        Err(problem) => Err(problem),
                    };
                }
                return Ok(CobolValue::Bytes(bytes));
            }
            Err(problem)
                if tokens.len() > 1
                    && tokens
                        .iter()
                        .any(|token| matches!(token.as_str(), "(" | "OF" | "IN")) =>
            {
                return Err(problem);
            }
            Err(_) => {}
        }
        if tokens.len() == 1
            && let Some(value) = decimal_text(&tokens[0])
            && !matches!(tokens[0].as_bytes().first(), Some(b'\'' | b'"'))
        {
            return Ok(CobolValue::Decimal(value));
        }
        if tokens.len() == 1 {
            return Ok(CobolValue::Bytes(self.resolve(&tokens[0])?));
        }
        Err(MachineProblem::UnsupportedForm)
    }

    fn eval_function(&self, tokens: &[String]) -> Result<CobolValue, MachineProblem> {
        let name = tokens.get(1).ok_or(MachineProblem::InvalidOperation)?;
        if name == "CURRENT-DATE" && tokens.len() == 2 {
            let value = self
                .invocation
                .bindings
                .get("cobol.current-date")
                .map(|payload| payload.bytes().to_vec())
                .unwrap_or_else(|| b"1970010100000000+0000".to_vec());
            if value.len() != 21 || !value[..16].iter().all(u8::is_ascii_digit) {
                return Err(MachineProblem::DataException);
            }
            return Ok(CobolValue::Bytes(value));
        }
        let open = tokens
            .iter()
            .position(|token| token == "(")
            .ok_or(MachineProblem::InvalidOperation)?;
        let close = matching_close(tokens, open).ok_or(MachineProblem::InvalidOperation)?;
        let arguments = &tokens[open + 1..close];
        match name.as_str() {
            "TRIM" => {
                let bytes = value_bytes(self.eval_value(arguments)?)?;
                Ok(CobolValue::Bytes(
                    String::from_utf8_lossy(&bytes).trim().as_bytes().to_vec(),
                ))
            }
            "UPPER-CASE" => {
                let bytes = value_bytes(self.eval_value(arguments)?)?;
                Ok(CobolValue::Bytes(
                    String::from_utf8_lossy(&bytes)
                        .to_ascii_uppercase()
                        .into_bytes(),
                ))
            }
            "LOWER-CASE" => {
                let bytes = value_bytes(self.eval_value(arguments)?)?;
                Ok(CobolValue::Bytes(
                    String::from_utf8_lossy(&bytes)
                        .to_ascii_lowercase()
                        .into_bytes(),
                ))
            }
            "LENGTH" => {
                let bytes = value_bytes(self.eval_value(arguments)?)?;
                Ok(CobolValue::Decimal(Decimal {
                    coefficient: bytes.len() as i128,
                    scale: 0,
                }))
            }
            "NUMVAL" | "NUMVAL-C" => {
                let bytes = value_bytes(self.eval_value(arguments)?)?;
                let text = String::from_utf8_lossy(&bytes);
                Ok(CobolValue::Decimal(
                    decimal_text(&text).ok_or(MachineProblem::DataException)?,
                ))
            }
            "TEST-NUMVAL" | "TEST-NUMVAL-C" => {
                let bytes = value_bytes(self.eval_value(arguments)?)?;
                let text = String::from_utf8_lossy(&bytes);
                Ok(CobolValue::Decimal(Decimal {
                    coefficient: i128::from(decimal_text(&text).is_none()),
                    scale: 0,
                }))
            }
            "INTEGER-OF-DATE" => {
                let value = match self.eval_value(arguments)? {
                    CobolValue::Decimal(value) if value.scale == 0 => value.coefficient,
                    CobolValue::Bytes(bytes) => {
                        decimal_text(&String::from_utf8_lossy(&bytes))
                            .ok_or(MachineProblem::DataException)?
                            .coefficient
                    }
                    _ => return Err(MachineProblem::DataException),
                };
                let (year, month, day) = split_yyyymmdd(value)?;
                Ok(CobolValue::Decimal(Decimal {
                    coefficient: i128::from(cobol_integer_of_date(year, month, day)?),
                    scale: 0,
                }))
            }
            "DATE-OF-INTEGER" => {
                let value = match self.eval_value(arguments)? {
                    CobolValue::Decimal(value) if value.scale == 0 => value.coefficient,
                    _ => return Err(MachineProblem::DataException),
                };
                let value = i64::try_from(value).map_err(|_| MachineProblem::DataException)?;
                let (year, month, day) = cobol_date_of_integer(value)?;
                Ok(CobolValue::Decimal(Decimal {
                    coefficient: i128::from(year) * 10_000
                        + i128::from(month) * 100
                        + i128::from(day),
                    scale: 0,
                }))
            }
            "MOD" => {
                let split = arguments.iter().position(|token| token == ",").unwrap_or(1);
                let right_start =
                    split + usize::from(arguments.get(split).is_some_and(|token| token == ","));
                let left = self.eval_value(&arguments[..split])?;
                let right = self.eval_value(&arguments[right_start..])?;
                let (CobolValue::Decimal(left), CobolValue::Decimal(right)) = (left, right) else {
                    return Err(MachineProblem::DataException);
                };
                let (left, right) = decimal_aligned(left, right)?;
                if right.coefficient == 0 {
                    return Err(MachineProblem::SizeError);
                }
                Ok(CobolValue::Decimal(Decimal {
                    coefficient: left.coefficient % right.coefficient,
                    scale: left.scale,
                }))
            }
            _ => Err(MachineProblem::UnsupportedForm),
        }
    }

    fn eval_condition(&self, tokens: &[String]) -> Result<bool, MachineProblem> {
        let normalized;
        let tokens = if tokens.windows(2).any(comparison_pair) {
            normalized = normalize_comparison_tokens(tokens);
            normalized.as_slice()
        } else {
            tokens
        };
        let tokens = strip_condition_parentheses(tokens);
        let tokens = if tokens.last().is_some_and(|token| token == "THEN") {
            &tokens[..tokens.len() - 1]
        } else {
            tokens
        };
        if let Some(or) = top_level_position(tokens, "OR") {
            let left = &tokens[..or];
            let right_tokens = &tokens[or + 1..];
            let right = if self.is_standalone_condition(right_tokens)? {
                right_tokens.to_vec()
            } else {
                abbreviated_condition(left, right_tokens)
            };
            return Ok(self.eval_condition(left)? || self.eval_condition(&right)?);
        }
        if let Some(and) = top_level_position(tokens, "AND") {
            let left = &tokens[..and];
            let right_tokens = &tokens[and + 1..];
            let right = if self.is_standalone_condition(right_tokens)? {
                right_tokens.to_vec()
            } else {
                abbreviated_condition(left, right_tokens)
            };
            return Ok(self.eval_condition(left)? && self.eval_condition(&right)?);
        }
        if tokens.first().is_some_and(|token| token == "NOT") {
            return Ok(!self.eval_condition(&tokens[1..])?);
        }
        if let Some(matched) = self.condition_name_matches(tokens)? {
            return Ok(matched);
        }
        if tokens.len() >= 2
            && !tokens.iter().any(|token| {
                matches!(
                    token.as_str(),
                    "=" | "<>" | ">" | "<" | ">=" | "<=" | "EQUAL" | "GREATER" | "LESS"
                )
            })
        {
            let class = tokens.last().map(String::as_str).unwrap_or_default();
            if matches!(
                class,
                "NUMERIC" | "ALPHABETIC" | "POSITIVE" | "NEGATIVE" | "ZERO"
            ) {
                let is = tokens
                    .iter()
                    .position(|token| token == "IS")
                    .unwrap_or(tokens.len() - 1);
                let negate = tokens[..tokens.len() - 1]
                    .iter()
                    .any(|token| token == "NOT");
                let value_end =
                    is - usize::from(tokens[..is].last().is_some_and(|token| token == "NOT"));
                let value = self.eval_value(&tokens[..value_end])?;
                let matched = match class {
                    "NUMERIC" => match value {
                        CobolValue::Decimal(_) => true,
                        CobolValue::Bytes(bytes) => {
                            decimal_text(&String::from_utf8_lossy(&bytes)).is_some()
                        }
                    },
                    "ALPHABETIC" => value_bytes(value)?
                        .iter()
                        .all(|byte| byte.is_ascii_alphabetic() || *byte == b' '),
                    "POSITIVE" => value_decimal(value)?.coefficient > 0,
                    "NEGATIVE" => value_decimal(value)?.coefficient < 0,
                    "ZERO" => value_decimal(value)?.coefficient == 0,
                    _ => false,
                };
                return Ok(matched != negate);
            }
        }
        if tokens.len() < 3 {
            return Err(MachineProblem::UnsupportedForm);
        }
        let operator_index = tokens
            .iter()
            .position(|token| {
                matches!(
                    token.as_str(),
                    "=" | "<>" | ">" | "<" | ">=" | "<=" | "EQUAL" | "GREATER" | "LESS"
                )
            })
            .ok_or(MachineProblem::UnsupportedForm)?;
        let mut operator = tokens[operator_index].as_str();
        let negate = tokens[..operator_index]
            .last()
            .is_some_and(|token| token == "NOT");
        let mut left_end = operator_index - usize::from(negate);
        if tokens[..left_end].last().is_some_and(|token| token == "IS") {
            left_end -= 1;
        }
        let mut right_start = operator_index + 1;
        if tokens.get(right_start).is_some_and(|token| token == "TO") {
            right_start += 1;
        }
        if operator == "EQUAL" {
            operator = "=";
        } else if operator == "GREATER" {
            operator = ">";
            if tokens.get(right_start).is_some_and(|token| token == "THAN") {
                right_start += 1;
            }
        } else if operator == "LESS" {
            operator = "<";
            if tokens.get(right_start).is_some_and(|token| token == "THAN") {
                right_start += 1;
            }
        }
        let left = self.eval_value(&tokens[..left_end])?;
        let right = if matches!(left, CobolValue::Decimal(_))
            && tokens[right_start..].len() == 1
            && matches!(tokens[right_start].as_str(), "ZERO" | "ZEROS" | "ZEROES")
        {
            CobolValue::Decimal(Decimal {
                coefficient: 0,
                scale: 0,
            })
        } else {
            self.eval_value(&tokens[right_start..])?
        };
        match (left, right) {
            (CobolValue::Decimal(left), CobolValue::Decimal(right)) => {
                let (left, right) = decimal_aligned(left, right)?;
                Ok(compare(left.coefficient, operator, right.coefficient) != negate)
            }
            (CobolValue::Bytes(left), CobolValue::Bytes(right)) => {
                let (left, right) = padded_bytes(left, right);
                let matched = match operator {
                    "=" => left == right,
                    "<>" => left != right,
                    ">" => left > right,
                    "<" => left < right,
                    ">=" => left >= right,
                    "<=" => left <= right,
                    _ => false,
                };
                Ok(matched != negate)
            }
            (CobolValue::Decimal(left), CobolValue::Bytes(right))
                if matches!(operator, "=" | "<>") =>
            {
                let left = self
                    .reference(&tokens[..left_end])
                    .and_then(|reference| self.read_reference(&reference))
                    .unwrap_or_else(|_| decimal_string(left).into_bytes());
                let (left, right) = padded_bytes(left, right);
                Ok((if operator == "=" {
                    left == right
                } else {
                    left != right
                }) != negate)
            }
            (CobolValue::Bytes(left), CobolValue::Decimal(right))
                if matches!(operator, "=" | "<>") =>
            {
                let right = self
                    .reference(&tokens[right_start..])
                    .and_then(|reference| self.read_reference(&reference))
                    .unwrap_or_else(|_| decimal_string(right).into_bytes());
                let (left, right) = padded_bytes(left, right);
                Ok((if operator == "=" {
                    left == right
                } else {
                    left != right
                }) != negate)
            }
            _ => Err(MachineProblem::DataException),
        }
    }

    fn condition_name_matches(&self, tokens: &[String]) -> Result<Option<bool>, MachineProblem> {
        let open = tokens.iter().position(|token| token == "(");
        if open.is_some_and(|open| matching_close(tokens, open) != Some(tokens.len() - 1)) {
            return Ok(None);
        }
        let name_end = open.unwrap_or(tokens.len());
        let Some(condition) = self
            .layout_qualified(&tokens[..name_end])
            .filter(|layout| layout.category == LayoutCategory::Condition)
        else {
            return Ok(None);
        };
        let parent = condition
            .parent
            .clone()
            .ok_or(MachineProblem::InvalidOperation)?;
        let values = condition.condition_values.clone();
        let mut parent_reference = vec![parent];
        if let Some(open) = open {
            parent_reference.extend_from_slice(&tokens[open..]);
        }
        let reference = match self.reference(&parent_reference) {
            Ok(reference) => reference,
            Err(MachineProblem::SubscriptError) => return Ok(Some(false)),
            Err(problem) => return Err(problem),
        };
        let actual = self.read_reference(&reference)?;
        Ok(Some(condition_matches(
            &actual,
            &values,
            &reference.layout,
        )?))
    }

    fn is_standalone_condition(&self, tokens: &[String]) -> Result<bool, MachineProblem> {
        if self.condition_name_matches(tokens)?.is_some() {
            return Ok(true);
        }
        Ok(tokens.first().is_some_and(|token| token == "NOT")
            && self.condition_name_matches(&tokens[1..])?.is_some())
    }

    fn write_value(&mut self, target: &str, value: &CobolValue) -> Result<(), MachineProblem> {
        match value {
            CobolValue::Decimal(value) => self.write_decimal(target, *value),
            CobolValue::Bytes(bytes) => {
                if self
                    .layout(target)
                    .is_some_and(|layout| is_numeric(layout.category))
                {
                    let text = String::from_utf8_lossy(bytes);
                    self.write_decimal(
                        target,
                        decimal_text(&text).ok_or(MachineProblem::DataException)?,
                    )
                } else {
                    self.write(target, bytes)
                }
            }
        }
    }

    fn reapply_entry_context(&mut self) -> Result<(), MachineProblem> {
        let entry_initials = self
            .entry_initials
            .iter()
            .map(|(storage, value)| (*storage, value.clone()))
            .collect::<Vec<_>>();
        for (storage, value) in entry_initials {
            self.write_storage(storage, &value)?;
        }
        if let Some(CobolValue::Decimal(value)) = self.implicit.get("EIBCALEN").cloned() {
            self.write_decimal("EIBCALEN", value)?;
        }
        for name in ["EIBAID", "EIBTRNID"] {
            if let Some(CobolValue::Bytes(value)) = self.implicit.get(name).cloned() {
                self.write(name, &value)?;
            }
        }
        Ok(())
    }

    fn write_reference_value(
        &mut self,
        target: &[String],
        value: &CobolValue,
    ) -> Result<(), MachineProblem> {
        let reference = self.reference(target)?;
        let bytes = match value {
            CobolValue::Bytes(bytes) => FixedValue::fit(
                bytes,
                reference.length,
                reference.length == reference.layout.length && reference.layout.justified_right,
            )
            .bytes()
            .to_vec(),
            CobolValue::Decimal(value)
                if reference.length == reference.layout.length
                    && is_numeric(reference.layout.category) =>
            {
                encode_decimal(
                    &reference.layout,
                    decimal_rescale(*value, reference.layout.scale)?,
                )?
            }
            CobolValue::Decimal(value) => {
                FixedValue::fit(decimal_string(*value).as_bytes(), reference.length, false)
                    .bytes()
                    .to_vec()
            }
        };
        self.write_reference(&reference, &bytes)
    }

    fn decimal(&self, token: &str) -> Result<Decimal, MachineProblem> {
        if matches!(normalize(token).as_str(), "ZERO" | "ZEROS" | "ZEROES") {
            return Ok(Decimal {
                coefficient: 0,
                scale: 0,
            });
        }
        if let Some(layout) = self.layout(token) {
            if !is_numeric(layout.category) {
                return Err(MachineProblem::DataException);
            }
            return decode_decimal(layout, &self.read(&layout.name)?);
        }
        decimal_text(token).ok_or(MachineProblem::DataException)
    }

    fn write_decimal(&mut self, target: &str, value: Decimal) -> Result<(), MachineProblem> {
        self.write_decimal_mode(target, value, false)
    }

    fn write_decimal_mode(
        &mut self,
        target: &str,
        value: Decimal,
        rounded: bool,
    ) -> Result<(), MachineProblem> {
        let implicit = normalize(target);
        if let Some(slot) = self.implicit.get_mut(&implicit) {
            *slot = CobolValue::Decimal(value);
            if self.layout(target).is_none() {
                return Ok(());
            }
        }
        let layout = self
            .layout(target)
            .cloned()
            .ok_or(MachineProblem::UnknownStorage)?;
        let value = if rounded {
            decimal_rescale_rounded(value, layout.scale)?
        } else {
            decimal_rescale(value, layout.scale)?
        };
        let bytes = encode_decimal(&layout, value)?;
        self.write_raw(&layout.name, &bytes)
    }

    fn reference(&self, tokens: &[String]) -> Result<ResolvedReference, MachineProblem> {
        if tokens.is_empty() {
            return Err(MachineProblem::UnknownStorage);
        }
        let open = tokens.iter().position(|token| token == "(");
        let name_tokens = &tokens[..open.unwrap_or(tokens.len())];
        let layout = self
            .layout_qualified(name_tokens)
            .cloned()
            .ok_or(MachineProblem::UnknownStorage)?;
        let mut offset = 0usize;
        let mut length = layout.length;
        let mut cursor = open;
        while let Some(open) = cursor {
            let close = matching_close(tokens, open).ok_or(MachineProblem::InvalidOperation)?;
            let contents = &tokens[open + 1..close];
            if contents.iter().any(|token| token.contains(':')) {
                let (start, requested) = split_reference_modification(contents)?;
                let resolve_bound = |tokens: &[String]| -> Result<usize, MachineProblem> {
                    let value = self
                        .eval_expression(tokens)
                        .map_err(|_| MachineProblem::ReferenceModificationError)?;
                    if value.scale != 0 || value.coefficient < 0 {
                        return Err(MachineProblem::ReferenceModificationError);
                    }
                    usize::try_from(value.coefficient)
                        .map_err(|_| MachineProblem::ReferenceModificationError)
                };
                let start = resolve_bound(&start)?;
                let requested = resolve_bound(&requested)?;
                if start == 0 || start - 1 + requested > length {
                    return Err(MachineProblem::ReferenceModificationError);
                }
                offset = offset
                    .checked_add(start - 1)
                    .ok_or(MachineProblem::ReferenceModificationError)?;
                length = requested;
            } else {
                let mut dimensions = std::iter::successors(layout.parent.as_ref(), |parent| {
                    self.layouts
                        .get(*parent)
                        .and_then(|layout| layout.parent.as_ref())
                })
                .filter_map(|parent| self.layouts.get(parent))
                .filter(|layout| layout.occurs > 1)
                .map(|layout| (layout.occurs, layout.element_length))
                .collect::<Vec<_>>();
                dimensions.reverse();
                if layout.occurs > 1 {
                    dimensions.push((layout.occurs, layout.element_length));
                }
                if dimensions.is_empty() {
                    return Err(MachineProblem::SubscriptError);
                }
                let index_tokens = if contents.len() == dimensions.len() {
                    contents
                        .iter()
                        .map(std::slice::from_ref)
                        .collect::<Vec<_>>()
                } else if contents.len() == 1 {
                    vec![contents]
                } else {
                    return Err(MachineProblem::SubscriptError);
                };
                let dimensions = if index_tokens.len() == 1 {
                    &dimensions[dimensions.len() - 1..]
                } else {
                    dimensions.as_slice()
                };
                for (tokens, (occurs, stride)) in index_tokens.into_iter().zip(dimensions) {
                    let index = match self.eval_value(tokens)? {
                        CobolValue::Decimal(value) if value.scale == 0 => {
                            usize::try_from(value.coefficient)
                                .map_err(|_| MachineProblem::SubscriptError)?
                        }
                        _ => return Err(MachineProblem::SubscriptError),
                    };
                    if index == 0 || index > *occurs {
                        return Err(MachineProblem::SubscriptError);
                    }
                    offset = offset
                        .checked_add(
                            (index - 1)
                                .checked_mul(*stride)
                                .ok_or(MachineProblem::SubscriptError)?,
                        )
                        .ok_or(MachineProblem::SubscriptError)?;
                }
                length = layout.element_length.min(layout.length);
            }
            cursor = if close + 1 == tokens.len() {
                None
            } else if tokens.get(close + 1).is_some_and(|token| token == "(") {
                Some(close + 1)
            } else {
                return Err(MachineProblem::InvalidOperation);
            };
        }
        Ok(ResolvedReference {
            layout,
            offset,
            length,
        })
    }

    fn layout_qualified(&self, tokens: &[String]) -> Option<&LayoutMetadata> {
        let simple = tokens.first()?.to_ascii_uppercase();
        if tokens.len() == 1 {
            return self.layout(&simple);
        }
        let qualifiers = tokens
            .iter()
            .skip(1)
            .filter(|token| !matches!(token.as_str(), "OF" | "IN"))
            .map(|token| token.to_ascii_uppercase())
            .collect::<Vec<_>>();
        self.simple_layouts
            .get(&simple)?
            .iter()
            .filter_map(|name| self.layouts.get(name))
            .find(|layout| {
                let ancestors = layout.name.split('.').rev().skip(1).collect::<Vec<_>>();
                let mut at = 0usize;
                for qualifier in &qualifiers {
                    let Some(found) = ancestors[at..]
                        .iter()
                        .position(|ancestor| qualifier == ancestor)
                    else {
                        return false;
                    };
                    at += found + 1;
                }
                true
            })
    }

    fn read_reference(&self, reference: &ResolvedReference) -> Result<Vec<u8>, MachineProblem> {
        let view = self
            .views
            .get(&reference.layout.name)
            .ok_or(MachineProblem::UnknownStorage)?;
        let start = view
            .offset
            .checked_add(reference.offset)
            .ok_or(MachineProblem::ResourceExhausted)?;
        let end = start
            .checked_add(reference.length)
            .ok_or(MachineProblem::ResourceExhausted)?;
        self.bases[view.base]
            .get(start..end)
            .map(<[u8]>::to_vec)
            .ok_or(MachineProblem::SubscriptError)
    }

    fn write_reference(
        &mut self,
        reference: &ResolvedReference,
        value: &[u8],
    ) -> Result<(), MachineProblem> {
        if value.len() != reference.length {
            return Err(MachineProblem::SizeError);
        }
        let view = self
            .views
            .get(&reference.layout.name)
            .ok_or(MachineProblem::UnknownStorage)?
            .clone();
        let start = view
            .offset
            .checked_add(reference.offset)
            .ok_or(MachineProblem::ResourceExhausted)?;
        let end = start
            .checked_add(reference.length)
            .ok_or(MachineProblem::ResourceExhausted)?;
        let target = self.bases[view.base]
            .get_mut(start..end)
            .ok_or(MachineProblem::SubscriptError)?;
        target.copy_from_slice(value);
        Ok(())
    }

    fn resolve(&self, token: &str) -> Result<Vec<u8>, MachineProblem> {
        let clean = token.trim_matches(['\'', '"']);
        if clean != token {
            return Ok(clean.as_bytes().to_vec());
        }
        match normalize(token).as_str() {
            "SPACE" | "SPACES" => return Ok(vec![b' ']),
            "ZERO" | "ZEROS" | "ZEROES" => return Ok(vec![b'0']),
            "LOW-VALUE" | "LOW-VALUES" => return Ok(vec![0]),
            "HIGH-VALUE" | "HIGH-VALUES" => return Ok(vec![0xff]),
            _ => {}
        }
        if self.layout(token).is_none()
            && let Some(value) = self.implicit.get(&normalize(token))
        {
            return value_bytes(value.clone());
        }
        if self.layout(token).is_some() || self.views.contains_key(&normalize(token)) {
            self.read(token)
        } else {
            Ok(token.as_bytes().to_vec())
        }
    }

    fn write_cics_value(&mut self, target: &str, value: &CobolValue) -> Result<(), MachineProblem> {
        let tokens = target
            .split_whitespace()
            .map(str::to_string)
            .collect::<Vec<_>>();
        self.write_reference_value(&tokens, value)
    }
    fn read(&self, name: &str) -> Result<Vec<u8>, MachineProblem> {
        if self.layout(name).is_none()
            && let Some(value) = self.implicit.get(&normalize(name))
        {
            return value_bytes(value.clone());
        }
        let resolved = self
            .layout(name)
            .map(|layout| layout.name.clone())
            .unwrap_or_else(|| normalize(name));
        let view = self
            .views
            .get(&resolved)
            .ok_or(MachineProblem::UnknownStorage)?;
        Ok(self.bases[view.base][view.offset..view.offset + view.length].to_vec())
    }
    fn write(&mut self, name: &str, value: &[u8]) -> Result<(), MachineProblem> {
        let implicit = normalize(name);
        if let Some(current) = self.implicit.get(&implicit) {
            let next = match current {
                CobolValue::Decimal(_) => CobolValue::Decimal(
                    decimal_text(&String::from_utf8_lossy(value))
                        .ok_or(MachineProblem::DataException)?,
                ),
                CobolValue::Bytes(_) => CobolValue::Bytes(value.to_vec()),
            };
            self.implicit.insert(implicit, next);
            if self.layout(name).is_none() {
                return Ok(());
            }
        }
        let resolved = self
            .layout(name)
            .map(|layout| layout.name.clone())
            .unwrap_or_else(|| normalize(name));
        self.write_raw(&resolved, value)
    }

    fn write_raw(&mut self, name: &str, value: &[u8]) -> Result<(), MachineProblem> {
        let justified_right = self
            .layouts
            .get(&normalize(name))
            .is_some_and(|layout| layout.justified_right);
        let view = self
            .views
            .get(&normalize(name))
            .ok_or(MachineProblem::UnknownStorage)?
            .clone();
        let current = &self.bases[view.base][view.offset..view.offset + view.length];
        let numeric = current.iter().all(u8::is_ascii_digit)
            && value
                .iter()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'+' | b'-'));
        if numeric {
            let mut fitted = vec![b'0'; view.length];
            let copy = value.len().min(view.length);
            fitted[view.length - copy..].copy_from_slice(&value[value.len() - copy..]);
            self.bases[view.base][view.offset..view.offset + view.length].copy_from_slice(&fitted);
        } else {
            let fitted = FixedValue::fit(value, view.length, justified_right);
            self.bases[view.base][view.offset..view.offset + view.length]
                .copy_from_slice(fitted.bytes());
        }
        Ok(())
    }

    fn layout(&self, reference: &str) -> Option<&LayoutMetadata> {
        let normalized = normalize(reference);
        if let Some(layout) = self.layouts.get(&normalized) {
            return Some(layout);
        }
        let simple = normalized
            .split([' ', '(', ':'])
            .next()
            .unwrap_or(normalized.as_str());
        let candidates = self.simple_layouts.get(simple)?;
        match candidates.as_slice() {
            [name] => self.layouts.get(name),
            names
                if names.iter().all(|name| {
                    self.layouts
                        .get(name)
                        .is_some_and(|layout| layout.category == LayoutCategory::Condition)
                }) =>
            {
                names.first().and_then(|name| self.layouts.get(name))
            }
            _ => None,
        }
    }
    fn mq_descriptor_field(&self, descriptor: &str, simple_name: &str) -> Option<String> {
        let root = self.layout(descriptor)?.name.clone();
        self.simple_layouts
            .get(&normalize(simple_name))?
            .iter()
            .find(|candidate| {
                let mut current = Some(candidate.as_str());
                while let Some(name) = current {
                    if name == root {
                        return true;
                    }
                    current = self
                        .layouts
                        .get(name)
                        .and_then(|layout| layout.parent.as_deref());
                }
                false
            })
            .cloned()
    }
    fn mq_descriptor_decimal(
        &self,
        descriptor: &str,
        simple_name: &str,
    ) -> Option<Result<i32, MachineProblem>> {
        self.mq_descriptor_field(descriptor, simple_name)
            .map(|target| {
                let value = self.decimal(&target)?;
                if value.scale != 0 {
                    return Err(MachineProblem::DataException);
                }
                i32::try_from(value.coefficient).map_err(|_| MachineProblem::DataException)
            })
    }
    fn mq_descriptor_bytes(
        &self,
        descriptor: &str,
        simple_name: &str,
    ) -> Result<Option<Vec<u8>>, MachineProblem> {
        let Some(target) = self.mq_descriptor_field(descriptor, simple_name) else {
            return Ok(None);
        };
        let value = self.read(&target)?;
        Ok(value.iter().any(|byte| *byte != 0).then_some(value))
    }
    fn mq_queue_name(&self, descriptor: &str) -> Result<String, MachineProblem> {
        let target = self
            .mq_descriptor_field(descriptor, "MQOD-OBJECTNAME")
            .ok_or(MachineProblem::UnknownStorage)?;
        let value = self.read(&target)?;
        let queue = String::from_utf8_lossy(&value)
            .trim_matches([' ', '\0'])
            .to_ascii_uppercase();
        if queue.is_empty() {
            Err(MachineProblem::DataException)
        } else {
            Ok(queue)
        }
    }
    fn write_storage(&mut self, id: StorageId, value: &[u8]) -> Result<(), MachineProblem> {
        let view = self
            .views_by_id
            .get(&id)
            .ok_or(MachineProblem::UnknownStorage)?
            .clone();
        let fitted = FixedValue::fit(value, view.length, false);
        self.bases[view.base][view.offset..view.offset + view.length]
            .copy_from_slice(fitted.bytes());
        Ok(())
    }
    fn append_output(&mut self, bytes: &[u8]) -> Result<(), MachineProblem> {
        let next = self
            .output
            .len()
            .checked_add(bytes.len())
            .ok_or(MachineProblem::ResourceExhausted)?;
        if next > self.invocation.limits.max_output_bytes as usize {
            return Err(MachineProblem::ResourceExhausted);
        }
        self.output.extend_from_slice(bytes);
        Ok(())
    }
    fn label(&self, name: &str) -> Result<usize, MachineProblem> {
        let normalized = normalize(name);
        let target = self.altered.get(&normalized).unwrap_or(&normalized);
        self.labels
            .get(target)
            .copied()
            .ok_or(MachineProblem::UnknownLabel)
    }
    fn complete(&self) -> Result<Completion, MachineProblem> {
        let limits = InvocationLimits {
            max_payload_bytes: self.invocation.limits.max_output_bytes as usize,
            ..InvocationLimits::default()
        };
        Ok(Completion {
            return_code: match self.implicit.get("RETURN-CODE") {
                Some(CobolValue::Decimal(value)) if value.scale == 0 => {
                    i32::try_from(value.coefficient).map_err(|_| MachineProblem::SizeError)?
                }
                _ => 0,
            },
            output: BoundedPayload::new("mainframe-env.output@1", self.output.clone(), limits)
                .map_err(|_| MachineProblem::ResourceExhausted)?,
        })
    }
}

fn abbreviated_condition(left: &[String], right: &[String]) -> Vec<String> {
    if right.iter().any(|token| {
        matches!(
            token.as_str(),
            "=" | "<>" | ">" | "<" | ">=" | "<=" | "EQUAL" | "GREATER" | "LESS"
        )
    }) {
        return right.to_vec();
    }
    let Some(operator) = left.iter().position(|token| {
        matches!(
            token.as_str(),
            "=" | "<>" | ">" | "<" | ">=" | "<=" | "EQUAL" | "GREATER" | "LESS"
        )
    }) else {
        return right.to_vec();
    };
    left[..=operator]
        .iter()
        .cloned()
        .chain(right.iter().cloned())
        .collect()
}

enum Step {
    Next,
    Jump(usize),
    Effect(Box<EffectRequest>),
    Complete,
}

impl Machine for ReferenceMachine {
    type Effect = EffectRequest;
    type EffectResult = EffectResult;
    fn drive(
        &mut self,
        resume: MachineResume<Self::EffectResult>,
        quantum: Quantum,
    ) -> MachineDrive<Self::Effect> {
        let result = (|| -> Result<MachineDrive<Self::Effect>, MachineProblem> {
            match resume {
                MachineResume::Start if self.pending.is_none() => {}
                MachineResume::HostResult(result) => self.resume_host(result)?,
                MachineResume::Cancelled => {
                    return Ok(failure_drive(
                        FailureCategory::Cancelled,
                        "execution cancelled",
                    ));
                }
                MachineResume::TimedOut => {
                    return Ok(failure_drive(
                        FailureCategory::TimedOut,
                        "execution timed out",
                    ));
                }
                _ => return Err(MachineProblem::UnexpectedResume),
            }
            if let Some(drive) = self.deferred_drive.take() {
                return Ok(drive);
            }
            let mut steps = 0;
            while steps < quantum.max_steps {
                if self.pc >= self.operations.len() {
                    return Ok(MachineDrive::Completed(self.complete()?));
                }
                self.executed_steps = self
                    .executed_steps
                    .checked_add(1)
                    .ok_or(MachineProblem::ResourceExhausted)?;
                if self.executed_steps > self.invocation.limits.max_steps {
                    return Err(MachineProblem::ResourceExhausted);
                }
                let operation = self.operations[self.pc].clone();
                match self.execute(&operation).map_err(|problem| match problem {
                    MachineProblem::Host(_) | MachineProblem::ResourceExhausted => problem,
                    MachineProblem::UnsupportedForm => MachineProblem::UnsupportedForm,
                    problem => MachineProblem::InvalidArtifact(format!(
                        "operation {} {:?} failed: {problem:?}",
                        operation.identity.name(),
                        arguments(&operation)
                    )),
                })? {
                    Step::Next => {
                        if self.at_perform_endpoint() {
                            self.pc = self.finish_perform()?;
                        } else {
                            self.pc += 1;
                        }
                    }
                    Step::Jump(target) => self.pc = target,
                    Step::Effect(effect) => {
                        self.pc = if self.at_perform_endpoint() {
                            self.finish_perform()?
                        } else {
                            self.pc + 1
                        };
                        return Ok(MachineDrive::HostCall(*effect));
                    }
                    Step::Complete => return Ok(MachineDrive::Completed(self.complete()?)),
                }
                steps += 1;
            }
            Ok(MachineDrive::Continue)
        })();
        match result {
            Ok(drive) => drive,
            Err(problem) => MachineDrive::Failed(problem.execution_problem()),
        }
    }

    fn checkpoint(&self) -> Option<BoundedPayload> {
        if self.pending.is_some() {
            return None;
        }
        let bytes = encode_snapshot(&self.snapshot())?;
        BoundedPayload::new(
            "mainframe-env.reference-machine-checkpoint@7",
            bytes,
            InvocationLimits {
                max_payload_bytes: usize::try_from(
                    self.invocation
                        .limits
                        .max_storage_bytes
                        .saturating_add(self.invocation.limits.max_output_bytes)
                        .saturating_add(1024 * 1024),
                )
                .ok()?,
                ..InvocationLimits::default()
            },
        )
        .ok()
    }

    fn effect_sequence(&self) -> u64 {
        self.effect_sequence
    }
}

fn encode_snapshot(snapshot: &MachineSnapshot) -> Option<Vec<u8>> {
    let mut bytes = b"MECP0007".to_vec();
    bytes.extend_from_slice(&snapshot.schema_version.to_be_bytes());
    bytes.extend_from_slice(&u64::try_from(snapshot.program_counter).ok()?.to_be_bytes());
    bytes.extend_from_slice(&snapshot.effect_sequence.to_be_bytes());
    bytes.extend_from_slice(&snapshot.executed_steps.to_be_bytes());
    push_bytes(&mut bytes, &snapshot.output)?;
    bytes.extend_from_slice(
        &u32::try_from(snapshot.base_storage.len())
            .ok()?
            .to_be_bytes(),
    );
    for storage in &snapshot.base_storage {
        push_bytes(&mut bytes, storage)?;
    }
    bytes.extend_from_slice(
        &u32::try_from(snapshot.perform_stack.len())
            .ok()?
            .to_be_bytes(),
    );
    for target in &snapshot.perform_stack {
        bytes.extend_from_slice(&u64::try_from(*target).ok()?.to_be_bytes());
    }
    bytes.extend_from_slice(
        &u32::try_from(snapshot.altered_targets.len())
            .ok()?
            .to_be_bytes(),
    );
    for (from, to) in &snapshot.altered_targets {
        push_bytes(&mut bytes, from.as_bytes())?;
        push_bytes(&mut bytes, to.as_bytes())?;
    }
    bytes.extend_from_slice(
        &u32::try_from(snapshot.loop_reentry.len())
            .ok()?
            .to_be_bytes(),
    );
    for node in &snapshot.loop_reentry {
        bytes.extend_from_slice(&u64::try_from(*node).ok()?.to_be_bytes());
    }
    bytes.extend_from_slice(
        &u32::try_from(snapshot.loop_counts.len())
            .ok()?
            .to_be_bytes(),
    );
    for (node, count) in &snapshot.loop_counts {
        bytes.extend_from_slice(&u64::try_from(*node).ok()?.to_be_bytes());
        bytes.extend_from_slice(&count.to_be_bytes());
    }
    push_bytes(&mut bytes, snapshot.last_file_status.as_bytes())?;
    bytes.extend_from_slice(
        &u32::try_from(snapshot.dataset_cursors.len())
            .ok()?
            .to_be_bytes(),
    );
    for (dataset, cursor) in &snapshot.dataset_cursors {
        push_bytes(&mut bytes, dataset.as_bytes())?;
        push_bytes(&mut bytes, cursor.as_bytes())?;
    }
    bytes.push(snapshot.condition_statuses);
    Some(bytes)
}

fn push_bytes(output: &mut Vec<u8>, value: &[u8]) -> Option<()> {
    output.extend_from_slice(&u64::try_from(value.len()).ok()?.to_be_bytes());
    output.extend_from_slice(value);
    Some(())
}

fn decode_snapshot(
    bytes: &[u8],
    max_storage: usize,
    max_output: usize,
    max_frames: usize,
) -> Result<MachineSnapshot, MachineProblem> {
    let mut input = SnapshotInput::new(bytes);
    let header = input.take(8)?;
    let header_version = match header {
        b"MECP0001" => 1,
        b"MECP0002" => 2,
        b"MECP0003" => 3,
        b"MECP0004" => 4,
        b"MECP0005" => 5,
        b"MECP0006" => 6,
        b"MECP0007" => 7,
        _ => return Err(MachineProblem::IncompatibleSnapshot),
    };
    let schema_version = input.u32()?;
    if schema_version != header_version {
        return Err(MachineProblem::IncompatibleSnapshot);
    }
    let program_counter =
        usize::try_from(input.u64()?).map_err(|_| MachineProblem::IncompatibleSnapshot)?;
    let effect_sequence = input.u64()?;
    let executed_steps = if header_version >= 6 { input.u64()? } else { 0 };
    let output = input.bytes(max_output)?;
    let base_count =
        usize::try_from(input.u32()?).map_err(|_| MachineProblem::IncompatibleSnapshot)?;
    if base_count > max_frames.saturating_mul(1024) {
        return Err(MachineProblem::IncompatibleSnapshot);
    }
    let mut remaining_storage = max_storage;
    let mut base_storage = Vec::with_capacity(base_count);
    for _ in 0..base_count {
        let storage = input.bytes(remaining_storage)?;
        remaining_storage = remaining_storage
            .checked_sub(storage.len())
            .ok_or(MachineProblem::IncompatibleSnapshot)?;
        base_storage.push(storage);
    }
    let stack_count =
        usize::try_from(input.u32()?).map_err(|_| MachineProblem::IncompatibleSnapshot)?;
    if stack_count > max_frames {
        return Err(MachineProblem::IncompatibleSnapshot);
    }
    let mut perform_stack = Vec::with_capacity(stack_count);
    for _ in 0..stack_count {
        perform_stack
            .push(usize::try_from(input.u64()?).map_err(|_| MachineProblem::IncompatibleSnapshot)?);
    }
    let altered_count =
        usize::try_from(input.u32()?).map_err(|_| MachineProblem::IncompatibleSnapshot)?;
    if altered_count > max_frames.saturating_mul(4) {
        return Err(MachineProblem::IncompatibleSnapshot);
    }
    let mut altered_targets = BTreeMap::new();
    for _ in 0..altered_count {
        let from = String::from_utf8(input.bytes(4096)?)
            .map_err(|_| MachineProblem::IncompatibleSnapshot)?;
        let to = String::from_utf8(input.bytes(4096)?)
            .map_err(|_| MachineProblem::IncompatibleSnapshot)?;
        if altered_targets.insert(from, to).is_some() {
            return Err(MachineProblem::IncompatibleSnapshot);
        }
    }
    let mut loop_reentry = BTreeSet::new();
    let mut loop_counts = BTreeMap::new();
    if header_version >= 2 {
        let reentry_count =
            usize::try_from(input.u32()?).map_err(|_| MachineProblem::IncompatibleSnapshot)?;
        if reentry_count > max_frames {
            return Err(MachineProblem::IncompatibleSnapshot);
        }
        for _ in 0..reentry_count {
            let node =
                usize::try_from(input.u64()?).map_err(|_| MachineProblem::IncompatibleSnapshot)?;
            if !loop_reentry.insert(node) {
                return Err(MachineProblem::IncompatibleSnapshot);
            }
        }
        let loop_count =
            usize::try_from(input.u32()?).map_err(|_| MachineProblem::IncompatibleSnapshot)?;
        if loop_count > max_frames {
            return Err(MachineProblem::IncompatibleSnapshot);
        }
        for _ in 0..loop_count {
            let node =
                usize::try_from(input.u64()?).map_err(|_| MachineProblem::IncompatibleSnapshot)?;
            let count = i128::from_be_bytes(
                input
                    .take(16)?
                    .try_into()
                    .map_err(|_| MachineProblem::IncompatibleSnapshot)?,
            );
            if loop_counts.insert(node, count).is_some() {
                return Err(MachineProblem::IncompatibleSnapshot);
            }
        }
    }
    let last_file_status = if header_version >= 4 {
        String::from_utf8(input.bytes(2)?).map_err(|_| MachineProblem::IncompatibleSnapshot)?
    } else {
        "00".into()
    };
    let mut dataset_cursors = BTreeMap::new();
    if header_version >= 5 {
        let cursor_count =
            usize::try_from(input.u32()?).map_err(|_| MachineProblem::IncompatibleSnapshot)?;
        if cursor_count > max_frames.saturating_mul(4) {
            return Err(MachineProblem::IncompatibleSnapshot);
        }
        for _ in 0..cursor_count {
            let dataset = String::from_utf8(input.bytes(128)?)
                .map_err(|_| MachineProblem::IncompatibleSnapshot)?;
            let cursor = String::from_utf8(input.bytes(128)?)
                .map_err(|_| MachineProblem::IncompatibleSnapshot)?;
            if dataset.is_empty()
                || cursor.is_empty()
                || dataset_cursors.insert(dataset, cursor).is_some()
            {
                return Err(MachineProblem::IncompatibleSnapshot);
            }
        }
    }
    let condition_statuses = if header_version >= 7 {
        *input
            .take(1)?
            .first()
            .ok_or(MachineProblem::IncompatibleSnapshot)?
    } else {
        0
    };
    if ConditionStatus::from_bits(condition_statuses).is_none() {
        return Err(MachineProblem::IncompatibleSnapshot);
    }
    if !input.finished() {
        return Err(MachineProblem::IncompatibleSnapshot);
    }
    Ok(MachineSnapshot {
        schema_version,
        program_counter,
        effect_sequence,
        executed_steps,
        output,
        base_storage,
        perform_stack,
        altered_targets,
        loop_reentry,
        loop_counts,
        last_file_status,
        dataset_cursors,
        condition_statuses,
    })
}

struct SnapshotInput<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> SnapshotInput<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], MachineProblem> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or(MachineProblem::IncompatibleSnapshot)?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(MachineProblem::IncompatibleSnapshot)?;
        self.offset = end;
        Ok(value)
    }

    fn u32(&mut self) -> Result<u32, MachineProblem> {
        Ok(u32::from_be_bytes(
            self.take(4)?
                .try_into()
                .map_err(|_| MachineProblem::IncompatibleSnapshot)?,
        ))
    }

    fn u64(&mut self) -> Result<u64, MachineProblem> {
        Ok(u64::from_be_bytes(
            self.take(8)?
                .try_into()
                .map_err(|_| MachineProblem::IncompatibleSnapshot)?,
        ))
    }

    fn bytes(&mut self, max: usize) -> Result<Vec<u8>, MachineProblem> {
        let length =
            usize::try_from(self.u64()?).map_err(|_| MachineProblem::IncompatibleSnapshot)?;
        if length > max {
            return Err(MachineProblem::IncompatibleSnapshot);
        }
        Ok(self.take(length)?.to_vec())
    }

    fn finished(&self) -> bool {
        self.offset == self.bytes.len()
    }
}

fn cics_arguments(tokens: &[String]) -> Result<BTreeMap<String, BoundedPayload>, MachineProblem> {
    let mut arguments = BTreeMap::new();
    let command = tokens
        .iter()
        .position(|token| {
            !matches!(
                token.to_ascii_uppercase().as_str(),
                "EXEC" | "CICS" | "END-EXEC"
            )
        })
        .ok_or(MachineProblem::InvalidOperation)?;
    let first = tokens[command].to_ascii_uppercase();
    let mut index = command + 1;
    if matches!(first.as_str(), "HANDLE" | "RECEIVE" | "SEND" | "WRITEQ")
        && tokens.get(index).is_some_and(|token| {
            matches!(
                token.to_ascii_uppercase().as_str(),
                "ABEND" | "CONDITION" | "MAP" | "TEXT" | "TD"
            )
        })
    {
        let subcommand = tokens[index].to_ascii_uppercase();
        index += 1;
        if tokens.get(index).is_some_and(|token| token == "(") {
            let end = matching_close(tokens, index).ok_or(MachineProblem::InvalidOperation)?;
            let raw = tokens[index + 1..end].join(" ");
            let literal = raw.starts_with(['\'', '"']) && raw.ends_with(['\'', '"']);
            arguments.insert(
                subcommand,
                BoundedPayload::new(
                    if literal {
                        "mainframe-env.cics.literal@1"
                    } else {
                        "mainframe-env.cics.argument@1"
                    },
                    raw.trim_matches(['\'', '"']).as_bytes().to_vec(),
                    InvocationLimits::default(),
                )
                .map_err(|_| MachineProblem::ResourceExhausted)?,
            );
            index = end + 1;
        }
    }
    while index < tokens.len() {
        let key = tokens[index].to_ascii_uppercase();
        if key == "END-EXEC" {
            break;
        }
        if tokens.get(index + 1).is_some_and(|token| token == "(") {
            let end = matching_close(tokens, index + 1).ok_or(MachineProblem::InvalidOperation)?;
            let raw = tokens[index + 2..end].join(" ");
            let literal = raw.starts_with(['\'', '"']) && raw.ends_with(['\'', '"']);
            let value = raw.trim_matches(['\'', '"']).to_string();
            arguments.insert(
                key,
                BoundedPayload::new(
                    if literal {
                        "mainframe-env.cics.literal@1"
                    } else {
                        "mainframe-env.cics.argument@1"
                    },
                    value.into_bytes(),
                    InvocationLimits::default(),
                )
                .map_err(|_| MachineProblem::ResourceExhausted)?,
            );
            index = end + 1;
        } else {
            arguments.insert(
                format!("OPTION.{key}"),
                BoundedPayload::new(
                    "mainframe-env.cics.option@1",
                    Vec::new(),
                    InvocationLimits::default(),
                )
                .map_err(|_| MachineProblem::ResourceExhausted)?,
            );
            index += 1;
        }
    }
    Ok(arguments)
}

fn cics_destination(arguments: &BTreeMap<String, BoundedPayload>, key: &str) -> Option<String> {
    arguments
        .get(key)
        .map(|value| String::from_utf8_lossy(value.bytes()).into_owned())
}

type StorageState = (
    Vec<Vec<u8>>,
    BTreeMap<String, StorageView>,
    BTreeMap<StorageId, StorageView>,
);

fn storage(module: &Module, max: u64) -> Result<StorageState, MachineProblem> {
    let total = module
        .storage()
        .iter()
        .filter(|item| item.alias_of.is_none())
        .try_fold(0u64, |sum, item| sum.checked_add(item.size))
        .ok_or(MachineProblem::ResourceExhausted)?;
    if total > max {
        return Err(MachineProblem::ResourceExhausted);
    }
    let mut bases: Vec<Vec<u8>> = Vec::new();
    let mut by_id: BTreeMap<StorageId, StorageView> = BTreeMap::new();
    let mut names: BTreeMap<String, StorageView> = BTreeMap::new();
    for item in module.storage() {
        let view = if let Some(alias) = &item.alias_of {
            let target = by_id
                .get(&alias.storage)
                .ok_or(MachineProblem::InvalidArtifact("forward alias".into()))?;
            StorageView {
                base: target.base,
                offset: target.offset + alias.offset as usize,
                length: alias.length as usize,
            }
        } else {
            let base = bases.len();
            bases.push(vec![0; item.size as usize]);
            StorageView {
                base,
                offset: 0,
                length: item.size as usize,
            }
        };
        by_id.insert(item.id, view.clone());
        names.insert(item.name.to_ascii_uppercase(), view);
    }
    Ok((bases, names, by_id))
}
fn validate_module(module: &Module) -> Result<(), MachineProblem> {
    let supported = supported_operations();
    let operations: Vec<_> = module
        .regions()
        .iter()
        .flat_map(|region| &region.blocks)
        .flat_map(|block| &block.operations)
        .collect();
    if operations.is_empty()
        || operations
            .last()
            .is_none_or(|op| op.identity.name() != "halt")
        || operations
            .iter()
            .any(|op| !supported.contains(&op.identity))
        || operations.iter().any(|operation| {
            operation
                .attributes
                .get("arguments")
                .is_some_and(|attribute| match attribute {
                    Attribute::Bytes(bytes) => decode_arguments(bytes).is_none(),
                    _ => true,
                })
        })
    {
        return Err(MachineProblem::InvalidArtifact(
            "illegal operation or terminator".into(),
        ));
    }
    Ok(())
}
pub fn supported_operations() -> &'static BTreeSet<OperationIdentity> {
    static SET: OnceLock<BTreeSet<OperationIdentity>> = OnceLock::new();
    SET.get_or_init(|| {
        let names = [
            "init",
            "define",
            "control",
            "file",
            "accept",
            "add",
            "allocate",
            "alter",
            "call",
            "cancel",
            "close",
            "compute",
            "continue",
            "display",
            "divide",
            "entry",
            "evaluate",
            "exec_cics",
            "exec_dli",
            "exec_sql",
            "exit",
            "free",
            "go_back",
            "go_to",
            "if",
            "initialize",
            "inspect",
            "json_generate",
            "json_parse",
            "move",
            "multiply",
            "next_sentence",
            "open",
            "perform",
            "read",
            "rewrite",
            "search",
            "set",
            "stop_run",
            "string",
            "subtract",
            "unstring",
            "write",
            "xml_generate",
            "xml_parse",
            "label",
            "halt",
        ];
        names
            .into_iter()
            .map(|name| OperationIdentity::new(NAMESPACE, name, 1).expect("static operation"))
            .collect()
    })
}
fn arguments(operation: &Operation) -> Vec<String> {
    if let Some(Attribute::Bytes(bytes)) = operation.attributes.get("arguments") {
        return decode_arguments(bytes).unwrap_or_default();
    }
    operation
        .attributes
        .iter()
        .filter_map(|(name, value)| name.starts_with("arg_").then_some(value))
        .filter_map(|value| match value {
            Attribute::Text(text) => Some(text.clone()),
            _ => None,
        })
        .collect()
}

fn decode_arguments(bytes: &[u8]) -> Option<Vec<String>> {
    let mut offset = 0usize;
    let mut arguments = Vec::new();
    while offset < bytes.len() {
        let length = u64::from_be_bytes(bytes.get(offset..offset + 8)?.try_into().ok()?);
        offset += 8;
        let length = usize::try_from(length).ok()?;
        let end = offset.checked_add(length)?;
        arguments.push(String::from_utf8(bytes.get(offset..end)?.to_vec()).ok()?);
        if arguments.len() > 65_536 {
            return None;
        }
        offset = end;
    }
    Some(arguments)
}

type LayoutState = (
    BTreeMap<String, LayoutMetadata>,
    BTreeMap<String, Vec<String>>,
);

fn layout_metadata(operations: &[Operation]) -> Result<LayoutState, MachineProblem> {
    let mut layouts = BTreeMap::new();
    let mut simple = BTreeMap::<String, Vec<String>>::new();
    for operation in operations
        .iter()
        .filter(|operation| operation.identity.name() == "define")
    {
        let name = text_attribute(operation, "name")?.to_ascii_uppercase();
        let simple_name = text_attribute(operation, "simple_name")?.to_ascii_uppercase();
        let category = match text_attribute(operation, "category")? {
            "alphabetic" => LayoutCategory::Alphabetic,
            "alphanumeric" => LayoutCategory::Alphanumeric,
            "alphanumeric_edited" => LayoutCategory::AlphanumericEdited,
            "dbcs" => LayoutCategory::Dbcs,
            "national" => LayoutCategory::National,
            "national_edited" => LayoutCategory::NationalEdited,
            "utf8" => LayoutCategory::Utf8,
            "numeric_display" => LayoutCategory::NumericDisplay,
            "numeric_edited" => LayoutCategory::NumericEdited,
            "packed_decimal" => LayoutCategory::PackedDecimal,
            "binary" => LayoutCategory::Binary,
            "float_short" => LayoutCategory::FloatShort,
            "float_long" => LayoutCategory::FloatLong,
            "index" => LayoutCategory::Index,
            "pointer" => LayoutCategory::Pointer,
            "pointer_32" => LayoutCategory::Pointer32,
            "procedure_pointer" => LayoutCategory::ProcedurePointer,
            "function_pointer" => LayoutCategory::FunctionPointer,
            "object_reference" => LayoutCategory::ObjectReference,
            "group" => LayoutCategory::Group,
            "national_group" => LayoutCategory::NationalGroup,
            "utf8_group" => LayoutCategory::Utf8Group,
            "condition" => LayoutCategory::Condition,
            "rename" => LayoutCategory::Rename,
            _ => {
                return Err(MachineProblem::InvalidArtifact(
                    "unknown layout category".into(),
                ));
            }
        };
        let metadata = LayoutMetadata {
            name: name.clone(),
            simple_name: simple_name.clone(),
            category,
            picture: text_attribute(operation, "picture")?.to_string(),
            digits: usize_attribute(operation, "digits")?,
            scale: u32::try_from(usize_attribute(operation, "scale")?)
                .map_err(|_| MachineProblem::InvalidOperation)?,
            signed: integer_attribute(operation, "signed")? != 0,
            sign_separate: integer_attribute(operation, "sign_separate")? != 0,
            justified_right: optional_integer_attribute(operation, "justified_right")
                .unwrap_or_default()
                != 0,
            linkage: optional_text_attribute(operation, "section") == Some("linkage"),
            offset: usize_attribute(operation, "offset")?,
            length: usize_attribute(operation, "length")?,
            element_length: usize_attribute(operation, "element_length")?,
            occurs: usize_attribute(operation, "occurs")?,
            parent: match text_attribute(operation, "parent")? {
                "" => None,
                parent => Some(parent.to_ascii_uppercase()),
            },
            condition_values: text_attribute(operation, "condition_values")?
                .split('\u{1f}')
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect(),
        };
        if layouts.insert(name.clone(), metadata).is_some() {
            return Err(MachineProblem::InvalidArtifact(
                "duplicate layout metadata".into(),
            ));
        }
        if simple_name != "FILLER" {
            simple.entry(simple_name).or_default().push(name);
        }
    }
    Ok((layouts, simple))
}

fn file_metadata(
    operations: &[Operation],
) -> Result<BTreeMap<String, FileMetadata>, MachineProblem> {
    let mut files = BTreeMap::new();
    for operation in operations
        .iter()
        .filter(|operation| operation.identity.name() == "file")
    {
        let name = text_attribute(operation, "name")?.to_ascii_uppercase();
        let optional = |field: &str| -> Result<Option<String>, MachineProblem> {
            Ok(match text_attribute(operation, field)? {
                "" => None,
                value => Some(value.to_ascii_uppercase()),
            })
        };
        let metadata = FileMetadata {
            assignment: text_attribute(operation, "assignment")?.to_ascii_uppercase(),
            record_name: optional("record_name")?,
            organization: text_attribute(operation, "organization")?.to_ascii_uppercase(),
            access_mode: text_attribute(operation, "access_mode")?.to_ascii_uppercase(),
            record_key: optional("record_key")?,
            alternate_record_keys: text_attribute(operation, "alternate_record_keys")?
                .split('\u{1f}')
                .filter(|value| !value.is_empty())
                .map(str::to_ascii_uppercase)
                .collect(),
            relative_key: optional("relative_key")?,
            file_status: optional("file_status")?,
        };
        if files.insert(name, metadata).is_some() {
            return Err(MachineProblem::InvalidArtifact(
                "duplicate file metadata".into(),
            ));
        }
    }
    Ok(files)
}

fn text_attribute<'a>(operation: &'a Operation, name: &str) -> Result<&'a str, MachineProblem> {
    operation
        .attributes
        .get(name)
        .and_then(|attribute| match attribute {
            Attribute::Text(value) => Some(value.as_str()),
            _ => None,
        })
        .ok_or(MachineProblem::InvalidOperation)
}

fn optional_text_attribute<'a>(operation: &'a Operation, name: &str) -> Option<&'a str> {
    operation
        .attributes
        .get(name)
        .and_then(|attribute| match attribute {
            Attribute::Text(value) => Some(value.as_str()),
            _ => None,
        })
}

fn integer_attribute(operation: &Operation, name: &str) -> Result<i64, MachineProblem> {
    operation
        .attributes
        .get(name)
        .and_then(|attribute| match attribute {
            Attribute::Integer(value) => Some(*value),
            _ => None,
        })
        .ok_or(MachineProblem::InvalidOperation)
}

fn optional_integer_attribute(operation: &Operation, name: &str) -> Option<i64> {
    operation
        .attributes
        .get(name)
        .and_then(|attribute| match attribute {
            Attribute::Integer(value) => Some(*value),
            _ => None,
        })
}

fn usize_attribute(operation: &Operation, name: &str) -> Result<usize, MachineProblem> {
    usize::try_from(integer_attribute(operation, name)?)
        .map_err(|_| MachineProblem::InvalidOperation)
}
fn bytes_attribute<'a>(operation: &'a Operation, name: &str) -> Result<&'a [u8], MachineProblem> {
    operation
        .attributes
        .get(name)
        .and_then(|value| match value {
            Attribute::Bytes(bytes) => Some(bytes.as_slice()),
            _ => None,
        })
        .ok_or(MachineProblem::InvalidOperation)
}
fn position(args: &[String], needle: &str) -> Option<usize> {
    args.iter().position(|arg| arg.eq_ignore_ascii_case(needle))
}
fn normalize(value: &str) -> String {
    value
        .trim_matches(['\'', '"', '.', ','])
        .to_ascii_uppercase()
}

fn alternate_dd_name(assignment: &str, ordinal: usize) -> String {
    let suffix = ordinal.to_string();
    let keep = 8usize.saturating_sub(suffix.len());
    let mut name = normalize(assignment).chars().take(keep).collect::<String>();
    name.push_str(&suffix);
    name
}

fn control_tokens(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    for character in text.chars() {
        if matches!(character, '\'' | '"') {
            current.push(character);
            if quote == Some(character) {
                quote = None;
                tokens.push(std::mem::take(&mut current));
            } else if quote.is_none() {
                if current.len() > 1 {
                    let quote = current.pop().expect("just pushed quote");
                    tokens.push(std::mem::take(&mut current));
                    current.push(quote);
                }
                quote = Some(character);
            }
        } else if quote.is_some() {
            current.push(character);
        } else if character.is_whitespace() || matches!(character, ',' | '(' | ')' | '=') {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current).to_ascii_uppercase());
            }
            if matches!(character, '(' | ')' | '=') {
                tokens.push(character.to_string());
            }
        } else {
            current.push(character);
        }
    }
    if !current.is_empty() {
        tokens.push(if quote.is_some() {
            current
        } else {
            current.to_ascii_uppercase()
        });
    }
    tokens
}
fn compare(left: i128, operator: &str, right: i128) -> bool {
    match operator {
        "=" => left == right,
        ">" => left > right,
        "<" => left < right,
        ">=" => left >= right,
        "<=" => left <= right,
        "<>" => left != right,
        _ => false,
    }
}

fn matching_close(tokens: &[String], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().skip(open) {
        match token.as_str() {
            "(" => depth = depth.checked_add(1)?,
            ")" => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

fn split_reference_modification(
    tokens: &[String],
) -> Result<(Vec<String>, Vec<String>), MachineProblem> {
    let mut start = Vec::new();
    let mut requested = Vec::new();
    let mut found = false;
    for token in tokens {
        if !found {
            if let Some((before, after)) = token.split_once(':') {
                if !before.is_empty() {
                    start.push(before.to_string());
                }
                if !after.is_empty() {
                    requested.push(after.to_string());
                }
                found = true;
            } else {
                start.push(token.clone());
            }
        } else {
            requested.push(token.clone());
        }
    }
    if !found || start.is_empty() || requested.is_empty() {
        return Err(MachineProblem::ReferenceModificationError);
    }
    Ok((start, requested))
}

fn value_bytes(value: CobolValue) -> Result<Vec<u8>, MachineProblem> {
    match value {
        CobolValue::Bytes(bytes) => Ok(bytes),
        CobolValue::Decimal(value) => Ok(decimal_string(value).into_bytes()),
    }
}

fn value_decimal(value: CobolValue) -> Result<Decimal, MachineProblem> {
    match value {
        CobolValue::Decimal(value) => Ok(value),
        CobolValue::Bytes(bytes) => {
            decimal_text(&String::from_utf8_lossy(&bytes)).ok_or(MachineProblem::DataException)
        }
    }
}

fn strip_condition_parentheses(mut tokens: &[String]) -> &[String] {
    while tokens.first().is_some_and(|token| token == "(")
        && matching_close(tokens, 0) == Some(tokens.len().saturating_sub(1))
    {
        tokens = &tokens[1..tokens.len() - 1];
    }
    tokens
}

fn top_level_position(tokens: &[String], needle: &str) -> Option<usize> {
    let mut depth = 0usize;
    let mut found = None;
    for (index, token) in tokens.iter().enumerate() {
        match token.as_str() {
            "(" => depth = depth.saturating_add(1),
            ")" => depth = depth.saturating_sub(1),
            _ if depth == 0 && token == needle => found = Some(index),
            _ => {}
        }
    }
    found
}

fn normalize_comparison_tokens(tokens: &[String]) -> Vec<String> {
    let mut normalized = Vec::with_capacity(tokens.len());
    let mut index = 0usize;
    while index < tokens.len() {
        if index + 1 < tokens.len() && comparison_pair(&tokens[index..index + 2]) {
            normalized.push(format!("{}{}", tokens[index], tokens[index + 1]));
            index += 2;
        } else {
            normalized.push(tokens[index].clone());
            index += 1;
        }
    }
    normalized
}

fn comparison_pair(pair: &[String]) -> bool {
    pair.len() == 2
        && ((pair[0] == ">" && pair[1] == "=")
            || (pair[0] == "<" && matches!(pair[1].as_str(), "=" | ">")))
}

fn padded_bytes(mut left: Vec<u8>, mut right: Vec<u8>) -> (Vec<u8>, Vec<u8>) {
    let length = left.len().max(right.len());
    left.resize(length, b' ');
    right.resize(length, b' ');
    (left, right)
}

const fn is_numeric(category: LayoutCategory) -> bool {
    matches!(
        category,
        LayoutCategory::NumericDisplay
            | LayoutCategory::NumericEdited
            | LayoutCategory::PackedDecimal
            | LayoutCategory::Binary
    )
}

const fn is_group(category: LayoutCategory) -> bool {
    matches!(
        category,
        LayoutCategory::Group | LayoutCategory::NationalGroup | LayoutCategory::Utf8Group
    )
}

const fn is_pointer_like(category: LayoutCategory) -> bool {
    matches!(
        category,
        LayoutCategory::Index
            | LayoutCategory::Pointer
            | LayoutCategory::Pointer32
            | LayoutCategory::ProcedurePointer
            | LayoutCategory::FunctionPointer
            | LayoutCategory::ObjectReference
    )
}

fn decimal_text(text: &str) -> Option<Decimal> {
    let trimmed = text.trim().trim_matches(['\'', '"']);
    if trimmed.is_empty() {
        return None;
    }
    let negative = trimmed.starts_with('-') || (trimmed.starts_with('(') && trimmed.ends_with(')'));
    let mut coefficient = 0i128;
    let mut scale = 0u32;
    let mut fractional = false;
    let mut digits = 0usize;
    for byte in trimmed.bytes() {
        if byte.is_ascii_digit() {
            coefficient = coefficient
                .checked_mul(10)?
                .checked_add(i128::from(byte - b'0'))?;
            digits += 1;
            if fractional {
                scale = scale.checked_add(1)?;
            }
        } else if byte == b'.' && !fractional {
            fractional = true;
        } else if !matches!(byte, b'+' | b'-' | b',' | b'$' | b' ' | b'(' | b')') {
            return None;
        }
    }
    if digits == 0 {
        return None;
    }
    Some(Decimal {
        coefficient: if negative { -coefficient } else { coefficient },
        scale,
    })
}

fn decimal_string(value: Decimal) -> String {
    let negative = value.coefficient < 0;
    let mut digits = value.coefficient.unsigned_abs().to_string();
    if value.scale > 0 {
        let scale = value.scale as usize;
        if digits.len() <= scale {
            digits = format!("{}{}", "0".repeat(scale + 1 - digits.len()), digits);
        }
        digits.insert(digits.len() - scale, '.');
    }
    if negative {
        digits.insert(0, '-');
    }
    digits
}

fn decimal_aligned(left: Decimal, right: Decimal) -> Result<(Decimal, Decimal), MachineProblem> {
    let scale = left.scale.max(right.scale);
    Ok((
        decimal_rescale(left, scale)?,
        decimal_rescale(right, scale)?,
    ))
}

fn decimal_rescale(value: Decimal, scale: u32) -> Result<Decimal, MachineProblem> {
    if value.scale == scale {
        return Ok(value);
    }
    if value.scale < scale {
        let factor = ten_power(scale - value.scale)?;
        Ok(Decimal {
            coefficient: value
                .coefficient
                .checked_mul(factor)
                .ok_or(MachineProblem::SizeError)?,
            scale,
        })
    } else {
        let factor = ten_power(value.scale - scale)?;
        Ok(Decimal {
            coefficient: value.coefficient / factor,
            scale,
        })
    }
}

fn decimal_rescale_rounded(value: Decimal, scale: u32) -> Result<Decimal, MachineProblem> {
    if value.scale <= scale {
        return decimal_rescale(value, scale);
    }
    let factor = ten_power(value.scale - scale)?;
    let quotient = value.coefficient / factor;
    let remainder = value.coefficient % factor;
    let increment = if remainder.unsigned_abs().saturating_mul(2) >= factor as u128 {
        value.coefficient.signum()
    } else {
        0
    };
    Ok(Decimal {
        coefficient: quotient
            .checked_add(increment)
            .ok_or(MachineProblem::SizeError)?,
        scale,
    })
}

fn decimal_add(left: Decimal, right: Decimal) -> Result<Decimal, MachineProblem> {
    let (left, right) = decimal_aligned(left, right)?;
    Ok(Decimal {
        coefficient: left
            .coefficient
            .checked_add(right.coefficient)
            .ok_or(MachineProblem::SizeError)?,
        scale: left.scale,
    })
}

fn decimal_subtract(left: Decimal, right: Decimal) -> Result<Decimal, MachineProblem> {
    let (left, right) = decimal_aligned(left, right)?;
    Ok(Decimal {
        coefficient: left
            .coefficient
            .checked_sub(right.coefficient)
            .ok_or(MachineProblem::SizeError)?,
        scale: left.scale,
    })
}

fn decimal_multiply(left: Decimal, right: Decimal) -> Result<Decimal, MachineProblem> {
    Ok(Decimal {
        coefficient: left
            .coefficient
            .checked_mul(right.coefficient)
            .ok_or(MachineProblem::SizeError)?,
        scale: left
            .scale
            .checked_add(right.scale)
            .ok_or(MachineProblem::SizeError)?,
    })
}

fn decimal_divide(
    dividend: Decimal,
    divisor: Decimal,
    result_scale: u32,
) -> Result<Decimal, MachineProblem> {
    if divisor.coefficient == 0 {
        return Err(MachineProblem::SizeError);
    }
    let exponent = result_scale
        .checked_add(divisor.scale)
        .and_then(|value| value.checked_sub(dividend.scale))
        .ok_or(MachineProblem::SizeError)?;
    let numerator = dividend
        .coefficient
        .checked_mul(ten_power(exponent)?)
        .ok_or(MachineProblem::SizeError)?;
    Ok(Decimal {
        coefficient: numerator / divisor.coefficient,
        scale: result_scale,
    })
}

fn ten_power(exponent: u32) -> Result<i128, MachineProblem> {
    10i128
        .checked_pow(exponent)
        .ok_or(MachineProblem::SizeError)
}

fn decode_decimal(layout: &LayoutMetadata, bytes: &[u8]) -> Result<Decimal, MachineProblem> {
    let coefficient = match layout.category {
        LayoutCategory::NumericDisplay => decode_display(bytes, layout.sign_separate)?,
        LayoutCategory::PackedDecimal => decode_packed(bytes)?,
        LayoutCategory::Binary => decode_binary_integer(bytes)?,
        LayoutCategory::NumericEdited => {
            decimal_text(&String::from_utf8_lossy(bytes))
                .ok_or(MachineProblem::DataException)?
                .coefficient
        }
        _ => return Err(MachineProblem::DataException),
    };
    Ok(Decimal {
        coefficient,
        scale: layout.scale,
    })
}

fn decode_display(bytes: &[u8], separate: bool) -> Result<i128, MachineProblem> {
    let mut negative = false;
    let mut digits = Vec::new();
    for (index, byte) in bytes.iter().copied().enumerate() {
        if separate && matches!(byte, b'+' | b'-') {
            negative = byte == b'-';
            continue;
        }
        let digit = if byte.is_ascii_digit() {
            byte - b'0'
        } else if byte == 0 {
            // Some migrated transaction files use a low-values sentinel
            // record. Enterprise COBOL's permissive zoned-decimal compatibility
            // treats each low-value byte as digit zero when that alphanumeric
            // key is moved to a numeric-display receiver.
            0
        } else if let Some(position) = b"}JKLMNOPQR".iter().position(|value| *value == byte) {
            negative = true;
            position as u8
        } else if let Some(position) = b"{ABCDEFGHI".iter().position(|value| *value == byte) {
            position as u8
        } else if byte == b' ' && index == 0 {
            0
        } else {
            return Err(MachineProblem::DataException);
        };
        digits.push(digit);
    }
    let mut value = 0i128;
    for digit in digits {
        value = value
            .checked_mul(10)
            .and_then(|value| value.checked_add(i128::from(digit)))
            .ok_or(MachineProblem::SizeError)?;
    }
    Ok(if negative { -value } else { value })
}

fn decode_packed(bytes: &[u8]) -> Result<i128, MachineProblem> {
    if bytes.is_empty() {
        return Err(MachineProblem::DataException);
    }
    let mut value = 0i128;
    for (index, byte) in bytes.iter().copied().enumerate() {
        let high = byte >> 4;
        let low = byte & 0x0f;
        if high > 9 {
            return Err(MachineProblem::DataException);
        }
        value = value
            .checked_mul(10)
            .and_then(|value| value.checked_add(i128::from(high)))
            .ok_or(MachineProblem::SizeError)?;
        if index + 1 < bytes.len() {
            if low > 9 {
                return Err(MachineProblem::DataException);
            }
            value = value
                .checked_mul(10)
                .and_then(|value| value.checked_add(i128::from(low)))
                .ok_or(MachineProblem::SizeError)?;
        } else if !matches!(low, 0x0b..=0x0f) {
            return Err(MachineProblem::DataException);
        } else if matches!(low, 0x0b | 0x0d) {
            value = -value;
        }
    }
    Ok(value)
}

fn decode_binary_integer(bytes: &[u8]) -> Result<i128, MachineProblem> {
    if bytes.is_empty() || bytes.len() > 16 {
        return Err(MachineProblem::DataException);
    }
    let fill = if bytes[0] & 0x80 == 0 { 0 } else { 0xff };
    let mut value = [fill; 16];
    value[16 - bytes.len()..].copy_from_slice(bytes);
    Ok(i128::from_be_bytes(value))
}

fn encode_decimal(layout: &LayoutMetadata, value: Decimal) -> Result<Vec<u8>, MachineProblem> {
    let digits = value.coefficient.unsigned_abs().to_string();
    if layout.digits > 0 && digits.len() > layout.digits {
        return Err(MachineProblem::SizeError);
    }
    match layout.category {
        LayoutCategory::NumericDisplay => {
            let digit_length = layout.length - usize::from(layout.sign_separate);
            let mut output = vec![b'0'; digit_length];
            let copy = digits.len().min(output.len());
            let start = output.len() - copy;
            output[start..].copy_from_slice(&digits.as_bytes()[digits.len() - copy..]);
            if layout.signed && !layout.sign_separate {
                let last = output.last_mut().ok_or(MachineProblem::SizeError)?;
                let overpunch = if value.coefficient < 0 {
                    b"}JKLMNOPQR"
                } else {
                    b"{ABCDEFGHI"
                };
                *last = overpunch[usize::from(*last - b'0')];
            }
            if layout.sign_separate {
                output.push(if value.coefficient < 0 { b'-' } else { b'+' });
            }
            Ok(output)
        }
        LayoutCategory::PackedDecimal => {
            let digit_count = layout.length.saturating_mul(2).saturating_sub(1);
            let mut nibbles = vec![0u8; digit_count];
            let copy = digits.len().min(nibbles.len());
            let start = nibbles.len() - copy;
            for (slot, digit) in nibbles[start..]
                .iter_mut()
                .zip(digits.bytes().skip(digits.len() - copy))
            {
                *slot = digit - b'0';
            }
            nibbles.push(if value.coefficient < 0 { 0x0d } else { 0x0c });
            Ok(nibbles
                .chunks(2)
                .map(|pair| (pair[0] << 4) | pair[1])
                .collect())
        }
        LayoutCategory::Binary => {
            let bytes = value.coefficient.to_be_bytes();
            let output = bytes[bytes.len() - layout.length..].to_vec();
            if decode_binary_integer(&output)? != value.coefficient {
                return Err(MachineProblem::SizeError);
            }
            Ok(output)
        }
        LayoutCategory::NumericEdited => encode_edited(layout, value),
        _ => Err(MachineProblem::DataException),
    }
}

fn encode_edited(layout: &LayoutMetadata, value: Decimal) -> Result<Vec<u8>, MachineProblem> {
    let mut digits = value.coefficient.unsigned_abs().to_string();
    if digits.len() < layout.digits {
        digits = format!("{}{}", "0".repeat(layout.digits - digits.len()), digits);
    }
    let mut digit_index = 0usize;
    let mut output = Vec::with_capacity(layout.length);
    let mut suppressing = true;
    let picture = expanded_picture(&layout.picture, layout.length.saturating_add(layout.digits))?;
    let first_nonzero = digits.bytes().position(|digit| digit != b'0');
    let has_floating_plus = picture.windows(2).any(|pair| pair == b"++");
    let floating_sign_slot = if value.coefficient < 0 || has_floating_plus {
        first_nonzero.and_then(|position| position.checked_sub(1))
    } else {
        None
    };
    if (value.coefficient < 0 || has_floating_plus)
        && first_nonzero == Some(0)
        && picture
            .first()
            .is_some_and(|symbol| matches!(symbol, b'+' | b'-'))
    {
        return Err(MachineProblem::SizeError);
    }
    for (picture_index, byte) in picture.iter().copied().enumerate() {
        match byte {
            b'9' => {
                output.push(*digits.as_bytes().get(digit_index).unwrap_or(&b'0'));
                digit_index += 1;
                suppressing = false;
            }
            b'Z' => {
                let digit = *digits.as_bytes().get(digit_index).unwrap_or(&b'0');
                output.push(
                    if suppressing && digit == b'0' && digit_index + 1 < layout.digits {
                        b' '
                    } else {
                        suppressing = false;
                        digit
                    },
                );
                digit_index += 1;
            }
            b'*' => {
                let digit = *digits.as_bytes().get(digit_index).unwrap_or(&b'0');
                output.push(
                    if suppressing && digit == b'0' && digit_index + 1 < layout.digits {
                        b'*'
                    } else {
                        suppressing = false;
                        digit
                    },
                );
                digit_index += 1;
            }
            b'+' | b'-'
                if picture.get(picture_index.wrapping_sub(1)) == Some(&byte)
                    || picture.get(picture_index + 1) == Some(&byte) =>
            {
                let digit = *digits.as_bytes().get(digit_index).unwrap_or(&b'0');
                if floating_sign_slot == Some(digit_index) {
                    output.push(if value.coefficient < 0 { b'-' } else { b'+' });
                    suppressing = false;
                } else if suppressing && digit == b'0' && digit_index + 1 < layout.digits {
                    output.push(b' ');
                } else {
                    output.push(digit);
                    suppressing = false;
                }
                digit_index += 1;
            }
            b'+' => output.push(if value.coefficient < 0 { b'-' } else { b'+' }),
            b'-' => output.push(if value.coefficient < 0 { b'-' } else { b' ' }),
            b'V' | b'S' | b'P' => {}
            b'B' => output.push(b' '),
            b'.' => {
                output.push(b'.');
                suppressing = false;
            }
            other => output.push(other),
        }
    }
    if output.len() != layout.length {
        return Err(MachineProblem::UnsupportedForm);
    }
    Ok(output)
}

fn expanded_picture(picture: &str, limit: usize) -> Result<Vec<u8>, MachineProblem> {
    let bytes = picture.as_bytes();
    let mut output = Vec::new();
    let mut index = 0usize;
    while index < bytes.len() {
        let symbol = bytes[index].to_ascii_uppercase();
        index += 1;
        let repeat = if bytes.get(index) == Some(&b'(') {
            let close = bytes[index + 1..]
                .iter()
                .position(|byte| *byte == b')')
                .ok_or(MachineProblem::UnsupportedForm)?
                + index
                + 1;
            let count = std::str::from_utf8(&bytes[index + 1..close])
                .ok()
                .and_then(|value| value.parse::<usize>().ok())
                .filter(|value| *value > 0)
                .ok_or(MachineProblem::UnsupportedForm)?;
            index = close + 1;
            count
        } else {
            1
        };
        if output.len().saturating_add(repeat) > limit.saturating_add(32) {
            return Err(MachineProblem::ResourceExhausted);
        }
        output.extend(std::iter::repeat_n(symbol, repeat));
    }
    Ok(output)
}

fn condition_matches(
    actual: &[u8],
    values: &[String],
    layout: &LayoutMetadata,
) -> Result<bool, MachineProblem> {
    if is_numeric(layout.category) && actual.len() == layout.length {
        let actual = decode_decimal(layout, actual)?;
        let mut index = 0usize;
        while index < values.len() {
            let start = values[index].trim_matches(['\'', '"']);
            let Some(start) = decimal_text(start) else {
                break;
            };
            if values
                .get(index + 1)
                .is_some_and(|value| value == "THRU" || value == "THROUGH")
            {
                let end = values
                    .get(index + 2)
                    .and_then(|value| decimal_text(value.trim_matches(['\'', '"'])))
                    .ok_or(MachineProblem::InvalidOperation)?;
                let (actual_start, start) = decimal_aligned(actual, start)?;
                let (actual_end, end) = decimal_aligned(actual, end)?;
                if actual_start.coefficient >= start.coefficient
                    && actual_end.coefficient <= end.coefficient
                {
                    return Ok(true);
                }
                index += 3;
            } else {
                let (actual, expected) = decimal_aligned(actual, start)?;
                if actual.coefficient == expected.coefficient {
                    return Ok(true);
                }
                index += 1;
            }
        }
        if index == values.len() {
            return Ok(false);
        }
    }
    let actual_text = String::from_utf8_lossy(actual).trim().to_string();
    let mut index = 0usize;
    while index < values.len() {
        let normalized = normalize(&values[index]);
        let figurative = match normalized.as_str() {
            "SPACE" | "SPACES" => Some(b' '),
            "ZERO" | "ZEROS" | "ZEROES" => Some(b'0'),
            "LOW-VALUE" | "LOW-VALUES" => Some(0),
            "HIGH-VALUE" | "HIGH-VALUES" => Some(0xff),
            _ => None,
        };
        if figurative.is_some_and(|byte| actual.iter().all(|actual| *actual == byte)) {
            return Ok(true);
        }
        let start = values[index].trim_matches(['\'', '"']).to_string();
        if values
            .get(index + 1)
            .is_some_and(|value| value == "THRU" || value == "THROUGH")
        {
            let end = values
                .get(index + 2)
                .ok_or(MachineProblem::InvalidOperation)?
                .trim_matches(['\'', '"']);
            let matched = match (
                decimal_text(&actual_text),
                decimal_text(&start),
                decimal_text(end),
            ) {
                (Some(actual), Some(start), Some(end)) => {
                    let (actual, start) = decimal_aligned(actual, start)?;
                    let (actual, end) = decimal_aligned(actual, end)?;
                    actual.coefficient >= start.coefficient && actual.coefficient <= end.coefficient
                }
                _ => actual_text.as_str() >= start.as_str() && actual_text.as_str() <= end,
            };
            if matched {
                return Ok(true);
            }
            index += 3;
        } else {
            if actual_text == start {
                return Ok(true);
            }
            index += 1;
        }
    }
    Ok(false)
}

fn encode_dataset_record(ccsid: Option<u16>, record: &[u8]) -> Result<Vec<u8>, MachineProblem> {
    match ccsid {
        None | Some(1208) => Ok(record.to_vec()),
        Some(37) => CodePage::Cp037
            .encode(
                std::str::from_utf8(record).map_err(|_| MachineProblem::DataException)?,
                record.len().saturating_mul(4).max(1),
            )
            .map_err(|_| MachineProblem::DataException),
        Some(_) => Err(MachineProblem::UnsupportedForm),
    }
}

fn decode_dataset_record(ccsid: Option<u16>, record: &[u8]) -> Result<Vec<u8>, MachineProblem> {
    match ccsid {
        None | Some(1208) => Ok(record.to_vec()),
        Some(37) => CodePage::Cp037
            .decode(record, record.len().saturating_mul(4).max(1))
            .map(String::into_bytes)
            .map_err(|_| MachineProblem::DataException),
        Some(_) => Err(MachineProblem::UnsupportedForm),
    }
}

fn dataset_file_status(name: &str, response: i32) -> String {
    match (name, response) {
        ("NOTFND", _) => "23",
        ("DUPREC" | "DUPKEY", _) => "22",
        ("ENDFILE", _) => "10",
        ("LENGERR", _) => "44",
        ("INVREQ", _) => "39",
        _ => "30",
    }
    .into()
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    (!needle.is_empty())
        .then(|| {
            haystack
                .windows(needle.len())
                .position(|window| window == needle)
        })
        .flatten()
}

fn split_bytes<'a>(source: &'a [u8], delimiter: &[u8]) -> Vec<&'a [u8]> {
    let mut fields = Vec::new();
    let mut rest = source;
    while let Some(position) = find_bytes(rest, delimiter) {
        fields.push(&rest[..position]);
        rest = &rest[position + delimiter.len()..];
    }
    fields.push(rest);
    fields
}

fn replace_bytes(source: &[u8], from: &[u8], to: &[u8]) -> Result<Vec<u8>, MachineProblem> {
    if from.is_empty() || from.len() != to.len() {
        return Err(MachineProblem::UnsupportedForm);
    }
    let mut result = source.to_vec();
    let mut offset = 0usize;
    while let Some(position) = find_bytes(&result[offset..], from) {
        let start = offset + position;
        result[start..start + from.len()].copy_from_slice(to);
        offset = start + from.len();
    }
    Ok(result)
}

fn count_bytes(source: &[u8], needle: &[u8]) -> Result<usize, MachineProblem> {
    if needle.is_empty() {
        return Err(MachineProblem::InvalidOperation);
    }
    let mut count = 0usize;
    let mut offset = 0usize;
    while let Some(position) = find_bytes(&source[offset..], needle) {
        count = count
            .checked_add(1)
            .ok_or(MachineProblem::ResourceExhausted)?;
        offset = offset
            .checked_add(position + needle.len())
            .ok_or(MachineProblem::ResourceExhausted)?;
    }
    Ok(count)
}

fn split_yyyymmdd(value: i128) -> Result<(i32, u32, u32), MachineProblem> {
    let value = i64::try_from(value).map_err(|_| MachineProblem::DataException)?;
    let year = i32::try_from(value / 10_000).map_err(|_| MachineProblem::DataException)?;
    let month = u32::try_from((value / 100) % 100).map_err(|_| MachineProblem::DataException)?;
    let day = u32::try_from(value % 100).map_err(|_| MachineProblem::DataException)?;
    if !valid_date(year, month, day) {
        return Err(MachineProblem::DataException);
    }
    Ok((year, month, day))
}

fn cobol_integer_of_date(year: i32, month: u32, day: u32) -> Result<i64, MachineProblem> {
    if !valid_date(year, month, day) || !(1601..=9999).contains(&year) {
        return Err(MachineProblem::DataException);
    }
    let base = days_from_civil(1600, 12, 31);
    days_from_civil(year, month, day)
        .checked_sub(base)
        .ok_or(MachineProblem::DataException)
}

fn cobol_date_of_integer(value: i64) -> Result<(i32, u32, u32), MachineProblem> {
    if value <= 0 {
        return Err(MachineProblem::DataException);
    }
    let base = days_from_civil(1600, 12, 31);
    let days = base
        .checked_add(value)
        .ok_or(MachineProblem::DataException)?;
    let date = civil_from_days(days);
    if date.0 > 9999 {
        return Err(MachineProblem::DataException);
    }
    Ok(date)
}

fn valid_date(year: i32, month: u32, day: u32) -> bool {
    if year <= 0 || !(1..=12).contains(&month) || day == 0 {
        return false;
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let length = match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    day <= length
}

fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let year = i64::from(year) - i64::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month = i64::from(month);
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let days = days + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (year as i32, month as u32, day as u32)
}
fn encode_call_values(
    names: &[String],
    values: &[Vec<u8>],
) -> Result<BoundedPayload, MachineProblem> {
    if names.len() != values.len() {
        return Err(MachineProblem::InvalidOperation);
    }
    let mut bytes = u32::try_from(values.len())
        .map_err(|_| MachineProblem::ResourceExhausted)?
        .to_be_bytes()
        .to_vec();
    for (name, value) in names.iter().zip(values) {
        push_host_field(&mut bytes, name.as_bytes())?;
        bytes.push(1);
        push_host_field(&mut bytes, value)?;
    }
    BoundedPayload::new(
        "mainframe-env.cobol.call@1",
        bytes,
        InvocationLimits::default(),
    )
    .map_err(|_| MachineProblem::ResourceExhausted)
}

fn decode_call_arguments(payload: &BoundedPayload) -> Result<Vec<Vec<u8>>, MachineProblem> {
    if payload.schema() != "mainframe-env.cobol.call@1" {
        return Err(MachineProblem::UnexpectedHostResult);
    }
    let mut input = SnapshotInput::new(payload.bytes());
    let count = usize::try_from(input.u32()?).map_err(|_| MachineProblem::ResourceExhausted)?;
    if count > InvocationLimits::default().max_bindings {
        return Err(MachineProblem::ResourceExhausted);
    }
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        let _name = input.bytes(4096)?;
        if input.take(1)? != [1] {
            return Err(MachineProblem::UnexpectedHostResult);
        }
        values.push(input.bytes(InvocationLimits::default().max_payload_bytes)?);
    }
    if !input.finished() {
        return Err(MachineProblem::UnexpectedHostResult);
    }
    Ok(values)
}

pub fn encode_cobol_call_result(values: &[Vec<u8>]) -> Result<BoundedPayload, MachineProblem> {
    let mut bytes = u32::try_from(values.len())
        .map_err(|_| MachineProblem::ResourceExhausted)?
        .to_be_bytes()
        .to_vec();
    for value in values {
        push_host_field(&mut bytes, value)?;
    }
    BoundedPayload::new(
        "mainframe-env.cobol.call-result@1",
        bytes,
        InvocationLimits::default(),
    )
    .map_err(|_| MachineProblem::ResourceExhausted)
}

fn ims_option_groups(args: &[String]) -> Result<Vec<(String, Vec<String>)>, MachineProblem> {
    let mut groups = Vec::new();
    for (index, token) in args.iter().enumerate().filter(|(_, token)| {
        matches!(
            token.as_str(),
            "PCB" | "SEGMENT" | "INTO" | "FROM" | "WHERE" | "PSB" | "ID" | "SEGLENGTH"
        )
    }) {
        let open = index + 1;
        if args.get(open).is_none_or(|token| token != "(") {
            continue;
        }
        let close = matching_close(args, open).ok_or(MachineProblem::InvalidOperation)?;
        groups.push((token.clone(), args[open + 1..close].to_vec()));
    }
    Ok(groups)
}

fn push_host_field(output: &mut Vec<u8>, value: &[u8]) -> Result<(), MachineProblem> {
    output.extend_from_slice(
        &u64::try_from(value.len())
            .map_err(|_| MachineProblem::ResourceExhausted)?
            .to_be_bytes(),
    );
    output.extend_from_slice(value);
    Ok(())
}

fn decode_call_values(payload: &BoundedPayload) -> Result<Vec<Vec<u8>>, MachineProblem> {
    if payload.schema() != "mainframe-env.cobol.call-result@1" {
        return Err(MachineProblem::UnexpectedHostResult);
    }
    let mut input = SnapshotInput::new(payload.bytes());
    let count = usize::try_from(input.u32()?).map_err(|_| MachineProblem::ResourceExhausted)?;
    if count > InvocationLimits::default().max_bindings {
        return Err(MachineProblem::ResourceExhausted);
    }
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        values.push(input.bytes(InvocationLimits::default().max_payload_bytes)?);
    }
    if !input.finished() {
        return Err(MachineProblem::UnexpectedHostResult);
    }
    Ok(values)
}
fn failure_drive(category: FailureCategory, message: &str) -> MachineDrive<EffectRequest> {
    MachineDrive::Failed(
        ExecutionProblem::new(
            DiagnosticCode::new("MEEXEC0001").expect("static code"),
            category,
            Phase::Execute,
            message,
            false,
            false,
            DiagnosticLimits::default(),
        )
        .expect("static problem"),
    )
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MachineProblem {
    InvalidArtifact(String),
    InvalidOperation,
    UnsupportedForm,
    UnknownStorage,
    UnknownLabel,
    DataException,
    SizeError,
    SubscriptError,
    ReferenceModificationError,
    ResourceExhausted,
    UnexpectedResume,
    UnexpectedHostResult,
    IncompatibleSnapshot,
    Host(HostProblem),
}
impl MachineProblem {
    fn execution_problem(&self) -> ExecutionProblem {
        let (category, message) = match self {
            Self::ResourceExhausted => (
                FailureCategory::ResourceExhausted,
                "machine resource exhausted".to_string(),
            ),
            Self::Host(HostProblem::Unauthorized) => (
                FailureCategory::Unauthorized,
                "host authorization denied".into(),
            ),
            Self::Host(HostProblem::Cancelled) => {
                (FailureCategory::Cancelled, "host cancelled".into())
            }
            Self::Host(HostProblem::TimedOut) => {
                (FailureCategory::TimedOut, "host timed out".into())
            }
            Self::Host(HostProblem::UnknownOutcome) => (
                FailureCategory::UnknownOutcome,
                "host outcome unknown".into(),
            ),
            Self::Host(problem) => (
                FailureCategory::ProviderFailure,
                format!("host provider failed: {problem:?}"),
            ),
            Self::UnsupportedForm => (
                FailureCategory::Unsupported,
                "unsupported COBOL form".into(),
            ),
            Self::InvalidArtifact(detail) => (FailureCategory::MalformedInput, detail.clone()),
            _ => (
                FailureCategory::MalformedInput,
                "invalid executable artifact or data".into(),
            ),
        };
        ExecutionProblem::new(
            DiagnosticCode::new("MEEXEC0002").expect("static code"),
            category,
            Phase::Execute,
            message,
            false,
            category == FailureCategory::UnknownOutcome,
            DiagnosticLimits::default(),
        )
        .expect("static problem")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_execution_api::{
        ArtifactRef, CapabilityId, ExecutionId, IdempotencyKey, Principal, PrincipalId, RequestId,
        ResourceLimits, RunUnitId, Selector, ServiceClass, TraceId,
    };
    use mainframe_env_ir::{Effect, IrLimits, ModuleBuilder};
    fn invocation() -> Invocation {
        let l = InvocationLimits::default();
        Invocation::new(
            RequestId::new("req", l).unwrap(),
            ExecutionId::new("exec", l).unwrap(),
            RunUnitId::new("run", l).unwrap(),
            None,
            Selector::new("program:HELLO", l).unwrap(),
            ArtifactRef::new("artifact", l).unwrap(),
            Principal::new(
                PrincipalId::new("IBMUSER", l).unwrap(),
                BTreeSet::<CapabilityId>::new(),
                l,
            )
            .unwrap(),
            ServiceClass::Batch,
            0,
            100,
            TraceId::new("trace", l).unwrap(),
            IdempotencyKey::new("idem", l).unwrap(),
            1,
            ResourceLimits::default(),
            BTreeMap::new(),
            l,
        )
        .unwrap()
    }
    fn binary() -> Vec<u8> {
        let mut b = ModuleBuilder::new(IrLimits::default());
        let s = b.add_storage("msg", 5, None).unwrap();
        let r = b.add_region().unwrap();
        let block = b.add_block(r).unwrap();
        b.add_operation(
            block,
            OperationIdentity::new(NAMESPACE, "init", 1).unwrap(),
            Vec::new(),
            0,
            BTreeMap::from([("initial".into(), Attribute::Bytes(b"HELLO".to_vec()))]),
            vec![Effect::MemoryWrite],
            vec![mainframe_env_ir::StorageReference {
                storage: s,
                offset: 0,
                length: 5,
            }],
            None,
        )
        .unwrap();
        b.add_operation(
            block,
            OperationIdentity::new(NAMESPACE, "display", 1).unwrap(),
            Vec::new(),
            0,
            BTreeMap::from([("arg_000".into(), Attribute::Text("MSG".into()))]),
            vec![Effect::MemoryRead, Effect::TerminalWrite],
            Vec::new(),
            None,
        )
        .unwrap();
        b.add_operation(
            block,
            OperationIdentity::new(NAMESPACE, "halt", 1).unwrap(),
            Vec::new(),
            0,
            BTreeMap::new(),
            Vec::new(),
            Vec::new(),
            None,
        )
        .unwrap();
        mainframe_env_ir::encode_binary(&b.finish().unwrap(), CodecLimits::default()).unwrap()
    }
    #[test]
    fn hello_executes_in_bounded_quanta() {
        let mut m =
            ReferenceMachine::from_binary(&binary(), invocation(), CodecLimits::default()).unwrap();
        let drive = m.drive(MachineResume::Start, Quantum::new(100, 1024).unwrap());
        match drive {
            MachineDrive::Completed(done) => assert_eq!(done.output.bytes(), b"HELLO\n"),
            other => panic!("{other:?}"),
        }
    }
    #[test]
    fn output_limit_fails_typed() {
        let mut i = invocation();
        i.limits.max_output_bytes = 1;
        let mut m = ReferenceMachine::from_binary(&binary(), i, CodecLimits::default()).unwrap();
        assert!(matches!(
            m.drive(MachineResume::Start, Quantum::new(100, 1024).unwrap()),
            MachineDrive::Failed(_)
        ));
    }
    #[test]
    fn cumulative_step_limit_survives_quantum_boundaries() {
        let mut i = invocation();
        i.limits.max_steps = 1;
        let mut m = ReferenceMachine::from_binary(&binary(), i, CodecLimits::default()).unwrap();
        assert_eq!(
            m.drive(MachineResume::Start, Quantum::new(1, 1024).unwrap()),
            MachineDrive::Continue
        );
        assert!(matches!(
            m.drive(MachineResume::Start, Quantum::new(1, 1024).unwrap()),
            MachineDrive::Failed(_)
        ));
    }
    #[test]
    fn checkpoint_roundtrip_restores_exact_machine_state_and_rejects_corruption() {
        let invocation = invocation();
        let mut first =
            ReferenceMachine::from_binary(&binary(), invocation.clone(), CodecLimits::default())
                .unwrap();
        assert_eq!(
            first.drive(MachineResume::Start, Quantum::new(1, 1024).unwrap()),
            MachineDrive::Continue
        );
        first.last_file_status = "10".into();
        first.condition_status.arithmetic_size_error = true;
        first
            .dataset_cursors
            .insert("IBMUSER.INPUT".into(), "CURSOR-1".into());
        let checkpoint = first.checkpoint().unwrap();
        assert_eq!(
            checkpoint.schema(),
            "mainframe-env.reference-machine-checkpoint@7"
        );
        let mut restored =
            ReferenceMachine::from_binary(&binary(), invocation, CodecLimits::default()).unwrap();
        restored.restore_checkpoint(&checkpoint).unwrap();
        assert_eq!(restored.last_file_status, "10");
        assert_eq!(restored.condition_status, first.condition_status);
        assert_eq!(restored.dataset_cursors, first.dataset_cursors);
        assert_eq!(restored.executed_steps, first.executed_steps);
        let first_done = first.drive(MachineResume::Start, Quantum::new(100, 1024).unwrap());
        let restored_done = restored.drive(MachineResume::Start, Quantum::new(100, 1024).unwrap());
        assert_eq!(first_done, restored_done);

        let mut damaged = checkpoint.bytes().to_vec();
        damaged.push(0);
        let damaged =
            BoundedPayload::new(checkpoint.schema(), damaged, InvocationLimits::default()).unwrap();
        assert_eq!(
            restored.restore_checkpoint(&damaged),
            Err(MachineProblem::IncompatibleSnapshot)
        );
    }
    #[test]
    fn cics_tokens_lower_to_named_typed_arguments() {
        let tokens = vec![
            "EXEC", "CICS", "SEND", "MAP", "MAPSET", "(", "MENUMS", ")", "MAP", "(", "MENU", ")",
            "ERASE", "END-EXEC",
        ]
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
        assert_eq!(
            CicsOperation::from_tokens(&tokens),
            Some(CicsOperation::SendMap)
        );
        let arguments = cics_arguments(&tokens).unwrap();
        assert_eq!(arguments["MAPSET"].bytes(), b"MENUMS");
        assert_eq!(arguments["MAP"].bytes(), b"MENU");
        assert!(arguments.contains_key("OPTION.ERASE"));
        assert!(arguments.keys().all(|name| !name.starts_with("arg_")));
    }

    #[test]
    fn numeric_edited_picture_expands_repetition_before_encoding() {
        let layout = LayoutMetadata {
            name: "AMOUNT".into(),
            simple_name: "AMOUNT".into(),
            category: LayoutCategory::NumericEdited,
            picture: "9(9).99-".into(),
            digits: 11,
            scale: 2,
            signed: true,
            sign_separate: false,
            justified_right: false,
            linkage: false,
            offset: 0,
            length: 13,
            element_length: 13,
            occurs: 1,
            parent: None,
            condition_values: Vec::new(),
        };
        assert_eq!(
            encode_edited(
                &layout,
                Decimal {
                    coefficient: 12345,
                    scale: 2,
                }
            ),
            Ok(b"000000123.45 ".to_vec())
        );
    }
}
