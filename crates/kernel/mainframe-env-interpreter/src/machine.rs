use crate::FixedValue;
use crate::runtime::CobolArithmeticMode;
use crate::storage64::{
    Storage64Allocation, Storage64Arena, Storage64Attributes, Storage64Key, Storage64Limits,
    Storage64Location, Storage64Problem, Storage64Snapshot,
};
use mainframe_env_diagnostics::{
    DiagnosticCode, DiagnosticLimits, ExecutionProblem, FailureCategory, Phase,
};
use mainframe_env_encoding::CodePage;
use mainframe_env_execution_api::{
    Abend, AbendDumpDisposition, BoundedPayload, Completion, Condition, IdempotencyKey, Invocation,
    InvocationLimits, Machine, MachineDrive, MachineResume, Quantum, Selector, Suspension,
    Transfer,
};
use mainframe_env_host_api::{
    CicsConditionPolicy, CicsDisposition, CicsOperation, CicsRequest, ClassName, ClockRequest,
    DatasetCloseControl, DatasetName, DatasetReadControl, DatasetReadLockMode, DatasetReelUnit,
    DatasetRequest, Db2HostVariable, Db2Operation, Db2Request, EffectRequest, EffectResult,
    HostLimits, HostProblem, HostRequest, HostResult, ImsOperation, ImsQualifier, ImsRequest,
    KeyRelation, MethodName, MqOperation, MqRequest, Mutation, ProgramName, ProgramRequest,
    RuntimeServiceKind, RuntimeServiceName, RuntimeServiceSelector, TerminalRequest,
};
use mainframe_env_ir::{
    Attribute, CodecLimits, Module, Operation, OperationIdentity, StorageId, decode_binary,
};
use sha2::{Digest as _, Sha256};
use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use typed_decimal::{decimal_add, decimal_divide, decimal_multiply, decimal_subtract};
mod amode64_access;
mod completion;
mod condition_literals;
mod corresponding;
mod decimal_commit;
mod eib;
mod layout_admission;
mod layout_resolution;
mod snapshot_codec;
mod typed_cics;
mod typed_decimal;
use condition_literals::{condition_matches, condition_true_value_bytes};

const NAMESPACE: &str = "mainframe.core.cobol";
pub const SUPPORTED_LAYOUT_CATEGORIES: &[&str] = &[
    "alphabetic",
    "alphanumeric",
    "alphanumeric_edited",
    "binary",
    "condition",
    "dbcs",
    "float_long",
    "float_short",
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
    native_binary: bool,
    signed: bool,
    sign_separate: bool,
    justified_right: bool,
    blank_when_zero: bool,
    linkage: bool,
    offset: usize,
    length: usize,
    element_length: usize,
    occurs: usize,
    occurs_min: usize,
    unbounded: bool,
    depending_on: Option<String>,
    indexes: Vec<String>,
    keys: Vec<(bool, String)>,
    dynamic: bool,
    dynamic_limit: usize,
    parent: Option<String>,
    alias_of: Option<String>,
    occurs_clause: bool,
    condition_values: Vec<String>,
    object_class: Option<String>,
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
    record_names: Vec<String>,
    organization: String,
    access_mode: String,
    record_key: Option<String>,
    alternate_record_keys: Vec<String>,
    relative_key: Option<String>,
    file_status: Option<String>,
    sort_merge: bool,
    description: String,
    record_min: Option<usize>,
    record_max: Option<usize>,
    ccsid: Option<u16>,
    linage: Option<usize>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SortWorkspace {
    records: Vec<Vec<u8>>,
    cursor: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct XmlNode {
    name: String,
    attributes: Vec<(String, String)>,
    text: String,
    children: Vec<XmlNode>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct XmlEvent {
    kind: String,
    text: Vec<u8>,
    namespace: Vec<u8>,
    prefix: Vec<u8>,
}

impl XmlEvent {
    fn new(kind: &str, text: Vec<u8>) -> Self {
        Self {
            kind: kind.into(),
            text,
            namespace: Vec::new(),
            prefix: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct JsonClauses {
    names: BTreeMap<String, Option<String>>,
    suppressed: BTreeSet<String>,
    conditional_suppressions: BTreeMap<String, Vec<String>>,
    generic_suppressions: Vec<(Option<bool>, Vec<String>)>,
    conversions: BTreeMap<String, JsonConversion>,
    ignore_null_all: bool,
    ignored_nulls: BTreeSet<String>,
    encoding: Option<String>,
    encoding_from_codepage: bool,
    indicators: BTreeMap<String, JsonIndicator>,
    indicator_items: BTreeSet<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum JsonConversion {
    GenerateBoolean(String),
    GenerateNull(String),
    ParseBoolean(String, String),
    ParseNull(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct JsonIndicator {
    null_value: String,
    nonnull_value: Option<String>,
    item: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SortProcedurePhase {
    Input,
    Output,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ActiveSortProcedure {
    sort_pc: usize,
    sort_file: String,
    phase: SortProcedurePhase,
    arguments: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SortIoState {
    sort_pc: usize,
    sort_file: String,
    arguments: Vec<String>,
    inputs: Vec<String>,
    outputs: Vec<String>,
    next_input: usize,
    next_output: usize,
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
    end_of_page: bool,
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
            | u8::from(self.end_of_page) << 7
    }

    fn from_bits(bits: u8) -> Option<Self> {
        Some(Self {
            arithmetic_size_error: bits & 1 != 0,
            accept_exception: bits & (1 << 1) != 0,
            call_exception: bits & (1 << 2) != 0,
            string_overflow: bits & (1 << 3) != 0,
            unstring_overflow: bits & (1 << 4) != 0,
            json_exception: bits & (1 << 5) != 0,
            xml_exception: bits & (1 << 6) != 0,
            end_of_page: bits & (1 << 7) != 0,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PendingKind {
    Accept {
        target: String,
        handles_exception: bool,
    },
    AcceptClock {
        target: String,
        format: AcceptClockFormat,
        handles_exception: bool,
    },
    DatasetRead {
        target: Option<String>,
        into: Option<String>,
        depending_on: Option<String>,
        status: Option<String>,
        ccsid: Option<u16>,
        declarative: Option<String>,
    },
    DatasetStatus {
        status: Option<String>,
        cursor: Option<DatasetCursorAction>,
        linage_advance: usize,
        linage_limit: Option<usize>,
        page_advance: bool,
        declarative: Option<String>,
    },
    ProgramCall {
        targets: Vec<String>,
        returning: Option<String>,
        handles_exception: bool,
    },
    SortRead,
    SortWrite,
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
        storage64_intent: Option<typed_cics::Storage64Intent>,
        argument_summary: String,
        into: Option<typed_cics::CicsTarget>,
        outputs: BTreeMap<String, typed_cics::CicsTarget>,
        response: Option<typed_cics::CicsTarget>,
        response2: Option<typed_cics::CicsTarget>,
        address_set: Option<typed_cics::CicsAddressSet>,
        no_handle: bool,
    },
    Ignore,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AcceptClockFormat {
    DateYymmdd,
    DateYyyymmdd,
    DayYyddd,
    DayYyyyddd,
    DayOfWeek,
    Time,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum DatasetCursorAction {
    Start(String),
    End(String),
}

fn pending_declarative(kind: &PendingKind) -> Option<&str> {
    match kind {
        PendingKind::DatasetRead { declarative, .. }
        | PendingKind::DatasetStatus { declarative, .. } => declarative.as_deref(),
        _ => None,
    }
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
    pub dynamic_lengths: BTreeMap<String, usize>,
    pub implicit_values: BTreeMap<String, MachineSnapshotValue>,
    pub search_results: BTreeMap<usize, bool>,
    pub sql_cursors: BTreeMap<String, Vec<String>>,
    pub sort_workspaces: BTreeMap<String, (Vec<Vec<u8>>, usize)>,
    pub active_sort_procedure: Option<(usize, String, u8, Vec<String>)>,
    pub sort_io: Option<MachineSnapshotSortIo>,
    pub linkage_addresses: BTreeMap<String, Option<(usize, usize, usize)>>,
    pub freed_allocations: BTreeSet<usize>,
    pub random_state: Option<u64>,
    pub storage64: Storage64Snapshot,
    pub storage64_area_bindings: BTreeMap<String, u64>,
}

pub type MachineSnapshotSortIo = (
    usize,
    String,
    Vec<String>,
    Vec<String>,
    Vec<String>,
    usize,
    usize,
);
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MachineSnapshotValue {
    Bytes(Vec<u8>),
    Decimal { coefficient: i128, scale: u32 },
}
pub struct ReferenceMachine {
    invocation: Invocation,
    operations: Vec<Operation>,
    arithmetic_mode: CobolArithmeticMode,
    display_sign_separate: bool,
    bases: Vec<Vec<u8>>,
    static_base_count: usize,
    views: BTreeMap<String, StorageView>,
    views_by_id: BTreeMap<StorageId, StorageView>,
    storage_names_by_id: BTreeMap<StorageId, String>,
    entry_initials: BTreeMap<StorageId, Vec<u8>>,
    implicit: BTreeMap<String, CobolValue>,
    layouts: BTreeMap<String, LayoutMetadata>,
    entry_formals: Vec<String>,
    simple_layouts: BTreeMap<String, Vec<String>>,
    files: BTreeMap<String, FileMetadata>,
    declaratives: BTreeMap<String, String>,
    labels: BTreeMap<String, usize>,
    control_nodes: BTreeMap<usize, usize>,
    loop_reentry: BTreeSet<usize>,
    loop_counts: BTreeMap<usize, i128>,
    altered: BTreeMap<String, String>,
    last_file_status: String,
    condition_status: ConditionStatus,
    dataset_cursors: BTreeMap<String, String>,
    sql_cursors: BTreeMap<String, Vec<String>>,
    sort_workspaces: BTreeMap<String, SortWorkspace>,
    active_sort_procedure: Option<ActiveSortProcedure>,
    sort_io: Option<SortIoState>,
    dynamic_lengths: BTreeMap<String, usize>,
    search_results: BTreeMap<usize, bool>,
    linkage_addresses: BTreeMap<String, Option<StorageView>>,
    freed_allocations: BTreeSet<usize>,
    storage64: Storage64Arena,
    storage64_area_bindings: BTreeMap<String, u64>,
    random_state: Cell<Option<u64>>,
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
        let (bases, views, views_by_id, storage_names_by_id) =
            storage(&module, invocation.limits.max_storage_bytes)?;
        let static_base_count = bases.len();
        let storage64_limits = Storage64Limits {
            max_allocations: invocation.limits.max_frames,
            max_bytes: invocation.limits.max_storage_bytes,
        };
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
        let selected_entry = invocation
            .bindings
            .get("cobol.entry")
            .map(|value| {
                std::str::from_utf8(value.bytes())
                    .map(normalize)
                    .map_err(|_| MachineProblem::InvalidOperation)
            })
            .transpose()?;
        let mut implicit =
            eib::implicit_values(entry_commarea_len, entry_aid, entry_transaction.as_deref())?;
        implicit.extend([
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
            ("DEBUG-ITEM".into(), CobolValue::Bytes(Vec::new())),
            (
                "IGY-JAVAIOP-CALL-EXCEPTION".into(),
                CobolValue::Decimal(Decimal {
                    coefficient: 0,
                    scale: 0,
                }),
            ),
            ("JNIENVPTR".into(), CobolValue::Bytes(vec![0; 8])),
            (
                "JSON-CODE".into(),
                CobolValue::Decimal(Decimal {
                    coefficient: 0,
                    scale: 0,
                }),
            ),
            (
                "JSON-STATUS".into(),
                CobolValue::Decimal(Decimal {
                    coefficient: 0,
                    scale: 0,
                }),
            ),
            (
                "LINAGE-COUNTER".into(),
                CobolValue::Decimal(Decimal {
                    coefficient: 1,
                    scale: 0,
                }),
            ),
            ("SHIFT-OUT".into(), CobolValue::Bytes(vec![0x0e])),
            ("SHIFT-IN".into(), CobolValue::Bytes(vec![0x0f])),
            (
                "SORT-CONTROL".into(),
                CobolValue::Decimal(Decimal {
                    coefficient: 0,
                    scale: 0,
                }),
            ),
            (
                "SORT-CORE-SIZE".into(),
                CobolValue::Decimal(Decimal {
                    coefficient: i128::from(invocation.limits.max_storage_bytes),
                    scale: 0,
                }),
            ),
            (
                "SORT-FILE-SIZE".into(),
                CobolValue::Decimal(Decimal {
                    coefficient: 0,
                    scale: 0,
                }),
            ),
            ("SORT-MESSAGE".into(), CobolValue::Bytes(Vec::new())),
            (
                "SORT-MODE-SIZE".into(),
                CobolValue::Decimal(Decimal {
                    coefficient: 0,
                    scale: 0,
                }),
            ),
            (
                "SORT-RETURN".into(),
                CobolValue::Decimal(Decimal {
                    coefficient: 0,
                    scale: 0,
                }),
            ),
            (
                "TALLY".into(),
                CobolValue::Decimal(Decimal {
                    coefficient: 0,
                    scale: 0,
                }),
            ),
            (
                "XML-CODE".into(),
                CobolValue::Decimal(Decimal {
                    coefficient: 0,
                    scale: 0,
                }),
            ),
            ("XML-EVENT".into(), CobolValue::Bytes(Vec::new())),
            ("XML-INFORMATION".into(), CobolValue::Bytes(Vec::new())),
            ("XML-NAMESPACE".into(), CobolValue::Bytes(Vec::new())),
            ("XML-NNAMESPACE".into(), CobolValue::Bytes(Vec::new())),
            ("XML-NAMESPACE-PREFIX".into(), CobolValue::Bytes(Vec::new())),
            (
                "XML-NNAMESPACE-PREFIX".into(),
                CobolValue::Bytes(Vec::new()),
            ),
            ("XML-NTEXT".into(), CobolValue::Bytes(Vec::new())),
            ("XML-TEXT".into(), CobolValue::Bytes(Vec::new())),
        ]);
        implicit.insert(
            "WHEN-COMPILED".into(),
            CobolValue::Bytes(
                invocation
                    .bindings
                    .get("cobol.when-compiled")
                    .map(|payload| payload.bytes().to_vec())
                    .unwrap_or_else(|| b"1970010100000000+0000".to_vec()),
            ),
        );
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
        let arithmetic_modes = operations
            .iter()
            .filter(|operation| operation.identity.name() == "config")
            .map(|operation| optional_text_attribute(operation, "arithmetic_mode"))
            .collect::<Vec<_>>();
        let arithmetic_mode = match arithmetic_modes.as_slice() {
            [] => CobolArithmeticMode::Extended,
            [Some("compatible")] => CobolArithmeticMode::Compatible,
            [Some("extended")] => CobolArithmeticMode::Extended,
            _ => {
                return Err(MachineProblem::InvalidArtifact(
                    "invalid COBOL runtime config".into(),
                ));
            }
        };
        let display_signs = operations
            .iter()
            .filter(|operation| operation.identity.name() == "config")
            .map(|operation| optional_text_attribute(operation, "display_sign"))
            .collect::<Vec<_>>();
        let display_sign_separate = match display_signs.as_slice() {
            [] | [None] | [Some("compatible")] => false,
            [Some("separate")] => true,
            _ => {
                return Err(MachineProblem::InvalidArtifact(
                    "invalid COBOL display-sign config".into(),
                ));
            }
        };
        let declaratives = operations
            .iter()
            .find(|operation| operation.identity.name() == "config")
            .and_then(|operation| optional_text_attribute(operation, "declaratives"))
            .map(declarative_handlers)
            .transpose()?
            .unwrap_or_default();
        let (layouts, simple_layouts) = layout_metadata(&operations)?;
        let entry_formals = mainframe_env_ir::cobol_entry_formals(&module)
            .map_err(|problem| MachineProblem::InvalidArtifact(problem.to_string()))?;
        let dynamic_lengths = layouts
            .values()
            .filter(|layout| layout.dynamic)
            .map(|layout| (layout.name.clone(), 0usize))
            .collect();
        for index in layouts.values().flat_map(|layout| &layout.indexes) {
            implicit
                .entry(index.clone())
                .or_insert(CobolValue::Decimal(Decimal {
                    coefficient: 1,
                    scale: 0,
                }));
        }
        if let Some(call) = invocation.bindings.get("cobol.call.arguments") {
            let batch_main = call.schema() == "mainframe-env.cobol.batch-main@1";
            let values = decode_call_arguments(call)?;
            let formals: &[String] = match entry_formals.as_ref() {
                Some(formals) => formals,
                None if batch_main
                    && !layouts
                        .values()
                        .any(|layout| layout.linkage && layout.parent.is_none()) =>
                {
                    &[]
                }
                None => {
                    return Err(MachineProblem::InvalidArtifact(
                        "missing entry_formals_v1".into(),
                    ));
                }
            };
            if formals.len() < values.len() && !(batch_main && formals.is_empty()) {
                return Err(MachineProblem::InvalidOperation);
            }
            for (formal, value) in formals.iter().zip(values) {
                let layout = layouts
                    .get(&normalize(&formal))
                    .ok_or(MachineProblem::UnknownStorage)?;
                if !layout.linkage || layout.parent.is_some() || layout.length == 0 {
                    return Err(MachineProblem::InvalidArtifact(
                        "invalid entry formal layout".into(),
                    ));
                }
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
            arithmetic_mode,
            display_sign_separate,
            bases,
            static_base_count,
            views,
            views_by_id,
            storage_names_by_id,
            entry_initials,
            implicit: std::mem::take(&mut implicit),
            layouts,
            entry_formals: entry_formals.unwrap_or_default(),
            simple_layouts,
            files,
            declaratives,
            labels,
            control_nodes,
            loop_reentry: BTreeSet::new(),
            loop_counts: BTreeMap::new(),
            altered: BTreeMap::new(),
            last_file_status: "00".into(),
            condition_status: ConditionStatus::default(),
            dataset_cursors: BTreeMap::new(),
            sql_cursors: BTreeMap::new(),
            sort_workspaces: BTreeMap::new(),
            active_sort_procedure: None,
            sort_io: None,
            dynamic_lengths,
            search_results: BTreeMap::new(),
            linkage_addresses: BTreeMap::new(),
            freed_allocations: BTreeSet::new(),
            storage64: Storage64Arena::new(storage64_limits),
            storage64_area_bindings: BTreeMap::new(),
            random_state: Cell::new(None),
            pc: 0,
            output: Vec::new(),
            effect_sequence: 0,
            executed_steps: 0,
            pending: None,
            perform_stack: Vec::new(),
            deferred_drive: None,
        };
        typed_decimal::validate_machine(&machine)?;
        typed_cics::validate_machine(&machine)?;
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
        if let Some(selected_entry) = selected_entry {
            let entry_pc = machine
                .operations
                .iter()
                .enumerate()
                .filter(|(_, operation)| operation.identity.name() == "entry")
                .find_map(|(pc, operation)| {
                    arguments(operation)
                        .first()
                        .filter(|name| normalize(name) == selected_entry)
                        .map(|_| pc.saturating_add(1))
                })
                .ok_or(MachineProblem::InvalidOperation)?;
            let initializers = machine
                .operations
                .iter()
                .filter(|operation| operation.identity.name() == "init")
                .cloned()
                .collect::<Vec<_>>();
            for initializer in initializers {
                let reference = initializer
                    .storage
                    .first()
                    .ok_or(MachineProblem::InvalidOperation)?;
                let bytes = machine
                    .entry_initials
                    .get(&reference.storage)
                    .cloned()
                    .unwrap_or(bytes_attribute(&initializer, "initial")?.to_vec());
                machine.write_storage(reference.storage, &bytes)?;
            }
            machine.reapply_entry_context()?;
            machine.pc = entry_pc;
        }
        Ok(machine)
    }

    #[must_use]
    pub fn snapshot(&self) -> MachineSnapshot {
        MachineSnapshot {
            schema_version: 12,
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
            dynamic_lengths: self.dynamic_lengths.clone(),
            implicit_values: self
                .implicit
                .iter()
                .map(|(name, value)| {
                    (
                        name.clone(),
                        match value {
                            CobolValue::Bytes(bytes) => MachineSnapshotValue::Bytes(bytes.clone()),
                            CobolValue::Decimal(value) => MachineSnapshotValue::Decimal {
                                coefficient: value.coefficient,
                                scale: value.scale,
                            },
                        },
                    )
                })
                .collect(),
            search_results: self.search_results.clone(),
            sql_cursors: self.sql_cursors.clone(),
            sort_workspaces: self
                .sort_workspaces
                .iter()
                .map(|(name, workspace)| {
                    (name.clone(), (workspace.records.clone(), workspace.cursor))
                })
                .collect(),
            active_sort_procedure: self.active_sort_procedure.as_ref().map(|state| {
                (
                    state.sort_pc,
                    state.sort_file.clone(),
                    match state.phase {
                        SortProcedurePhase::Input => 0,
                        SortProcedurePhase::Output => 1,
                    },
                    state.arguments.clone(),
                )
            }),
            sort_io: self.sort_io.as_ref().map(|state| {
                (
                    state.sort_pc,
                    state.sort_file.clone(),
                    state.arguments.clone(),
                    state.inputs.clone(),
                    state.outputs.clone(),
                    state.next_input,
                    state.next_output,
                )
            }),
            linkage_addresses: self
                .linkage_addresses
                .iter()
                .map(|(name, view)| {
                    (
                        name.clone(),
                        view.as_ref()
                            .map(|view| (view.base, view.offset, view.length)),
                    )
                })
                .collect(),
            freed_allocations: self.freed_allocations.clone(),
            random_state: self.random_state.get(),
            storage64: self.storage64.snapshot(),
            storage64_area_bindings: self.storage64_area_bindings.clone(),
        }
    }

    pub fn restore(&mut self, snapshot: MachineSnapshot) -> Result<(), MachineProblem> {
        if !matches!(snapshot.schema_version, 1..=12)
            || snapshot.program_counter > self.operations.len()
            || snapshot.base_storage.iter().map(Vec::len).sum::<usize>()
                > self.invocation.limits.max_storage_bytes as usize
            || snapshot.base_storage.len() < self.static_base_count
            || self
                .bases
                .iter()
                .take(self.static_base_count)
                .zip(snapshot.base_storage.iter())
                .any(|(expected, actual)| expected.len() != actual.len())
            || snapshot.last_file_status.len() != 2
        {
            return Err(MachineProblem::IncompatibleSnapshot);
        }
        let mut restored_storage64 = self.new_storage64_arena();
        self.restore_storage64_snapshot(&mut restored_storage64, &snapshot)?;
        let base_used = snapshot
            .base_storage
            .iter()
            .enumerate()
            .filter(|(index, _)| !snapshot.freed_allocations.contains(index))
            .try_fold(0u64, |total, (_, bytes)| {
                total.checked_add(bytes.len() as u64)
            })
            .ok_or(MachineProblem::IncompatibleSnapshot)?;
        let live_base_count = (self.static_base_count..snapshot.base_storage.len())
            .filter(|base| !snapshot.freed_allocations.contains(base))
            .count();
        if base_used
            .checked_add(
                restored_storage64
                    .charged_bytes()
                    .ok_or(MachineProblem::IncompatibleSnapshot)?,
            )
            .is_none_or(|total| total > self.invocation.limits.max_storage_bytes)
            || live_base_count + restored_storage64.live_allocations()
                > self.invocation.limits.max_frames as usize
        {
            return Err(MachineProblem::IncompatibleSnapshot);
        }
        let area_bindings = self.restore_area64_bindings(&snapshot, &restored_storage64)?;
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
        if snapshot.schema_version >= 8 {
            let expected_dynamic = self
                .layouts
                .values()
                .filter(|layout| layout.dynamic)
                .map(|layout| layout.name.clone())
                .collect::<BTreeSet<_>>();
            if snapshot
                .dynamic_lengths
                .keys()
                .cloned()
                .collect::<BTreeSet<_>>()
                != expected_dynamic
                || snapshot.dynamic_lengths.iter().any(|(name, length)| {
                    self.layouts
                        .get(name)
                        .is_none_or(|layout| *length > layout.dynamic_limit)
                })
                || snapshot
                    .search_results
                    .keys()
                    .any(|node| !self.control_nodes.contains_key(node))
                || snapshot
                    .active_sort_procedure
                    .as_ref()
                    .is_some_and(|(pc, ..)| *pc >= self.operations.len())
                || snapshot
                    .sort_io
                    .as_ref()
                    .is_some_and(|(pc, ..)| *pc >= self.operations.len())
            {
                return Err(MachineProblem::IncompatibleSnapshot);
            }
            self.dynamic_lengths = snapshot.dynamic_lengths;
            self.implicit = snapshot
                .implicit_values
                .into_iter()
                .map(|(name, value)| {
                    (
                        name,
                        match value {
                            MachineSnapshotValue::Bytes(bytes) => CobolValue::Bytes(bytes),
                            MachineSnapshotValue::Decimal { coefficient, scale } => {
                                CobolValue::Decimal(Decimal { coefficient, scale })
                            }
                        },
                    )
                })
                .collect();
            self.search_results = snapshot.search_results;
            self.sql_cursors = snapshot.sql_cursors;
            self.sort_workspaces = snapshot
                .sort_workspaces
                .into_iter()
                .map(|(name, (records, cursor))| (name, SortWorkspace { records, cursor }))
                .collect();
            self.active_sort_procedure = snapshot
                .active_sort_procedure
                .map(|(sort_pc, sort_file, phase, arguments)| {
                    Ok(ActiveSortProcedure {
                        sort_pc,
                        sort_file,
                        phase: match phase {
                            0 => SortProcedurePhase::Input,
                            1 => SortProcedurePhase::Output,
                            _ => return Err(MachineProblem::IncompatibleSnapshot),
                        },
                        arguments,
                    })
                })
                .transpose()?;
            self.sort_io = snapshot.sort_io.map(
                |(sort_pc, sort_file, arguments, inputs, outputs, next_input, next_output)| {
                    SortIoState {
                        sort_pc,
                        sort_file,
                        arguments,
                        inputs,
                        outputs,
                        next_input,
                        next_output,
                    }
                },
            );
        } else {
            self.search_results.clear();
            self.sql_cursors.clear();
            self.sort_workspaces.clear();
            self.active_sort_procedure = None;
            self.sort_io = None;
        }
        if snapshot.schema_version >= 9 {
            if snapshot.linkage_addresses.len() > self.invocation.limits.max_frames as usize
                || snapshot.freed_allocations.len() > self.invocation.limits.max_frames as usize
                || snapshot
                    .freed_allocations
                    .iter()
                    .any(|base| *base < self.static_base_count || *base >= self.bases.len())
                || snapshot.linkage_addresses.iter().any(|(name, view)| {
                    !self.layouts.get(name).is_some_and(|layout| layout.linkage)
                        || self.views.get(name).is_none_or(|original| {
                            view.as_ref()
                                .is_some_and(|(_, _, length)| *length != original.length)
                        })
                        || view.as_ref().is_some_and(|(base, offset, length)| {
                            self.bases.get(*base).is_none_or(|storage| {
                                offset
                                    .checked_add(*length)
                                    .is_none_or(|end| end > storage.len())
                            })
                        })
                })
            {
                return Err(MachineProblem::IncompatibleSnapshot);
            }
            self.linkage_addresses = snapshot
                .linkage_addresses
                .into_iter()
                .map(|(name, view)| {
                    (
                        name,
                        view.map(|(base, offset, length)| StorageView {
                            base,
                            offset,
                            length,
                        }),
                    )
                })
                .collect();
            self.freed_allocations = snapshot.freed_allocations;
        } else {
            self.linkage_addresses.clear();
            self.freed_allocations.clear();
        }
        self.random_state.set(if snapshot.schema_version >= 10 {
            snapshot.random_state
        } else {
            None
        });
        self.install_storage64_snapshot(restored_storage64, area_bindings);
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
                | "mainframe-env.reference-machine-checkpoint@8"
                | "mainframe-env.reference-machine-checkpoint@9"
                | "mainframe-env.reference-machine-checkpoint@10"
                | "mainframe-env.reference-machine-checkpoint@11"
                | "mainframe-env.reference-machine-checkpoint@12"
        ) {
            return Err(MachineProblem::IncompatibleSnapshot);
        }
        let snapshot = decode_snapshot(
            payload.bytes(),
            self.invocation.limits.max_storage_bytes as usize,
            self.invocation.limits.max_output_bytes as usize,
            self.invocation.limits.max_frames as usize,
        )?;
        self.restore(snapshot)
    }

    #[must_use]
    pub fn output(&self) -> &[u8] {
        &self.output
    }
    /// Current task priority after any completed CICS scheduling command.
    #[must_use]
    pub fn invocation_priority(&self) -> u8 {
        self.invocation.priority
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

    /// Installs the bounded low-storage chain used by batch programs that inspect
    /// the MVS PSA, TCB, and TIOT through COBOL linkage pointers.
    /// The compatibility storage is only installed when the compiled module
    /// declares the complete structure. Modules with none of the conventional
    /// names are left unchanged; partial declarations fail closed.
    pub fn install_mvs_tiot<'a, I>(
        &mut self,
        job_name: &str,
        step_name: &str,
        raw_dd_names: I,
    ) -> Result<bool, MachineProblem>
    where
        I: IntoIterator<Item = &'a str>,
    {
        const REQUIRED: [&str; 16] = [
            "PSAPTR",
            "PSA-BLOCK",
            "TCB-POINT",
            "TCB-BLOCK",
            "TIOT-POINT",
            "TIOT-BLOCK",
            "TIOTNJOB",
            "TIOTJSTP",
            "TIOTPSTP",
            "TIOT-INDEX",
            "TIOT-ENTRY",
            "TIOT-SEG",
            "TIO-LEN",
            "TIOCDDNM",
            "UCB-ADDR",
            "END-OF-TIOT",
        ];

        let declared = REQUIRED
            .iter()
            .filter(|name| self.layout(name).is_some())
            .count();
        if declared == 0 {
            return Ok(false);
        }
        if declared != REQUIRED.len() {
            return Err(MachineProblem::InvalidArtifact(
                "partial MVS PSA/TCB/TIOT layout".into(),
            ));
        }

        let valid_mvs_name = |name: &str| {
            !name.is_empty()
                && name.len() <= 8
                && name.bytes().enumerate().all(|(index, byte)| {
                    byte.is_ascii_uppercase()
                        || matches!(byte, b'$' | b'@' | b'#')
                        || (index > 0 && byte.is_ascii_digit())
                })
        };
        let job_name = job_name.to_ascii_uppercase();
        let step_name = step_name.to_ascii_uppercase();
        if !valid_mvs_name(&job_name) || !valid_mvs_name(&step_name) {
            return Err(MachineProblem::InvalidOperation);
        }
        let mut seen = BTreeSet::new();
        let mut dd_names = Vec::new();
        for name in raw_dd_names {
            let name = name.to_ascii_uppercase();
            if !valid_mvs_name(&name) {
                return Err(MachineProblem::InvalidOperation);
            }
            if seen.insert(name.clone()) {
                dd_names.push(name);
            }
        }
        if dd_names.len() > self.invocation.limits.max_frames as usize {
            return Err(MachineProblem::ResourceExhausted);
        }

        let required_layout = |name: &str| {
            self.layout(name).cloned().ok_or_else(|| {
                MachineProblem::InvalidArtifact(format!("missing MVS layout {name}"))
            })
        };
        let psaptr = required_layout("PSAPTR")?;
        let psa = required_layout("PSA-BLOCK")?;
        let tcb_point = required_layout("TCB-POINT")?;
        let tcb = required_layout("TCB-BLOCK")?;
        let tiot_point = required_layout("TIOT-POINT")?;
        let tiot = required_layout("TIOT-BLOCK")?;
        let tiot_job = required_layout("TIOTNJOB")?;
        let tiot_job_step = required_layout("TIOTJSTP")?;
        let tiot_proc_step = required_layout("TIOTPSTP")?;
        let tiot_index = required_layout("TIOT-INDEX")?;
        let entry = required_layout("TIOT-ENTRY")?;
        let segment = required_layout("TIOT-SEG")?;
        let segment_length = required_layout("TIO-LEN")?;
        let dd_name = required_layout("TIOCDDNM")?;
        let ucb_address = required_layout("UCB-ADDR")?;
        let end_of_tiot = required_layout("END-OF-TIOT")?;
        let end_marker = end_of_tiot
            .parent
            .as_ref()
            .and_then(|parent| self.layouts.get(parent))
            .cloned()
            .ok_or_else(|| {
                MachineProblem::InvalidArtifact("missing MVS TIOT end-marker storage".into())
            })?;

        let pointer_like = |layout: &LayoutMetadata| is_pointer_like(layout.category);
        if !pointer_like(&psaptr)
            || !pointer_like(&tcb_point)
            || !pointer_like(&tiot_point)
            || !pointer_like(&tiot_index)
            || psaptr.length != tcb_point.length
            || psaptr.length != tiot_point.length
            || psaptr.length != tiot_index.length
            || !psa.linkage
            || !tcb.linkage
            || !tiot.linkage
            || !entry.linkage
            || segment.length == 0
            || segment.length > usize::from(u8::MAX)
            || entry.length < segment.length
            || segment_length.length != 1
            || dd_name.length != 8
            || ucb_address.length == 0
            || tiot_job.length != 8
            || tiot_job_step.length != 8
            || tiot_proc_step.length != 8
            || end_marker.length < 4
        {
            return Err(MachineProblem::InvalidArtifact(format!(
                "invalid MVS PSA/TCB/TIOT layout: pointers={:?}/{:?}/{:?}/{:?} lengths={}/{}/{}/{} linkage={}/{}/{}/{} segment={} entry={} fields={}/{}/{}/{}/{}/{}/{} end={}",
                psaptr.category,
                tcb_point.category,
                tiot_point.category,
                tiot_index.category,
                psaptr.length,
                tcb_point.length,
                tiot_point.length,
                tiot_index.length,
                psa.linkage,
                tcb.linkage,
                tiot.linkage,
                entry.linkage,
                segment.length,
                entry.length,
                segment_length.length,
                dd_name.length,
                ucb_address.length,
                tiot_job.length,
                tiot_job_step.length,
                tiot_proc_step.length,
                end_of_tiot.length,
                end_of_tiot.offset,
            )));
        }

        let relative =
            |root: &LayoutMetadata, child: &LayoutMetadata| -> Result<usize, MachineProblem> {
                let root = self
                    .views
                    .get(&root.name)
                    .ok_or(MachineProblem::UnknownStorage)?;
                let child = self
                    .views
                    .get(&child.name)
                    .ok_or(MachineProblem::UnknownStorage)?;
                if root.base != child.base
                    || child.offset < root.offset
                    || child
                        .offset
                        .checked_add(child.length)
                        .is_none_or(|end| end > root.offset.saturating_add(root.length))
                {
                    return Err(MachineProblem::InvalidArtifact(
                        "disconnected MVS PSA/TCB/TIOT layout".into(),
                    ));
                }
                Ok(child.offset - root.offset)
            };
        let tcb_pointer_offset = relative(&psa, &tcb_point)?;
        let tiot_pointer_offset = relative(&tcb, &tiot_point)?;
        let tiot_job_offset = relative(&tiot, &tiot_job)?;
        let tiot_job_step_offset = relative(&tiot, &tiot_job_step)?;
        let tiot_proc_step_offset = relative(&tiot, &tiot_proc_step)?;
        let segment_length_offset = relative(&segment, &segment_length)?;
        let dd_name_offset = relative(&segment, &dd_name)?;
        let ucb_address_offset = relative(&segment, &ucb_address)?;
        let end_marker_offset = relative(&entry, &end_marker)?;
        if end_marker_offset < segment.length {
            return Err(MachineProblem::InvalidArtifact(
                "overlapping MVS TIOT end marker".into(),
            ));
        }

        let align = |value: usize| {
            value
                .checked_add(7)
                .map(|value| value & !7usize)
                .ok_or(MachineProblem::ResourceExhausted)
        };
        let psa_offset = 0usize;
        let tcb_offset = align(psa.length)?;
        let tiot_offset = align(
            tcb_offset
                .checked_add(tcb.length)
                .ok_or(MachineProblem::ResourceExhausted)?,
        )?;
        let entries_offset = tiot_offset
            .checked_add(tiot.length)
            .ok_or(MachineProblem::ResourceExhausted)?;
        let entries_length = if dd_names.is_empty() {
            entry.length
        } else {
            dd_names
                .len()
                .checked_sub(1)
                .and_then(|count| count.checked_mul(segment.length))
                .and_then(|length| length.checked_add(entry.length))
                .ok_or(MachineProblem::ResourceExhausted)?
        };
        let total_length = entries_offset
            .checked_add(entries_length)
            .ok_or(MachineProblem::ResourceExhausted)?;
        let current_storage = self.bases.iter().try_fold(0usize, |total, base| {
            total
                .checked_add(base.len())
                .ok_or(MachineProblem::ResourceExhausted)
        })?;
        let storage_limit = usize::try_from(self.invocation.limits.max_storage_bytes)
            .map_err(|_| MachineProblem::ResourceExhausted)?;
        if current_storage
            .checked_add(total_length)
            .is_none_or(|total| total > storage_limit)
        {
            return Err(MachineProblem::ResourceExhausted);
        }

        let base = self.bases.len();
        let psa_address = self.address_bytes_for(base, psa_offset, psaptr.length)?;
        let tcb_address = self.address_bytes_for(base, tcb_offset, tcb_point.length)?;
        let tiot_address = self.address_bytes_for(base, tiot_offset, tiot_point.length)?;
        let psaptr_view = self
            .views
            .get(&psaptr.name)
            .cloned()
            .ok_or(MachineProblem::UnknownStorage)?;
        let (initializer, initial_storage_id, initial_storage_view) = self
            .operations
            .iter()
            .filter(|operation| operation.identity.name() == "init")
            .filter_map(|operation| {
                let id = operation.storage.first()?.storage;
                let view = self.views_by_id.get(&id)?;
                (view.base == psaptr_view.base
                    && view.offset <= psaptr_view.offset
                    && psaptr_view
                        .offset
                        .checked_add(psaptr_view.length)
                        .is_some_and(|end| end <= view.offset.saturating_add(view.length)))
                .then_some((operation, id, view.clone()))
            })
            .min_by_key(|(_, _, view)| view.length)
            .ok_or(MachineProblem::UnknownStorage)?;
        let mut entry_initial = self
            .entry_initials
            .get(&initial_storage_id)
            .cloned()
            .unwrap_or(bytes_attribute(initializer, "initial")?.to_vec());
        let psaptr_initial_offset = psaptr_view
            .offset
            .checked_sub(initial_storage_view.offset)
            .ok_or(MachineProblem::InvalidOperation)?;
        let psaptr_initial_end = psaptr_initial_offset
            .checked_add(psa_address.len())
            .ok_or(MachineProblem::ResourceExhausted)?;
        entry_initial
            .get_mut(psaptr_initial_offset..psaptr_initial_end)
            .ok_or(MachineProblem::InvalidOperation)?
            .copy_from_slice(&psa_address);
        let mut storage = vec![0; total_length];
        storage[tcb_pointer_offset..tcb_pointer_offset + tcb_address.len()]
            .copy_from_slice(&tcb_address);
        let tiot_pointer_start = tcb_offset + tiot_pointer_offset;
        storage[tiot_pointer_start..tiot_pointer_start + tiot_address.len()]
            .copy_from_slice(&tiot_address);

        let write_name = |storage: &mut [u8], offset: usize, name: &str| {
            storage[offset..offset + 8].fill(b' ');
            storage[offset..offset + name.len()].copy_from_slice(name.as_bytes());
        };
        write_name(&mut storage, tiot_offset + tiot_job_offset, &job_name);
        write_name(&mut storage, tiot_offset + tiot_job_step_offset, &step_name);
        write_name(
            &mut storage,
            tiot_offset + tiot_proc_step_offset,
            &step_name,
        );
        for (index, name) in dd_names.iter().enumerate() {
            let offset = entries_offset
                + index
                    .checked_mul(segment.length)
                    .ok_or(MachineProblem::ResourceExhausted)?;
            storage[offset + segment_length_offset] = segment.length as u8;
            write_name(&mut storage, offset + dd_name_offset, name);
            storage[offset + ucb_address_offset + ucb_address.length - 1] = 1;
        }

        self.write(&psaptr.name, &psa_address)?;
        self.entry_initials
            .insert(initial_storage_id, entry_initial);
        self.bases.push(storage);
        self.static_base_count = self
            .static_base_count
            .checked_add(1)
            .ok_or(MachineProblem::ResourceExhausted)?;
        Ok(true)
    }

    #[must_use]
    pub fn variable(&self, name: &str) -> Option<FixedValue> {
        self.read(name).ok().map(FixedValue::new)
    }

    #[must_use]
    pub fn file_contract(&self, name: &str) -> Option<&str> {
        self.files
            .get(&normalize(name))
            .map(|file| file.description.as_str())
    }

    pub fn linkage_values(&self) -> Result<Vec<Vec<u8>>, MachineProblem> {
        self.entry_formals
            .iter()
            .map(|name| self.read(name))
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
        // An in-doubt effect is not a language-level exception. CALL/ACCEPT
        // handlers, declaratives and subsystem error translation must not
        // turn uncertainty about committed work into ordinary control flow.
        if matches!(&result.outcome, Err(HostProblem::UnknownOutcome)) {
            return Err(MachineProblem::Host(HostProblem::UnknownOutcome));
        }
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
            if let Some(handler) = pending_declarative(&pending.kind) {
                self.enter_declarative(handler)?;
            }
            return Ok(());
        }
        if result.outcome.is_err() {
            match &pending.kind {
                PendingKind::Accept {
                    handles_exception: true,
                    ..
                }
                | PendingKind::AcceptClock {
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
            if let Some(handler) = pending_declarative(&pending.kind) {
                self.last_file_status = "30".into();
                self.enter_declarative(handler)?;
                return Ok(());
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
            (PendingKind::AcceptClock { target, format, .. }, HostResult::Clock(value)) => {
                self.condition_status.accept_exception = false;
                let value = accept_clock_value(format, &value)?;
                self.write(&target, &value)?;
            }
            (
                PendingKind::DatasetRead {
                    target,
                    into,
                    depending_on,
                    status,
                    ccsid,
                    ..
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
                    let decoded = decode_dataset_record(ccsid, record)?;
                    if let Some(target) = target {
                        self.finish_dataset_read(
                            &target,
                            into.as_deref(),
                            depending_on.as_deref(),
                            &decoded,
                        )?;
                    } else if let Some(into) = into {
                        self.write(&into, &decoded)?;
                    }
                }
                if let Some(status) = status {
                    self.write(&status, if records.is_empty() { b"10" } else { b"00" })?;
                }
            }
            (
                PendingKind::DatasetRead {
                    target,
                    into,
                    depending_on,
                    status,
                    ccsid,
                    ..
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
                if let Some(record) = record.as_ref() {
                    let decoded = decode_dataset_record(ccsid, record)?;
                    if let Some(target) = target {
                        self.finish_dataset_read(
                            &target,
                            into.as_deref(),
                            depending_on.as_deref(),
                            &decoded,
                        )?;
                    } else if let Some(into) = into {
                        self.write(&into, &decoded)?;
                    }
                }
                if let Some(status) = status {
                    self.write(&status, if record.is_some() { b"00" } else { b"10" })?;
                }
            }
            (
                PendingKind::DatasetStatus {
                    status,
                    cursor: Some(DatasetCursorAction::Start(dataset)),
                    ..
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
                    ..
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
            (PendingKind::DatasetRead { status, .. }, HostResult::Dataset(_)) => {
                self.last_file_status = "00".into();
                if let Some(status) = status {
                    self.write(&status, b"00")?;
                }
            }
            (
                PendingKind::DatasetStatus {
                    status,
                    cursor: None,
                    linage_advance,
                    linage_limit,
                    page_advance,
                    ..
                },
                HostResult::Dataset(_),
            ) => {
                self.last_file_status = "00".into();
                if let Some(status) = status {
                    self.write(&status, b"00")?;
                }
                self.condition_status.end_of_page = false;
                if page_advance {
                    self.condition_status.end_of_page = true;
                    self.implicit.insert(
                        "LINAGE-COUNTER".into(),
                        CobolValue::Decimal(Decimal {
                            coefficient: 1,
                            scale: 0,
                        }),
                    );
                } else if linage_advance > 0
                    && let Some(CobolValue::Decimal(counter)) =
                        self.implicit.get("LINAGE-COUNTER").cloned()
                {
                    let next = decimal_add(
                        self.arithmetic_mode,
                        counter,
                        Decimal {
                            coefficient: i128::try_from(linage_advance)
                                .map_err(|_| MachineProblem::ResourceExhausted)?,
                            scale: 0,
                        },
                    )?;
                    self.condition_status.end_of_page = linage_limit.is_some_and(|limit| {
                        next.scale == 0
                            && usize::try_from(next.coefficient)
                                .is_ok_and(|counter| counter >= limit)
                    });
                    self.implicit
                        .insert("LINAGE-COUNTER".into(), CobolValue::Decimal(next));
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
                    returning,
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
                        dump: AbendDumpDisposition::Unspecified,
                    }));
                    return Ok(());
                }
                self.condition_status.call_exception = false;
                let values = decode_call_values(&payload)?;
                if values.len() != targets.len() + usize::from(returning.is_some()) {
                    return Err(MachineProblem::UnexpectedHostResult);
                }
                for (target, value) in targets.iter().zip(&values) {
                    self.write(target, value)?;
                }
                if let Some(target) = returning {
                    self.write(
                        &target,
                        values.last().ok_or(MachineProblem::UnexpectedHostResult)?,
                    )?;
                }
            }
            (
                PendingKind::SortRead,
                HostResult::Dataset(mainframe_env_host_api::DatasetResult::Records {
                    records, ..
                }),
            ) => {
                if let Some(step) = self.continue_sort_after_read(records)? {
                    self.defer_sort_step(step)?;
                }
            }
            (PendingKind::SortWrite, HostResult::Dataset(_)) => {
                if let Some(step) = self.continue_sort_output()? {
                    self.defer_sort_step(step)?;
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
                    storage64_intent,
                    argument_summary: _,
                    into,
                    outputs,
                    response: response_target,
                    response2: response2_target,
                    address_set,
                    no_handle,
                },
                HostResult::Cics(response),
            ) => {
                let responded = response_target.is_some() || no_handle;
                let load_base = typed_cics::write_response_state(
                    self,
                    operation,
                    storage64_intent,
                    response_target.as_ref(),
                    response2_target.as_ref(),
                    address_set.as_ref(),
                    &outputs,
                    &response,
                )?;
                if let Some(target) = into
                    && matches!(
                        typed_cics::into_payload_schema(operation, &response),
                        Some("mainframe-env.cics.into@1" | "mainframe-env.cics.payload@1")
                    )
                {
                    typed_cics::write_target(
                        self,
                        &target,
                        &CobolValue::Bytes(response.payload.bytes().to_vec()),
                    )?;
                }
                for (name, value) in &response.outputs {
                    if typed_cics::write_runtime_output(self, operation, name, value)? {
                        continue;
                    }
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
                    typed_cics::write_output(self, operation, name, target, value, load_base)?;
                }
                eib::write_context(self, operation, &response)?;
                self.deferred_drive =
                    typed_cics::drive_response(self, operation, response, responded)?;
            }
            (
                PendingKind::DatasetRead { .. }
                | PendingKind::DatasetStatus { .. }
                | PendingKind::ProgramCall { .. }
                | PendingKind::SortRead
                | PendingKind::SortWrite
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

    fn enter_declarative(&mut self, handler: &str) -> Result<(), MachineProblem> {
        let call_pc = self
            .pc
            .checked_sub(1)
            .ok_or(MachineProblem::InvalidOperation)?;
        self.altered
            .insert(declarative_state_key(call_pc), normalize(handler));
        self.perform_stack.push(call_pc);
        self.pc = self.label(handler)?;
        Ok(())
    }

    fn execute(&mut self, operation: &Operation) -> Result<Step, MachineProblem> {
        if typed_cics::is_typed(operation) {
            return typed_cics::execute(self, operation);
        }
        let name = operation.identity.name();
        let args = arguments(operation);
        if let Some(step) = self.execute_control(operation, name, &args)? {
            return Ok(step);
        }
        match name {
            "config" | "define" | "file" => {}
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
                let operand_end = [position(&args, "UPON"), position(&args, "WITH")]
                    .into_iter()
                    .flatten()
                    .min()
                    .unwrap_or(args.len());
                let mut at = 0usize;
                while at < operand_end {
                    let reference_end = (at..operand_end)
                        .find(|index| display_literal(&args[*index]))
                        .unwrap_or(operand_end);
                    if display_literal(&args[at]) {
                        line.extend(self.resolve(&args[at])?);
                        at += 1;
                    } else if args[at] == "ALL"
                        && args.get(at + 1).is_some_and(|token| display_literal(token))
                    {
                        line.extend(self.resolve(&args[at + 1])?);
                        at += 2;
                    } else if matches!(args[at].as_str(), "ADDRESS" | "LENGTH")
                        && args.get(at + 1).is_some_and(|token| token == "OF")
                    {
                        let end = (at + 3..=reference_end)
                            .rev()
                            .find(|end| self.eval_value(&args[at..*end]).is_ok())
                            .ok_or(MachineProblem::InvalidOperation)?;
                        line.extend(value_bytes(self.eval_value(&args[at..end])?)?);
                        at = end;
                    } else if let Some((end, reference)) =
                        (at + 1..=reference_end).rev().find_map(|end| {
                            self.reference(&args[at..end])
                                .ok()
                                .map(|reference| (end, reference))
                        })
                    {
                        line.extend(self.display_reference(&reference)?);
                        at = end;
                    } else if args[at] == "FUNCTION" {
                        let end = (at + 1..=operand_end)
                            .rev()
                            .find(|end| self.eval_value(&args[at..*end]).is_ok())
                            .ok_or(MachineProblem::InvalidOperation)?;
                        line.extend(value_bytes(self.eval_value(&args[at..end])?)?);
                        at = end;
                    } else {
                        line.extend(self.resolve(&args[at])?);
                        at += 1;
                    }
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
            "assign" if typed_decimal::is_assign(operation) => {
                typed_decimal::execute_with_condition(self, operation)?;
            }
            "add" | "subtract" | "multiply" | "divide" | "compute" => {
                let handles_size_error = self.has_condition_handler(operation, "ON SIZE ERROR");
                match self.arithmetic(name, &args, handles_size_error) {
                    Ok(receiver_size_error) => {
                        self.condition_status.arithmetic_size_error = receiver_size_error
                    }
                    Err(MachineProblem::SizeError) => {
                        self.condition_status.arithmetic_size_error = true;
                        if !handles_size_error {
                            return Err(MachineProblem::SizeError);
                        }
                    }
                    Err(problem) => return Err(problem),
                }
            }
            "initialize" => self.initialize_op(&args)?,
            "set" => self.set_op(&args)?,
            "allocate" => self.allocate_op(&args)?,
            "free" => self.free_op(&args)?,
            "string" => {
                self.condition_status.string_overflow = self.string_op(&args)?;
            }
            "unstring" => {
                self.condition_status.unstring_overflow = self.unstring_op(&args)?;
            }
            "inspect" => self.inspect_op(&args)?,
            "xml_parse" if position(&args, "PROCESSING").is_some() => {
                match self.xml_processing_step(&args) {
                    Ok(step) => {
                        self.condition_status.xml_exception = false;
                        self.implicit.insert(
                            "XML-CODE".into(),
                            CobolValue::Decimal(Decimal {
                                coefficient: 0,
                                scale: 0,
                            }),
                        );
                        return Ok(step);
                    }
                    Err(_problem) if self.has_condition_handler(operation, "ON EXCEPTION") => {
                        self.condition_status.xml_exception = true;
                        self.implicit.insert(
                            "XML-CODE".into(),
                            CobolValue::Decimal(Decimal {
                                coefficient: -1,
                                scale: 0,
                            }),
                        );
                    }
                    Err(problem) => return Err(problem),
                }
            }
            "json_generate" | "json_parse" | "xml_generate" | "xml_parse" => {
                let json = name.starts_with("json_");
                if json {
                    self.implicit.insert(
                        "JSON-STATUS".into(),
                        CobolValue::Decimal(Decimal {
                            coefficient: 0,
                            scale: 0,
                        }),
                    );
                }
                let result = if name.ends_with("_generate") {
                    self.generate(&args, json)
                } else {
                    self.parse_generated(&args, json)
                };
                let failed = match result {
                    Ok(()) => false,
                    Err(_problem) if self.has_condition_handler(operation, "ON EXCEPTION") => {
                        if name == "json_parse"
                            && args.windows(2).any(|window| window == ["WITH", "DETAIL"])
                        {
                            self.append_output(b"IGZ0335W JSON PARSE input is invalid\n")?;
                        }
                        true
                    }
                    Err(problem) => return Err(problem),
                };
                if json {
                    self.condition_status.json_exception = failed;
                    let code = CobolValue::Decimal(Decimal {
                        coefficient: if failed { -1 } else { 0 },
                        scale: 0,
                    });
                    self.implicit.insert("JSON-CODE".into(), code.clone());
                    if failed {
                        self.implicit.insert("JSON-STATUS".into(), code);
                    }
                } else {
                    self.condition_status.xml_exception = failed;
                    self.implicit.insert(
                        "XML-CODE".into(),
                        CobolValue::Decimal(Decimal {
                            coefficient: if failed { -1 } else { 0 },
                            scale: 0,
                        }),
                    );
                }
            }
            "if" => self.if_op(&args)?,
            "evaluate" => self.evaluate_op(&args)?,
            "search" => self.search_op(&args)?,
            "go_to" => {
                return self.go_to_step(&args);
            }
            "alter" => {
                if args.len() < 5 {
                    return Err(MachineProblem::InvalidOperation);
                }
                self.altered
                    .insert(normalize(&args[0]), normalize(args.last().unwrap()));
            }
            "perform" => {
                return self.out_of_line_perform_step(&args);
            }
            "exit"
                if args.iter().any(|argument| {
                    matches!(argument.as_str(), "PROGRAM" | "METHOD" | "FUNCTION")
                }) =>
            {
                return Ok(Step::Complete);
            }
            "exit" if args.iter().any(|argument| argument == "PERFORM") => {
                return self.exit_perform_step(operation);
            }
            "exit" if args.iter().any(|argument| argument == "SECTION") => {
                return self.exit_section_step();
            }
            "exit" if args.iter().any(|argument| argument == "PARAGRAPH") => {
                return self.exit_paragraph_step();
            }
            "exit" => {}
            "label"
                if args.first().is_some_and(|name| {
                    self.declaratives
                        .values()
                        .any(|handler| handler == &normalize(name))
                }) && !self.active_declarative() =>
            {
                return self.skip_declarative_section(&args);
            }
            "entry" | "label" | "continue" => {}
            "accept" => return self.accept_effect(operation, &args),
            "call" | "cancel" | "invoke" => {
                return self.program_effect(operation, name, &args);
            }
            "open" | "close" | "delete" | "read" | "rewrite" | "start" | "write" => {
                return self.dataset_effect(name, &args);
            }
            "sort" | "merge" => return self.sort_effect(name, &args),
            "release" => self.release_sort_record(&args)?,
            "return_statement" => self.return_sort_record(&args)?,
            "exec_cics" => return typed_cics::execute_legacy(self, &args),
            "exec_sql" => return self.sql_effect(&args),
            "exec_dli" => return self.ims_effect(&args),
            "stop_run" => {
                if let Some(returning) = position(&args, "RETURNING")
                    && let Some(value) = args.get(returning + 1)
                {
                    let value = self.decimal(value)?;
                    if value.scale != 0 {
                        return Err(MachineProblem::DataException);
                    }
                    self.implicit
                        .insert("RETURN-CODE".into(), CobolValue::Decimal(value));
                }
                return Ok(Step::Complete);
            }
            "go_back" | "halt" => return Ok(Step::Complete),
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
            "block_start" if scope == "evaluate" => Ok(Some(Step::Next)),
            "block_start" if scope == "search" => {
                self.prepare_search(operation, args)?;
                Ok(Some(Step::Next))
            }
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
            "transfer" if name == "go_to" => self.go_to_step(args).map(Some),
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

    fn go_to_step(&self, args: &[String]) -> Result<Step, MachineProblem> {
        if let Some(depending) = position(args, "DEPENDING") {
            let selector = position(args, "ON")
                .and_then(|on| args.get(on + 1))
                .ok_or(MachineProblem::InvalidOperation)?;
            let selector = self.decimal(selector)?;
            if selector.scale != 0 {
                return Err(MachineProblem::DataException);
            }
            let index = usize::try_from(selector.coefficient)
                .ok()
                .and_then(|value| value.checked_sub(1));
            let targets = args[..depending]
                .iter()
                .filter(|target| !matches!(target.as_str(), "TO" | ","))
                .collect::<Vec<_>>();
            return index
                .and_then(|index| targets.get(index))
                .map_or(Ok(Step::Next), |target| self.label(target).map(Step::Jump));
        }
        self.label(
            args.iter()
                .find(|target| target.as_str() != "TO")
                .ok_or(MachineProblem::InvalidOperation)?,
        )
        .map(Step::Jump)
    }
    fn control_branch(&self, operation: &Operation) -> Result<bool, MachineProblem> {
        if let Some(status) = typed_decimal::control_branch(self, operation)? {
            return Ok(status);
        }
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
            let search_result = parent
                .and_then(|parent| optional_integer_attribute(parent, "control_node"))
                .and_then(|node| usize::try_from(node).ok())
                .and_then(|node| self.search_results.get(&node).copied());
            let positive = match (text.to_ascii_uppercase().as_str(), parent_name) {
                ("AT END", Some("search")) => search_result.map(|found| !found),
                ("AT END", _) => Some(self.last_file_status == "10"),
                ("AT END-OF-PAGE", Some("write")) => Some(self.condition_status.end_of_page),
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
                ("NOT AT END", Some("search")) => search_result,
                ("NOT AT END", _) => Some(self.last_file_status != "10"),
                ("NOT AT END-OF-PAGE", Some("write")) => Some(!self.condition_status.end_of_page),
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
        if let Some(status) = status_branch() {
            return Ok(status);
        }
        let Some(parent) = parent else {
            return Ok(false);
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
            Ok(true)
        }
    }
    fn prepare_search(
        &mut self,
        operation: &Operation,
        args: &[String],
    ) -> Result<(), MachineProblem> {
        let node = optional_integer_attribute(operation, "control_node")
            .and_then(|node| usize::try_from(node).ok())
            .ok_or(MachineProblem::InvalidOperation)?;
        let table_name = args
            .iter()
            .find(|token| !matches!(token.as_str(), "ALL" | "SEARCH"))
            .ok_or(MachineProblem::InvalidOperation)?;
        let table = self
            .layout(table_name)
            .cloned()
            .ok_or(MachineProblem::UnknownStorage)?;
        if table.occurs <= 1 {
            return Err(MachineProblem::SubscriptError);
        }
        let index_name = position(args, "VARYING")
            .and_then(|position| args.get(position + 1))
            .cloned()
            .or_else(|| table.indexes.first().cloned())
            .unwrap_or_else(|| format!("__SEARCH.{node}"));
        self.implicit
            .entry(normalize(&index_name))
            .or_insert(CobolValue::Decimal(Decimal {
                coefficient: 1,
                scale: 0,
            }));
        let active = if let Some(depending_on) = &table.depending_on {
            let value = self.decimal(depending_on)?;
            if value.scale != 0 {
                return Err(MachineProblem::DataException);
            }
            usize::try_from(value.coefficient)
                .map_err(|_| MachineProblem::SubscriptError)?
                .clamp(table.occurs_min, table.occurs)
        } else {
            table.occurs
        };
        let branch_conditions = self
            .operations
            .iter()
            .filter(|candidate| {
                optional_integer_attribute(candidate, "control_parent") == i64::try_from(node).ok()
                    && optional_text_attribute(candidate, "control_role") == Some("branch")
            })
            .filter_map(|candidate| optional_text_attribute(candidate, "control_text"))
            .filter(|text| text.to_ascii_uppercase().starts_with("WHEN "))
            .map(control_tokens)
            .map(|tokens| tokens.into_iter().skip(1).collect::<Vec<_>>())
            .collect::<Vec<_>>();
        let mut found = false;
        for occurrence in 1..=active {
            self.implicit.insert(
                normalize(&index_name),
                CobolValue::Decimal(Decimal {
                    coefficient: occurrence as i128,
                    scale: 0,
                }),
            );
            if branch_conditions
                .iter()
                .any(|condition| self.eval_condition(condition).unwrap_or(false))
            {
                found = true;
                break;
            }
        }
        if !found {
            self.implicit.insert(
                normalize(&index_name),
                CobolValue::Decimal(Decimal {
                    coefficient: active.saturating_add(1) as i128,
                    scale: 0,
                }),
            );
        }
        self.search_results.insert(node, found);
        Ok(())
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
            let value = decimal_add(
                self.arithmetic_mode,
                self.decimal(variable)?,
                self.decimal(by)?,
            )?;
            self.write_decimal(variable, value)?;
        }
        if let Some(count) = self.loop_counts.get_mut(&parent) {
            *count = count.saturating_sub(1);
        }
        self.loop_reentry.insert(parent);
        Ok(Step::Jump(self.control_target(operation, "edge_loop")?))
    }

    fn out_of_line_perform_step(&mut self, args: &[String]) -> Result<Step, MachineProblem> {
        let target = args.first().ok_or(MachineProblem::InvalidOperation)?;
        let key = out_of_line_perform_key(self.pc);
        if let Some(varying) = position(args, "VARYING") {
            let variable = args
                .get(varying + 1)
                .ok_or(MachineProblem::InvalidOperation)?;
            let from = position(args, "FROM")
                .and_then(|at| args.get(at + 1))
                .ok_or(MachineProblem::InvalidOperation)?;
            let value = self.decimal(from)?;
            self.write_decimal(variable, value)?;
            let until = position(args, "UNTIL").ok_or(MachineProblem::InvalidOperation)?;
            if self.eval_condition(&args[until + 1..])? {
                return Ok(Step::Next);
            }
            self.loop_counts.insert(key, 1);
        } else if let Some(until) = position(args, "UNTIL") {
            if self.eval_condition(&args[until + 1..])? {
                return Ok(Step::Next);
            }
            self.loop_counts.insert(key, 1);
        } else if let Some(times) = position(args, "TIMES") {
            let clause = 1 + usize::from(
                args.get(1)
                    .is_some_and(|token| matches!(token.as_str(), "THROUGH" | "THRU")),
            ) * 2;
            if clause >= times {
                return Err(MachineProblem::InvalidOperation);
            }
            let count = value_decimal(self.eval_value(&args[clause..times])?)?;
            if count.scale != 0 {
                return Err(MachineProblem::DataException);
            }
            if count.coefficient <= 0 {
                return Ok(Step::Next);
            }
            self.loop_counts.insert(key, count.coefficient - 1);
        }
        self.perform_stack.push(self.pc);
        self.label(target).map(Step::Jump)
    }

    fn exit_perform_step(&self, operation: &Operation) -> Result<Step, MachineProblem> {
        let mut parent = optional_integer_attribute(operation, "control_parent")
            .and_then(|value| usize::try_from(value).ok());
        while let Some(node) = parent {
            let pc = self
                .control_nodes
                .get(&node)
                .copied()
                .ok_or(MachineProblem::InvalidOperation)?;
            let owner = self
                .operations
                .get(pc)
                .ok_or(MachineProblem::InvalidOperation)?;
            if optional_text_attribute(owner, "control_scope") == Some("perform") {
                let end = self
                    .operations
                    .iter()
                    .enumerate()
                    .skip(pc + 1)
                    .find(|(_, candidate)| {
                        optional_text_attribute(candidate, "control_role") == Some("block_end")
                            && optional_text_attribute(candidate, "control_scope")
                                == Some("perform")
                            && optional_integer_attribute(candidate, "control_parent")
                                .and_then(|value| usize::try_from(value).ok())
                                == Some(node)
                    })
                    .map(|(pc, _)| pc + 1)
                    .ok_or(MachineProblem::InvalidOperation)?;
                return Ok(Step::Jump(end));
            }
            parent = optional_integer_attribute(owner, "control_parent")
                .and_then(|value| usize::try_from(value).ok());
        }
        Err(MachineProblem::InvalidOperation)
    }

    fn exit_paragraph_step(&mut self) -> Result<Step, MachineProblem> {
        if let Some(call_pc) = self.perform_stack.last().copied()
            && self
                .perform_endpoint(call_pc)
                .is_some_and(|endpoint| self.pc <= endpoint)
        {
            return self.finish_perform().map(Step::Jump);
        }
        let next = self
            .labels
            .values()
            .copied()
            .filter(|label| *label > self.pc)
            .min()
            .unwrap_or_else(|| self.operations.len().saturating_sub(1));
        Ok(Step::Jump(next))
    }

    fn exit_section_step(&mut self) -> Result<Step, MachineProblem> {
        if let Some(call_pc) = self.perform_stack.last().copied()
            && self
                .perform_endpoint(call_pc)
                .is_some_and(|endpoint| self.pc <= endpoint)
        {
            return self.finish_perform().map(Step::Jump);
        }
        let next = self
            .operations
            .iter()
            .enumerate()
            .skip(self.pc.saturating_add(1))
            .find(|(_, operation)| {
                operation.identity.name() == "label"
                    && arguments(operation)
                        .get(1)
                        .is_some_and(|marker| marker == "SECTION")
            })
            .map(|(pc, _)| pc)
            .unwrap_or_else(|| self.operations.len().saturating_sub(1));
        Ok(Step::Jump(next))
    }

    fn active_declarative(&self) -> bool {
        self.perform_stack
            .last()
            .is_some_and(|call_pc| self.altered.contains_key(&declarative_state_key(*call_pc)))
    }

    fn declarative_section_boundary(&self, handler: &str) -> Option<usize> {
        let start = self.labels.get(&normalize(handler)).copied()?;
        self.operations
            .iter()
            .enumerate()
            .skip(start + 1)
            .find(|(_, operation)| {
                if operation.identity.name() != "label" {
                    return false;
                }
                let arguments = arguments(operation);
                arguments.first().is_some_and(|name| name == "DECLARATIVES")
                    || arguments.get(1).is_some_and(|marker| marker == "SECTION")
            })
            .map(|(pc, _)| pc)
    }

    fn skip_declarative_section(&self, args: &[String]) -> Result<Step, MachineProblem> {
        let handler = args.first().ok_or(MachineProblem::InvalidOperation)?;
        Ok(Step::Jump(
            self.declarative_section_boundary(handler)
                .unwrap_or_else(|| self.operations.len().saturating_sub(1)),
        ))
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
        if let Some(handler) = self.altered.get(&declarative_state_key(call_pc)) {
            return self
                .declarative_section_boundary(handler)
                .map(|boundary| boundary.saturating_sub(1));
        }
        if matches!(call.identity.name(), "sort" | "merge") {
            let active = self
                .active_sort_procedure
                .as_ref()
                .filter(|active| active.sort_pc == call_pc)?;
            let kind = match active.phase {
                SortProcedurePhase::Input => "INPUT",
                SortProcedurePhase::Output => "OUTPUT",
            };
            let target =
                sort_procedure_end(&args, kind).or_else(|| sort_procedure_target(&args, kind))?;
            return self.paragraph_endpoint(&target);
        }
        if call.identity.name() == "xml_parse" {
            let target = position(&args, "THRU")
                .or_else(|| position(&args, "THROUGH"))
                .and_then(|index| args.get(index + 1))
                .map(String::as_str)
                .or_else(|| xml_processing_target(&args))?;
            return self.paragraph_endpoint(target);
        }
        let target = position(&args, "THRU")
            .or_else(|| position(&args, "THROUGH"))
            .and_then(|index| args.get(index + 1))
            .or_else(|| args.first());
        self.paragraph_endpoint(target?)
    }

    fn paragraph_endpoint(&self, target: &str) -> Option<usize> {
        let start = self.labels.get(&normalize(target)).copied()?;
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
        if self
            .altered
            .remove(&declarative_state_key(call_pc))
            .is_some()
        {
            return Ok(call_pc.saturating_add(1));
        }
        if matches!(operation.identity.name(), "sort" | "merge") {
            let active = self
                .active_sort_procedure
                .take()
                .filter(|active| active.sort_pc == call_pc)
                .ok_or(MachineProblem::InvalidOperation)?;
            return match active.phase {
                SortProcedurePhase::Input => {
                    self.order_sort_workspace(&active.sort_file, &active.arguments)?;
                    let outputs = sort_file_list(&active.arguments, "GIVING");
                    if !outputs.is_empty() {
                        self.sort_io = Some(SortIoState {
                            sort_pc: call_pc,
                            sort_file: active.sort_file,
                            arguments: active.arguments,
                            inputs: Vec::new(),
                            outputs,
                            next_input: 0,
                            next_output: 0,
                        });
                        let step = self.issue_sort_write()?;
                        self.defer_sort_step(step)?;
                        Ok(call_pc.saturating_add(1))
                    } else if let Some(target) = sort_procedure_target(&active.arguments, "OUTPUT")
                    {
                        self.active_sort_procedure = Some(ActiveSortProcedure {
                            sort_pc: call_pc,
                            sort_file: active.sort_file,
                            phase: SortProcedurePhase::Output,
                            arguments: active.arguments,
                        });
                        self.perform_stack.push(call_pc);
                        self.label(&target)
                    } else {
                        Ok(call_pc.saturating_add(1))
                    }
                }
                SortProcedurePhase::Output => Ok(call_pc.saturating_add(1)),
            };
        }
        if operation.identity.name() == "xml_parse" {
            let arguments = arguments(&operation);
            let source = String::from_utf8(
                self.resolve(arguments.first().ok_or(MachineProblem::InvalidOperation)?)?,
            )
            .map_err(|_| MachineProblem::DataException)?;
            let events = xml_document_events(source.trim())?;
            let key = xml_state_key(call_pc);
            let next = self
                .loop_counts
                .get(&key)
                .copied()
                .and_then(|next| usize::try_from(next).ok())
                .ok_or(MachineProblem::InvalidOperation)?;
            if let Some(event) = events.get(next) {
                self.install_xml_event(event);
                self.loop_counts.insert(
                    key,
                    i128::try_from(next.saturating_add(1))
                        .map_err(|_| MachineProblem::ResourceExhausted)?,
                );
                self.perform_stack.push(call_pc);
                return self.label(
                    xml_processing_target(&arguments).ok_or(MachineProblem::InvalidOperation)?,
                );
            }
            self.loop_counts.remove(&key);
        }
        if operation.identity.name() == "perform" {
            let arguments = arguments(&operation);
            let key = out_of_line_perform_key(call_pc);
            if let Some(remaining) = self.loop_counts.get(&key).copied() {
                let repeat = if remaining > 0 && position(&arguments, "TIMES").is_some() {
                    self.loop_counts.insert(key, remaining - 1);
                    true
                } else if let Some(varying) = position(&arguments, "VARYING") {
                    let variable = arguments
                        .get(varying + 1)
                        .ok_or(MachineProblem::InvalidOperation)?;
                    let by = position(&arguments, "BY")
                        .and_then(|at| arguments.get(at + 1))
                        .ok_or(MachineProblem::InvalidOperation)?;
                    let value = decimal_add(
                        self.arithmetic_mode,
                        self.decimal(variable)?,
                        self.decimal(by)?,
                    )?;
                    self.write_decimal(variable, value)?;
                    let until =
                        position(&arguments, "UNTIL").ok_or(MachineProblem::InvalidOperation)?;
                    !self.eval_condition(&arguments[until + 1..])?
                } else if let Some(until) = position(&arguments, "UNTIL") {
                    !self.eval_condition(&arguments[until + 1..])?
                } else {
                    false
                };
                if repeat {
                    self.perform_stack.push(call_pc);
                    return self.label(arguments.first().ok_or(MachineProblem::InvalidOperation)?);
                }
                self.loop_counts.remove(&key);
            }
        }
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
        let handles_exception = self.has_condition_handler(operation, "ON EXCEPTION");
        self.condition_status.accept_exception = false;
        if let Some(from) = position(args, "FROM") {
            let source = args.get(from + 1).ok_or(MachineProblem::InvalidOperation)?;
            let clock = match source.as_str() {
                "DATE" if args.get(from + 2).is_some_and(|token| token == "YYYYMMDD") => {
                    Some((ClockRequest::Date, AcceptClockFormat::DateYyyymmdd))
                }
                "DATE" => Some((ClockRequest::Date, AcceptClockFormat::DateYymmdd)),
                "DAY" if args.get(from + 2).is_some_and(|token| token == "YYYYDDD") => {
                    Some((ClockRequest::Date, AcceptClockFormat::DayYyyyddd))
                }
                "DAY" => Some((ClockRequest::Date, AcceptClockFormat::DayYyddd)),
                "DAY-OF-WEEK" => Some((ClockRequest::Date, AcceptClockFormat::DayOfWeek)),
                "TIME" => Some((ClockRequest::Time, AcceptClockFormat::Time)),
                _ => None,
            };
            if let Some((request, format)) = clock {
                return self.effect(
                    HostRequest::Clock(request),
                    PendingKind::AcceptClock {
                        target,
                        format,
                        handles_exception,
                    },
                );
            }
            let binding = match source.as_str() {
                "COMMAND-LINE" => Some("cobol.command-line".to_string()),
                "ENVIRONMENT" => args.get(from + 2).map(|name| {
                    format!(
                        "cobol.environment.{}",
                        normalize(name.trim_matches(['\'', '"']))
                    )
                }),
                "ARGUMENT-NUMBER" => Some("cobol.argument-number".to_string()),
                "ARGUMENT-VALUE" => Some("cobol.argument-value".to_string()),
                _ => None,
            };
            if let Some(binding) = binding {
                if let Some(value) = self.invocation.bindings.get(&binding) {
                    let bytes = value.bytes().to_vec();
                    self.write(&target, &bytes)?;
                    return Ok(Step::Next);
                }
                if handles_exception {
                    self.condition_status.accept_exception = true;
                    return Ok(Step::Next);
                }
                return Err(MachineProblem::DataException);
            }
        }
        let request = HostRequest::Terminal(TerminalRequest::Read {
            session: mainframe_env_host_api::SessionId::new(
                self.invocation.run_unit_id.as_str(),
                128,
            )
            .map_err(|_| MachineProblem::InvalidOperation)?,
        });
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
        if name == "cancel" {
            let programs = args
                .iter()
                .filter(|argument| !matches!(argument.as_str(), "," | "END-CANCEL"))
                .map(|argument| {
                    let value = if argument.starts_with(['\'', '"']) {
                        argument.trim_matches(['\'', '"']).to_string()
                    } else {
                        String::from_utf8(self.read(argument)?)
                            .map_err(|_| MachineProblem::DataException)?
                            .trim()
                            .to_string()
                    };
                    ProgramName::new(value, HostLimits::default().max_name_bytes)
                        .map_err(|_| MachineProblem::DataException)
                })
                .collect::<Result<Vec<_>, _>>()?;
            return self.effect(
                HostRequest::Program(ProgramRequest::Cancel { programs }),
                PendingKind::Ignore,
            );
        }
        let targets = position(args, "USING").map_or_else(Vec::new, |using| {
            args[using + 1..]
                .iter()
                .take_while(|argument| {
                    !matches!(
                        argument.as_str(),
                        "RETURNING" | "ON" | "NOT" | "END-CALL" | "END-INVOKE"
                    )
                })
                .filter(|argument| {
                    !matches!(argument.as_str(), "BY" | "REFERENCE" | "CONTENT" | "VALUE")
                })
                .cloned()
                .collect()
        });
        let returning = position(args, "RETURNING").and_then(|at| args.get(at + 1).cloned());
        let values = targets
            .iter()
            .map(|target| self.read(target))
            .collect::<Result<Vec<_>, _>>()?;
        let payload = encode_call_values(&targets, &values)?;
        let request = if name == "invoke" {
            let receiver_name = args.first().ok_or(MachineProblem::InvalidOperation)?;
            let receiver_layout = self
                .layout(receiver_name)
                .filter(|layout| layout.category == LayoutCategory::ObjectReference)
                .ok_or(MachineProblem::DataException)?;
            let class = ClassName::new(
                receiver_layout
                    .object_class
                    .as_deref()
                    .ok_or(MachineProblem::InvalidOperation)?,
                HostLimits::default().max_name_bytes,
            )
            .map_err(|_| MachineProblem::InvalidOperation)?;
            let method = MethodName::new(
                args.get(1)
                    .ok_or(MachineProblem::InvalidOperation)?
                    .trim_matches(['\'', '"']),
                HostLimits::default().max_name_bytes,
            )
            .map_err(|_| MachineProblem::InvalidOperation)?;
            let receiver_bytes = self.read(receiver_name)?;
            if receiver_bytes.iter().all(|byte| *byte == 0) {
                if self.has_condition_handler(operation, "ON EXCEPTION") {
                    self.condition_status.call_exception = true;
                    return Ok(Step::Next);
                }
                return Err(MachineProblem::DataException);
            }
            let receiver = BoundedPayload::new(
                "mainframe-env.cobol-object-reference@1",
                receiver_bytes,
                InvocationLimits::default(),
            )
            .map_err(|_| MachineProblem::ResourceExhausted)?;
            HostRequest::Program(ProgramRequest::Invoke {
                class,
                method,
                receiver,
                payload,
            })
        } else {
            let program = ProgramName::new(
                args.first()
                    .ok_or(MachineProblem::InvalidOperation)?
                    .trim_matches(['\'', '"']),
                HostLimits::default().max_name_bytes,
            )
            .map_err(|_| MachineProblem::InvalidOperation)?;
            let service = self.runtime_service_selector(program.as_str())?;
            HostRequest::Program(ProgramRequest::Call {
                program,
                payload,
                service,
            })
        };
        let handles_exception = self.has_condition_handler(operation, "ON EXCEPTION");
        self.condition_status.call_exception = false;
        self.effect(
            request,
            PendingKind::ProgramCall {
                targets,
                returning,
                handles_exception,
            },
        )
    }

    fn runtime_service_selector(
        &self,
        program: &str,
    ) -> Result<Option<RuntimeServiceSelector>, MachineProblem> {
        let Some(binding) = self
            .invocation
            .bindings
            .get(&format!("cobol.runtime-service.{}", normalize(program)))
        else {
            return Ok(None);
        };
        if binding.schema() != "mainframe-env.runtime-service-selector@1" {
            return Err(MachineProblem::InvalidOperation);
        }
        let text =
            std::str::from_utf8(binding.bytes()).map_err(|_| MachineProblem::InvalidOperation)?;
        let mut fields = text.split(':');
        let kind = match fields.next() {
            Some("le") => RuntimeServiceKind::LanguageEnvironment,
            Some("extension") => RuntimeServiceKind::HostExtension,
            _ => return Err(MachineProblem::InvalidOperation),
        };
        let name = RuntimeServiceName::new(
            fields.next().ok_or(MachineProblem::InvalidOperation)?,
            HostLimits::default().max_name_bytes,
        )
        .map_err(|_| MachineProblem::InvalidOperation)?;
        let abi_version = fields
            .next()
            .and_then(|value| value.parse::<u16>().ok())
            .filter(|value| *value > 0)
            .ok_or(MachineProblem::InvalidOperation)?;
        if fields.next().is_some() {
            return Err(MachineProblem::InvalidOperation);
        }
        Ok(Some(RuntimeServiceSelector {
            kind,
            name,
            abi_version,
        }))
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

    fn sort_effect(&mut self, name: &str, args: &[String]) -> Result<Step, MachineProblem> {
        let sort_file = normalize(args.first().ok_or(MachineProblem::InvalidOperation)?);
        if !self
            .files
            .get(&sort_file)
            .is_some_and(|file| file.sort_merge)
        {
            return Err(MachineProblem::InvalidOperation);
        }
        self.sort_workspaces.insert(
            sort_file.clone(),
            SortWorkspace {
                records: Vec::new(),
                cursor: 0,
            },
        );
        let inputs = sort_file_list(args, "USING");
        let outputs = sort_file_list(args, "GIVING");
        if name == "merge" && inputs.is_empty() {
            return Err(MachineProblem::InvalidOperation);
        }
        if !inputs.is_empty() {
            self.sort_io = Some(SortIoState {
                sort_pc: self.pc,
                sort_file,
                arguments: args.to_vec(),
                inputs,
                outputs,
                next_input: 0,
                next_output: 0,
            });
            return self.issue_sort_read();
        }
        if let Some(target) = sort_procedure_target(args, "INPUT") {
            self.active_sort_procedure = Some(ActiveSortProcedure {
                sort_pc: self.pc,
                sort_file,
                phase: SortProcedurePhase::Input,
                arguments: args.to_vec(),
            });
            self.perform_stack.push(self.pc);
            return Ok(Step::Jump(self.label(&target)?));
        }
        self.order_sort_workspace(&sort_file, args)?;
        if !outputs.is_empty() {
            self.sort_io = Some(SortIoState {
                sort_pc: self.pc,
                sort_file,
                arguments: args.to_vec(),
                inputs,
                outputs,
                next_input: 0,
                next_output: 0,
            });
            return self.issue_sort_write();
        }
        if let Some(target) = sort_procedure_target(args, "OUTPUT") {
            self.active_sort_procedure = Some(ActiveSortProcedure {
                sort_pc: self.pc,
                sort_file,
                phase: SortProcedurePhase::Output,
                arguments: args.to_vec(),
            });
            self.perform_stack.push(self.pc);
            return Ok(Step::Jump(self.label(&target)?));
        }
        Ok(Step::Next)
    }

    fn release_sort_record(&mut self, args: &[String]) -> Result<(), MachineProblem> {
        let record_name = normalize(args.first().ok_or(MachineProblem::InvalidOperation)?);
        let sort_file = self
            .files
            .iter()
            .find(|(_, file)| {
                file.sort_merge && file.record_name.as_deref() == Some(record_name.as_str())
            })
            .map(|(name, _)| name.clone())
            .or_else(|| {
                (self.sort_workspaces.len() == 1)
                    .then(|| self.sort_workspaces.keys().next().cloned())
                    .flatten()
            })
            .ok_or(MachineProblem::InvalidOperation)?;
        let source = position(args, "FROM")
            .and_then(|from| args.get(from + 1))
            .map_or(record_name.as_str(), String::as_str);
        let value = self.resolve(source)?;
        let record_length = self
            .files
            .get(&sort_file)
            .and_then(|file| file.record_name.as_deref())
            .and_then(|record| self.layout(record))
            .map(|layout| layout.length)
            .ok_or(MachineProblem::InvalidOperation)?;
        let value = FixedValue::fit(&value, record_length, false)
            .bytes()
            .to_vec();
        if source != record_name {
            self.write(&record_name, &value)?;
        }
        self.reserve_sort_record(&sort_file, value)
    }

    fn return_sort_record(&mut self, args: &[String]) -> Result<(), MachineProblem> {
        let sort_file = normalize(args.first().ok_or(MachineProblem::InvalidOperation)?);
        let target = position(args, "INTO")
            .and_then(|into| args.get(into + 1))
            .cloned()
            .or_else(|| {
                self.files
                    .get(&sort_file)
                    .and_then(|file| file.record_name.clone())
            })
            .ok_or(MachineProblem::InvalidOperation)?;
        let record = {
            let workspace = self
                .sort_workspaces
                .get_mut(&sort_file)
                .ok_or(MachineProblem::InvalidOperation)?;
            let record = workspace.records.get(workspace.cursor).cloned();
            if record.is_some() {
                workspace.cursor = workspace
                    .cursor
                    .checked_add(1)
                    .ok_or(MachineProblem::ResourceExhausted)?;
            }
            record
        };
        if let Some(record) = record {
            self.last_file_status = "00".into();
            self.write(&target, &record)?;
        } else {
            self.last_file_status = "10".into();
        }
        Ok(())
    }

    fn reserve_sort_record(
        &mut self,
        sort_file: &str,
        record: Vec<u8>,
    ) -> Result<(), MachineProblem> {
        let workspace = self
            .sort_workspaces
            .get_mut(sort_file)
            .ok_or(MachineProblem::InvalidOperation)?;
        if workspace.records.len() as u64 >= self.invocation.limits.max_events
            || workspace
                .records
                .iter()
                .map(Vec::len)
                .sum::<usize>()
                .checked_add(record.len())
                .is_none_or(|bytes| bytes as u64 > self.invocation.limits.max_storage_bytes)
        {
            return Err(MachineProblem::ResourceExhausted);
        }
        workspace.records.push(record);
        Ok(())
    }

    fn order_sort_workspace(
        &mut self,
        sort_file: &str,
        args: &[String],
    ) -> Result<(), MachineProblem> {
        let record = self
            .files
            .get(sort_file)
            .and_then(|file| file.record_name.as_deref())
            .and_then(|name| self.layout(name))
            .cloned()
            .ok_or(MachineProblem::InvalidOperation)?;
        let mut descending = false;
        let mut keys = Vec::new();
        for token in args
            .iter()
            .skip(1)
            .take_while(|token| !matches!(token.as_str(), "USING" | "GIVING" | "INPUT" | "OUTPUT"))
        {
            match token.as_str() {
                "ASCENDING" => descending = false,
                "DESCENDING" => descending = true,
                "ON" | "KEY" | "IS" => {}
                _ => {
                    if let Some(layout) = self.layout(token)
                        && layout.offset >= record.offset
                        && layout.offset.saturating_add(layout.length)
                            <= record.offset.saturating_add(record.length)
                    {
                        keys.push((layout.offset - record.offset, layout.length, descending));
                    }
                }
            }
        }
        let workspace = self
            .sort_workspaces
            .get_mut(sort_file)
            .ok_or(MachineProblem::InvalidOperation)?;
        workspace.records.sort_by(|left, right| {
            if keys.is_empty() {
                return left.cmp(right);
            }
            for (offset, length, descending) in &keys {
                let left_key = left.get(*offset..offset.saturating_add(*length));
                let right_key = right.get(*offset..offset.saturating_add(*length));
                let ordering = left_key
                    .map(cobol_collation_key)
                    .cmp(&right_key.map(cobol_collation_key));
                if !ordering.is_eq() {
                    return if *descending {
                        ordering.reverse()
                    } else {
                        ordering
                    };
                }
            }
            std::cmp::Ordering::Equal
        });
        workspace.cursor = 0;
        Ok(())
    }

    fn issue_sort_read(&mut self) -> Result<Step, MachineProblem> {
        let logical = {
            let state = self
                .sort_io
                .as_mut()
                .ok_or(MachineProblem::InvalidOperation)?;
            let logical = state
                .inputs
                .get(state.next_input)
                .cloned()
                .ok_or(MachineProblem::InvalidOperation)?;
            state.next_input += 1;
            logical
        };
        let (dataset, _) = self.dataset_endpoint(&logical)?;
        let maximum = self
            .invocation
            .limits
            .max_events
            .min(HostLimits::default().max_records as u64);
        self.effect(
            HostRequest::Dataset(DatasetRequest::Read {
                dataset,
                member: None,
                key: None,
                max_records: u32::try_from(maximum)
                    .map_err(|_| MachineProblem::ResourceExhausted)?,
                control: Default::default(),
            }),
            PendingKind::SortRead,
        )
    }

    fn issue_sort_write(&mut self) -> Result<Step, MachineProblem> {
        let (logical, sort_file) = {
            let state = self
                .sort_io
                .as_mut()
                .ok_or(MachineProblem::InvalidOperation)?;
            let logical = state
                .outputs
                .get(state.next_output)
                .cloned()
                .ok_or(MachineProblem::InvalidOperation)?;
            state.next_output += 1;
            (logical, state.sort_file.clone())
        };
        let (dataset, ccsid) = self.dataset_endpoint(&logical)?;
        let records = self
            .sort_workspaces
            .get(&sort_file)
            .ok_or(MachineProblem::InvalidOperation)?
            .records
            .iter()
            .map(|record| encode_dataset_record(ccsid, record))
            .collect::<Result<Vec<_>, _>>()?;
        self.effect(
            HostRequest::Dataset(DatasetRequest::Write {
                dataset,
                member: None,
                records,
                expected_version: None,
                mutation: self.mutation()?,
            }),
            PendingKind::SortWrite,
        )
    }

    fn dataset_endpoint(
        &self,
        logical: &str,
    ) -> Result<(DatasetName, Option<u16>), MachineProblem> {
        let logical_name = normalize(logical);
        let file = self.files.get(&logical_name);
        let dataset_name = self
            .invocation
            .bindings
            .get(&format!("cobol.dd.{logical_name}"))
            .or_else(|| {
                file.and_then(|file| {
                    self.invocation
                        .bindings
                        .get(&format!("cobol.dd.{}", normalize(&file.assignment)))
                })
            })
            .map(|payload| String::from_utf8_lossy(payload.bytes()).into_owned())
            .unwrap_or_else(|| {
                file.map(|file| file.assignment.clone())
                    .unwrap_or(logical_name)
            });
        let dataset =
            DatasetName::new(dataset_name, 128).map_err(|_| MachineProblem::InvalidOperation)?;
        let ccsid = self
            .invocation
            .bindings
            .get(&format!("cobol.dd.{}.ccsid", normalize(logical)))
            .or_else(|| {
                file.and_then(|file| {
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
            .transpose()?
            .or_else(|| file.as_ref().and_then(|file| file.ccsid));
        Ok((dataset, ccsid))
    }

    fn continue_sort_after_read(
        &mut self,
        records: Vec<Vec<u8>>,
    ) -> Result<Option<Step>, MachineProblem> {
        let (sort_file, input, more_inputs) = {
            let state = self
                .sort_io
                .as_ref()
                .ok_or(MachineProblem::InvalidOperation)?;
            (
                state.sort_file.clone(),
                state
                    .inputs
                    .get(state.next_input.saturating_sub(1))
                    .cloned()
                    .ok_or(MachineProblem::InvalidOperation)?,
                state.next_input < state.inputs.len(),
            )
        };
        let (_, ccsid) = self.dataset_endpoint(&input)?;
        for record in records {
            self.reserve_sort_record(&sort_file, decode_dataset_record(ccsid, &record)?)?;
        }
        if more_inputs {
            return self.issue_sort_read().map(Some);
        }
        let arguments = self
            .sort_io
            .as_ref()
            .ok_or(MachineProblem::InvalidOperation)?
            .arguments
            .clone();
        self.order_sort_workspace(&sort_file, &arguments)?;
        self.continue_sort_output()
    }

    fn continue_sort_output(&mut self) -> Result<Option<Step>, MachineProblem> {
        let (sort_pc, sort_file, arguments, more_outputs) = {
            let state = self
                .sort_io
                .as_ref()
                .ok_or(MachineProblem::InvalidOperation)?;
            (
                state.sort_pc,
                state.sort_file.clone(),
                state.arguments.clone(),
                state.next_output < state.outputs.len(),
            )
        };
        if more_outputs {
            return self.issue_sort_write().map(Some);
        }
        self.sort_io = None;
        if let Some(target) = sort_procedure_target(&arguments, "OUTPUT") {
            self.active_sort_procedure = Some(ActiveSortProcedure {
                sort_pc,
                sort_file,
                phase: SortProcedurePhase::Output,
                arguments,
            });
            self.perform_stack.push(sort_pc);
            return self.label(&target).map(Step::Jump).map(Some);
        }
        Ok(None)
    }

    fn defer_sort_step(&mut self, step: Step) -> Result<(), MachineProblem> {
        match step {
            Step::Next => {}
            Step::Jump(target) => self.pc = target,
            Step::Effect(effect) => self.deferred_drive = Some(MachineDrive::HostCall(*effect)),
            Step::Complete => {
                self.deferred_drive = Some(MachineDrive::Completed(self.complete()?));
            }
        }
        Ok(())
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
                .find(|file| {
                    file.record_name.as_deref() == Some(logical_name.as_str())
                        || file.record_names.iter().any(|name| name == &logical_name)
                })
                .cloned()
        });
        let start_key = (name == "start")
            .then(|| start_relation_and_key(args))
            .flatten();
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
            .flatten()
            .or_else(|| start_key.as_ref().map(|(_, key)| key.clone()));
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
            .transpose()?
            .or_else(|| file.as_ref().and_then(|file| file.ccsid));
        let default_key = file
            .as_ref()
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
        let read_control = DatasetReadControl {
            lock: if args.windows(2).any(|window| window == ["WITH", "KEPT"]) {
                DatasetReadLockMode::KeptLock
            } else if args.windows(2).any(|window| window == ["WITH", "NO"]) {
                DatasetReadLockMode::NoLock
            } else if args.iter().any(|token| token == "IGNORE") {
                DatasetReadLockMode::IgnoreLock
            } else if args.windows(2).any(|window| window == ["WITH", "LOCK"]) {
                DatasetReadLockMode::Lock
            } else {
                DatasetReadLockMode::Default
            },
            wait: if args.windows(2).any(|window| window == ["NO", "WAIT"]) {
                Some(false)
            } else if args.iter().any(|token| token == "WAIT") {
                Some(true)
            } else {
                None
            },
        };
        let (request, cursor_action) = match name {
            "read"
                if current_cursor.is_some()
                    && (sequential
                        || args
                            .iter()
                            .any(|argument| matches!(argument.as_str(), "NEXT" | "PREVIOUS"))) =>
            {
                (
                    DatasetRequest::ReadNext {
                        dataset,
                        cursor: current_cursor.ok_or(MachineProblem::InvalidOperation)?,
                        reverse: args.iter().any(|argument| argument == "PREVIOUS"),
                        control: read_control,
                    },
                    None,
                )
            }
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
                    control: read_control,
                },
                None,
            ),
            "write" => {
                let record_name = args.first().ok_or(MachineProblem::InvalidOperation)?;
                let from = position(args, "FROM").and_then(|index| args.get(index + 1));
                let record = self.dataset_output_record(file.as_ref(), record_name, from)?;
                validate_record_length(file.as_ref(), record.len())?;
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
                let record_name = args.first().ok_or(MachineProblem::InvalidOperation)?;
                let from = position(args, "FROM").and_then(|index| args.get(index + 1));
                let record = self.dataset_output_record(file.as_ref(), record_name, from)?;
                validate_record_length(file.as_ref(), record.len())?;
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
            "delete"
                if file
                    .as_ref()
                    .is_some_and(|file| file.organization == "RELATIVE") =>
            {
                let relative_key = file
                    .as_ref()
                    .and_then(|file| file.relative_key.as_ref())
                    .ok_or(MachineProblem::InvalidOperation)?;
                let number = self.decimal(relative_key)?;
                if number.scale != 0 || number.coefficient <= 0 {
                    return Err(MachineProblem::DataException);
                }
                (
                    DatasetRequest::DeleteRelative {
                        dataset,
                        record_number: u64::try_from(number.coefficient)
                            .map_err(|_| MachineProblem::DataException)?,
                        expected_version: None,
                        mutation: self.mutation()?,
                    },
                    None,
                )
            }
            "delete" => (
                DatasetRequest::DeleteRecord {
                    dataset,
                    key: default_key.ok_or(MachineProblem::InvalidOperation)?,
                    expected_version: None,
                    mutation: self.mutation()?,
                },
                None,
            ),
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
                    relation: KeyRelation::GreaterOrEqual,
                },
                Some(DatasetCursorAction::Start(dataset_name.clone())),
            ),
            "start" => {
                let (relation, _) = start_key
                    .clone()
                    .unwrap_or((KeyRelation::GreaterOrEqual, String::new()));
                (
                    DatasetRequest::StartBrowse {
                        dataset,
                        key: explicit_key
                            .as_ref()
                            .map(|key| self.resolve(key))
                            .transpose()?
                            .map(|key| encode_dataset_record(ccsid, &key))
                            .transpose()?
                            .unwrap_or_default(),
                        relation,
                    },
                    Some(DatasetCursorAction::Start(dataset_name.clone())),
                )
            }
            "close" => (
                DatasetRequest::Close {
                    dataset,
                    cursor: current_cursor.clone(),
                    control: DatasetCloseControl {
                        reel_or_unit: if args.iter().any(|token| token == "REEL") {
                            Some(DatasetReelUnit::Reel)
                        } else if args.iter().any(|token| token == "UNIT") {
                            Some(DatasetReelUnit::Unit)
                        } else {
                            None
                        },
                        no_rewind: args.windows(2).any(|window| window == ["NO", "REWIND"]),
                        removal: args.windows(2).any(|window| window == ["FOR", "REMOVAL"]),
                        lock: args.windows(2).any(|window| window == ["WITH", "LOCK"]),
                    },
                },
                current_cursor
                    .is_some()
                    .then(|| DatasetCursorAction::End(dataset_name.clone())),
            ),
            _ => (DatasetRequest::Attributes { dataset }, None),
        };
        let target = (name == "read")
            .then(|| file.as_ref().and_then(|file| file.record_name.clone()))
            .flatten();
        let into = (name == "read")
            .then(|| position(args, "INTO").and_then(|index| args.get(index + 1).cloned()))
            .flatten();
        let declarative = self
            .declaratives
            .get(&logical_name)
            .or_else(|| {
                let mode = match name {
                    "read" | "start" => "INPUT",
                    "write" => "OUTPUT",
                    "rewrite" | "delete" => "I-O",
                    "open" => open_mode.unwrap_or("INPUT"),
                    "close" => "INPUT",
                    _ => "INPUT",
                };
                self.declaratives.get(mode)
            })
            .cloned();
        let pending = if name == "read" {
            PendingKind::DatasetRead {
                target,
                into,
                depending_on: file
                    .as_ref()
                    .and_then(|file| file_record_depending(&file.description)),
                status,
                ccsid,
                declarative,
            }
        } else {
            let (linage_advance, page_advance) =
                if name == "write" && file.as_ref().is_some_and(|file| file.linage.is_some()) {
                    write_linage_advance(self, args)?
                } else {
                    (0, false)
                };
            PendingKind::DatasetStatus {
                status,
                cursor: cursor_action,
                linage_advance,
                linage_limit: file.as_ref().and_then(|file| file.linage),
                page_advance,
                declarative,
            }
        };
        self.effect(HostRequest::Dataset(request), pending)
    }

    fn dataset_output_record(
        &mut self,
        file: Option<&FileMetadata>,
        record_name: &str,
        from: Option<&String>,
    ) -> Result<Vec<u8>, MachineProblem> {
        let mut record = if let Some(from) = from {
            let source = self.resolve(from)?;
            if file.is_some() {
                self.write(record_name, &source)?;
                self.resolve(record_name)?
            } else {
                source
            }
        } else {
            self.resolve(record_name)?
        };
        if let Some(file) = file
            && let Some(depending_on) = file_record_depending(&file.description)
        {
            let value = self.decimal(&depending_on)?;
            if value.scale != 0 || value.coefficient < 0 {
                return Err(MachineProblem::SizeError);
            }
            let length =
                usize::try_from(value.coefficient).map_err(|_| MachineProblem::SizeError)?;
            validate_record_length(Some(file), length)?;
            if length > record.len() {
                return Err(MachineProblem::SizeError);
            }
            record.truncate(length);
        }
        Ok(record)
    }

    fn finish_dataset_read(
        &mut self,
        target: &str,
        into: Option<&str>,
        depending_on: Option<&str>,
        record: &[u8],
    ) -> Result<(), MachineProblem> {
        self.write(target, record)?;
        if let Some(depending_on) = depending_on {
            self.write_decimal(
                depending_on,
                Decimal {
                    coefficient: record.len() as i128,
                    scale: 0,
                },
            )?;
        }
        if let Some(into) = into {
            self.write(into, record)?;
        }
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
        if args
            .first()
            .is_some_and(|token| matches!(token.as_str(), "CORRESPONDING" | "CORR"))
        {
            return self.move_corresponding(args, to);
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
        let source_category = self
            .reference(&args[..to])
            .ok()
            .map(|reference| reference.layout.category);
        let alphanumeric_sender = matches!(source_category, Some(LayoutCategory::Alphanumeric))
            || (to == 1
                && args[0].len() >= 2
                && matches!(args[0].as_bytes().first(), Some(b'\'' | b'"'))
                && args[0].as_bytes().first() == args[0].as_bytes().last());
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
                .find(|end| self.reference(&targets[at..*end]).is_ok());
            let Some(end) = end else {
                let problem = self
                    .reference(&targets[at..control])
                    .err()
                    .unwrap_or(MachineProblem::UnknownStorage);
                return Err(
                    if problem == MachineProblem::UnknownStorage
                        && targets[at..control].iter().any(|token| token == "(")
                    {
                        MachineProblem::SubscriptError
                    } else {
                        problem
                    },
                );
            };
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
            let value = match (source_category, target.layout.category, value) {
                (
                    Some(LayoutCategory::National | LayoutCategory::NationalEdited),
                    target,
                    CobolValue::Bytes(bytes),
                ) if !matches!(
                    target,
                    LayoutCategory::National
                        | LayoutCategory::NationalEdited
                        | LayoutCategory::NationalGroup
                ) =>
                {
                    CobolValue::Bytes(national_to_utf8(&bytes)?)
                }
                (
                    source,
                    LayoutCategory::National | LayoutCategory::NationalEdited,
                    CobolValue::Bytes(bytes),
                ) if !matches!(
                    source,
                    Some(
                        LayoutCategory::National
                            | LayoutCategory::NationalEdited
                            | LayoutCategory::NationalGroup
                    )
                ) =>
                {
                    CobolValue::Bytes(utf8_to_national(&bytes)?)
                }
                (_, _, value) => value,
            };
            // Digit-only elementary alphanumeric senders have an implicit integer point.
            if alphanumeric_sender
                && target.layout.category == LayoutCategory::NumericDisplay
                && target.length == target.layout.length
                && let CobolValue::Bytes(bytes) = &value
                && !bytes.is_empty()
                && bytes.iter().all(u8::is_ascii_digit)
            {
                let integer_places = target
                    .layout
                    .digits
                    .saturating_sub(target.layout.scale as usize);
                let digits = &bytes[bytes.len().saturating_sub(integer_places)..];
                let significant = digits
                    .iter()
                    .position(|digit| *digit != b'0')
                    .unwrap_or(digits.len());
                let coefficient = std::str::from_utf8(&digits[significant..])
                    .ok()
                    .and_then(|digits| {
                        if digits.is_empty() {
                            Some(0)
                        } else {
                            digits.parse::<i128>().ok()
                        }
                    })
                    .ok_or(MachineProblem::SizeError)?;
                self.write_reference_value(
                    &targets[at..end],
                    &CobolValue::Decimal(Decimal {
                        coefficient,
                        scale: 0,
                    }),
                )?;
            } else {
                self.write_reference_value(&targets[at..end], &value)?;
            }
            at = end;
        }
        Ok(())
    }

    fn move_corresponding(&mut self, args: &[String], to: usize) -> Result<(), MachineProblem> {
        if to <= 1 {
            return Err(MachineProblem::InvalidOperation);
        }
        let source = self.reference(&args[1..to])?;
        if !is_group(source.layout.category) {
            return Err(MachineProblem::DataException);
        }
        let sources = self.group_elementary_descendants(&source.layout.name);
        let targets = self.arithmetic_items(&args[to + 1..], true)?;
        if targets.is_empty() {
            return Err(MachineProblem::InvalidOperation);
        }
        let mut staged = Vec::new();
        for (target_tokens, _) in targets {
            let target = self.reference(&target_tokens)?;
            if !is_group(target.layout.category) {
                return Err(MachineProblem::DataException);
            }
            for target in self.group_elementary_descendants(&target.layout.name) {
                let matching = sources
                    .iter()
                    .filter(|source| {
                        source.simple_name == target.simple_name
                            && is_numeric(source.category) == is_numeric(target.category)
                    })
                    .collect::<Vec<_>>();
                if matching.len() != 1 {
                    continue;
                }
                let source = self.reference(std::slice::from_ref(&matching[0].name))?;
                let target = self.reference(std::slice::from_ref(&target.name))?;
                let bytes = if is_numeric(source.layout.category) {
                    let value = decode_decimal(&source.layout, &self.read_reference(&source)?)?;
                    encode_decimal(&target.layout, decimal_rescale(value, target.layout.scale)?)?
                } else {
                    FixedValue::fit(
                        &self.read_reference(&source)?,
                        target.length,
                        target.layout.justified_right,
                    )
                    .bytes()
                    .to_vec()
                };
                if bytes.len() != target.length {
                    return Err(MachineProblem::SizeError);
                }
                staged.push((target, bytes));
            }
        }
        for (target, bytes) in staged {
            self.write_reference(&target, &bytes)?;
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
            let target = self.reference(&args[2..3])?;
            if !target.layout.linkage {
                return Err(MachineProblem::DataException);
            }
            let address = if args[4] == "NULL" {
                None
            } else {
                let pointer = self.reference(&args[4..5])?;
                if !is_pointer_like(pointer.layout.category) {
                    return Err(MachineProblem::DataException);
                }
                self.decode_address(&self.read_reference(&pointer)?)?
            };
            return self.assign_linkage_address(&target.layout.name, address);
        }
        if let Some(direction) = args
            .iter()
            .position(|token| matches!(token.as_str(), "UP" | "DOWN"))
        {
            if direction == 0 || args.get(direction + 1).is_none_or(|token| token != "BY") {
                return Err(MachineProblem::InvalidOperation);
            }
            let amount = value_decimal(self.eval_value(&args[direction + 2..])?)?;
            let mut layout_assignments = Vec::new();
            let mut implicit_assignments = Vec::new();
            for target in args[..direction]
                .iter()
                .filter(|token| token.as_str() != ",")
            {
                let current = value_decimal(self.eval_value(std::slice::from_ref(target))?)?;
                let value = if args[direction] == "UP" {
                    decimal_add(self.arithmetic_mode, current, amount)?
                } else {
                    decimal_subtract(self.arithmetic_mode, current, amount)?
                };
                if self.layout(target).is_some() {
                    layout_assignments.push((vec![target.clone()], value, false));
                } else if matches!(
                    self.implicit.get(&normalize(target)),
                    Some(CobolValue::Decimal(_))
                ) {
                    implicit_assignments.push((normalize(target), value));
                } else {
                    return Err(MachineProblem::DataException);
                }
            }
            if layout_assignments.is_empty() && implicit_assignments.is_empty() {
                return Err(MachineProblem::InvalidOperation);
            }
            self.commit_decimal_assignments(layout_assignments)?;
            for (target, value) in implicit_assignments {
                self.implicit.insert(target, CobolValue::Decimal(value));
            }
            return Ok(());
        }
        let to = position(args, "TO").ok_or(MachineProblem::InvalidOperation)?;
        if to == 0 || to + 1 >= args.len() {
            return Err(MachineProblem::InvalidOperation);
        }
        let targets = self
            .arithmetic_items(&args[..to], true)?
            .into_iter()
            .map(|(target, _)| target)
            .collect::<Vec<_>>();
        if targets.is_empty() {
            return Err(MachineProblem::InvalidOperation);
        }
        if args
            .get(to + 1)
            .is_some_and(|argument| argument == "ADDRESS")
            && args.get(to + 2).is_some_and(|argument| argument == "OF")
        {
            let source = self.reference(&args[to + 3..])?;
            let resolved = targets
                .iter()
                .map(|target| self.reference(target))
                .collect::<Result<Vec<_>, _>>()?;
            if resolved
                .iter()
                .any(|target| !is_pointer_like(target.layout.category))
            {
                return Err(MachineProblem::DataException);
            }
            for target in resolved {
                let address = self.address_bytes(&source, target.length)?;
                self.write_reference(&target, &address)?;
            }
            return Ok(());
        }
        if args.get(to + 1).is_some_and(|argument| argument == "NULL") {
            let resolved = targets
                .iter()
                .map(|target| self.reference(target))
                .collect::<Result<Vec<_>, _>>()?;
            if resolved
                .iter()
                .any(|target| !is_pointer_like(target.layout.category))
            {
                return Err(MachineProblem::DataException);
            }
            for target in resolved {
                self.write_reference(&target, &vec![0; target.length])?;
            }
            return Ok(());
        }
        if args[to + 1] == "TRUE" {
            for target in targets {
                let condition = self.reference(&target)?.layout;
                if condition.category != LayoutCategory::Condition {
                    return Err(MachineProblem::InvalidOperation);
                }
                let parent = condition.parent.ok_or(MachineProblem::InvalidOperation)?;
                let literal = condition
                    .condition_values
                    .first()
                    .ok_or(MachineProblem::InvalidOperation)?;
                let parent_layout = self
                    .layouts
                    .get(&parent)
                    .cloned()
                    .ok_or(MachineProblem::UnknownStorage)?;
                let normalized = normalize(literal);
                let value = match normalized.as_str() {
                    "SPACE" | "SPACES" => CobolValue::Bytes(vec![b' '; parent_layout.length]),
                    "LOW-VALUE" | "LOW-VALUES" => CobolValue::Bytes(vec![0; parent_layout.length]),
                    "HIGH-VALUE" | "HIGH-VALUES" => {
                        CobolValue::Bytes(vec![0xff; parent_layout.length])
                    }
                    "ZERO" | "ZEROS" | "ZEROES" if is_numeric(parent_layout.category) => {
                        CobolValue::Decimal(Decimal {
                            coefficient: 0,
                            scale: parent_layout.scale,
                        })
                    }
                    "ZERO" | "ZEROS" | "ZEROES" => {
                        CobolValue::Bytes(vec![b'0'; parent_layout.length])
                    }
                    _ if is_numeric(parent_layout.category) => CobolValue::Decimal(
                        decimal_text(literal).ok_or(MachineProblem::DataException)?,
                    ),
                    _ => CobolValue::Bytes(condition_true_value_bytes(literal)),
                };
                self.write_reference_value(&[parent], &value)?;
            }
            return Ok(());
        }
        let value = self.eval_value(&args[to + 1..])?;
        for target in targets {
            self.write_reference_value(&target, &value)?;
        }
        Ok(())
    }

    fn allocate_op(&mut self, args: &[String]) -> Result<(), MachineProblem> {
        let returning = position(args, "RETURNING");
        let pointer = returning
            .map(|returning| {
                let target_name = args
                    .get(returning + 1)
                    .ok_or(MachineProblem::InvalidOperation)?;
                let target = self.reference(std::slice::from_ref(target_name))?;
                if !is_pointer_like(target.layout.category) {
                    return Err(MachineProblem::DataException);
                }
                Ok(target)
            })
            .transpose()?;
        let characters = position(args, "CHARACTERS");
        let initialized = args.iter().any(|token| token == "INITIALIZED");
        let data_target = if characters.is_none() {
            let end = [position(args, "INITIALIZED"), returning]
                .into_iter()
                .flatten()
                .min()
                .unwrap_or(args.len());
            let target = self.reference(&args[..end])?;
            if !target.layout.linkage || target.layout.parent.is_some() {
                return Err(MachineProblem::DataException);
            }
            Some(target)
        } else {
            None
        };
        if characters.is_some() && pointer.is_none() {
            return Err(MachineProblem::InvalidOperation);
        }
        let size = if let Some(characters) = characters {
            let value = value_decimal(self.eval_value(&args[..characters])?)?;
            if value.coefficient <= 0 {
                if let Some(pointer) = &pointer {
                    return self.write_reference(pointer, &vec![0; pointer.length]);
                }
                return Err(MachineProblem::InvalidOperation);
            }
            let divisor = ten_power(value.scale)?;
            let rounded = value
                .coefficient
                .checked_add(divisor - 1)
                .ok_or(MachineProblem::ResourceExhausted)?
                / divisor;
            usize::try_from(rounded).map_err(|_| MachineProblem::ResourceExhausted)?
        } else {
            data_target
                .as_ref()
                .ok_or(MachineProblem::InvalidOperation)?
                .length
        };
        let allocated_count = (self.static_base_count..self.bases.len())
            .filter(|base| !self.freed_allocations.contains(base))
            .count();
        let used = self
            .bases
            .iter()
            .enumerate()
            .filter(|(base, _)| !self.freed_allocations.contains(base))
            .try_fold(0usize, |total, (_, storage)| {
                total.checked_add(storage.len())
            })
            .ok_or(MachineProblem::ResourceExhausted)?;
        let used = used
            .checked_add(
                usize::try_from(
                    self.storage64
                        .charged_bytes()
                        .ok_or(MachineProblem::ResourceExhausted)?,
                )
                .map_err(|_| MachineProblem::ResourceExhausted)?,
            )
            .ok_or(MachineProblem::ResourceExhausted)?;
        if allocated_count + self.storage64.live_allocations()
            >= self.invocation.limits.max_frames as usize
            || used
                .checked_add(size)
                .is_none_or(|total| total > self.invocation.limits.max_storage_bytes as usize)
        {
            return Err(MachineProblem::ResourceExhausted);
        }
        let base = self.bases.len();
        let bytes = if initialized {
            data_target
                .as_ref()
                .and_then(|target| {
                    self.operations.iter().find_map(|operation| {
                        (operation.identity.name() == "init"
                            && optional_text_attribute(operation, "name")
                                .is_some_and(|name| normalize(name) == target.layout.name))
                        .then(|| {
                            bytes_attribute(operation, "initial")
                                .ok()
                                .map(<[u8]>::to_vec)
                        })
                        .flatten()
                    })
                })
                .map(|bytes| FixedValue::fit(&bytes, size, false).bytes().to_vec())
                .unwrap_or_else(|| vec![0; size])
        } else {
            vec![0; size]
        };
        let address = pointer
            .as_ref()
            .map(|pointer| self.address_bytes_for(base, 0, pointer.length))
            .transpose()?;
        self.bases.push(bytes);
        if let Some(target) = &data_target {
            self.assign_linkage_address(&target.layout.name, Some((base, 0)))?;
        }
        if let (Some(pointer), Some(address)) = (pointer, address) {
            self.write_reference(&pointer, &address)?;
        }
        Ok(())
    }

    fn free_op(&mut self, args: &[String]) -> Result<(), MachineProblem> {
        let mut targets = Vec::new();
        let mut allocations = BTreeSet::new();
        for token in args
            .iter()
            .filter(|token| !matches!(token.as_str(), "," | "END-FREE"))
        {
            let reference = self.reference(std::slice::from_ref(token))?;
            if !is_pointer_like(reference.layout.category) {
                return Err(MachineProblem::DataException);
            }
            let Some((base, offset)) = self.decode_address(&self.read_reference(&reference)?)?
            else {
                return Err(MachineProblem::DataException);
            };
            if base < self.static_base_count
                || offset != 0
                || self.freed_allocations.contains(&base)
                || !allocations.insert(base)
            {
                return Err(MachineProblem::DataException);
            }
            targets.push((reference, base));
        }
        if targets.is_empty() {
            return Err(MachineProblem::InvalidOperation);
        }
        for (target, _) in &targets {
            self.write_reference(target, &vec![0; target.length])?;
        }
        self.freed_allocations
            .extend(targets.into_iter().map(|(_, base)| base));
        Ok(())
    }

    fn assign_linkage_address(
        &mut self,
        target_name: &str,
        address: Option<(usize, usize)>,
    ) -> Result<(), MachineProblem> {
        let root = self
            .views
            .get(target_name)
            .cloned()
            .ok_or(MachineProblem::UnknownStorage)?;
        let mut updates = Vec::new();
        for layout in self.layouts.values().filter(|layout| layout.linkage) {
            let Some(original) = self.views.get(&layout.name) else {
                continue;
            };
            if original.base != root.base
                || original.offset < root.offset
                || original.offset.saturating_add(original.length)
                    > root.offset.saturating_add(root.length)
            {
                continue;
            }
            let relative = original.offset - root.offset;
            let next = address
                .map(|(base, offset)| {
                    let offset = offset
                        .checked_add(relative)
                        .ok_or(MachineProblem::ResourceExhausted)?;
                    let end = offset
                        .checked_add(original.length)
                        .ok_or(MachineProblem::ResourceExhausted)?;
                    if self.freed_allocations.contains(&base)
                        || self
                            .bases
                            .get(base)
                            .is_none_or(|storage| end > storage.len())
                    {
                        return Err(MachineProblem::DataException);
                    }
                    Ok(StorageView {
                        base,
                        offset,
                        length: original.length,
                    })
                })
                .transpose()?;
            updates.push((layout.name.clone(), next));
        }
        if updates.is_empty() {
            return Err(MachineProblem::UnknownStorage);
        }
        self.linkage_addresses.extend(updates);
        Ok(())
    }

    fn address_bytes(
        &self,
        reference: &ResolvedReference,
        width: usize,
    ) -> Result<Vec<u8>, MachineProblem> {
        let view = self.storage_view(&reference.layout.name)?;
        self.address_bytes_for(
            view.base,
            view.offset
                .checked_add(reference.offset)
                .ok_or(MachineProblem::ResourceExhausted)?,
            width,
        )
    }

    fn address_bytes_for(
        &self,
        base: usize,
        offset: usize,
        width: usize,
    ) -> Result<Vec<u8>, MachineProblem> {
        let base = base
            .checked_add(1)
            .ok_or(MachineProblem::ResourceExhausted)?;
        match width {
            4 if base < (1 << 12) && offset < (1 << 20) => Ok(((u32::try_from(base)
                .map_err(|_| MachineProblem::ResourceExhausted)?
                << 20)
                | u32::try_from(offset).map_err(|_| MachineProblem::ResourceExhausted)?)
            .to_be_bytes()
            .to_vec()),
            8 if u32::try_from(base).is_ok() && u32::try_from(offset).is_ok() => Ok(
                ((u64::try_from(base).map_err(|_| MachineProblem::ResourceExhausted)? << 32)
                    | u64::try_from(offset).map_err(|_| MachineProblem::ResourceExhausted)?)
                .to_be_bytes()
                .to_vec(),
            ),
            _ => Err(MachineProblem::ResourceExhausted),
        }
    }

    fn decode_address(&self, bytes: &[u8]) -> Result<Option<(usize, usize)>, MachineProblem> {
        if bytes.iter().all(|byte| *byte == 0) {
            return Ok(None);
        }
        let (base, offset) = match bytes {
            [a, b, c, d] => {
                let value = u32::from_be_bytes([*a, *b, *c, *d]);
                ((value >> 20) as u64, (value & 0x000f_ffff) as u64)
            }
            [a, b, c, d, e, f, g, h] => {
                let value = u64::from_be_bytes([*a, *b, *c, *d, *e, *f, *g, *h]);
                (value >> 32, value & 0xffff_ffff)
            }
            _ => return Err(MachineProblem::DataException),
        };
        let base = usize::try_from(base)
            .map_err(|_| MachineProblem::DataException)?
            .checked_sub(1)
            .ok_or(MachineProblem::DataException)?;
        let offset = usize::try_from(offset).map_err(|_| MachineProblem::DataException)?;
        if self
            .bases
            .get(base)
            .is_none_or(|storage| offset > storage.len())
        {
            return Err(MachineProblem::DataException);
        }
        Ok(Some((base, offset)))
    }

    fn arithmetic(
        &mut self,
        name: &str,
        args: &[String],
        preserve_failed_receiver: bool,
    ) -> Result<bool, MachineProblem> {
        if matches!(name, "add" | "subtract") {
            return self.add_or_subtract(name, args, preserve_failed_receiver);
        }
        if name == "multiply" {
            return self.multiply_statement(args, preserve_failed_receiver);
        }
        if name == "divide" {
            return self.divide_statement(args, preserve_failed_receiver);
        }
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
                    sum = decimal_add(self.arithmetic_mode, sum, self.decimal(operand)?)?;
                }
                if let Some(to) = to {
                    let receiver = args
                        .get(to + 1)
                        .ok_or(MachineProblem::InvalidOperation)?
                        .clone();
                    sum = decimal_add(self.arithmetic_mode, sum, self.decimal(&receiver)?)?;
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
                    decimal_subtract(
                        self.arithmetic_mode,
                        self.decimal(&target)?,
                        self.decimal(&args[0])?,
                    )?,
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
                    decimal_multiply(
                        self.arithmetic_mode,
                        self.decimal(&target)?,
                        self.decimal(&args[0])?,
                    )?,
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
                (
                    target,
                    decimal_divide(self.arithmetic_mode, dividend, divisor, scale)?,
                )
            }
            _ => return Err(MachineProblem::InvalidOperation),
        };
        self.commit_decimal_assignments_receiver_local(
            vec![(vec![target], value, rounded)],
            preserve_failed_receiver,
        )
    }

    fn multiply_statement(
        &mut self,
        args: &[String],
        preserve_failed_receiver: bool,
    ) -> Result<bool, MachineProblem> {
        let by = position(args, "BY").ok_or(MachineProblem::InvalidOperation)?;
        let giving = position(args, "GIVING");
        let left = value_decimal(self.eval_value(&args[..by])?)?;
        let right_end = giving.unwrap_or_else(|| {
            args.iter()
                .position(|token| token == "ROUNDED")
                .unwrap_or(args.len())
        });
        let right = value_decimal(self.eval_value(&args[by + 1..right_end])?)?;
        let target = giving.map_or_else(
            || args[by + 1..right_end].to_vec(),
            |giving| {
                args.get(giving + 1)
                    .cloned()
                    .map(|target| vec![target])
                    .unwrap_or_default()
            },
        );
        if target.is_empty() {
            return Err(MachineProblem::InvalidOperation);
        }
        self.commit_decimal_assignments_receiver_local(
            vec![(
                target,
                decimal_multiply(self.arithmetic_mode, left, right)?,
                args.iter().any(|token| token == "ROUNDED"),
            )],
            preserve_failed_receiver,
        )
    }

    fn divide_statement(
        &mut self,
        args: &[String],
        preserve_failed_receiver: bool,
    ) -> Result<bool, MachineProblem> {
        let giving = position(args, "GIVING");
        let remainder = position(args, "REMAINDER");
        let rounded = args.iter().any(|token| token == "ROUNDED");
        let (dividend, divisor, default_target) = if let Some(into) = position(args, "INTO") {
            let dividend_end = giving.or(remainder).unwrap_or_else(|| {
                args.iter()
                    .position(|token| token == "ROUNDED")
                    .unwrap_or(args.len())
            });
            (
                value_decimal(self.eval_value(&args[into + 1..dividend_end])?)?,
                value_decimal(self.eval_value(&args[..into])?)?,
                args[into + 1..dividend_end].to_vec(),
            )
        } else {
            let by = position(args, "BY").ok_or(MachineProblem::InvalidOperation)?;
            let divisor_end = giving.or(remainder).unwrap_or_else(|| {
                args.iter()
                    .position(|token| token == "ROUNDED")
                    .unwrap_or(args.len())
            });
            (
                value_decimal(self.eval_value(&args[..by])?)?,
                value_decimal(self.eval_value(&args[by + 1..divisor_end])?)?,
                Vec::new(),
            )
        };
        let target = giving.map_or(default_target, |giving| {
            args.get(giving + 1)
                .cloned()
                .map(|target| vec![target])
                .unwrap_or_default()
        });
        if target.is_empty() {
            return Err(MachineProblem::InvalidOperation);
        }
        let target_reference = self.reference(&target)?;
        let quotient_scale = target_reference
            .layout
            .scale
            .checked_add(u32::from(rounded))
            .ok_or(MachineProblem::SizeError)?;
        let mut quotient = decimal_divide(self.arithmetic_mode, dividend, divisor, quotient_scale)?;
        let truncated_quotient = decimal_rescale(quotient, target_reference.layout.scale)?;
        quotient = if rounded {
            decimal_rescale_rounded(quotient, target_reference.layout.scale)?
        } else {
            truncated_quotient
        };
        let mut assignments = vec![(target, quotient, rounded)];
        if let Some(remainder) = remainder {
            let target = args
                .get(remainder + 1)
                .cloned()
                .ok_or(MachineProblem::InvalidOperation)?;
            let value = decimal_subtract(
                self.arithmetic_mode,
                dividend,
                decimal_multiply(self.arithmetic_mode, divisor, truncated_quotient)?,
            )?;
            assignments.push((vec![target], value, false));
        }
        self.commit_decimal_assignments_receiver_local(assignments, preserve_failed_receiver)
    }

    fn add_or_subtract(
        &mut self,
        name: &str,
        args: &[String],
        preserve_failed_receiver: bool,
    ) -> Result<bool, MachineProblem> {
        if args
            .first()
            .is_some_and(|token| matches!(token.as_str(), "CORRESPONDING" | "CORR"))
        {
            return self.add_or_subtract_corresponding(name, args, preserve_failed_receiver);
        }
        let separator = position(args, if name == "add" { "TO" } else { "FROM" });
        let giving = position(args, "GIVING");
        let operand_end = separator
            .or(giving)
            .ok_or(MachineProblem::InvalidOperation)?;
        let operands = self.arithmetic_items(&args[..operand_end], false)?;
        if operands.is_empty() {
            return Err(MachineProblem::InvalidOperation);
        }
        let mut operand_sum = Decimal {
            coefficient: 0,
            scale: 0,
        };
        for (operand, _) in operands {
            operand_sum = decimal_add(
                self.arithmetic_mode,
                operand_sum,
                value_decimal(self.eval_value(&operand)?)?,
            )?;
        }

        let primary = separator
            .map(|separator| {
                self.arithmetic_items(
                    &args[separator + 1..giving.unwrap_or(args.len())],
                    giving.is_none(),
                )
            })
            .transpose()?
            .unwrap_or_default();
        let receivers = giving
            .map(|giving| self.arithmetic_items(&args[giving + 1..], true))
            .transpose()?
            .unwrap_or_else(|| primary.clone());
        if receivers.is_empty() {
            return Err(MachineProblem::InvalidOperation);
        }

        let mut assignments = Vec::new();
        if giving.is_some() {
            let value = if let Some((source, _)) = primary.first() {
                let source = value_decimal(self.eval_value(source)?)?;
                if name == "add" {
                    decimal_add(self.arithmetic_mode, operand_sum, source)?
                } else {
                    decimal_subtract(self.arithmetic_mode, source, operand_sum)?
                }
            } else if name == "add" {
                operand_sum
            } else {
                return Err(MachineProblem::InvalidOperation);
            };
            assignments.extend(
                receivers
                    .into_iter()
                    .map(|(target, rounded)| (target, value, rounded)),
            );
        } else {
            for (target, rounded) in receivers {
                let current = value_decimal(self.eval_value(&target)?)?;
                let value = if name == "add" {
                    decimal_add(self.arithmetic_mode, current, operand_sum)?
                } else {
                    decimal_subtract(self.arithmetic_mode, current, operand_sum)?
                };
                assignments.push((target, value, rounded));
            }
        }
        self.commit_decimal_assignments_receiver_local(assignments, preserve_failed_receiver)
    }

    fn group_elementary_descendants(&self, root: &str) -> Vec<LayoutMetadata> {
        self.layouts
            .values()
            .filter(|layout| {
                layout.length > 0
                    && !is_group(layout.category)
                    && !matches!(
                        layout.category,
                        LayoutCategory::Condition | LayoutCategory::Rename
                    )
                    && layout.simple_name != "FILLER"
                    && self.layout_is_descendant_of(layout, root)
            })
            .cloned()
            .collect()
    }

    fn layout_is_descendant_of(&self, layout: &LayoutMetadata, root: &str) -> bool {
        let mut parent = layout.parent.as_deref();
        while let Some(name) = parent {
            if name == root {
                return true;
            }
            parent = self
                .layouts
                .get(name)
                .and_then(|layout| layout.parent.as_deref());
        }
        false
    }

    fn arithmetic_items(
        &self,
        tokens: &[String],
        receiving: bool,
    ) -> Result<Vec<(Vec<String>, bool)>, MachineProblem> {
        let mut items = Vec::new();
        let mut at = 0usize;
        while at < tokens.len() {
            if matches!(tokens[at].as_str(), "," | "ROUNDED") {
                at += 1;
                continue;
            }
            let end = (at + 1..=tokens.len())
                .rev()
                .find(|end| {
                    if receiving {
                        self.reference(&tokens[at..*end]).is_ok()
                    } else {
                        self.eval_value(&tokens[at..*end]).is_ok()
                    }
                })
                .ok_or(MachineProblem::InvalidOperation)?;
            let rounded = tokens.get(end).is_some_and(|token| token == "ROUNDED");
            items.push((tokens[at..end].to_vec(), rounded));
            at = end + usize::from(rounded);
        }
        Ok(items)
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
                decimal_add(self.arithmetic_mode, value, right)?
            } else {
                decimal_subtract(self.arithmetic_mode, value, right)?
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
                decimal_multiply(self.arithmetic_mode, value, right)?
            } else {
                decimal_divide(
                    self.arithmetic_mode,
                    value,
                    right,
                    value.scale.max(right.scale).saturating_add(9),
                )?
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
        let target_end = position(&args[into + 1..], "WITH")
            .or_else(|| position(&args[into + 1..], "ON"))
            .map(|offset| into + 1 + offset)
            .unwrap_or(args.len());
        let target = self.reference(&args[into + 1..target_end])?;
        let pointer = position(args, "POINTER")
            .and_then(|at| args.get(at + 1))
            .cloned();
        let start = pointer
            .as_deref()
            .map(|name| self.decimal(name))
            .transpose()?
            .map_or(1, |value| {
                if value.scale == 0 {
                    usize::try_from(value.coefficient).unwrap_or(0)
                } else {
                    0
                }
            });
        if start == 0 {
            return Err(MachineProblem::DataException);
        }
        let mut target_bytes = self.read_reference(&target)?;
        let offset = start - 1;
        let available = target_bytes.len().saturating_sub(offset);
        let copied = available.min(value.len());
        if copied > 0 {
            target_bytes[offset..offset + copied].copy_from_slice(&value[..copied]);
            self.write_reference(&target, &target_bytes)?;
        }
        if let Some(pointer) = pointer {
            self.write_decimal(
                &pointer,
                Decimal {
                    coefficient: i128::try_from(start.saturating_add(copied))
                        .map_err(|_| MachineProblem::ResourceExhausted)?,
                    scale: 0,
                },
            )?;
        }
        let overflow = offset > target_bytes.len() || copied < value.len();
        Ok(overflow)
    }
    fn unstring_op(&mut self, args: &[String]) -> Result<bool, MachineProblem> {
        let into = position(args, "INTO").ok_or(MachineProblem::InvalidOperation)?;
        let source = self.resolve(args.first().ok_or(MachineProblem::InvalidOperation)?)?;
        let pointer = position(args, "POINTER")
            .and_then(|at| args.get(at + 1))
            .cloned();
        let start = pointer
            .as_deref()
            .map(|name| self.decimal(name))
            .transpose()?
            .map_or(1, |value| {
                if value.scale == 0 {
                    usize::try_from(value.coefficient).unwrap_or(0)
                } else {
                    0
                }
            });
        if start == 0 || start > source.len().saturating_add(1) {
            return Err(MachineProblem::DataException);
        }
        let delimiter_start = position(args, "BY").ok_or(MachineProblem::InvalidOperation)? + 1;
        let mut delimiters = Vec::new();
        let mut at = delimiter_start;
        while at < into {
            if matches!(args[at].as_str(), "OR" | "ALL") {
                at += 1;
                continue;
            }
            let delimiter = self.resolve(&args[at])?;
            if !delimiter.is_empty() {
                delimiters.push(delimiter);
            }
            at += 1;
        }
        if delimiters.is_empty() {
            return Err(MachineProblem::InvalidOperation);
        }
        let target_end = args[into + 1..]
            .iter()
            .position(|target| matches!(target.as_str(), "WITH" | "TALLYING" | "ON" | "NOT"))
            .map(|offset| into + 1 + offset)
            .unwrap_or(args.len());
        let targets = &args[into + 1..target_end];
        let mut fields = Vec::new();
        let mut cursor = start - 1;
        while cursor <= source.len() {
            let next = delimiters
                .iter()
                .filter_map(|delimiter| {
                    find_bytes(&source[cursor..], delimiter)
                        .map(|offset| (cursor + offset, delimiter.len()))
                })
                .min_by_key(|(offset, length)| (*offset, *length));
            match next {
                Some((end, delimiter_length)) => {
                    fields.push(source[cursor..end].to_vec());
                    cursor = end.saturating_add(delimiter_length);
                }
                None => {
                    fields.push(source[cursor..].to_vec());
                    cursor = source.len().saturating_add(1);
                    break;
                }
            }
            if cursor == source.len() {
                fields.push(Vec::new());
                cursor = cursor.saturating_add(1);
                break;
            }
        }
        let overflow = fields.len() > targets.len();
        let moved = targets.len().min(fields.len());
        for (target, value) in targets.iter().zip(&fields) {
            self.write(target, value)?;
        }
        if let Some(pointer) = pointer {
            self.write_decimal(
                &pointer,
                Decimal {
                    coefficient: i128::try_from(cursor.min(source.len()).saturating_add(1))
                        .map_err(|_| MachineProblem::ResourceExhausted)?,
                    scale: 0,
                },
            )?;
        }
        if let Some(tallying) = position(args, "TALLYING")
            && args.get(tallying + 1).is_some_and(|token| token == "IN")
            && let Some(target) = args.get(tallying + 2)
        {
            self.write_decimal(
                target,
                Decimal {
                    coefficient: moved as i128,
                    scale: 0,
                },
            )?;
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
        let range = self.inspect_range(&source, args)?;
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
            let mut converted = source.clone();
            for byte in &mut converted[range.clone()] {
                *byte = from
                    .iter()
                    .position(|candidate| candidate == byte)
                    .map_or(*byte, |index| to[index]);
            }
            self.write_reference(&reference, &converted)?;
        } else if let Some(replacing) = position(args, "REPLACING") {
            let mode = args
                .get(replacing + 1)
                .ok_or(MachineProblem::InvalidOperation)?;
            let by = position(args, "BY").ok_or(MachineProblem::InvalidOperation)?;
            let to = self.resolve(args.get(by + 1).ok_or(MachineProblem::InvalidOperation)?)?;
            let mut replaced = source.clone();
            match mode.as_str() {
                "CHARACTERS" => {
                    if to.len() != 1 {
                        return Err(MachineProblem::DataException);
                    }
                    replaced[range.clone()].fill(to[0]);
                }
                "ALL" | "LEADING" | "FIRST" => {
                    let from = self.resolve(
                        args.get(replacing + 2)
                            .ok_or(MachineProblem::InvalidOperation)?,
                    )?;
                    if from.is_empty() || from.len() != to.len() {
                        return Err(MachineProblem::DataException);
                    }
                    let inspected = &mut replaced[range.clone()];
                    if mode == "LEADING" {
                        let mut offset = 0usize;
                        while inspected
                            .get(offset..offset.saturating_add(from.len()))
                            .is_some_and(|candidate| candidate == from)
                        {
                            inspected[offset..offset + from.len()].copy_from_slice(&to);
                            offset += from.len();
                        }
                    } else if mode == "FIRST" {
                        if let Some(at) = find_bytes(inspected, &from) {
                            inspected[at..at + from.len()].copy_from_slice(&to);
                        }
                    } else {
                        let all = replace_bytes(inspected, &from, &to)?;
                        inspected.copy_from_slice(&all);
                    }
                }
                _ => return Err(MachineProblem::UnsupportedForm),
            }
            self.write_reference(&reference, &replaced)?;
        } else if let Some(tallying) = position(args, "TALLYING") {
            let target = args
                .get(tallying + 1)
                .ok_or(MachineProblem::InvalidOperation)?;
            let for_at = position(args, "FOR").ok_or(MachineProblem::InvalidOperation)?;
            let mode = args
                .get(for_at + 1)
                .ok_or(MachineProblem::InvalidOperation)?;
            let inspected = &source[range];
            let count = match mode.as_str() {
                "CHARACTERS" => inspected.len(),
                "ALL" | "LEADING" => {
                    let needle = self.resolve(
                        args.get(for_at + 2)
                            .ok_or(MachineProblem::InvalidOperation)?,
                    )?;
                    if mode == "ALL" {
                        count_bytes(inspected, &needle)?
                    } else if needle.is_empty() {
                        return Err(MachineProblem::InvalidOperation);
                    } else {
                        let mut offset = 0usize;
                        while inspected
                            .get(offset..offset.saturating_add(needle.len()))
                            .is_some_and(|candidate| candidate == needle)
                        {
                            offset += needle.len();
                        }
                        offset / needle.len()
                    }
                }
                _ => return Err(MachineProblem::UnsupportedForm),
            };
            let next = decimal_add(
                self.arithmetic_mode,
                self.decimal(target)?,
                Decimal {
                    coefficient: i128::try_from(count)
                        .map_err(|_| MachineProblem::ResourceExhausted)?,
                    scale: 0,
                },
            )?;
            self.write_decimal(target, next)?;
        }
        Ok(())
    }

    fn inspect_range(
        &self,
        source: &[u8],
        args: &[String],
    ) -> Result<std::ops::Range<usize>, MachineProblem> {
        let delimiter = |keyword: &str| -> Result<Option<Vec<u8>>, MachineProblem> {
            position(args, keyword)
                .map(|at| {
                    let at = at
                        + 1
                        + usize::from(args.get(at + 1).is_some_and(|token| token == "INITIAL"));
                    self.resolve(args.get(at).ok_or(MachineProblem::InvalidOperation)?)
                })
                .transpose()
        };
        let after = delimiter("AFTER")?;
        let before = delimiter("BEFORE")?;
        let start = if let Some(after) = after {
            let Some(at) = find_bytes(source, &after) else {
                return Ok(source.len()..source.len());
            };
            at.checked_add(after.len())
                .ok_or(MachineProblem::ResourceExhausted)?
        } else {
            0
        };
        let end = before
            .as_ref()
            .and_then(|before| find_bytes(&source[start..], before))
            .map_or(source.len(), |at| start + at);
        Ok(start..end)
    }

    fn initialize_op(&mut self, args: &[String]) -> Result<(), MachineProblem> {
        let with_filler = args
            .windows(2)
            .any(|window| window[0] == "WITH" && window[1] == "FILLER");
        let replacements = if let Some(replacing) = position(args, "REPLACING") {
            let mut replacements = BTreeMap::new();
            let mut at = replacing + 1;
            while at < args.len() && args[at] != "THEN" {
                let category = args[at].clone();
                if !matches!(
                    category.as_str(),
                    "ALPHABETIC"
                        | "ALPHANUMERIC"
                        | "ALPHANUMERIC-EDITED"
                        | "DBCS"
                        | "EGCS"
                        | "NATIONAL"
                        | "NATIONAL-EDITED"
                        | "NUMERIC"
                        | "NUMERIC-EDITED"
                        | "UTF-8"
                ) || replacements.contains_key(&category)
                {
                    return Err(MachineProblem::InvalidOperation);
                }
                at += 1;
                if args.get(at).is_some_and(|token| token == "DATA") {
                    at += 1;
                }
                if args.get(at).is_none_or(|token| token != "BY") {
                    return Err(MachineProblem::InvalidOperation);
                }
                at += 1;
                let value = self.eval_value(std::slice::from_ref(
                    args.get(at).ok_or(MachineProblem::InvalidOperation)?,
                ))?;
                replacements.insert(category, value);
                at += 1;
            }
            replacements
        } else {
            BTreeMap::new()
        };
        let initialize_unmatched = replacements.is_empty()
            || args
                .windows(3)
                .any(|window| window == ["THEN", "TO", "DEFAULT"]);
        let control = args
            .iter()
            .position(|target| matches!(target.as_str(), "REPLACING" | "WITH" | "THEN"))
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
                            && (candidate.simple_name != "FILLER" || with_filler)
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
                if let Some(value) = initialize_category(target.category)
                    .and_then(|category| replacements.get(category))
                {
                    self.write_reference_value(std::slice::from_ref(&target.name), value)?;
                } else if !initialize_unmatched {
                    continue;
                } else if is_numeric(target.category) {
                    self.write_decimal(
                        &target.name,
                        Decimal {
                            coefficient: 0,
                            scale: target.scale,
                        },
                    )?;
                } else if is_pointer_like(target.category) {
                    self.write_raw(&target.name, &vec![0; target.length])?;
                } else if matches!(
                    target.category,
                    LayoutCategory::National | LayoutCategory::NationalEdited
                ) {
                    self.write_raw(
                        &target.name,
                        &[0x00, 0x20].repeat(target.length.saturating_add(1) / 2)[..target.length],
                    )?;
                } else if target.category == LayoutCategory::Dbcs {
                    self.write_raw(
                        &target.name,
                        &[0x40, 0x40].repeat(target.length.saturating_add(1) / 2)[..target.length],
                    )?;
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
        let json_clauses = json.then(|| JsonClauses::parse(args, false)).transpose()?;
        let generated = if let Some(layout) = self.layout(from).cloned() {
            if json {
                let clauses = json_clauses
                    .as_ref()
                    .ok_or(MachineProblem::InvalidOperation)?;
                let value = self
                    .json_layout_value(&layout, clauses, &[], true)?
                    .ok_or(MachineProblem::DataException)?;
                if clauses.omitted(&layout) {
                    serde_json::to_string(&value).map_err(|_| MachineProblem::DataException)?
                } else {
                    serde_json::to_string(&BTreeMap::from([(
                        clauses.name(&layout).to_string(),
                        value,
                    )]))
                    .map_err(|_| MachineProblem::DataException)?
                }
            } else if is_group(layout.category) {
                self.xml_layout_value(&layout, &[])?
            } else {
                let value = String::from_utf8_lossy(&self.resolve(from)?)
                    .trim()
                    .to_string();
                format!("<{from}>{}</{from}>", xml_escape(&value))
            }
        } else {
            let value = String::from_utf8_lossy(&self.resolve(from)?)
                .trim()
                .to_string();
            if json {
                serde_json::to_string(&BTreeMap::from([(from.as_str(), value.as_str())]))
                    .map_err(|_| MachineProblem::DataException)?
            } else {
                format!("<{from}>{}</{from}>", xml_escape(&value))
            }
        };
        let generated_bytes = if let Some(clauses) = &json_clauses {
            self.encode_json_text(generated.as_bytes(), clauses)?
        } else {
            generated.as_bytes().to_vec()
        };
        let count_assignment = position(args, "COUNT")
            .map(|count| {
                let mode = args.get(count + 1).map(String::as_str);
                let target_at = count
                    + if matches!(mode, Some("BYTES" | "CHARACTERS")) {
                        3
                    } else {
                        2
                    };
                if args.get(target_at - 1).is_none_or(|token| token != "IN") {
                    return Err(MachineProblem::InvalidOperation);
                }
                let target = args
                    .get(target_at)
                    .ok_or(MachineProblem::InvalidOperation)?;
                let reference = self.reference(std::slice::from_ref(target))?;
                if reference.length != reference.layout.length
                    || !is_numeric(reference.layout.category)
                {
                    return Err(MachineProblem::DataException);
                }
                let value = Decimal {
                    coefficient: i128::try_from(if mode == Some("CHARACTERS") {
                        generated.chars().count()
                    } else {
                        generated_bytes.len()
                    })
                    .map_err(|_| MachineProblem::ResourceExhausted)?,
                    scale: 0,
                };
                let bytes = encode_decimal(
                    &reference.layout,
                    decimal_rescale(value, reference.layout.scale)?,
                )?;
                Ok((reference, bytes))
            })
            .transpose()?;
        if let Some(clauses) = &json_clauses {
            self.write_json_text(target, &generated_bytes, clauses)?;
        } else {
            self.write(target, &generated_bytes)?;
        }
        if let Some((target, bytes)) = count_assignment {
            self.write_reference(&target, &bytes)?;
        }
        Ok(())
    }

    fn json_layout_value(
        &self,
        layout: &LayoutMetadata,
        clauses: &JsonClauses,
        indexes: &[usize],
        root: bool,
    ) -> Result<Option<serde_json::Value>, MachineProblem> {
        if !root && clauses.suppressed(layout) {
            return Ok(None);
        }
        if !root
            && layout.occurs <= 1
            && !is_group(layout.category)
            && self.json_suppression_matches(layout, clauses, indexes)?
        {
            return Ok(None);
        }
        if layout.occurs > 1 {
            let mut values = Vec::new();
            for occurrence in 1..=self.active_occurs(layout)? {
                let mut indexes = indexes.to_vec();
                indexes.push(occurrence);
                values.push(self.json_layout_single_value(layout, clauses, &indexes)?);
            }
            return Ok(Some(serde_json::Value::Array(values)));
        }
        self.json_layout_single_value(layout, clauses, indexes)
            .map(Some)
    }

    fn json_layout_single_value(
        &self,
        layout: &LayoutMetadata,
        clauses: &JsonClauses,
        indexes: &[usize],
    ) -> Result<serde_json::Value, MachineProblem> {
        if is_group(layout.category) {
            let mut children = self
                .layouts
                .values()
                .filter(|candidate| candidate.parent.as_deref() == Some(layout.name.as_str()))
                .filter(|candidate| {
                    candidate.simple_name != "FILLER"
                        && !matches!(
                            candidate.category,
                            LayoutCategory::Condition | LayoutCategory::Rename
                        )
                })
                .cloned()
                .collect::<Vec<_>>();
            children.sort_by(|left, right| {
                left.offset
                    .cmp(&right.offset)
                    .then_with(|| left.name.cmp(&right.name))
            });
            if children.is_empty() {
                return Err(MachineProblem::DataException);
            }
            let mut object = serde_json::Map::new();
            for child in children {
                if clauses.is_indicator_item(&child) {
                    continue;
                }
                if let Some(value) = self.json_layout_value(&child, clauses, indexes, false)? {
                    object.insert(clauses.name(&child).to_string(), value);
                }
            }
            return Ok(serde_json::Value::Object(object));
        }
        let reference = self.layout_occurrence_reference(layout, indexes)?;
        let bytes = self.read_reference(&reference)?;
        if let Some(indicator) = clauses.indicator(layout) {
            let indicator_layout = self
                .layout(&indicator.item)
                .cloned()
                .ok_or(MachineProblem::InvalidOperation)?;
            let indicator_reference =
                self.layout_occurrence_reference(&indicator_layout, indexes)?;
            let indicator_bytes = self.read_reference(&indicator_reference)?;
            if self.json_value_matches(
                &indicator_layout,
                &indicator_bytes,
                &indicator.null_value,
            )? {
                return Ok(serde_json::Value::Null);
            }
        }
        if let Some(conversion) = clauses.conversion(layout) {
            match conversion {
                JsonConversion::GenerateBoolean(true_value) => {
                    return Ok(serde_json::Value::Bool(
                        self.json_value_matches(layout, &bytes, true_value)?,
                    ));
                }
                JsonConversion::GenerateNull(null_value)
                    if self.json_value_matches(layout, &bytes, null_value)? =>
                {
                    return Ok(serde_json::Value::Null);
                }
                JsonConversion::GenerateNull(_) => {}
                JsonConversion::ParseBoolean(_, _) | JsonConversion::ParseNull(_) => {
                    return Err(MachineProblem::InvalidOperation);
                }
            }
        }
        if is_numeric(layout.category) {
            let value = decimal_string(decode_decimal(layout, &bytes)?);
            return value
                .parse::<serde_json::Number>()
                .map(serde_json::Value::Number)
                .map_err(|_| MachineProblem::DataException);
        }
        let text = if matches!(
            layout.category,
            LayoutCategory::National | LayoutCategory::NationalEdited
        ) {
            String::from_utf8(national_to_utf8(&bytes)?)
                .map_err(|_| MachineProblem::DataException)?
        } else {
            String::from_utf8(bytes).map_err(|_| MachineProblem::DataException)?
        };
        Ok(serde_json::Value::String(text.trim_end().into()))
    }

    fn xml_layout_value(
        &self,
        layout: &LayoutMetadata,
        indexes: &[usize],
    ) -> Result<String, MachineProblem> {
        if layout.occurs > 1 {
            let mut values = Vec::new();
            for occurrence in 1..=self.active_occurs(layout)? {
                let mut indexes = indexes.to_vec();
                indexes.push(occurrence);
                values.push(self.xml_layout_single_value(layout, &indexes)?);
            }
            return Ok(values.concat());
        }
        self.xml_layout_single_value(layout, indexes)
    }

    fn xml_layout_single_value(
        &self,
        layout: &LayoutMetadata,
        indexes: &[usize],
    ) -> Result<String, MachineProblem> {
        let value = if is_group(layout.category) {
            let mut children = self
                .layouts
                .values()
                .filter(|candidate| candidate.parent.as_deref() == Some(layout.name.as_str()))
                .filter(|candidate| {
                    candidate.simple_name != "FILLER"
                        && !matches!(
                            candidate.category,
                            LayoutCategory::Condition | LayoutCategory::Rename
                        )
                })
                .cloned()
                .collect::<Vec<_>>();
            children.sort_by(|left, right| {
                left.offset
                    .cmp(&right.offset)
                    .then_with(|| left.name.cmp(&right.name))
            });
            if children.is_empty() {
                return Err(MachineProblem::DataException);
            }
            children
                .iter()
                .map(|child| self.xml_layout_value(child, indexes))
                .collect::<Result<Vec<_>, _>>()?
                .concat()
        } else if is_numeric(layout.category) {
            decimal_string(decode_decimal(
                layout,
                &self.read_reference(&self.layout_occurrence_reference(layout, indexes)?)?,
            )?)
        } else {
            let bytes = self.read_reference(&self.layout_occurrence_reference(layout, indexes)?)?;
            let text = if matches!(
                layout.category,
                LayoutCategory::National | LayoutCategory::NationalEdited
            ) {
                String::from_utf8(national_to_utf8(&bytes)?)
                    .map_err(|_| MachineProblem::DataException)?
            } else {
                String::from_utf8(bytes).map_err(|_| MachineProblem::DataException)?
            };
            xml_escape(text.trim_end())
        };
        Ok(format!(
            "<{}>{value}</{}>",
            layout.simple_name, layout.simple_name
        ))
    }

    fn parse_generated(&mut self, args: &[String], json: bool) -> Result<(), MachineProblem> {
        if args.len() < 3 {
            return Err(MachineProblem::InvalidOperation);
        }
        let source_bytes = self.resolve(&args[0])?;
        let json_clauses = json.then(|| JsonClauses::parse(args, true)).transpose()?;
        let source = if let Some(clauses) = &json_clauses {
            self.decode_json_text(&source_bytes, clauses)?
        } else {
            String::from_utf8(source_bytes).map_err(|_| MachineProblem::DataException)?
        };
        let into = position(args, "INTO").ok_or(MachineProblem::UnsupportedForm)?;
        let target = args.get(into + 1).ok_or(MachineProblem::InvalidOperation)?;
        let value = if json {
            let clauses = json_clauses
                .as_ref()
                .ok_or(MachineProblem::InvalidOperation)?;
            let value: serde_json::Value =
                serde_json::from_str(source.trim()).map_err(|_| MachineProblem::DataException)?;
            if let Some(layout) = self.layout(target).cloned() {
                let value = if clauses.omitted(&layout) {
                    &value
                } else {
                    value
                        .as_object()
                        .and_then(|object| object.get(clauses.name(&layout)))
                        .ok_or(MachineProblem::DataException)?
                };
                self.parse_json_layout(&layout, value, clauses, &[], true)?;
                return Ok(());
            }
            let value = value
                .as_object()
                .and_then(|object| object.values().next())
                .ok_or(MachineProblem::DataException)?;
            match value {
                serde_json::Value::String(value) => value.clone(),
                serde_json::Value::Number(value) => value.to_string(),
                serde_json::Value::Bool(value) => value.to_string(),
                serde_json::Value::Null => String::new(),
                _ => return Err(MachineProblem::DataException),
            }
        } else {
            let source = source.trim();
            if let Some(layout) = self.layout(target).cloned()
                && is_group(layout.category)
            {
                let document = xml_document(source)?;
                if document.name != layout.simple_name {
                    return Err(MachineProblem::DataException);
                }
                let mut assignments = Vec::new();
                self.stage_xml_group(&layout, &document, &mut assignments, &[])?;
                for (reference, bytes) in assignments {
                    self.write_reference(&reference, &bytes)?;
                }
                return Ok(());
            }
            let open_end = source.find('>').ok_or(MachineProblem::DataException)?;
            let name = source
                .get(1..open_end)
                .filter(|name| !name.is_empty() && !name.contains(['<', '>', ' ']))
                .ok_or(MachineProblem::DataException)?;
            let closing = format!("</{name}>");
            let body = source
                .strip_suffix(&closing)
                .and_then(|value| value.get(open_end + 1..))
                .ok_or(MachineProblem::DataException)?;
            xml_unescape(body)?
        };
        self.write(target, value.as_bytes())
    }

    fn stage_xml_group(
        &self,
        layout: &LayoutMetadata,
        node: &XmlNode,
        assignments: &mut Vec<(ResolvedReference, Vec<u8>)>,
        indexes: &[usize],
    ) -> Result<(), MachineProblem> {
        let mut children = self
            .layouts
            .values()
            .filter(|candidate| candidate.parent.as_deref() == Some(layout.name.as_str()))
            .filter(|candidate| {
                candidate.simple_name != "FILLER"
                    && !matches!(
                        candidate.category,
                        LayoutCategory::Condition | LayoutCategory::Rename
                    )
            })
            .cloned()
            .collect::<Vec<_>>();
        children.sort_by(|left, right| {
            left.offset
                .cmp(&right.offset)
                .then_with(|| left.name.cmp(&right.name))
        });
        for child in children {
            let matches = node
                .children
                .iter()
                .filter(|node| node.name == child.simple_name)
                .collect::<Vec<_>>();
            let occurs = if child.occurs > 1 {
                self.active_occurs(&child)?
            } else {
                1
            };
            if matches.len() != occurs {
                return Err(MachineProblem::DataException);
            }
            for (index, node) in matches.into_iter().enumerate() {
                let mut child_indexes = indexes.to_vec();
                if child.occurs > 1 {
                    child_indexes.push(index + 1);
                }
                if is_group(child.category) {
                    self.stage_xml_group(&child, node, assignments, &child_indexes)?;
                } else {
                    if !node.children.is_empty() {
                        return Err(MachineProblem::DataException);
                    }
                    let reference = self.layout_occurrence_reference(&child, &child_indexes)?;
                    let bytes = if is_numeric(child.category) {
                        let mut element = child.clone();
                        element.length = reference.length;
                        element.occurs = 1;
                        encode_decimal(
                            &element,
                            decimal_rescale(
                                decimal_text(&node.text).ok_or(MachineProblem::DataException)?,
                                element.scale,
                            )?,
                        )?
                    } else {
                        let bytes = if matches!(
                            child.category,
                            LayoutCategory::National | LayoutCategory::NationalEdited
                        ) {
                            utf8_to_national(node.text.as_bytes())?
                        } else {
                            node.text.as_bytes().to_vec()
                        };
                        FixedValue::fit(&bytes, reference.length, child.justified_right)
                            .bytes()
                            .to_vec()
                    };
                    assignments.push((reference, bytes));
                }
            }
        }
        Ok(())
    }

    fn parse_json_layout(
        &mut self,
        layout: &LayoutMetadata,
        value: &serde_json::Value,
        clauses: &JsonClauses,
        indexes: &[usize],
        root: bool,
    ) -> Result<(), MachineProblem> {
        if !root && clauses.suppressed(layout) {
            return Ok(());
        }
        if layout.occurs > 1 {
            let values = value.as_array().ok_or(MachineProblem::DataException)?;
            let occurs = self.active_occurs(layout)?;
            if values.len() != occurs {
                return Err(MachineProblem::DataException);
            }
            for (index, value) in values.iter().enumerate() {
                let mut indexes = indexes.to_vec();
                indexes.push(index + 1);
                self.parse_json_single_value(layout, value, clauses, &indexes)?;
            }
            return Ok(());
        }
        self.parse_json_single_value(layout, value, clauses, indexes)
    }

    fn parse_json_single_value(
        &mut self,
        layout: &LayoutMetadata,
        value: &serde_json::Value,
        clauses: &JsonClauses,
        indexes: &[usize],
    ) -> Result<(), MachineProblem> {
        if !is_group(layout.category) {
            let reference = self.layout_occurrence_reference(layout, indexes)?;
            if let Some(indicator) = clauses.indicator(layout) {
                let indicator_layout = self
                    .layout(&indicator.item)
                    .cloned()
                    .ok_or(MachineProblem::InvalidOperation)?;
                let indicator_reference =
                    self.layout_occurrence_reference(&indicator_layout, indexes)?;
                let indicator_value = if value.is_null() {
                    &indicator.null_value
                } else {
                    indicator
                        .nonnull_value
                        .as_ref()
                        .ok_or(MachineProblem::InvalidOperation)?
                };
                let bytes = self.json_conversion_bytes(
                    &indicator_layout,
                    &indicator_reference,
                    indicator_value,
                )?;
                self.write_reference(&indicator_reference, &bytes)?;
                if value.is_null() {
                    return Ok(());
                }
            }
            if let Some(conversion) = clauses.conversion(layout) {
                match conversion {
                    JsonConversion::ParseBoolean(true_value, false_value) => {
                        let value = value.as_bool().ok_or(MachineProblem::DataException)?;
                        let bytes = self.json_conversion_bytes(
                            layout,
                            &reference,
                            if value { true_value } else { false_value },
                        )?;
                        return self.write_reference(&reference, &bytes);
                    }
                    JsonConversion::ParseNull(null_value) if value.is_null() => {
                        let bytes = self.json_conversion_bytes(layout, &reference, null_value)?;
                        return self.write_reference(&reference, &bytes);
                    }
                    JsonConversion::ParseNull(_) => {}
                    JsonConversion::GenerateBoolean(_) | JsonConversion::GenerateNull(_) => {
                        return Err(MachineProblem::InvalidOperation);
                    }
                }
            }
            if value.is_null() {
                if !clauses.ignores_null(layout) {
                    self.implicit.insert(
                        "JSON-STATUS".into(),
                        CobolValue::Decimal(Decimal {
                            coefficient: 32,
                            scale: 0,
                        }),
                    );
                }
                return Ok(());
            }
            let bytes = if is_numeric(layout.category) {
                let text = value
                    .as_number()
                    .map(ToString::to_string)
                    .or_else(|| value.as_str().map(str::to_string))
                    .ok_or(MachineProblem::DataException)?;
                let mut element = layout.clone();
                element.length = reference.length;
                element.occurs = 1;
                encode_decimal(
                    &element,
                    decimal_rescale(
                        decimal_text(&text).ok_or(MachineProblem::DataException)?,
                        element.scale,
                    )?,
                )?
            } else {
                let text = value.as_str().ok_or(MachineProblem::DataException)?;
                let bytes = if matches!(
                    layout.category,
                    LayoutCategory::National | LayoutCategory::NationalEdited
                ) {
                    utf8_to_national(text.as_bytes())?
                } else {
                    text.as_bytes().to_vec()
                };
                FixedValue::fit(&bytes, reference.length, layout.justified_right)
                    .bytes()
                    .to_vec()
            };
            return self.write_reference(&reference, &bytes);
        }
        let object = value.as_object().ok_or(MachineProblem::DataException)?;
        let mut children = self
            .layouts
            .values()
            .filter(|candidate| candidate.parent.as_deref() == Some(layout.name.as_str()))
            .filter(|candidate| {
                candidate.simple_name != "FILLER"
                    && !matches!(
                        candidate.category,
                        LayoutCategory::Condition | LayoutCategory::Rename
                    )
            })
            .cloned()
            .collect::<Vec<_>>();
        children.sort_by(|left, right| {
            left.offset
                .cmp(&right.offset)
                .then_with(|| left.name.cmp(&right.name))
        });
        for child in children {
            if clauses.is_indicator_item(&child) {
                continue;
            }
            if clauses.suppressed(&child) {
                continue;
            }
            let value = object
                .get(clauses.name(&child))
                .ok_or(MachineProblem::DataException)?;
            self.parse_json_layout(&child, value, clauses, indexes, false)?;
        }
        Ok(())
    }

    fn layout_occurrence_reference(
        &self,
        layout: &LayoutMetadata,
        indexes: &[usize],
    ) -> Result<ResolvedReference, MachineProblem> {
        if indexes.is_empty() {
            return self.reference(std::slice::from_ref(&layout.name));
        }
        let mut tokens = Vec::with_capacity(indexes.len() + 3);
        tokens.push(layout.name.clone());
        tokens.push("(".into());
        tokens.extend(indexes.iter().map(ToString::to_string));
        tokens.push(")".into());
        self.reference(&tokens)
    }

    fn json_value_matches(
        &self,
        layout: &LayoutMetadata,
        actual: &[u8],
        expected: &str,
    ) -> Result<bool, MachineProblem> {
        if let Some(condition) = self.layout(expected)
            && condition.category == LayoutCategory::Condition
        {
            return condition_matches(actual, &condition.condition_values, layout);
        }
        if matches!(expected, "ZERO" | "ZEROES" | "ZEROS") && is_numeric(layout.category) {
            return decode_decimal(layout, actual).map(|value| value.coefficient == 0);
        }
        if let Some(expected) = json_figurative_bytes(layout, expected, actual.len()) {
            return Ok(actual == expected);
        }
        Ok(actual == self.resolve(expected)?)
    }

    fn json_suppression_matches(
        &self,
        layout: &LayoutMetadata,
        clauses: &JsonClauses,
        indexes: &[usize],
    ) -> Result<bool, MachineProblem> {
        let reference = self.layout_occurrence_reference(layout, indexes)?;
        let actual = self.read_reference(&reference)?;
        if let Some(values) = clauses.conditional_suppression(layout)
            && values
                .iter()
                .map(|value| self.json_value_matches(layout, &actual, value))
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .any(|matched| matched)
        {
            return Ok(true);
        }
        for (numeric, values) in &clauses.generic_suppressions {
            if numeric.is_some_and(|numeric| numeric != is_numeric(layout.category)) {
                continue;
            }
            if values
                .iter()
                .map(|value| self.json_value_matches(layout, &actual, value))
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .any(|matched| matched)
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn json_conversion_bytes(
        &self,
        layout: &LayoutMetadata,
        reference: &ResolvedReference,
        value: &str,
    ) -> Result<Vec<u8>, MachineProblem> {
        if let Some(condition) = self.layout(value)
            && condition.category == LayoutCategory::Condition
        {
            let literal = condition
                .condition_values
                .first()
                .cloned()
                .ok_or(MachineProblem::DataException)?;
            return self.json_conversion_bytes(layout, reference, &literal);
        }
        if matches!(value, "ZERO" | "ZEROES" | "ZEROS") && is_numeric(layout.category) {
            let mut element = layout.clone();
            element.length = reference.length;
            element.occurs = 1;
            return encode_decimal(
                &element,
                Decimal {
                    coefficient: 0,
                    scale: element.scale,
                },
            );
        }
        if let Some(bytes) = json_figurative_bytes(layout, value, reference.length) {
            return Ok(bytes);
        }
        let bytes = self.resolve(value)?;
        Ok(
            FixedValue::fit(&bytes, reference.length, layout.justified_right)
                .bytes()
                .to_vec(),
        )
    }

    fn json_ccsid(&self, clauses: &JsonClauses) -> Result<u16, MachineProblem> {
        if clauses.encoding_from_codepage {
            return self.display_ccsid();
        }
        let Some(value) = &clauses.encoding else {
            return Ok(1_208);
        };
        let value = value_decimal(self.eval_value(std::slice::from_ref(value))?)?;
        if value.scale != 0 {
            return Err(MachineProblem::DataException);
        }
        u16::try_from(value.coefficient).map_err(|_| MachineProblem::DataException)
    }

    fn encode_json_text(
        &self,
        utf8: &[u8],
        clauses: &JsonClauses,
    ) -> Result<Vec<u8>, MachineProblem> {
        match self.json_ccsid(clauses)? {
            1_208 => Ok(utf8.to_vec()),
            37 => CodePage::Cp037
                .encode(
                    std::str::from_utf8(utf8).map_err(|_| MachineProblem::DataException)?,
                    utf8.len().saturating_mul(4).max(1),
                )
                .map_err(|_| MachineProblem::DataException),
            _ => Err(MachineProblem::UnsupportedForm),
        }
    }

    fn decode_json_text(
        &self,
        source: &[u8],
        clauses: &JsonClauses,
    ) -> Result<String, MachineProblem> {
        match self.json_ccsid(clauses)? {
            1_208 => String::from_utf8(source.to_vec()).map_err(|_| MachineProblem::DataException),
            37 => CodePage::Cp037
                .decode(source, source.len().saturating_mul(4).max(1))
                .map_err(|_| MachineProblem::DataException),
            _ => Err(MachineProblem::UnsupportedForm),
        }
    }

    fn write_json_text(
        &mut self,
        target: &str,
        generated: &[u8],
        clauses: &JsonClauses,
    ) -> Result<(), MachineProblem> {
        let reference = self.reference(std::slice::from_ref(&target.to_string()))?;
        if generated.len() > reference.length {
            return Err(MachineProblem::SizeError);
        }
        let fill = if self.json_ccsid(clauses)? == 37 {
            0x40
        } else {
            b' '
        };
        let mut bytes = vec![fill; reference.length];
        bytes[..generated.len()].copy_from_slice(generated);
        self.write_reference(&reference, &bytes)
    }

    fn xml_processing_step(&mut self, args: &[String]) -> Result<Step, MachineProblem> {
        let source =
            String::from_utf8(self.resolve(args.first().ok_or(MachineProblem::InvalidOperation)?)?)
                .map_err(|_| MachineProblem::DataException)?;
        let events = xml_document_events(source.trim())?;
        let event = events.first().ok_or(MachineProblem::DataException)?;
        self.install_xml_event(event);
        self.loop_counts.insert(xml_state_key(self.pc), 1);
        let target = xml_processing_target(args).ok_or(MachineProblem::InvalidOperation)?;
        self.perform_stack.push(self.pc);
        self.label(target).map(Step::Jump)
    }

    fn install_xml_event(&mut self, event: &XmlEvent) {
        self.implicit.insert(
            "XML-EVENT".into(),
            CobolValue::Bytes(event.kind.as_bytes().to_vec()),
        );
        self.implicit
            .insert("XML-TEXT".into(), CobolValue::Bytes(event.text.clone()));
        self.implicit.insert(
            "XML-NTEXT".into(),
            CobolValue::Bytes(utf8_to_national(&event.text).unwrap_or_default()),
        );
        self.implicit.insert(
            "XML-NAMESPACE".into(),
            CobolValue::Bytes(event.namespace.clone()),
        );
        self.implicit.insert(
            "XML-NNAMESPACE".into(),
            CobolValue::Bytes(utf8_to_national(&event.namespace).unwrap_or_default()),
        );
        self.implicit.insert(
            "XML-NAMESPACE-PREFIX".into(),
            CobolValue::Bytes(event.prefix.clone()),
        );
        self.implicit.insert(
            "XML-NNAMESPACE-PREFIX".into(),
            CobolValue::Bytes(utf8_to_national(&event.prefix).unwrap_or_default()),
        );
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
        if tokens.first().is_some_and(|token| token == "ADDRESS")
            && tokens.get(1).is_some_and(|token| token == "OF")
        {
            let reference = self.reference(&tokens[2..])?;
            return self.address_bytes(&reference, 8).map(CobolValue::Bytes);
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
            && tokens[0].len() >= 2
            && matches!(tokens[0].as_bytes().first(), Some(b'\'' | b'"'))
            && tokens[0].as_bytes().first() == tokens[0].as_bytes().last()
        {
            return Ok(CobolValue::Bytes(
                tokens[0].as_bytes()[1..tokens[0].len() - 1].to_vec(),
            ));
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

    fn eval_function_argument(&self, tokens: &[String]) -> Result<CobolValue, MachineProblem> {
        self.eval_value(tokens)
            .or_else(|_| self.eval_expression(tokens).map(CobolValue::Decimal))
    }

    fn eval_function(&self, tokens: &[String]) -> Result<CobolValue, MachineProblem> {
        let name = tokens.get(1).ok_or(MachineProblem::InvalidOperation)?;
        let clock = || -> Result<Vec<u8>, MachineProblem> {
            let value = self
                .invocation
                .bindings
                .get("cobol.current-date")
                .map(|payload| payload.bytes().to_vec())
                .unwrap_or_else(|| b"1970010100000000+0000".to_vec());
            if value.len() != 21
                || !value[..16].iter().all(u8::is_ascii_digit)
                || !matches!(value[16], b'+' | b'-')
                || !value[17..].iter().all(u8::is_ascii_digit)
            {
                return Err(MachineProblem::DataException);
            }
            Ok(value)
        };
        if tokens.len() == 2 {
            return match name.as_str() {
                "CURRENT-DATE" => clock().map(CobolValue::Bytes),
                "E" => decimal_from_f64(std::f64::consts::E).map(CobolValue::Decimal),
                "PI" => decimal_from_f64(std::f64::consts::PI).map(CobolValue::Decimal),
                "SECONDS-PAST-MIDNIGHT" => {
                    let current = clock()?;
                    let seconds = parse_hhmmss(&current[8..14])?;
                    Ok(CobolValue::Decimal(Decimal {
                        coefficient: i128::from(seconds),
                        scale: 0,
                    }))
                }
                "RANDOM" => self.random(None),
                "UUID4" => Ok(CobolValue::Bytes(deterministic_uuid4(
                    self.invocation.execution_id.as_str(),
                    self.invocation.run_unit_id.as_str(),
                    self.executed_steps,
                ))),
                "WHEN-COMPILED" => Ok(CobolValue::Bytes(
                    self.invocation
                        .bindings
                        .get("cobol.when-compiled")
                        .map(|payload| payload.bytes().to_vec())
                        .unwrap_or_else(|| b"1970010100000000+0000".to_vec()),
                )),
                _ => Err(MachineProblem::UnsupportedForm),
            };
        }
        let open = tokens
            .iter()
            .position(|token| token == "(")
            .ok_or(MachineProblem::InvalidOperation)?;
        let close = matching_close(tokens, open).ok_or(MachineProblem::InvalidOperation)?;
        if close + 1 != tokens.len() {
            return Err(MachineProblem::InvalidOperation);
        }
        let arguments = split_function_arguments(self, &tokens[open + 1..close])?;
        let argument = |index: usize| {
            arguments
                .get(index)
                .copied()
                .ok_or(MachineProblem::InvalidOperation)
        };
        let decimal = |index: usize| -> Result<Decimal, MachineProblem> {
            value_decimal(self.eval_function_argument(argument(index)?)?)
        };
        let integer = |index: usize| -> Result<i128, MachineProblem> {
            let value = decimal(index)?;
            (value.scale == 0)
                .then_some(value.coefficient)
                .ok_or(MachineProblem::DataException)
        };
        let bytes = |index: usize| -> Result<Vec<u8>, MachineProblem> {
            value_bytes(self.eval_function_argument(argument(index)?)?)
        };
        let raw_bytes = |index: usize| -> Result<Vec<u8>, MachineProblem> {
            let argument = argument(index)?;
            self.reference(argument)
                .and_then(|reference| self.read_reference(&reference))
                .or_else(|_| bytes(index))
        };
        let national = |index: usize| -> Result<bool, MachineProblem> {
            Ok(self
                .reference(argument(index)?)
                .ok()
                .is_some_and(|reference| {
                    matches!(
                        reference.layout.category,
                        LayoutCategory::National
                            | LayoutCategory::NationalEdited
                            | LayoutCategory::NationalGroup
                    )
                }))
        };
        let numeric = || -> Result<Vec<Decimal>, MachineProblem> {
            (0..arguments.len()).map(&decimal).collect()
        };
        match name.as_str() {
            "ABS" => Ok(CobolValue::Decimal(decimal_abs(decimal(0)?)?)),
            "ACOS" => {
                decimal_from_f64(libm::acos(decimal_f64(decimal(0)?)?)).map(CobolValue::Decimal)
            }
            "ANNUITY" => {
                let rate = decimal_f64(decimal(0)?)?;
                let periods = decimal_f64(decimal(1)?)?;
                let value = if rate == 0.0 {
                    1.0 / periods
                } else {
                    rate / (1.0 - libm::pow(1.0 + rate, -periods))
                };
                decimal_from_f64(value).map(CobolValue::Decimal)
            }
            "ASIN" => {
                decimal_from_f64(libm::asin(decimal_f64(decimal(0)?)?)).map(CobolValue::Decimal)
            }
            "ATAN" => {
                decimal_from_f64(libm::atan(decimal_f64(decimal(0)?)?)).map(CobolValue::Decimal)
            }
            "BIT-OF" => Ok(CobolValue::Bytes(
                raw_bytes(0)?
                    .into_iter()
                    .flat_map(|byte| (0..8).rev().map(move |bit| b'0' + ((byte >> bit) & 1)))
                    .collect(),
            )),
            "BIT-TO-CHAR" => bit_to_char(&bytes(0)?).map(CobolValue::Bytes),
            "BYTE-LENGTH" => Ok(integer_value(raw_bytes(0)?.len() as i128)),
            "CHAR" => {
                let ordinal = integer(0)?;
                if !(1..=256).contains(&ordinal) {
                    return Err(MachineProblem::DataException);
                }
                CodePage::Cp037
                    .decode(&[(ordinal - 1) as u8], 4)
                    .map(|value| CobolValue::Bytes(value.into_bytes()))
                    .map_err(|_| MachineProblem::DataException)
            }
            "COMBINED-DATETIME" => {
                let date = decimal(0)?;
                let time = decimal_divide(
                    self.arithmetic_mode,
                    decimal(1)?,
                    Decimal {
                        coefficient: 86_400,
                        scale: 0,
                    },
                    18,
                )?;
                decimal_add(self.arithmetic_mode, date, time).map(CobolValue::Decimal)
            }
            "CONTENT-OF" => self.eval_value(argument(0)?),
            "COS" => {
                decimal_from_f64(libm::cos(decimal_f64(decimal(0)?)?)).map(CobolValue::Decimal)
            }
            "DATE-TO-YYYYMMDD" => windowed_year(
                integer(0)?,
                arguments
                    .get(1)
                    .map(|_| integer(1))
                    .transpose()?
                    .unwrap_or(50),
                current_year(&clock()?)?,
                4,
            ),
            "DAY-OF-INTEGER" => day_of_integer(integer(0)?).map(CobolValue::Decimal),
            "DAY-TO-YYYYDDD" => windowed_year(
                integer(0)?,
                arguments
                    .get(1)
                    .map(|_| integer(1))
                    .transpose()?
                    .unwrap_or(50),
                current_year(&clock()?)?,
                3,
            ),
            "DISPLAY-OF" => {
                let ccsid = arguments
                    .get(1)
                    .map(|_| integer(1).and_then(ccsid))
                    .transpose()?
                    .unwrap_or(self.display_ccsid()?);
                display_of(&bytes(0)?, ccsid).map(CobolValue::Bytes)
            }
            "EXP" => {
                decimal_from_f64(libm::exp(decimal_f64(decimal(0)?)?)).map(CobolValue::Decimal)
            }
            "EXP10" => {
                decimal_from_f64(libm::exp10(decimal_f64(decimal(0)?)?)).map(CobolValue::Decimal)
            }
            "FACTORIAL" => factorial(integer(0)?).map(CobolValue::Decimal),
            "FORMATTED-CURRENT-DATE" => {
                format_datetime(&String::from_utf8_lossy(&bytes(0)?), &clock()?)
                    .map(CobolValue::Bytes)
            }
            "FORMATTED-DATE" => formatted_date(&String::from_utf8_lossy(&bytes(0)?), integer(1)?)
                .map(CobolValue::Bytes),
            "FORMATTED-DATETIME" => formatted_datetime(
                &String::from_utf8_lossy(&bytes(0)?),
                integer(1)?,
                decimal(2)?,
                arguments
                    .get(3)
                    .map(|_| integer(3))
                    .transpose()?
                    .unwrap_or(0),
            )
            .map(CobolValue::Bytes),
            "FORMATTED-TIME" => formatted_time(
                &String::from_utf8_lossy(&bytes(0)?),
                decimal(1)?,
                arguments
                    .get(2)
                    .map(|_| integer(2))
                    .transpose()?
                    .unwrap_or(0),
            )
            .map(CobolValue::Bytes),
            "HEX-OF" => Ok(CobolValue::Bytes(hex_upper(&raw_bytes(0)?))),
            "HEX-TO-CHAR" => hex_to_char(&bytes(0)?).map(CobolValue::Bytes),
            "INTEGER" => decimal_floor(self.arithmetic_mode, decimal(0)?).map(CobolValue::Decimal),
            "INTEGER-OF-DAY" => integer_of_day(integer(0)?).map(CobolValue::Decimal),
            "INTEGER-OF-FORMATTED-DATE" => integer_of_formatted_date(
                &String::from_utf8_lossy(&bytes(0)?),
                &String::from_utf8_lossy(&bytes(1)?),
            )
            .map(CobolValue::Decimal),
            "INTEGER-PART" => decimal_rescale(decimal(0)?, 0).map(CobolValue::Decimal),
            "UPPER-CASE" => text_case(&bytes(0)?, national(0)?, true).map(CobolValue::Bytes),
            "LOWER-CASE" => text_case(&bytes(0)?, national(0)?, false).map(CobolValue::Bytes),
            "LENGTH" => {
                let value = bytes(0)?;
                let length = self.reference(argument(0)?).ok().map_or_else(
                    || value.len(),
                    |reference| match reference.layout.category {
                        LayoutCategory::National
                        | LayoutCategory::NationalEdited
                        | LayoutCategory::NationalGroup => value.len() / 2,
                        LayoutCategory::Utf8 if !reference.layout.dynamic => {
                            expanded_picture(&reference.layout.picture, reference.layout.length)
                                .map(|picture| {
                                    picture.iter().filter(|symbol| **symbol == b'U').count()
                                })
                                .unwrap_or_else(|_| {
                                    std::str::from_utf8(&value)
                                        .map(str::chars)
                                        .map(Iterator::count)
                                        .unwrap_or(value.len())
                                })
                        }
                        LayoutCategory::Utf8 | LayoutCategory::Utf8Group => {
                            std::str::from_utf8(&value)
                                .map(str::chars)
                                .map(Iterator::count)
                                .unwrap_or(value.len())
                        }
                        _ => reference.length,
                    },
                );
                Ok(integer_value(length as i128))
            }
            "LOG" => {
                decimal_from_f64(libm::log(decimal_f64(decimal(0)?)?)).map(CobolValue::Decimal)
            }
            "LOG10" => {
                decimal_from_f64(libm::log10(decimal_f64(decimal(0)?)?)).map(CobolValue::Decimal)
            }
            "MAX" => extrema(self, &arguments, true),
            "MEAN" => mean(self.arithmetic_mode, &numeric()?).map(CobolValue::Decimal),
            "MEDIAN" => median(self.arithmetic_mode, &numeric()?).map(CobolValue::Decimal),
            "MIDRANGE" => midrange(self.arithmetic_mode, &numeric()?).map(CobolValue::Decimal),
            "MIN" => extrema(self, &arguments, false),
            "MOD" => decimal_mod(decimal(0)?, decimal(1)?, true).map(CobolValue::Decimal),
            "NATIONAL-OF" => {
                let source_is_utf8 = self.reference(argument(0)?).ok().is_some_and(|reference| {
                    matches!(
                        reference.layout.category,
                        LayoutCategory::Utf8 | LayoutCategory::Utf8Group
                    )
                });
                let ccsid = arguments
                    .get(1)
                    .map(|_| integer(1).and_then(ccsid))
                    .transpose()?
                    .unwrap_or(if source_is_utf8 {
                        1_208
                    } else {
                        self.display_ccsid()?
                    });
                national_of(&bytes(0)?, ccsid).map(CobolValue::Bytes)
            }
            "NUMVAL" => parse_numval(&bytes(0)?, false, None).map(CobolValue::Decimal),
            "NUMVAL-C" => {
                let currency = arguments
                    .get(1)
                    .map(|_| bytes(1))
                    .transpose()?
                    .unwrap_or_else(|| b"$".to_vec());
                parse_numval(&bytes(0)?, false, Some(&currency)).map(CobolValue::Decimal)
            }
            "NUMVAL-F" => parse_numval(&bytes(0)?, true, None).map(CobolValue::Decimal),
            "ORD" => cobol_collation_key(&bytes(0)?)
                .first()
                .map(|byte| integer_value(i128::from(*byte) + 1))
                .ok_or(MachineProblem::DataException),
            "ORD-MAX" | "ORD-MIN" => {
                let values = (0..arguments.len())
                    .map(&bytes)
                    .collect::<Result<Vec<_>, _>>()?;
                let selected = values
                    .iter()
                    .enumerate()
                    .reduce(|left, right| {
                        let order = cobol_collation_key(left.1).cmp(&cobol_collation_key(right.1));
                        if (name == "ORD-MAX" && order.is_lt())
                            || (name == "ORD-MIN" && order.is_gt())
                        {
                            right
                        } else {
                            left
                        }
                    })
                    .ok_or(MachineProblem::InvalidOperation)?;
                Ok(integer_value((selected.0 + 1) as i128))
            }
            "PRESENT-VALUE" => present_value(&numeric()?).map(CobolValue::Decimal),
            "RANDOM" => self.random(arguments.first().map(|_| integer(0)).transpose()?),
            "RANGE" => decimal_range(self.arithmetic_mode, &numeric()?).map(CobolValue::Decimal),
            "REM" => decimal_mod(decimal(0)?, decimal(1)?, false).map(CobolValue::Decimal),
            "REVERSE" => text_reverse(&bytes(0)?, national(0)?).map(CobolValue::Bytes),
            "SECONDS-FROM-FORMATTED-TIME" => seconds_from_formatted_time(
                &String::from_utf8_lossy(&bytes(0)?),
                &String::from_utf8_lossy(&bytes(1)?),
            )
            .map(CobolValue::Decimal),
            "SIGN" => Ok(integer_value(decimal_sign(decimal(0)?))),
            "SIN" => {
                decimal_from_f64(libm::sin(decimal_f64(decimal(0)?)?)).map(CobolValue::Decimal)
            }
            "SQRT" => {
                decimal_from_f64(libm::sqrt(decimal_f64(decimal(0)?)?)).map(CobolValue::Decimal)
            }
            "STANDARD-DEVIATION" => variance(&numeric()?, true).map(CobolValue::Decimal),
            "SUM" => decimal_sum(self.arithmetic_mode, &numeric()?).map(CobolValue::Decimal),
            "TAN" => {
                decimal_from_f64(libm::tan(decimal_f64(decimal(0)?)?)).map(CobolValue::Decimal)
            }
            "TEST-DATE-YYYYMMDD" => Ok(integer_value(test_date_yyyymmdd(integer(0)?))),
            "TEST-DAY-YYYYDDD" => Ok(integer_value(test_day_yyyyddd(integer(0)?))),
            "TEST-FORMATTED-DATETIME" => Ok(integer_value(test_formatted_datetime(
                &String::from_utf8_lossy(&bytes(0)?),
                &String::from_utf8_lossy(&bytes(1)?),
            ) as i128)),
            "TEST-NUMVAL" => Ok(integer_value(test_numval(&bytes(0)?, false, None) as i128)),
            "TEST-NUMVAL-C" => {
                let currency = arguments
                    .get(1)
                    .map(|_| bytes(1))
                    .transpose()?
                    .unwrap_or_else(|| b"$".to_vec());
                Ok(integer_value(
                    test_numval(&bytes(0)?, false, Some(&currency)) as i128,
                ))
            }
            "TEST-NUMVAL-F" => Ok(integer_value(test_numval(&bytes(0)?, true, None) as i128)),
            "TRIM" => {
                let mode = arguments
                    .get(1)
                    .and_then(|tokens| tokens.first())
                    .map(String::as_str);
                text_trim(&bytes(0)?, national(0)?, mode).map(CobolValue::Bytes)
            }
            "ULENGTH" => unicode_length(
                &bytes(0)?,
                arguments.get(1).map(|_| integer(1)).transpose()?,
                arguments.get(2).map(|_| integer(2)).transpose()?,
            )
            .map(CobolValue::Decimal),
            "UPOS" => unicode_position(&bytes(0)?, integer(1)?).map(CobolValue::Decimal),
            "USUBSTR" => unicode_substring(
                &bytes(0)?,
                integer(1)?,
                arguments.get(2).map(|_| integer(2)).transpose()?,
            )
            .map(CobolValue::Bytes),
            "USUPPLEMENTARY" => Ok(integer_value(
                String::from_utf8(bytes(0)?)
                    .map_err(|_| MachineProblem::DataException)?
                    .chars()
                    .position(|character| u32::from(character) > 0xffff)
                    .map_or(0, |position| position + 1) as i128,
            )),
            "UVALID" => Ok(integer_value(test_utf8(&bytes(0)?) as i128)),
            "UWIDTH" => unicode_width(&bytes(0)?, integer(1)?).map(CobolValue::Decimal),
            "VARIANCE" => variance(&numeric()?, false).map(CobolValue::Decimal),
            "YEAR-TO-YYYY" => windowed_year(
                integer(0)?,
                arguments
                    .get(1)
                    .map(|_| integer(1))
                    .transpose()?
                    .unwrap_or(50),
                current_year(&clock()?)?,
                0,
            ),
            "INTEGER-OF-DATE" => {
                let value = integer(0)?;
                let (year, month, day) = split_yyyymmdd(value)?;
                Ok(CobolValue::Decimal(Decimal {
                    coefficient: i128::from(cobol_integer_of_date(year, month, day)?),
                    scale: 0,
                }))
            }
            "DATE-OF-INTEGER" => {
                let value =
                    i64::try_from(integer(0)?).map_err(|_| MachineProblem::DataException)?;
                let (year, month, day) = cobol_date_of_integer(value)?;
                Ok(CobolValue::Decimal(Decimal {
                    coefficient: i128::from(year) * 10_000
                        + i128::from(month) * 100
                        + i128::from(day),
                    scale: 0,
                }))
            }
            _ => Err(MachineProblem::InvalidArtifact(format!(
                "intrinsic {name} has no selected runtime route"
            ))),
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
                let left = cobol_collation_key(&left);
                let right = cobol_collation_key(&right);
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
                let left = cobol_collation_key(&left);
                let right = cobol_collation_key(&right);
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
                let left = cobol_collation_key(&left);
                let right = cobol_collation_key(&right);
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

    fn random(&self, seed: Option<i128>) -> Result<CobolValue, MachineProblem> {
        let state = match seed {
            Some(seed) => {
                u64::try_from(seed).map_err(|_| MachineProblem::DataException)? % 2_147_483_646
            }
            None => self.random_state.get().unwrap_or(0),
        };
        let (state, value) = deterministic_random(state);
        self.random_state.set(Some(state));
        Ok(CobolValue::Decimal(value))
    }

    fn display_ccsid(&self) -> Result<u16, MachineProblem> {
        self.invocation
            .bindings
            .get("cobol.display-ccsid")
            .map(|value| {
                std::str::from_utf8(value.bytes())
                    .ok()
                    .and_then(|value| value.parse::<u16>().ok())
                    .filter(|value| *value != 0)
                    .ok_or(MachineProblem::DataException)
            })
            .transpose()
            .map(|value| value.unwrap_or(37))
    }

    fn display_reference(&self, reference: &ResolvedReference) -> Result<Vec<u8>, MachineProblem> {
        if reference.length != reference.layout.length {
            return self.read_reference(reference);
        }
        match reference.layout.category {
            LayoutCategory::PackedDecimal | LayoutCategory::Binary => display_internal_numeric(
                &reference.layout,
                decode_decimal(&reference.layout, &self.read_reference(reference)?)?,
                self.display_sign_separate,
            ),
            LayoutCategory::FloatShort | LayoutCategory::FloatLong => {
                decode_decimal(&reference.layout, &self.read_reference(reference)?)
                    .map(decimal_string)
                    .map(String::into_bytes)
            }
            _ => self.read_reference(reference),
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
        if reference.layout.dynamic && reference.offset == 0 {
            let bytes = match value {
                CobolValue::Bytes(bytes) => bytes.clone(),
                CobolValue::Decimal(value) => decimal_string(*value).into_bytes(),
            };
            return self.write_dynamic(&reference.layout, &bytes);
        }
        let bytes = match value {
            CobolValue::Bytes(bytes)
                if is_pointer_like(reference.layout.category)
                    && bytes.iter().all(|byte| *byte == 0) =>
            {
                vec![0; reference.length]
            }
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
                let scaled = if matches!(
                    reference.layout.category,
                    LayoutCategory::FloatShort | LayoutCategory::FloatLong
                ) {
                    *value
                } else {
                    decimal_rescale(*value, reference.layout.scale)?
                };
                let scaled = if reference.layout.category == LayoutCategory::Binary
                    && !reference.layout.native_binary
                {
                    truncate_to_picture(&reference.layout, scaled)?
                } else {
                    scaled
                };
                encode_decimal(&reference.layout, scaled)?
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
        let value = if matches!(
            layout.category,
            LayoutCategory::FloatShort | LayoutCategory::FloatLong
        ) {
            value
        } else if rounded {
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
        let mut length = if layout.dynamic {
            *self.dynamic_lengths.get(&layout.name).unwrap_or(&0)
        } else {
            layout.length
        };
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
                let mut dimension_layouts =
                    std::iter::successors(layout.parent.as_ref(), |parent| {
                        self.layouts
                            .get(*parent)
                            .and_then(|layout| layout.parent.as_ref())
                    })
                    .filter_map(|parent| self.layouts.get(parent))
                    .filter(|layout| layout.occurs > 1)
                    .collect::<Vec<_>>();
                dimension_layouts.reverse();
                if layout.occurs > 1 {
                    dimension_layouts.push(&layout);
                }
                let dimensions = dimension_layouts
                    .into_iter()
                    .map(|layout| Ok((self.active_occurs(layout)?, layout.element_length)))
                    .collect::<Result<Vec<_>, MachineProblem>>()?;
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
        // A quoted literal is never a data reference, even when its text starts with, or
        // equals, a data name ('ACCT-ID   :' used to resolve as ACCT-ID via normalize; #277).
        if display_literal(tokens.first()?) {
            return None;
        }
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
        let view = self.storage_view(&reference.layout.name)?;
        let start = view
            .offset
            .checked_add(reference.offset)
            .ok_or(MachineProblem::ResourceExhausted)?;
        let end = start
            .checked_add(reference.length)
            .ok_or(MachineProblem::ResourceExhausted)?;
        self.bases
            .get(view.base)
            .ok_or(MachineProblem::DataException)?
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
        let view = self.storage_view(&reference.layout.name)?.clone();
        let start = view
            .offset
            .checked_add(reference.offset)
            .ok_or(MachineProblem::ResourceExhausted)?;
        let end = start
            .checked_add(reference.length)
            .ok_or(MachineProblem::ResourceExhausted)?;
        let target = self
            .bases
            .get_mut(view.base)
            .ok_or(MachineProblem::DataException)?
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
            "NULL" | "NULLS" => return Ok(vec![0]),
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
        let view = self.storage_view(&resolved)?;
        let length = self
            .layouts
            .get(&resolved)
            .filter(|layout| layout.dynamic)
            .and_then(|_| self.dynamic_lengths.get(&resolved).copied())
            .unwrap_or(view.length);
        self.bases
            .get(view.base)
            .and_then(|storage| storage.get(view.offset..view.offset + length))
            .map(<[u8]>::to_vec)
            .ok_or(MachineProblem::DataException)
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
        if let Some(layout) = self.layouts.get(&normalize(name)).cloned()
            && layout.dynamic
        {
            return self.write_dynamic(&layout, value);
        }
        let justified_right = self
            .layouts
            .get(&normalize(name))
            .is_some_and(|layout| layout.justified_right);
        let view = self.storage_view(&normalize(name))?.clone();
        let current = self
            .bases
            .get(view.base)
            .and_then(|storage| storage.get(view.offset..view.offset + view.length))
            .ok_or(MachineProblem::DataException)?;
        let numeric = current.iter().all(u8::is_ascii_digit)
            && value
                .iter()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'+' | b'-'));
        if numeric {
            let mut fitted = vec![b'0'; view.length];
            let copy = value.len().min(view.length);
            fitted[view.length - copy..].copy_from_slice(&value[value.len() - copy..]);
            self.bases
                .get_mut(view.base)
                .and_then(|storage| storage.get_mut(view.offset..view.offset + view.length))
                .ok_or(MachineProblem::DataException)?
                .copy_from_slice(&fitted);
        } else {
            let fitted = FixedValue::fit(value, view.length, justified_right);
            self.bases
                .get_mut(view.base)
                .and_then(|storage| storage.get_mut(view.offset..view.offset + view.length))
                .ok_or(MachineProblem::DataException)?
                .copy_from_slice(fitted.bytes());
        }
        Ok(())
    }

    fn write_dynamic(
        &mut self,
        layout: &LayoutMetadata,
        value: &[u8],
    ) -> Result<(), MachineProblem> {
        let view = self.storage_view(&layout.name)?.clone();
        if layout.dynamic_limit == 0
            || value.len() > layout.dynamic_limit
            || value.len() > view.length
        {
            return Err(MachineProblem::SizeError);
        }
        let storage = self
            .bases
            .get_mut(view.base)
            .and_then(|storage| storage.get_mut(view.offset..view.offset + view.length))
            .ok_or(MachineProblem::DataException)?;
        storage.fill(b' ');
        storage[..value.len()].copy_from_slice(value);
        self.dynamic_lengths
            .insert(layout.name.clone(), value.len());
        Ok(())
    }

    fn storage_view(&self, name: &str) -> Result<&StorageView, MachineProblem> {
        let normalized = normalize(name);
        let view = match self.linkage_addresses.get(&normalized) {
            Some(Some(view)) => view,
            Some(None) => return Err(MachineProblem::DataException),
            None => self
                .views
                .get(&normalized)
                .ok_or(MachineProblem::UnknownStorage)?,
        };
        if self.freed_allocations.contains(&view.base) {
            return Err(MachineProblem::DataException);
        }
        Ok(view)
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
    fn release_storage64_task(&mut self) {
        self.storage64
            .end_task(self.invocation.run_unit_id.as_str());
        self.retain_storage64_task_bindings();
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
                    self.release_storage64_task();
                    return Ok(failure_drive(
                        FailureCategory::Cancelled,
                        "execution cancelled",
                    ));
                }
                MachineResume::TimedOut => {
                    self.release_storage64_task();
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
                if let Some(drive) = self.deferred_drive.take() {
                    return Ok(drive);
                }
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
                    MachineProblem::Host(_)
                    | MachineProblem::ResourceExhausted
                    | MachineProblem::DataException
                    | MachineProblem::SizeError
                    | MachineProblem::SubscriptError
                    | MachineProblem::ReferenceModificationError => problem,
                    MachineProblem::UnsupportedForm => MachineProblem::InvalidArtifact(format!(
                        "operation {} {:?} reached an unsupported runtime form",
                        operation.identity.name(),
                        arguments(&operation)
                    )),
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
            Err(problem) => problem.runtime_condition().map_or_else(
                || MachineDrive::Failed(problem.execution_problem()),
                MachineDrive::Condition,
            ),
        }
    }

    fn checkpoint(&self) -> Option<BoundedPayload> {
        if self.pending.is_some() {
            return None;
        }
        let bytes = snapshot_codec::encode_snapshot(&self.snapshot())?;
        BoundedPayload::new(
            "mainframe-env.reference-machine-checkpoint@12",
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

fn push_bytes(output: &mut Vec<u8>, value: &[u8]) -> Option<()> {
    output.extend_from_slice(&u64::try_from(value.len()).ok()?.to_be_bytes());
    output.extend_from_slice(value);
    Some(())
}

fn push_string_list(output: &mut Vec<u8>, values: &[String]) -> Option<()> {
    output.extend_from_slice(&u32::try_from(values.len()).ok()?.to_be_bytes());
    for value in values {
        push_bytes(output, value.as_bytes())?;
    }
    Some(())
}

pub(super) fn encode_snapshot_prefix(snapshot: &MachineSnapshot) -> Option<Vec<u8>> {
    let mut bytes = b"MECP0012".to_vec();
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
    Some(bytes)
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
        b"MECP0008" => 8,
        b"MECP0009" => 9,
        b"MECP0010" => 10,
        b"MECP0011" => 11,
        b"MECP0012" => 12,
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
    let mut dynamic_lengths = BTreeMap::new();
    let mut implicit_values = BTreeMap::new();
    let mut search_results = BTreeMap::new();
    let mut sql_cursors = BTreeMap::new();
    let mut sort_workspaces = BTreeMap::new();
    let mut active_sort_procedure = None;
    let mut sort_io = None;
    let mut linkage_addresses = BTreeMap::new();
    let mut freed_allocations = BTreeSet::new();
    let mut random_state = None;
    if header_version >= 8 {
        let dynamic_count =
            usize::try_from(input.u32()?).map_err(|_| MachineProblem::IncompatibleSnapshot)?;
        if dynamic_count > max_frames {
            return Err(MachineProblem::IncompatibleSnapshot);
        }
        for _ in 0..dynamic_count {
            let name = snapshot_string(&mut input, 4096)?;
            let length =
                usize::try_from(input.u64()?).map_err(|_| MachineProblem::IncompatibleSnapshot)?;
            if length > max_storage || dynamic_lengths.insert(name, length).is_some() {
                return Err(MachineProblem::IncompatibleSnapshot);
            }
        }
        let implicit_count =
            usize::try_from(input.u32()?).map_err(|_| MachineProblem::IncompatibleSnapshot)?;
        if implicit_count > max_frames.saturating_mul(4) {
            return Err(MachineProblem::IncompatibleSnapshot);
        }
        for _ in 0..implicit_count {
            let name = snapshot_string(&mut input, 4096)?;
            let value = match input
                .take(1)?
                .first()
                .copied()
                .ok_or(MachineProblem::IncompatibleSnapshot)?
            {
                0 => MachineSnapshotValue::Bytes(input.bytes(max_storage)?),
                1 => MachineSnapshotValue::Decimal {
                    coefficient: i128::from_be_bytes(
                        input
                            .take(16)?
                            .try_into()
                            .map_err(|_| MachineProblem::IncompatibleSnapshot)?,
                    ),
                    scale: u32::from_be_bytes(
                        input
                            .take(4)?
                            .try_into()
                            .map_err(|_| MachineProblem::IncompatibleSnapshot)?,
                    ),
                },
                _ => return Err(MachineProblem::IncompatibleSnapshot),
            };
            if implicit_values.insert(name, value).is_some() {
                return Err(MachineProblem::IncompatibleSnapshot);
            }
        }
        let search_count =
            usize::try_from(input.u32()?).map_err(|_| MachineProblem::IncompatibleSnapshot)?;
        if search_count > max_frames {
            return Err(MachineProblem::IncompatibleSnapshot);
        }
        for _ in 0..search_count {
            let node =
                usize::try_from(input.u64()?).map_err(|_| MachineProblem::IncompatibleSnapshot)?;
            let found = match input.take(1)?.first() {
                Some(0) => false,
                Some(1) => true,
                _ => return Err(MachineProblem::IncompatibleSnapshot),
            };
            if search_results.insert(node, found).is_some() {
                return Err(MachineProblem::IncompatibleSnapshot);
            }
        }
        let sql_count =
            usize::try_from(input.u32()?).map_err(|_| MachineProblem::IncompatibleSnapshot)?;
        if sql_count > max_frames {
            return Err(MachineProblem::IncompatibleSnapshot);
        }
        for _ in 0..sql_count {
            let name = snapshot_string(&mut input, 4096)?;
            let values = snapshot_string_list(&mut input, max_frames, 4096)?;
            if sql_cursors.insert(name, values).is_some() {
                return Err(MachineProblem::IncompatibleSnapshot);
            }
        }
        let workspace_count =
            usize::try_from(input.u32()?).map_err(|_| MachineProblem::IncompatibleSnapshot)?;
        if workspace_count > max_frames {
            return Err(MachineProblem::IncompatibleSnapshot);
        }
        for _ in 0..workspace_count {
            let name = snapshot_string(&mut input, 4096)?;
            let cursor =
                usize::try_from(input.u64()?).map_err(|_| MachineProblem::IncompatibleSnapshot)?;
            let record_count =
                usize::try_from(input.u32()?).map_err(|_| MachineProblem::IncompatibleSnapshot)?;
            if record_count > max_frames.saturating_mul(1024) {
                return Err(MachineProblem::IncompatibleSnapshot);
            }
            let mut records = Vec::with_capacity(record_count);
            for _ in 0..record_count {
                let record = input.bytes(remaining_storage)?;
                remaining_storage = remaining_storage
                    .checked_sub(record.len())
                    .ok_or(MachineProblem::IncompatibleSnapshot)?;
                records.push(record);
            }
            if cursor > records.len() || sort_workspaces.insert(name, (records, cursor)).is_some() {
                return Err(MachineProblem::IncompatibleSnapshot);
            }
        }
        active_sort_procedure = match input.take(1)?.first() {
            Some(0) => None,
            Some(1) => {
                let sort_pc = usize::try_from(input.u64()?)
                    .map_err(|_| MachineProblem::IncompatibleSnapshot)?;
                let sort_file = snapshot_string(&mut input, 4096)?;
                let phase = *input
                    .take(1)?
                    .first()
                    .ok_or(MachineProblem::IncompatibleSnapshot)?;
                if phase > 1 {
                    return Err(MachineProblem::IncompatibleSnapshot);
                }
                let arguments = snapshot_string_list(&mut input, max_frames, 4096)?;
                Some((sort_pc, sort_file, phase, arguments))
            }
            _ => return Err(MachineProblem::IncompatibleSnapshot),
        };
        sort_io = match input.take(1)?.first() {
            Some(0) => None,
            Some(1) => {
                let sort_pc = usize::try_from(input.u64()?)
                    .map_err(|_| MachineProblem::IncompatibleSnapshot)?;
                let sort_file = snapshot_string(&mut input, 4096)?;
                let arguments = snapshot_string_list(&mut input, max_frames, 4096)?;
                let inputs = snapshot_string_list(&mut input, max_frames, 4096)?;
                let outputs = snapshot_string_list(&mut input, max_frames, 4096)?;
                let next_input = usize::try_from(input.u64()?)
                    .map_err(|_| MachineProblem::IncompatibleSnapshot)?;
                let next_output = usize::try_from(input.u64()?)
                    .map_err(|_| MachineProblem::IncompatibleSnapshot)?;
                if next_input > inputs.len() || next_output > outputs.len() {
                    return Err(MachineProblem::IncompatibleSnapshot);
                }
                Some((
                    sort_pc,
                    sort_file,
                    arguments,
                    inputs,
                    outputs,
                    next_input,
                    next_output,
                ))
            }
            _ => return Err(MachineProblem::IncompatibleSnapshot),
        };
    }
    if header_version >= 9 {
        let linkage_count =
            usize::try_from(input.u32()?).map_err(|_| MachineProblem::IncompatibleSnapshot)?;
        if linkage_count > max_frames {
            return Err(MachineProblem::IncompatibleSnapshot);
        }
        for _ in 0..linkage_count {
            let name = snapshot_string(&mut input, 4096)?;
            let view = match input.take(1)?.first() {
                Some(0) => None,
                Some(1) => Some((
                    usize::try_from(input.u64()?)
                        .map_err(|_| MachineProblem::IncompatibleSnapshot)?,
                    usize::try_from(input.u64()?)
                        .map_err(|_| MachineProblem::IncompatibleSnapshot)?,
                    usize::try_from(input.u64()?)
                        .map_err(|_| MachineProblem::IncompatibleSnapshot)?,
                )),
                _ => return Err(MachineProblem::IncompatibleSnapshot),
            };
            if linkage_addresses.insert(name, view).is_some() {
                return Err(MachineProblem::IncompatibleSnapshot);
            }
        }
        let freed_count =
            usize::try_from(input.u32()?).map_err(|_| MachineProblem::IncompatibleSnapshot)?;
        if freed_count > max_frames {
            return Err(MachineProblem::IncompatibleSnapshot);
        }
        for _ in 0..freed_count {
            let base =
                usize::try_from(input.u64()?).map_err(|_| MachineProblem::IncompatibleSnapshot)?;
            if !freed_allocations.insert(base) {
                return Err(MachineProblem::IncompatibleSnapshot);
            }
        }
    }
    if header_version >= 10 {
        random_state = match input.take(1)?.first() {
            Some(0) => None,
            Some(1) => Some(input.u64()?),
            _ => return Err(MachineProblem::IncompatibleSnapshot),
        };
    }
    let (storage64, storage64_area_bindings) = snapshot_codec::decode_storage64(
        &mut input,
        header_version,
        max_frames,
        &mut remaining_storage,
    )?;
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
        dynamic_lengths,
        implicit_values,
        search_results,
        sql_cursors,
        sort_workspaces,
        active_sort_procedure,
        sort_io,
        linkage_addresses,
        freed_allocations,
        random_state,
        storage64,
        storage64_area_bindings,
    })
}

fn snapshot_string(input: &mut SnapshotInput<'_>, max: usize) -> Result<String, MachineProblem> {
    String::from_utf8(input.bytes(max)?).map_err(|_| MachineProblem::IncompatibleSnapshot)
}

fn snapshot_string_list(
    input: &mut SnapshotInput<'_>,
    max_items: usize,
    max_bytes: usize,
) -> Result<Vec<String>, MachineProblem> {
    let count = usize::try_from(input.u32()?).map_err(|_| MachineProblem::IncompatibleSnapshot)?;
    if count > max_items {
        return Err(MachineProblem::IncompatibleSnapshot);
    }
    (0..count)
        .map(|_| snapshot_string(input, max_bytes))
        .collect()
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
type StorageState = (
    Vec<Vec<u8>>,
    BTreeMap<String, StorageView>,
    BTreeMap<StorageId, StorageView>,
    BTreeMap<StorageId, String>,
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
    let mut names_by_id = BTreeMap::new();
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
        let name = item.name.to_ascii_uppercase();
        names_by_id.insert(item.id, name.clone());
        names.insert(name, view);
    }
    Ok((bases, names, by_id, names_by_id))
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
    layout_admission::validate(module)?;
    Ok(())
}
pub fn supported_operations() -> &'static BTreeSet<OperationIdentity> {
    static SET: OnceLock<BTreeSet<OperationIdentity>> = OnceLock::new();
    SET.get_or_init(|| {
        let names = [
            "config",
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
            "delete",
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
            "invoke",
            "json_generate",
            "json_parse",
            "merge",
            "move",
            "multiply",
            "next_sentence",
            "open",
            "perform",
            "read",
            "release",
            "return_statement",
            "rewrite",
            "search",
            "set",
            "sort",
            "start",
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
        let mut operations = names
            .into_iter()
            .map(|name| OperationIdentity::new(NAMESPACE, name, 1).expect("static operation"))
            .collect::<BTreeSet<_>>();
        operations.extend(typed_cics::operation_identities());
        operations.extend(typed_decimal::operation_identities());
        operations
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
            native_binary: optional_integer_attribute(operation, "native_binary")
                .unwrap_or_default()
                != 0,
            signed: integer_attribute(operation, "signed")? != 0,
            sign_separate: integer_attribute(operation, "sign_separate")? != 0,
            justified_right: optional_integer_attribute(operation, "justified_right")
                .unwrap_or_default()
                != 0,
            blank_when_zero: optional_integer_attribute(operation, "blank_when_zero")
                .unwrap_or_default()
                != 0,
            linkage: optional_text_attribute(operation, "section") == Some("linkage"),
            offset: usize_attribute(operation, "offset")?,
            length: usize_attribute(operation, "length")?,
            element_length: usize_attribute(operation, "element_length")?,
            occurs: usize_attribute(operation, "occurs")?,
            occurs_min: optional_integer_attribute(operation, "occurs_min")
                .and_then(|value| usize::try_from(value).ok())
                .unwrap_or(1),
            unbounded: optional_integer_attribute(operation, "unbounded").unwrap_or_default() != 0,
            depending_on: optional_text_attribute(operation, "depending_on")
                .filter(|value| !value.is_empty())
                .map(str::to_ascii_uppercase),
            indexes: optional_text_attribute(operation, "indexes")
                .unwrap_or("")
                .split('\u{1f}')
                .filter(|value| !value.is_empty())
                .map(str::to_ascii_uppercase)
                .collect(),
            keys: optional_text_attribute(operation, "keys")
                .unwrap_or("")
                .split('\u{1f}')
                .filter(|value| !value.is_empty())
                .map(|value| {
                    value
                        .split_once(':')
                        .map(|(direction, name)| (direction == "D", name.to_ascii_uppercase()))
                        .ok_or(MachineProblem::InvalidArtifact("invalid table key".into()))
                })
                .collect::<Result<Vec<_>, _>>()?,
            dynamic: optional_integer_attribute(operation, "dynamic").unwrap_or_default() != 0,
            dynamic_limit: optional_integer_attribute(operation, "dynamic_limit")
                .and_then(|value| usize::try_from(value).ok())
                .unwrap_or_default(),
            parent: match text_attribute(operation, "parent")? {
                "" => None,
                parent => Some(parent.to_ascii_uppercase()),
            },
            alias_of: optional_text_attribute(operation, "alias_of")
                .filter(|value| !value.is_empty())
                .map(str::to_ascii_uppercase),
            occurs_clause: optional_integer_attribute(operation, "occurs_clause")
                .unwrap_or_default()
                != 0,
            condition_values: text_attribute(operation, "condition_values")?
                .split('\u{1f}')
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect(),
            object_class: optional_text_attribute(operation, "object_class")
                .filter(|value| !value.is_empty())
                .map(str::to_ascii_uppercase),
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
            record_names: optional_text_attribute(operation, "record_names")
                .unwrap_or("")
                .split('\u{1f}')
                .filter(|name| !name.is_empty())
                .map(str::to_ascii_uppercase)
                .collect(),
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
            sort_merge: optional_integer_attribute(operation, "sort_merge").unwrap_or_default()
                != 0,
            description: optional_text_attribute(operation, "description")
                .unwrap_or("")
                .to_string(),
            record_min: file_record_bounds(
                optional_text_attribute(operation, "description").unwrap_or(""),
            )
            .0,
            record_max: file_record_bounds(
                optional_text_attribute(operation, "description").unwrap_or(""),
            )
            .1,
            ccsid: file_ccsid(optional_text_attribute(operation, "description").unwrap_or("")),
            linage: file_linage(optional_text_attribute(operation, "description").unwrap_or("")),
        };
        if files.insert(name, metadata).is_some() {
            return Err(MachineProblem::InvalidArtifact(
                "duplicate file metadata".into(),
            ));
        }
    }
    Ok(files)
}

fn file_contract_words(description: &str) -> Vec<&str> {
    description.split_whitespace().collect()
}

fn declarative_handlers(value: &str) -> Result<BTreeMap<String, String>, MachineProblem> {
    let mut handlers = BTreeMap::new();
    for entry in value.split('\u{1e}').filter(|entry| !entry.is_empty()) {
        let fields = entry.split('\u{1f}').collect::<Vec<_>>();
        let section = fields
            .first()
            .map(|field| normalize(field))
            .filter(|field| !field.is_empty())
            .ok_or_else(|| MachineProblem::InvalidArtifact("invalid declarative section".into()))?;
        if fields.get(1).is_some_and(|field| *field == "FOR") {
            continue;
        }
        let on = fields
            .iter()
            .position(|field| *field == "ON")
            .ok_or_else(|| MachineProblem::InvalidArtifact("invalid declarative target".into()))?;
        for target in &fields[on + 1..] {
            let target = normalize(target);
            if target.is_empty() || handlers.insert(target, section.clone()).is_some() {
                return Err(MachineProblem::InvalidArtifact(
                    "duplicate declarative target".into(),
                ));
            }
        }
    }
    Ok(handlers)
}

fn file_record_bounds(description: &str) -> (Option<usize>, Option<usize>) {
    let words = file_contract_words(description);
    let Some(record) = words.iter().position(|word| *word == "RECORD") else {
        return (None, None);
    };
    if words.get(record + 1) == Some(&"CONTAINS") {
        let first = words.get(record + 2).and_then(|word| word.parse().ok());
        let second = (words.get(record + 3) == Some(&"TO"))
            .then(|| words.get(record + 4).and_then(|word| word.parse().ok()))
            .flatten();
        return (first, second.or(first));
    }
    if words.get(record + 1) == Some(&"IS") && words.get(record + 2) == Some(&"VARYING") {
        let from = words
            .iter()
            .position(|word| *word == "FROM")
            .and_then(|at| words.get(at + 1))
            .and_then(|word| word.parse().ok());
        let to = words
            .iter()
            .position(|word| *word == "TO")
            .and_then(|at| words.get(at + 1))
            .and_then(|word| word.parse().ok());
        return (from, to);
    }
    (None, None)
}

fn file_record_depending(description: &str) -> Option<String> {
    let words = file_contract_words(description);
    let record = words.iter().position(|word| *word == "RECORD")?;
    // "IS" is optional: RECORD [IS] VARYING [IN] [SIZE] ... DEPENDING [ON] data-name.
    let varying = if words.get(record + 1) == Some(&"IS") {
        record + 2
    } else {
        record + 1
    };
    if words.get(varying) != Some(&"VARYING") {
        return None;
    }
    let depending = words.iter().position(|word| *word == "DEPENDING")?;
    let name = if words.get(depending + 1) == Some(&"ON") {
        depending + 2
    } else {
        depending + 1
    };
    words.get(name).map(|name| name.to_string())
}

fn file_ccsid(description: &str) -> Option<u16> {
    let words = file_contract_words(description);
    let at = words.iter().position(|word| *word == "CODE-SET")?;
    let value = words.get(at + 1 + usize::from(words.get(at + 1) == Some(&"IS")))?;
    matches!(*value, "EBCDIC" | "CP037" | "IBM-037").then_some(37)
}

fn file_linage(description: &str) -> Option<usize> {
    let words = file_contract_words(description);
    let at = words.iter().position(|word| *word == "LINAGE")?;
    words
        .get(at + 1 + usize::from(words.get(at + 1) == Some(&"IS")))?
        .parse()
        .ok()
}

fn validate_record_length(
    file: Option<&FileMetadata>,
    length: usize,
) -> Result<(), MachineProblem> {
    if file.is_some_and(|file| {
        file.record_min.is_some_and(|minimum| length < minimum)
            || file.record_max.is_some_and(|maximum| length > maximum)
    }) {
        Err(MachineProblem::SizeError)
    } else {
        Ok(())
    }
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
fn start_relation_and_key(args: &[String]) -> Option<(KeyRelation, String)> {
    let key = position(args, "KEY")?;
    let mut at = key + 1;
    if args.get(at).is_some_and(|token| token == "IS") {
        at += 1;
    }
    let end = args[at..]
        .iter()
        .position(|token| matches!(token.as_str(), "INVALID" | "END-START"))
        .map_or(args.len(), |offset| at + offset);
    let phrase = &args[at..end];
    let reference = phrase.last()?.clone();
    let relation = if phrase
        .windows(3)
        .any(|window| window == ["NOT", "LESS", "THAN"])
        || phrase
            .iter()
            .any(|token| matches!(token.as_str(), ">=" | "NOT<"))
    {
        KeyRelation::GreaterOrEqual
    } else if phrase
        .windows(3)
        .any(|window| window == ["NOT", "GREATER", "THAN"])
        || phrase
            .iter()
            .any(|token| matches!(token.as_str(), "<=" | "NOT>"))
    {
        KeyRelation::LessOrEqual
    } else if phrase.iter().any(|token| token == ">")
        || phrase
            .windows(2)
            .any(|window| window == ["GREATER", "THAN"])
    {
        KeyRelation::Greater
    } else if phrase.iter().any(|token| token == "<")
        || phrase.windows(2).any(|window| window == ["LESS", "THAN"])
    {
        KeyRelation::Less
    } else {
        KeyRelation::Equal
    };
    Some((relation, reference))
}
fn sort_file_list(args: &[String], keyword: &str) -> Vec<String> {
    let Some(start) = position(args, keyword).map(|index| index + 1) else {
        return Vec::new();
    };
    args[start..]
        .iter()
        .take_while(|token| {
            !matches!(
                token.as_str(),
                "GIVING"
                    | "USING"
                    | "INPUT"
                    | "OUTPUT"
                    | "ASCENDING"
                    | "DESCENDING"
                    | "ON"
                    | "COLLATING"
                    | "END-SORT"
                    | "END-MERGE"
            )
        })
        .filter(|token| token.as_str() != ",")
        .map(|token| normalize(token))
        .collect()
}
fn sort_procedure_target(args: &[String], kind: &str) -> Option<String> {
    let start = args
        .windows(2)
        .position(|window| window == [kind, "PROCEDURE"])?
        + 2;
    let start = start + usize::from(args.get(start).is_some_and(|token| token == "IS"));
    args.get(start).map(|target| normalize(target))
}
fn sort_procedure_end(args: &[String], kind: &str) -> Option<String> {
    let start = args
        .windows(2)
        .position(|window| window == [kind, "PROCEDURE"])?
        + 2;
    let end = args[start..]
        .iter()
        .position(|token| matches!(token.as_str(), "INPUT" | "OUTPUT" | "GIVING" | "USING"))
        .map_or(args.len(), |offset| start + offset);
    args[start..end]
        .iter()
        .position(|token| matches!(token.as_str(), "THRU" | "THROUGH"))
        .and_then(|offset| args.get(start + offset + 1))
        .map(|target| normalize(target))
}
fn normalize(value: &str) -> String {
    value
        .trim_matches(['\'', '"', '.', ','])
        .to_ascii_uppercase()
}

fn display_literal(token: &str) -> bool {
    token.len() >= 2
        && matches!(token.as_bytes().first(), Some(b'\'' | b'"'))
        && token.as_bytes().first() == token.as_bytes().last()
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

fn integer_value(coefficient: i128) -> CobolValue {
    CobolValue::Decimal(Decimal {
        coefficient,
        scale: 0,
    })
}

fn split_function_arguments<'a>(
    machine: &ReferenceMachine,
    tokens: &'a [String],
) -> Result<Vec<&'a [String]>, MachineProblem> {
    if tokens.is_empty() {
        return Ok(Vec::new());
    }
    let mut arguments = Vec::new();
    let mut start = 0usize;
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate() {
        match token.as_str() {
            "(" => depth = depth.saturating_add(1),
            ")" => {
                depth = depth
                    .checked_sub(1)
                    .ok_or(MachineProblem::InvalidOperation)?
            }
            "," if depth == 0 => {
                if start == index {
                    return Err(MachineProblem::InvalidOperation);
                }
                arguments.push(&tokens[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    if depth != 0 || start == tokens.len() {
        return Err(MachineProblem::InvalidOperation);
    }
    arguments.push(&tokens[start..]);
    if arguments.len() > 1 {
        return Ok(arguments);
    }
    let mut depth = 0usize;
    let mut arithmetic = false;
    for token in tokens {
        match token.as_str() {
            "(" => depth += 1,
            ")" => depth = depth.saturating_sub(1),
            "+" | "-" | "*" | "/" if depth == 0 => arithmetic = true,
            _ => {}
        }
    }
    if arithmetic && machine.eval_expression(tokens).is_ok() {
        return Ok(arguments);
    }
    let mut inferred = Vec::new();
    let mut at = 0usize;
    while at < tokens.len() {
        let end = (at + 1..=tokens.len())
            .rev()
            .find(|end| machine.eval_function_argument(&tokens[at..*end]).is_ok());
        let Some(end) = end else {
            return if arithmetic && inferred.is_empty() {
                Ok(arguments)
            } else {
                Err(MachineProblem::InvalidOperation)
            };
        };
        inferred.push(&tokens[at..end]);
        at = end;
    }
    Ok(inferred)
}

fn decimal_abs(mut value: Decimal) -> Result<Decimal, MachineProblem> {
    value.coefficient = value
        .coefficient
        .checked_abs()
        .ok_or(MachineProblem::SizeError)?;
    Ok(value)
}

fn decimal_f64(value: Decimal) -> Result<f64, MachineProblem> {
    decimal_string(value)
        .parse()
        .map_err(|_| MachineProblem::DataException)
}

fn decimal_from_f64(value: f64) -> Result<Decimal, MachineProblem> {
    if !value.is_finite() {
        return Err(MachineProblem::DataException);
    }
    let text = format!("{value:.17}");
    decimal_text(text.trim_end_matches('0').trim_end_matches('.'))
        .ok_or(MachineProblem::DataException)
}

fn bit_to_char(bits: &[u8]) -> Result<Vec<u8>, MachineProblem> {
    let bits = String::from_utf8_lossy(bits);
    let bits = bits.trim();
    if bits.is_empty()
        || !bits.len().is_multiple_of(8)
        || !bits.bytes().all(|byte| matches!(byte, b'0' | b'1'))
    {
        return Err(MachineProblem::DataException);
    }
    bits.as_bytes()
        .chunks(8)
        .map(|chunk| {
            chunk.iter().try_fold(0u8, |value, bit| {
                value
                    .checked_mul(2)
                    .and_then(|value| value.checked_add(*bit - b'0'))
                    .ok_or(MachineProblem::DataException)
            })
        })
        .collect()
}

fn hex_upper(bytes: &[u8]) -> Vec<u8> {
    const DIGITS: &[u8; 16] = b"0123456789ABCDEF";
    bytes
        .iter()
        .flat_map(|byte| {
            [
                DIGITS[usize::from(byte >> 4)],
                DIGITS[usize::from(byte & 0x0f)],
            ]
        })
        .collect()
}

fn hex_to_char(bytes: &[u8]) -> Result<Vec<u8>, MachineProblem> {
    let text = String::from_utf8_lossy(bytes);
    let text = text.trim();
    if text.is_empty() || !text.len().is_multiple_of(2) {
        return Err(MachineProblem::DataException);
    }
    text.as_bytes()
        .chunks(2)
        .map(|pair| {
            let digit = |byte: u8| match byte {
                b'0'..=b'9' => Some(byte - b'0'),
                b'a'..=b'f' => Some(byte - b'a' + 10),
                b'A'..=b'F' => Some(byte - b'A' + 10),
                _ => None,
            };
            Ok(digit(pair[0]).ok_or(MachineProblem::DataException)? * 16
                + digit(pair[1]).ok_or(MachineProblem::DataException)?)
        })
        .collect()
}

fn decimal_floor(mode: CobolArithmeticMode, value: Decimal) -> Result<Decimal, MachineProblem> {
    let truncated = decimal_rescale(value, 0)?;
    if value.coefficient < 0 && decimal_rescale(truncated, value.scale)? != value {
        decimal_subtract(
            mode,
            truncated,
            Decimal {
                coefficient: 1,
                scale: 0,
            },
        )
    } else {
        Ok(truncated)
    }
}

fn decimal_sign(value: Decimal) -> i128 {
    value.coefficient.signum()
}

fn decimal_sum(mode: CobolArithmeticMode, values: &[Decimal]) -> Result<Decimal, MachineProblem> {
    values.iter().try_fold(
        Decimal {
            coefficient: 0,
            scale: 0,
        },
        |sum, value| decimal_add(mode, sum, *value),
    )
}

fn mean(mode: CobolArithmeticMode, values: &[Decimal]) -> Result<Decimal, MachineProblem> {
    if values.is_empty() {
        return Err(MachineProblem::InvalidOperation);
    }
    decimal_divide(
        mode,
        decimal_sum(mode, values)?,
        Decimal {
            coefficient: values.len() as i128,
            scale: 0,
        },
        18,
    )
}

fn median(mode: CobolArithmeticMode, values: &[Decimal]) -> Result<Decimal, MachineProblem> {
    if values.is_empty() {
        return Err(MachineProblem::InvalidOperation);
    }
    let mut values = values.to_vec();
    values.sort_by(|left, right| {
        decimal_aligned(*left, *right)
            .map(|(left, right)| left.coefficient.cmp(&right.coefficient))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    if values.len() % 2 == 1 {
        Ok(values[values.len() / 2])
    } else {
        mean(mode, &values[values.len() / 2 - 1..=values.len() / 2])
    }
}

fn decimal_extrema(values: &[Decimal]) -> Result<(Decimal, Decimal), MachineProblem> {
    if values.is_empty() {
        return Err(MachineProblem::InvalidOperation);
    }
    values
        .iter()
        .skip(1)
        .try_fold((values[0], values[0]), |(minimum, maximum), value| {
            let (value_for_minimum, aligned_minimum) = decimal_aligned(*value, minimum)?;
            let (value_for_maximum, aligned_maximum) = decimal_aligned(*value, maximum)?;
            Ok((
                if value_for_minimum.coefficient < aligned_minimum.coefficient {
                    *value
                } else {
                    minimum
                },
                if value_for_maximum.coefficient > aligned_maximum.coefficient {
                    *value
                } else {
                    maximum
                },
            ))
        })
}

fn decimal_range(mode: CobolArithmeticMode, values: &[Decimal]) -> Result<Decimal, MachineProblem> {
    let (minimum, maximum) = decimal_extrema(values)?;
    decimal_subtract(mode, maximum, minimum)
}

fn midrange(mode: CobolArithmeticMode, values: &[Decimal]) -> Result<Decimal, MachineProblem> {
    let (minimum, maximum) = decimal_extrema(values)?;
    let scale = values
        .iter()
        .map(|value| value.scale)
        .max()
        .unwrap_or(0)
        .saturating_add(1);
    decimal_divide(
        mode,
        decimal_add(mode, minimum, maximum)?,
        Decimal {
            coefficient: 2,
            scale: 0,
        },
        scale,
    )
}

fn variance(values: &[Decimal], standard_deviation: bool) -> Result<Decimal, MachineProblem> {
    if values.is_empty() {
        return Err(MachineProblem::InvalidOperation);
    }
    let floats = values
        .iter()
        .map(|value| decimal_f64(*value))
        .collect::<Result<Vec<_>, _>>()?;
    let mean = floats.iter().sum::<f64>() / floats.len() as f64;
    let variance = floats
        .iter()
        .map(|value| (value - mean) * (value - mean))
        .sum::<f64>()
        / floats.len() as f64;
    decimal_from_f64(if standard_deviation {
        libm::sqrt(variance)
    } else {
        variance
    })
}

fn present_value(values: &[Decimal]) -> Result<Decimal, MachineProblem> {
    let (rate, cashflows) = values
        .split_first()
        .ok_or(MachineProblem::InvalidOperation)?;
    let rate = decimal_f64(*rate)?;
    let value = cashflows
        .iter()
        .enumerate()
        .try_fold(0.0, |sum, (index, cashflow)| {
            Ok::<_, MachineProblem>(
                sum + decimal_f64(*cashflow)? / libm::pow(1.0 + rate, (index + 1) as f64),
            )
        })?;
    decimal_from_f64(value)
}

fn decimal_mod(left: Decimal, right: Decimal, floor: bool) -> Result<Decimal, MachineProblem> {
    let (left, right) = decimal_aligned(left, right)?;
    if right.coefficient == 0 {
        return Err(MachineProblem::SizeError);
    }
    let mut quotient = left.coefficient / right.coefficient;
    if floor
        && left.coefficient % right.coefficient != 0
        && (left.coefficient < 0) != (right.coefficient < 0)
    {
        quotient -= 1;
    }
    Ok(Decimal {
        coefficient: left.coefficient - quotient * right.coefficient,
        scale: left.scale,
    })
}

fn deterministic_random(state: u64) -> (u64, Decimal) {
    let state = state
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    let coefficient = state % 999_999_999_999_999_999 + 1;
    (
        state,
        Decimal {
            coefficient: i128::from(coefficient),
            scale: 18,
        },
    )
}

fn factorial(value: i128) -> Result<Decimal, MachineProblem> {
    if !(0..=33).contains(&value) {
        return Err(MachineProblem::SizeError);
    }
    let coefficient = (1..=value).try_fold(1i128, |product, value| {
        product.checked_mul(value).ok_or(MachineProblem::SizeError)
    })?;
    decimal_checked(Decimal {
        coefficient,
        scale: 0,
    })
}

fn extrema(
    machine: &ReferenceMachine,
    arguments: &[&[String]],
    maximum: bool,
) -> Result<CobolValue, MachineProblem> {
    let values = arguments
        .iter()
        .map(|argument| machine.eval_function_argument(argument))
        .collect::<Result<Vec<_>, _>>()?;
    values
        .into_iter()
        .reduce(|left, right| {
            let order = match (&left, &right) {
                (CobolValue::Decimal(left), CobolValue::Decimal(right)) => {
                    decimal_aligned(*left, *right)
                        .map(|(left, right)| left.coefficient.cmp(&right.coefficient))
                        .unwrap_or(std::cmp::Ordering::Equal)
                }
                _ => value_bytes(left.clone())
                    .map(|bytes| cobol_collation_key(&bytes))
                    .unwrap_or_default()
                    .cmp(
                        &value_bytes(right.clone())
                            .map(|bytes| cobol_collation_key(&bytes))
                            .unwrap_or_default(),
                    ),
            };
            if (maximum && order.is_lt()) || (!maximum && order.is_gt()) {
                right
            } else {
                left
            }
        })
        .ok_or(MachineProblem::InvalidOperation)
}

fn parse_numval(
    bytes: &[u8],
    floating: bool,
    currency: Option<&[u8]>,
) -> Result<Decimal, MachineProblem> {
    let text = String::from_utf8_lossy(bytes);
    let currency = currency
        .map(std::str::from_utf8)
        .transpose()
        .map_err(|_| MachineProblem::DataException)?
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let mut cleaned = text
        .trim()
        .chars()
        .filter(|character| *character != ',')
        .collect::<String>();
    if let Some(currency) = currency {
        cleaned = cleaned.replace(currency, "");
    }
    if cleaned.chars().any(|character| {
        !character.is_ascii_digit()
            && !matches!(character, ' ' | '+' | '-' | '.' | '(' | ')')
            && !(floating && matches!(character, 'e' | 'E'))
    }) {
        return Err(MachineProblem::DataException);
    }
    if floating && cleaned.bytes().any(|byte| matches!(byte, b'e' | b'E')) {
        return cleaned
            .parse::<f64>()
            .map_err(|_| MachineProblem::DataException)
            .and_then(decimal_from_f64);
    }
    decimal_text(&cleaned).ok_or(MachineProblem::DataException)
}

fn test_numval(bytes: &[u8], floating: bool, currency: Option<&[u8]>) -> usize {
    if parse_numval(bytes, floating, currency).is_ok() {
        return 0;
    }
    let text = String::from_utf8_lossy(bytes);
    let currency = currency
        .and_then(|value| std::str::from_utf8(value).ok())
        .map(str::trim)
        .unwrap_or("");
    for (index, character) in text.chars().enumerate() {
        let allowed = character.is_ascii_digit()
            || matches!(character, ' ' | '+' | '-' | '.' | ',' | '(' | ')')
            || (floating && matches!(character, 'e' | 'E'))
            || currency.contains(character);
        if !allowed {
            return index + 1;
        }
    }
    text.trim_end().chars().count().max(1)
}

fn utf8_to_national(bytes: &[u8]) -> Result<Vec<u8>, MachineProblem> {
    let text = std::str::from_utf8(bytes).map_err(|_| MachineProblem::DataException)?;
    Ok(text.encode_utf16().flat_map(u16::to_be_bytes).collect())
}

fn national_to_utf8(bytes: &[u8]) -> Result<Vec<u8>, MachineProblem> {
    let (pairs, remainder) = bytes.as_chunks::<2>();
    if !remainder.is_empty() {
        return Err(MachineProblem::DataException);
    }
    String::from_utf16(
        &pairs
            .iter()
            .map(|pair| u16::from_be_bytes(*pair))
            .collect::<Vec<_>>(),
    )
    .map(String::into_bytes)
    .map_err(|_| MachineProblem::DataException)
}

fn ccsid(value: i128) -> Result<u16, MachineProblem> {
    u16::try_from(value)
        .ok()
        .filter(|value| *value != 0)
        .ok_or(MachineProblem::DataException)
}

fn display_of(national: &[u8], ccsid: u16) -> Result<Vec<u8>, MachineProblem> {
    let utf8 = national_to_utf8(national)?;
    match ccsid {
        1_208 => Ok(utf8),
        37 => {
            let text = std::str::from_utf8(&utf8).map_err(|_| MachineProblem::DataException)?;
            Ok(text
                .chars()
                .flat_map(|character| {
                    CodePage::Cp037
                        .encode(&character.to_string(), 1)
                        .unwrap_or_else(|_| vec![0x3f])
                })
                .collect())
        }
        _ => Err(MachineProblem::UnsupportedForm),
    }
}

fn national_of(source: &[u8], ccsid: u16) -> Result<Vec<u8>, MachineProblem> {
    let utf8 = match ccsid {
        1_208 => std::str::from_utf8(source)
            .map(str::as_bytes)
            .map(<[u8]>::to_vec)
            .map_err(|_| MachineProblem::DataException)?,
        37 => CodePage::Cp037
            .decode(source, source.len().saturating_mul(2).max(1))
            .map(String::into_bytes)
            .map_err(|_| MachineProblem::DataException)?,
        _ => return Err(MachineProblem::UnsupportedForm),
    };
    utf8_to_national(&utf8)
}

fn function_text(bytes: &[u8], national: bool) -> Result<String, MachineProblem> {
    let utf8 = if national {
        national_to_utf8(bytes)?
    } else {
        bytes.to_vec()
    };
    String::from_utf8(utf8).map_err(|_| MachineProblem::DataException)
}

fn function_text_bytes(value: String, national: bool) -> Vec<u8> {
    if national {
        value.encode_utf16().flat_map(u16::to_be_bytes).collect()
    } else {
        value.into_bytes()
    }
}

fn text_case(bytes: &[u8], national: bool, uppercase: bool) -> Result<Vec<u8>, MachineProblem> {
    let value = function_text(bytes, national)?;
    Ok(function_text_bytes(
        if uppercase {
            value.to_uppercase()
        } else {
            value.to_lowercase()
        },
        national,
    ))
}

fn text_reverse(bytes: &[u8], national: bool) -> Result<Vec<u8>, MachineProblem> {
    let value = function_text(bytes, national)?;
    Ok(function_text_bytes(value.chars().rev().collect(), national))
}

fn text_trim(bytes: &[u8], national: bool, mode: Option<&str>) -> Result<Vec<u8>, MachineProblem> {
    let value = function_text(bytes, national)?;
    let value = match mode {
        Some("LEADING") => value.trim_start_matches(' '),
        Some("TRAILING") => value.trim_end_matches(' '),
        _ => value.trim_matches(' '),
    };
    Ok(function_text_bytes(value.to_string(), national))
}

fn unicode_length(
    bytes: &[u8],
    start: Option<i128>,
    length: Option<i128>,
) -> Result<Decimal, MachineProblem> {
    let text = std::str::from_utf8(bytes).map_err(|_| MachineProblem::DataException)?;
    let text = if start.is_some() || length.is_some() {
        let start = usize::try_from(start.ok_or(MachineProblem::DataException)?)
            .ok()
            .and_then(|start| start.checked_sub(1))
            .ok_or(MachineProblem::DataException)?;
        let length = usize::try_from(length.ok_or(MachineProblem::DataException)?)
            .map_err(|_| MachineProblem::DataException)?;
        let end = start
            .checked_add(length)
            .filter(|end| *end <= bytes.len())
            .ok_or(MachineProblem::DataException)?;
        std::str::from_utf8(bytes.get(start..end).ok_or(MachineProblem::DataException)?)
            .map_err(|_| MachineProblem::DataException)?
    } else {
        text
    };
    Ok(Decimal {
        coefficient: text.chars().count() as i128,
        scale: 0,
    })
}

fn unicode_position(bytes: &[u8], character: i128) -> Result<Decimal, MachineProblem> {
    let character = usize::try_from(character)
        .ok()
        .and_then(|value| value.checked_sub(1))
        .ok_or(MachineProblem::DataException)?;
    let text = std::str::from_utf8(bytes).map_err(|_| MachineProblem::DataException)?;
    let position = text
        .char_indices()
        .nth(character)
        .map(|(position, _)| position + 1)
        .ok_or(MachineProblem::DataException)?;
    Ok(Decimal {
        coefficient: position as i128,
        scale: 0,
    })
}

fn unicode_substring(
    bytes: &[u8],
    start: i128,
    length: Option<i128>,
) -> Result<Vec<u8>, MachineProblem> {
    let start = usize::try_from(start)
        .ok()
        .and_then(|value| value.checked_sub(1))
        .ok_or(MachineProblem::DataException)?;
    let length = length
        .map(|length| usize::try_from(length).map_err(|_| MachineProblem::DataException))
        .transpose()?;
    let text = std::str::from_utf8(bytes).map_err(|_| MachineProblem::DataException)?;
    let available = text.chars().count();
    if start >= available || length.is_some_and(|length| start.saturating_add(length) > available) {
        return Err(MachineProblem::DataException);
    }
    Ok(text
        .chars()
        .skip(start)
        .take(length.unwrap_or(usize::MAX))
        .collect::<String>()
        .into_bytes())
}

fn unicode_width(bytes: &[u8], character: i128) -> Result<Decimal, MachineProblem> {
    let Some(character) = usize::try_from(character)
        .ok()
        .and_then(|character| character.checked_sub(1))
    else {
        return Ok(Decimal {
            coefficient: 0,
            scale: 0,
        });
    };
    let text = std::str::from_utf8(bytes).map_err(|_| MachineProblem::DataException)?;
    Ok(Decimal {
        coefficient: text
            .chars()
            .nth(character)
            .map_or(0, |character| character.len_utf8()) as i128,
        scale: 0,
    })
}

fn test_utf8(bytes: &[u8]) -> usize {
    std::str::from_utf8(bytes)
        .err()
        .map_or(0, |error| error.valid_up_to() + 1)
}

fn deterministic_uuid4(execution: &str, run_unit: &str, step: u64) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(execution.as_bytes());
    hasher.update([0]);
    hasher.update(run_unit.as_bytes());
    hasher.update(step.to_be_bytes());
    let mut bytes: [u8; 16] = hasher.finalize()[..16].try_into().expect("fixed digest");
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex = hex_upper(&bytes);
    [
        &hex[..8],
        b"-",
        &hex[8..12],
        b"-",
        &hex[12..16],
        b"-",
        &hex[16..20],
        b"-",
        &hex[20..],
    ]
    .concat()
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

fn cobol_collation_key(bytes: &[u8]) -> Vec<u8> {
    std::str::from_utf8(bytes)
        .ok()
        .and_then(|text| CodePage::Cp037.encode(text, bytes.len()).ok())
        .unwrap_or_else(|| bytes.to_vec())
}

const fn is_numeric(category: LayoutCategory) -> bool {
    matches!(
        category,
        LayoutCategory::NumericDisplay
            | LayoutCategory::NumericEdited
            | LayoutCategory::PackedDecimal
            | LayoutCategory::Binary
            | LayoutCategory::FloatShort
            | LayoutCategory::FloatLong
    )
}

const fn initialize_category(category: LayoutCategory) -> Option<&'static str> {
    match category {
        LayoutCategory::Alphabetic => Some("ALPHABETIC"),
        LayoutCategory::Alphanumeric => Some("ALPHANUMERIC"),
        LayoutCategory::AlphanumericEdited => Some("ALPHANUMERIC-EDITED"),
        LayoutCategory::Dbcs => Some("DBCS"),
        LayoutCategory::National => Some("NATIONAL"),
        LayoutCategory::NationalEdited => Some("NATIONAL-EDITED"),
        LayoutCategory::NumericDisplay
        | LayoutCategory::PackedDecimal
        | LayoutCategory::Binary
        | LayoutCategory::FloatShort
        | LayoutCategory::FloatLong => Some("NUMERIC"),
        LayoutCategory::NumericEdited => Some("NUMERIC-EDITED"),
        LayoutCategory::Utf8 => Some("UTF-8"),
        _ => None,
    }
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
    let mut significant_digits = 0usize;
    let mut significant_started = false;
    for byte in trimmed.bytes() {
        if byte.is_ascii_digit() {
            coefficient = coefficient
                .checked_mul(10)?
                .checked_add(i128::from(byte - b'0'))?;
            if byte != b'0' || significant_started {
                significant_started = true;
                significant_digits += 1;
            }
            if fractional {
                scale = scale.checked_add(1)?;
            }
        } else if byte == b'.' && !fractional {
            fractional = true;
        } else if !matches!(byte, b'+' | b'-' | b',' | b'$' | b' ' | b'(' | b')') {
            return None;
        }
    }
    if !trimmed.bytes().any(|byte| byte.is_ascii_digit()) {
        return None;
    }
    (significant_digits.max(1) <= 34).then_some(Decimal {
        coefficient: if negative { -coefficient } else { coefficient },
        scale,
    })
}

fn decimal_checked(value: Decimal) -> Result<Decimal, MachineProblem> {
    (value.coefficient.unsigned_abs().to_string().len() <= 34)
        .then_some(value)
        .ok_or(MachineProblem::SizeError)
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

fn display_internal_numeric(
    layout: &LayoutMetadata,
    value: Decimal,
    separate: bool,
) -> Result<Vec<u8>, MachineProblem> {
    let mut digits = value.coefficient.unsigned_abs().to_string();
    if digits.len() > layout.digits {
        return Err(MachineProblem::SizeError);
    }
    if digits.len() < layout.digits {
        digits.insert_str(0, &"0".repeat(layout.digits - digits.len()));
    }
    let mut bytes = digits.into_bytes();
    if !layout.signed {
        return Ok(bytes);
    }
    if separate {
        bytes.insert(0, if value.coefficient < 0 { b'-' } else { b'+' });
        return Ok(bytes);
    }
    let last = bytes.last_mut().ok_or(MachineProblem::DataException)?;
    let digit = usize::from(last.saturating_sub(b'0'));
    if digit > 9 {
        return Err(MachineProblem::DataException);
    }
    *last = if value.coefficient < 0 {
        b"}JKLMNOPQR"[digit]
    } else {
        b"{ABCDEFGHI"[digit]
    };
    Ok(bytes)
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
        decimal_checked(Decimal {
            coefficient: value
                .coefficient
                .checked_mul(factor)
                .ok_or(MachineProblem::SizeError)?,
            scale,
        })
    } else {
        let factor = ten_power(value.scale - scale)?;
        decimal_checked(Decimal {
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
    decimal_checked(Decimal {
        coefficient: quotient
            .checked_add(increment)
            .ok_or(MachineProblem::SizeError)?,
        scale,
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
        LayoutCategory::Binary if layout.native_binary && !layout.signed => bytes
            .iter()
            .fold(0i128, |value, byte| (value << 8) | i128::from(*byte)),
        LayoutCategory::Binary => decode_binary_integer(bytes)?,
        LayoutCategory::NumericEdited => {
            decimal_text(&String::from_utf8_lossy(bytes))
                .ok_or(MachineProblem::DataException)?
                .coefficient
        }
        LayoutCategory::FloatShort => {
            let bytes: [u8; 4] = bytes
                .try_into()
                .map_err(|_| MachineProblem::DataException)?;
            decimal_from_float(f64::from(f32::from_be_bytes(bytes)))?.coefficient
        }
        LayoutCategory::FloatLong => {
            let bytes: [u8; 8] = bytes
                .try_into()
                .map_err(|_| MachineProblem::DataException)?;
            decimal_from_float(f64::from_be_bytes(bytes))?.coefficient
        }
        _ => return Err(MachineProblem::DataException),
    };
    if matches!(
        layout.category,
        LayoutCategory::FloatShort | LayoutCategory::FloatLong
    ) {
        let value = match layout.category {
            LayoutCategory::FloatShort => f64::from(f32::from_be_bytes(
                bytes
                    .try_into()
                    .map_err(|_| MachineProblem::DataException)?,
            )),
            LayoutCategory::FloatLong => f64::from_be_bytes(
                bytes
                    .try_into()
                    .map_err(|_| MachineProblem::DataException)?,
            ),
            _ => unreachable!(),
        };
        decimal_from_float(value)
    } else {
        Ok(Decimal {
            coefficient,
            scale: layout.scale,
        })
    }
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

fn truncate_to_picture(layout: &LayoutMetadata, value: Decimal) -> Result<Decimal, MachineProblem> {
    let digits = u32::try_from(layout.digits).map_err(|_| MachineProblem::SizeError)?;
    Ok(Decimal {
        coefficient: value.coefficient % ten_power(digits)?,
        scale: value.scale,
    })
}

fn encode_decimal(layout: &LayoutMetadata, value: Decimal) -> Result<Vec<u8>, MachineProblem> {
    let digits = value.coefficient.unsigned_abs().to_string();
    // The product has no TRUNC option: ordinary binary uses TRUNC(STD), while
    // COMP-5 uses its full native storage range.
    if layout.digits > 0
        && digits.len() > layout.digits
        && layout.category != LayoutCategory::NumericEdited
        && !(layout.category == LayoutCategory::Binary && layout.native_binary)
    {
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
            let stored = if layout.native_binary && !layout.signed {
                output
                    .iter()
                    .fold(0i128, |value, byte| (value << 8) | i128::from(*byte))
            } else {
                decode_binary_integer(&output)?
            };
            if stored != value.coefficient {
                return Err(MachineProblem::SizeError);
            }
            Ok(output)
        }
        LayoutCategory::NumericEdited => encode_edited(layout, value),
        LayoutCategory::FloatShort => {
            let value = decimal_to_float(value)?;
            if !value.is_finite() || value < f64::from(f32::MIN) || value > f64::from(f32::MAX) {
                return Err(MachineProblem::SizeError);
            }
            Ok((value as f32).to_be_bytes().to_vec())
        }
        LayoutCategory::FloatLong => Ok(decimal_to_float(value)?.to_be_bytes().to_vec()),
        _ => Err(MachineProblem::DataException),
    }
}

fn decimal_from_float(value: f64) -> Result<Decimal, MachineProblem> {
    if !value.is_finite() {
        return Err(MachineProblem::DataException);
    }
    decimal_text(&format!("{value:.17}"))
        .map(decimal_normalize)
        .ok_or(MachineProblem::DataException)
}

fn decimal_to_float(value: Decimal) -> Result<f64, MachineProblem> {
    decimal_string(value)
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
        .ok_or(MachineProblem::SizeError)
}

fn decimal_normalize(mut value: Decimal) -> Decimal {
    while value.scale > 0 && value.coefficient % 10 == 0 {
        value.coefficient /= 10;
        value.scale -= 1;
    }
    value
}

fn encode_edited(layout: &LayoutMetadata, value: Decimal) -> Result<Vec<u8>, MachineProblem> {
    if layout.blank_when_zero && value.coefficient == 0 {
        return Ok(vec![b' '; layout.length]);
    }
    let picture = expanded_picture(&layout.picture, layout.length.saturating_add(layout.digits))?;
    let floating_sign = picture
        .windows(2)
        .find(|pair| pair[0] == pair[1] && matches!(pair[0], b'+' | b'-'))
        .map(|pair| pair[0]);
    let floating_currency = (picture
        .iter()
        .take_while(|&&byte| matches!(byte, b'$' | b','))
        .filter(|&&byte| byte == b'$')
        .count()
        >= 2)
        .then_some(b'$');
    let floating_symbol = floating_sign.or(floating_currency);
    if value.coefficient == 0
        && !picture.contains(&b'9')
        && (picture.contains(&b'Z') || picture.contains(&b'*') || floating_symbol.is_some())
    {
        let asterisk_suppression = picture.contains(&b'*');
        let output = picture
            .iter()
            .filter(|&&byte| !matches!(byte, b'V' | b'S' | b'P'))
            .map(|&byte| {
                if asterisk_suppression {
                    if byte == b'.' { b'.' } else { b'*' }
                } else {
                    b' '
                }
            })
            .collect::<Vec<_>>();
        if output.len() != layout.length {
            return Err(MachineProblem::UnsupportedForm);
        }
        return Ok(output);
    }
    let leading_floating_slots = floating_symbol.map_or(0, |symbol| {
        picture
            .iter()
            .take_while(|&&byte| byte == symbol || byte == b',')
            .filter(|&&byte| byte == symbol)
            .count()
    });
    let digit_positions = layout.digits.max(
        picture
            .iter()
            .filter(|&&byte| matches!(byte, b'9' | b'Z' | b'*'))
            .count()
            + leading_floating_slots,
    );
    let mut digits = value.coefficient.unsigned_abs().to_string();
    if floating_symbol.is_some() {
        let numeric_capacity = digit_positions
            .checked_sub(1)
            .ok_or(MachineProblem::UnsupportedForm)?;
        if digits.len() > numeric_capacity {
            digits = digits[digits.len() - numeric_capacity..].to_string();
        }
        if digits.len() < numeric_capacity {
            digits = format!("{}{}", "0".repeat(numeric_capacity - digits.len()), digits);
        }
        // The first floating symbol is the insertion position; the remaining
        // symbols are numeric positions. A leading placeholder lets the
        // existing picture walk consume that reserved insertion position.
        digits.insert(0, '0');
    } else {
        if digits.len() > digit_positions {
            digits = digits[digits.len() - digit_positions..].to_string();
        }
        if digits.len() < digit_positions {
            digits = format!("{}{}", "0".repeat(digit_positions - digits.len()), digits);
        }
    }
    let mut digit_index = 0usize;
    let mut output = Vec::with_capacity(layout.length);
    let mut suppressing = true;
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
                    if suppressing && digit == b'0' && digit_index + 1 < digit_positions {
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
                    if suppressing && digit == b'0' && digit_index + 1 < digit_positions {
                        b'*'
                    } else {
                        suppressing = false;
                        digit
                    },
                );
                digit_index += 1;
            }
            b'+' | b'-' | b'$'
                if (byte != b'$'
                    && (picture.get(picture_index.wrapping_sub(1)) == Some(&byte)
                        || picture.get(picture_index + 1) == Some(&byte)))
                    || (floating_symbol == Some(byte)
                        && picture[..picture_index]
                            .iter()
                            .all(|&prefix| prefix == byte || prefix == b',')) =>
            {
                let digit = *digits.as_bytes().get(digit_index).unwrap_or(&b'0');
                if suppressing && digit == b'0' && digit_index + 1 < digit_positions {
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
            b',' => output.push(if suppressing {
                if picture.contains(&b'*') { b'*' } else { b' ' }
            } else {
                b','
            }),
            b'C' | b'R'
                if (byte == b'C' && picture.get(picture_index + 1) == Some(&b'R'))
                    || (byte == b'R'
                        && picture.get(picture_index.wrapping_sub(1)) == Some(&b'C')) =>
            {
                output.push(if value.coefficient < 0 { byte } else { b' ' });
            }
            b'D' | b'B'
                if (byte == b'D' && picture.get(picture_index + 1) == Some(&b'B'))
                    || (byte == b'B'
                        && picture.get(picture_index.wrapping_sub(1)) == Some(&b'D')) =>
            {
                output.push(if value.coefficient < 0 { byte } else { b' ' });
            }
            b'B' => output.push(b' '),
            b'.' => {
                output.push(b'.');
                suppressing = false;
            }
            other => output.push(other),
        }
    }
    if let Some(symbol) = floating_symbol {
        let insertion = match symbol {
            b'$' => Some(b'$'),
            _ if value.coefficient < 0 => Some(b'-'),
            b'+' => Some(b'+'),
            _ => None,
        };
        // The floating symbol takes the position immediately left of the first
        // significant digit, even when that position holds an insertion comma;
        // with no significant digit inside the floating string it takes the
        // string's last position.
        let region = picture
            .iter()
            .take_while(|&&byte| byte == symbol || byte == b',')
            .count();
        if let Some(insertion) = insertion
            && region > 0
        {
            let slot = output[..region.min(output.len())]
                .iter()
                .position(u8::is_ascii_digit)
                .map_or(region - 1, |first| first.saturating_sub(1));
            output[slot] = insertion;
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

fn write_linage_advance(
    machine: &ReferenceMachine,
    args: &[String],
) -> Result<(usize, bool), MachineProblem> {
    let Some(mut at) = [position(args, "AFTER"), position(args, "BEFORE")]
        .into_iter()
        .flatten()
        .min()
        .map(|at| at + 1)
    else {
        return Ok((1, false));
    };
    if args.get(at).is_some_and(|token| token == "ADVANCING") {
        at += 1;
    }
    let Some(value) = args.get(at) else {
        return Ok((1, false));
    };
    if value == "PAGE" {
        return Ok((0, true));
    }
    let value = value_decimal(machine.eval_value(std::slice::from_ref(value))?)?;
    if value.scale != 0 || value.coefficient < 0 {
        return Err(MachineProblem::DataException);
    }
    usize::try_from(value.coefficient)
        .map(|value| (value, false))
        .map_err(|_| MachineProblem::ResourceExhausted)
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

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

impl JsonClauses {
    fn parse(args: &[String], parsing: bool) -> Result<Self, MachineProblem> {
        let mut clauses = Self::default();
        if let Some(mut at) = position(args, "IGNORING").map(|position| position + 1) {
            loop {
                if args.get(at).is_none_or(|token| token != "JSON")
                    || args.get(at + 1).is_none_or(|token| token != "NULL")
                    || args.get(at + 2).is_none_or(|token| token != "FOR")
                {
                    return Err(MachineProblem::InvalidOperation);
                }
                at += 3;
                if args.get(at).is_some_and(|token| token == "ALL") {
                    clauses.ignore_null_all = true;
                    at += 1;
                } else {
                    clauses.ignored_nulls.insert(normalize(
                        args.get(at).ok_or(MachineProblem::InvalidOperation)?,
                    ));
                    at += 1;
                    while args
                        .get(at)
                        .is_some_and(|token| matches!(token.as_str(), "OF" | "IN"))
                    {
                        at += 2;
                    }
                }
                if args.get(at).is_none_or(|token| token != "ALSO") {
                    break;
                }
                at += 1;
            }
        }
        if let Some(mut at) = position(args, "INDICATING").map(|position| position + 1) {
            loop {
                let target = normalize(args.get(at).ok_or(MachineProblem::InvalidOperation)?);
                at += 1;
                while args.get(at).is_some_and(|token| token != "IS") {
                    at += 1;
                }
                if args.get(at).is_none_or(|token| token != "IS")
                    || args.get(at + 1).is_none_or(|token| token != "JSON")
                    || args.get(at + 2).is_none_or(|token| token != "NULL")
                    || args.get(at + 3).is_none_or(|token| token != "USING")
                {
                    return Err(MachineProblem::InvalidOperation);
                }
                let null_value = args
                    .get(at + 4)
                    .cloned()
                    .ok_or(MachineProblem::InvalidOperation)?;
                at += 5;
                let nonnull_value = if parsing {
                    if args.get(at).is_none_or(|token| token != "AND") {
                        return Err(MachineProblem::InvalidOperation);
                    }
                    let value = args
                        .get(at + 1)
                        .cloned()
                        .ok_or(MachineProblem::InvalidOperation)?;
                    at += 2;
                    Some(value)
                } else {
                    None
                };
                if args.get(at).is_none_or(|token| token != "IN") {
                    return Err(MachineProblem::InvalidOperation);
                }
                let item = normalize(args.get(at + 1).ok_or(MachineProblem::InvalidOperation)?);
                at += 2;
                clauses.indicator_items.insert(item.clone());
                clauses.indicators.insert(
                    target,
                    JsonIndicator {
                        null_value,
                        nonnull_value,
                        item,
                    },
                );
                if args.get(at).is_none_or(|token| token != "ALSO") {
                    break;
                }
                at += 1;
            }
        }
        if let Some(at) = position(args, "ENCODING").map(|position| position + 1) {
            if args.get(at).is_some_and(|token| token == "FROM") {
                if args.get(at + 1).is_none_or(|token| token != "CODEPAGE") {
                    return Err(MachineProblem::InvalidOperation);
                }
                clauses.encoding_from_codepage = true;
            } else {
                clauses.encoding = Some(
                    args.get(at)
                        .cloned()
                        .ok_or(MachineProblem::InvalidOperation)?,
                );
            }
        }
        if let Some(mut at) = position(args, "NAME").map(|position| position + 1) {
            while at < args.len() && !matches!(args[at].as_str(), "SUPPRESS" | "CONVERTING") {
                if args[at] == "OF" {
                    at += 1;
                }
                let target = normalize(args.get(at).ok_or(MachineProblem::InvalidOperation)?);
                at += 1;
                while args.get(at).is_some_and(|token| token != "IS") {
                    if matches!(args[at].as_str(), "SUPPRESS" | "CONVERTING") {
                        return Err(MachineProblem::InvalidOperation);
                    }
                    at += 1;
                }
                if args.get(at).is_none_or(|token| token != "IS") {
                    return Err(MachineProblem::InvalidOperation);
                }
                at += 1;
                let replacement = args.get(at).ok_or(MachineProblem::InvalidOperation)?;
                let replacement = if replacement == "OMITTED" {
                    None
                } else if replacement.len() >= 2
                    && matches!(replacement.as_bytes().first(), Some(b'\'' | b'"'))
                    && replacement.as_bytes().first() == replacement.as_bytes().last()
                {
                    Some(replacement[1..replacement.len() - 1].to_string())
                } else {
                    return Err(MachineProblem::InvalidOperation);
                };
                clauses.names.insert(target, replacement);
                at += 1;
            }
        }
        if let Some(mut at) = position(args, "SUPPRESS").map(|position| position + 1) {
            while at < args.len() && args[at] != "CONVERTING" {
                if args[at] == "EVERY" {
                    if parsing {
                        return Err(MachineProblem::InvalidOperation);
                    }
                    at += 1;
                    let class = match args.get(at).map(String::as_str) {
                        Some("NUMERIC") => {
                            at += 1;
                            Some(true)
                        }
                        Some("NONNUMERIC") => {
                            at += 1;
                            Some(false)
                        }
                        _ => None,
                    };
                    if args.get(at).is_none_or(|token| token != "WHEN") {
                        return Err(MachineProblem::InvalidOperation);
                    }
                    let (values, next) = json_when_values(args, at + 1)?;
                    clauses.generic_suppressions.push((class, values));
                    at = next;
                    continue;
                }
                let target = normalize(args.get(at).ok_or(MachineProblem::InvalidOperation)?);
                at += 1;
                if args.get(at).is_some_and(|token| token == "WHEN") {
                    if parsing {
                        return Err(MachineProblem::InvalidOperation);
                    }
                    let (values, next) = json_when_values(args, at + 1)?;
                    clauses.conditional_suppressions.insert(target, values);
                    at = next;
                } else {
                    clauses.suppressed.insert(target);
                }
            }
        }
        if let Some(mut at) = position(args, "CONVERTING").map(|position| position + 1) {
            loop {
                if args.get(at).is_some_and(|token| token == "OF") {
                    at += 1;
                }
                let target = normalize(args.get(at).ok_or(MachineProblem::InvalidOperation)?);
                at += 1;
                let direction = if parsing { "FROM" } else { "TO" };
                while args.get(at).is_some_and(|token| token != direction) {
                    at += 1;
                }
                if args.get(at).is_none_or(|token| token != direction)
                    || args.get(at + 1).is_none_or(|token| token != "JSON")
                {
                    return Err(MachineProblem::InvalidOperation);
                }
                at += 2;
                let boolean = matches!(args.get(at).map(String::as_str), Some("BOOLEAN" | "BOOL"));
                if !boolean && args.get(at).is_none_or(|token| token != "NULL") {
                    return Err(MachineProblem::InvalidOperation);
                }
                at += 1;
                if args.get(at).is_none_or(|token| token != "USING") {
                    return Err(MachineProblem::InvalidOperation);
                }
                let first = args
                    .get(at + 1)
                    .cloned()
                    .ok_or(MachineProblem::InvalidOperation)?;
                at += 2;
                let conversion = if parsing && boolean {
                    if args.get(at).is_none_or(|token| token != "AND") {
                        return Err(MachineProblem::InvalidOperation);
                    }
                    let second = args
                        .get(at + 1)
                        .cloned()
                        .ok_or(MachineProblem::InvalidOperation)?;
                    at += 2;
                    JsonConversion::ParseBoolean(first, second)
                } else if parsing {
                    JsonConversion::ParseNull(first)
                } else if boolean {
                    JsonConversion::GenerateBoolean(first)
                } else {
                    JsonConversion::GenerateNull(first)
                };
                clauses.conversions.insert(target, conversion);
                if args.get(at).is_none_or(|token| token != "ALSO") {
                    if at != args.len() {
                        return Err(MachineProblem::InvalidOperation);
                    }
                    break;
                }
                at += 1;
            }
        }
        Ok(clauses)
    }

    fn entry<'a>(&'a self, layout: &LayoutMetadata) -> Option<&'a Option<String>> {
        self.names
            .get(&layout.name)
            .or_else(|| self.names.get(&layout.simple_name))
    }

    fn name<'a>(&'a self, layout: &'a LayoutMetadata) -> &'a str {
        self.entry(layout)
            .and_then(Option::as_deref)
            .unwrap_or(&layout.simple_name)
    }

    fn omitted(&self, layout: &LayoutMetadata) -> bool {
        self.entry(layout).is_some_and(Option::is_none)
    }

    fn suppressed(&self, layout: &LayoutMetadata) -> bool {
        self.suppressed.contains(&layout.name) || self.suppressed.contains(&layout.simple_name)
    }

    fn conditional_suppression(&self, layout: &LayoutMetadata) -> Option<&Vec<String>> {
        self.conditional_suppressions
            .get(&layout.name)
            .or_else(|| self.conditional_suppressions.get(&layout.simple_name))
    }

    fn conversion(&self, layout: &LayoutMetadata) -> Option<&JsonConversion> {
        self.conversions
            .get(&layout.name)
            .or_else(|| self.conversions.get(&layout.simple_name))
    }

    fn ignores_null(&self, layout: &LayoutMetadata) -> bool {
        self.ignore_null_all
            || self.ignored_nulls.contains(&layout.name)
            || self.ignored_nulls.contains(&layout.simple_name)
    }

    fn indicator(&self, layout: &LayoutMetadata) -> Option<&JsonIndicator> {
        self.indicators
            .get(&layout.name)
            .or_else(|| self.indicators.get(&layout.simple_name))
    }

    fn is_indicator_item(&self, layout: &LayoutMetadata) -> bool {
        self.indicator_items.contains(&layout.name)
            || self.indicator_items.contains(&layout.simple_name)
    }
}

fn json_when_values(
    args: &[String],
    mut at: usize,
) -> Result<(Vec<String>, usize), MachineProblem> {
    let mut values = vec![
        args.get(at)
            .cloned()
            .ok_or(MachineProblem::InvalidOperation)?,
    ];
    at += 1;
    while args.get(at).is_some_and(|token| token == "OR") {
        values.push(
            args.get(at + 1)
                .cloned()
                .ok_or(MachineProblem::InvalidOperation)?,
        );
        at += 2;
    }
    Ok((values, at))
}

fn is_json_figurative(value: &str) -> bool {
    matches!(
        value,
        "SPACE"
            | "SPACES"
            | "ZERO"
            | "ZEROES"
            | "ZEROS"
            | "LOW-VALUE"
            | "LOW-VALUES"
            | "HIGH-VALUE"
            | "HIGH-VALUES"
    )
}

fn json_figurative_bytes(layout: &LayoutMetadata, value: &str, length: usize) -> Option<Vec<u8>> {
    if !is_json_figurative(value) {
        return None;
    }
    let national = matches!(
        layout.category,
        LayoutCategory::National | LayoutCategory::NationalEdited | LayoutCategory::NationalGroup
    );
    let unit = match value {
        "SPACE" | "SPACES" if national => &[0x00, 0x20][..],
        "LOW-VALUE" | "LOW-VALUES" if national => &[0x00, 0x00][..],
        "HIGH-VALUE" | "HIGH-VALUES" if national => &[0xff, 0xff][..],
        "SPACE" | "SPACES" => b" ".as_slice(),
        "ZERO" | "ZEROES" | "ZEROS" => b"0".as_slice(),
        "LOW-VALUE" | "LOW-VALUES" => &[0x00][..],
        "HIGH-VALUE" | "HIGH-VALUES" => &[0xff][..],
        _ => return None,
    };
    let mut bytes = unit.repeat(length.saturating_add(unit.len() - 1) / unit.len());
    bytes.truncate(length);
    Some(bytes)
}

fn xml_unescape(value: &str) -> Result<String, MachineProblem> {
    let mut output = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(at) = rest.find('&') {
        output.push_str(&rest[..at]);
        rest = &rest[at..];
        let (decoded, length) = if rest.starts_with("&amp;") {
            ('&', 5usize)
        } else if rest.starts_with("&lt;") {
            ('<', 4)
        } else if rest.starts_with("&gt;") {
            ('>', 4)
        } else if rest.starts_with("&quot;") {
            ('"', 6)
        } else if rest.starts_with("&apos;") {
            ('\'', 6)
        } else if let Some(reference) = rest.strip_prefix("&#") {
            let end = reference.find(';').ok_or(MachineProblem::DataException)?;
            let (digits, radix) = reference
                .get(..end)
                .and_then(|digits| {
                    digits
                        .strip_prefix(['x', 'X'])
                        .map(|digits| (digits, 16))
                        .or(Some((digits, 10)))
                })
                .ok_or(MachineProblem::DataException)?;
            let scalar = u32::from_str_radix(digits, radix)
                .ok()
                .and_then(char::from_u32)
                .filter(|character| xml_character_allowed(*character))
                .ok_or(MachineProblem::DataException)?;
            (scalar, end + 3)
        } else {
            return Err(MachineProblem::DataException);
        };
        output.push(decoded);
        rest = &rest[length..];
    }
    output.push_str(rest);
    Ok(output)
}

fn xml_character_allowed(character: char) -> bool {
    matches!(character, '\u{9}' | '\u{a}' | '\u{d}')
        || ('\u{20}'..='\u{d7ff}').contains(&character)
        || ('\u{e000}'..='\u{fffd}').contains(&character)
        || ('\u{10000}'..='\u{10ffff}').contains(&character)
}

fn xml_document(source: &str) -> Result<XmlNode, MachineProblem> {
    let (mut at, _) = xml_declaration(source)?;
    let node = xml_node(source, &mut at, 0)?;
    (at == source.len())
        .then_some(node)
        .ok_or(MachineProblem::DataException)
}

fn xml_declaration(source: &str) -> Result<(usize, Vec<XmlEvent>), MachineProblem> {
    if !source.starts_with("<?xml") {
        return Ok((0, Vec::new()));
    }
    let end = source.find("?>").ok_or(MachineProblem::DataException)?;
    let declaration = source.get(2..end).ok_or(MachineProblem::DataException)?;
    let (name, attributes) = xml_opening_tag(declaration)?;
    if name != "xml" {
        return Err(MachineProblem::DataException);
    }
    let mut events = Vec::new();
    let mut version = false;
    for (name, value) in attributes {
        let kind = match name.as_str() {
            "version" if matches!(value.as_str(), "1.0" | "1.1") => {
                version = true;
                "VERSION-INFORMATION"
            }
            "encoding" if !value.is_empty() => "ENCODING-DECLARATION",
            "standalone" if matches!(value.as_str(), "yes" | "no") => "STANDALONE-DECLARATION",
            _ => return Err(MachineProblem::DataException),
        };
        events.push(XmlEvent::new(kind, value.into_bytes()));
    }
    if !version {
        return Err(MachineProblem::DataException);
    }
    Ok((end + 2, events))
}

fn xml_node(source: &str, at: &mut usize, depth: usize) -> Result<XmlNode, MachineProblem> {
    if depth >= 64 || !source[*at..].starts_with('<') || source[*at..].starts_with("</") {
        return Err(MachineProblem::DataException);
    }
    let open_end = source[*at..]
        .find('>')
        .map(|offset| *at + offset)
        .ok_or(MachineProblem::DataException)?;
    let opening = source
        .get(*at + 1..open_end)
        .ok_or(MachineProblem::DataException)?;
    let empty = opening.trim_end().ends_with('/');
    let opening = if empty {
        opening
            .trim_end()
            .strip_suffix('/')
            .ok_or(MachineProblem::DataException)?
    } else {
        opening
    };
    let (name, attributes) = xml_opening_tag(opening)?;
    *at = open_end + 1;
    if empty {
        return Ok(XmlNode {
            name,
            attributes,
            text: String::new(),
            children: Vec::new(),
        });
    }
    let mut text = String::new();
    let mut children = Vec::new();
    loop {
        let rest = source.get(*at..).ok_or(MachineProblem::DataException)?;
        if rest.starts_with("</") {
            let close_end = rest.find('>').ok_or(MachineProblem::DataException)?;
            if rest.get(2..close_end) != Some(name.as_str()) {
                return Err(MachineProblem::DataException);
            }
            *at += close_end + 1;
            if !children.is_empty() && !text.trim().is_empty() {
                return Err(MachineProblem::DataException);
            }
            return Ok(XmlNode {
                name,
                attributes,
                text: xml_unescape(&text)?,
                children,
            });
        }
        if rest.starts_with('<') {
            children.push(xml_node(source, at, depth + 1)?);
            continue;
        }
        let next = rest.find('<').ok_or(MachineProblem::DataException)?;
        text.push_str(&rest[..next]);
        *at += next;
    }
}

fn xml_opening_tag(opening: &str) -> Result<(String, Vec<(String, String)>), MachineProblem> {
    let bytes = opening.as_bytes();
    let mut at = 0usize;
    let skip_space = |at: &mut usize| {
        while bytes.get(*at).is_some_and(u8::is_ascii_whitespace) {
            *at += 1;
        }
    };
    skip_space(&mut at);
    let name_start = at;
    while bytes.get(at).is_some_and(|byte| {
        !byte.is_ascii_whitespace() && !matches!(*byte, b'/' | b'=' | b'<' | b'>')
    }) {
        at += 1;
    }
    let name = opening
        .get(name_start..at)
        .filter(|name| !name.is_empty())
        .ok_or(MachineProblem::DataException)?
        .to_string();
    let mut attributes = Vec::new();
    let mut names = BTreeSet::new();
    loop {
        skip_space(&mut at);
        if at == bytes.len() {
            return Ok((name, attributes));
        }
        let attribute_start = at;
        while bytes.get(at).is_some_and(|byte| {
            !byte.is_ascii_whitespace() && !matches!(*byte, b'/' | b'=' | b'<' | b'>')
        }) {
            at += 1;
        }
        let attribute = opening
            .get(attribute_start..at)
            .filter(|attribute| !attribute.is_empty())
            .ok_or(MachineProblem::DataException)?
            .to_string();
        if !names.insert(attribute.clone()) || attributes.len() >= 4_096 {
            return Err(MachineProblem::DataException);
        }
        skip_space(&mut at);
        if bytes.get(at) != Some(&b'=') {
            return Err(MachineProblem::DataException);
        }
        at += 1;
        skip_space(&mut at);
        let quote = *bytes
            .get(at)
            .filter(|quote| matches!(quote, b'\'' | b'"'))
            .ok_or(MachineProblem::DataException)?;
        at += 1;
        let value_start = at;
        while bytes.get(at).is_some_and(|byte| *byte != quote) {
            if matches!(bytes[at], b'<' | b'>') {
                return Err(MachineProblem::DataException);
            }
            at += 1;
        }
        let value = opening
            .get(value_start..at)
            .ok_or(MachineProblem::DataException)?;
        if bytes.get(at) != Some(&quote) {
            return Err(MachineProblem::DataException);
        }
        at += 1;
        attributes.push((attribute, xml_unescape(value)?));
    }
}

fn xml_processing_target(args: &[String]) -> Option<&str> {
    position(args, "PROCESSING")
        .and_then(|at| {
            args.get(at + 1)
                .filter(|token| token.as_str() == "PROCEDURE")
        })
        .and_then(|_| position(args, "PROCESSING"))
        .and_then(|at| args.get(at + 2))
        .map(String::as_str)
}

const fn xml_state_key(pc: usize) -> usize {
    pc | (1usize << (usize::BITS - 1))
}

const fn out_of_line_perform_key(pc: usize) -> usize {
    pc | (1usize << (usize::BITS - 2))
}

fn declarative_state_key(pc: usize) -> String {
    format!("__COBOL_DECLARATIVE_RETURN_{pc}")
}

fn xml_document_events(source: &str) -> Result<Vec<XmlEvent>, MachineProblem> {
    let (_, declaration_events) = xml_declaration(source)?;
    let document = xml_document(source)?;
    let mut events = vec![XmlEvent::new("START-OF-DOCUMENT", Vec::new())];
    events.extend(declaration_events);
    let namespaces =
        BTreeMap::from([("xml".into(), "http://www.w3.org/XML/1998/namespace".into())]);
    append_xml_node_events(&document, &namespaces, &mut events)?;
    events.push(XmlEvent::new("END-OF-DOCUMENT", Vec::new()));
    Ok(events)
}

fn append_xml_node_events(
    node: &XmlNode,
    inherited_namespaces: &BTreeMap<String, String>,
    events: &mut Vec<XmlEvent>,
) -> Result<(), MachineProblem> {
    events
        .len()
        .checked_add(2usize.saturating_add(node.attributes.len().saturating_mul(2)))
        .filter(|count| *count <= 65_536)
        .ok_or(MachineProblem::ResourceExhausted)?;
    let mut namespaces = inherited_namespaces.clone();
    for (name, value) in &node.attributes {
        let prefix = if name == "xmlns" {
            Some("")
        } else {
            name.strip_prefix("xmlns:")
        };
        let Some(prefix) = prefix else {
            continue;
        };
        if prefix == "xmlns"
            || (prefix == "xml"
                && value != namespaces.get("xml").ok_or(MachineProblem::DataException)?)
        {
            return Err(MachineProblem::DataException);
        }
        if value.is_empty() {
            namespaces.remove(prefix);
        } else {
            namespaces.insert(prefix.into(), value.clone());
        }
        let mut event = XmlEvent::new("NAMESPACE-DECLARATION", Vec::new());
        event.namespace = value.as_bytes().to_vec();
        event.prefix = prefix.as_bytes().to_vec();
        events.push(event);
    }
    let (prefix, local) = split_xml_name(&node.name)?;
    let namespace = xml_namespace(&namespaces, prefix, true)?;
    let mut start = XmlEvent::new("START-OF-ELEMENT", local.as_bytes().to_vec());
    start.namespace = namespace.as_bytes().to_vec();
    start.prefix = prefix.as_bytes().to_vec();
    events.push(start);
    for (name, value) in &node.attributes {
        if name == "xmlns" || name.starts_with("xmlns:") {
            continue;
        }
        let (prefix, local) = split_xml_name(name)?;
        let namespace = xml_namespace(&namespaces, prefix, false)?;
        let mut attribute = XmlEvent::new("ATTRIBUTE-NAME", local.as_bytes().to_vec());
        attribute.namespace = namespace.as_bytes().to_vec();
        attribute.prefix = prefix.as_bytes().to_vec();
        events.push(attribute);
        events.push(XmlEvent::new(
            "ATTRIBUTE-CHARACTERS",
            value.as_bytes().to_vec(),
        ));
    }
    if !node.text.is_empty() {
        events.push(XmlEvent::new(
            "CONTENT-CHARACTERS",
            node.text.as_bytes().to_vec(),
        ));
    }
    for child in &node.children {
        append_xml_node_events(child, &namespaces, events)?;
    }
    let mut end = XmlEvent::new("END-OF-ELEMENT", local.as_bytes().to_vec());
    end.namespace = namespace.as_bytes().to_vec();
    end.prefix = prefix.as_bytes().to_vec();
    events.push(end);
    Ok(())
}

fn split_xml_name(name: &str) -> Result<(&str, &str), MachineProblem> {
    let mut parts = name.split(':');
    let first = parts.next().ok_or(MachineProblem::DataException)?;
    let second = parts.next();
    if first.is_empty() || parts.next().is_some() || second.is_some_and(str::is_empty) {
        return Err(MachineProblem::DataException);
    }
    Ok(second.map_or(("", first), |local| (first, local)))
}

fn xml_namespace<'a>(
    namespaces: &'a BTreeMap<String, String>,
    prefix: &str,
    default_for_unprefixed: bool,
) -> Result<&'a str, MachineProblem> {
    if prefix.is_empty() && !default_for_unprefixed {
        return Ok("");
    }
    namespaces
        .get(prefix)
        .map(String::as_str)
        .or_else(|| prefix.is_empty().then_some(""))
        .ok_or(MachineProblem::DataException)
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

fn day_of_integer(value: i128) -> Result<Decimal, MachineProblem> {
    let value = i64::try_from(value).map_err(|_| MachineProblem::DataException)?;
    let (year, month, day) = cobol_date_of_integer(value)?;
    let ordinal = days_from_civil(year, month, day) - days_from_civil(year, 1, 1) + 1;
    Ok(Decimal {
        coefficient: i128::from(year) * 1_000 + i128::from(ordinal),
        scale: 0,
    })
}

fn integer_of_day(value: i128) -> Result<Decimal, MachineProblem> {
    let year = i32::try_from(value / 1_000).map_err(|_| MachineProblem::DataException)?;
    let ordinal = i64::try_from(value % 1_000).map_err(|_| MachineProblem::DataException)?;
    let maximum = if valid_date(year, 2, 29) { 366 } else { 365 };
    if !(1..=maximum).contains(&ordinal) {
        return Err(MachineProblem::DataException);
    }
    let days = days_from_civil(year, 1, 1) + ordinal - 1;
    let (year, month, day) = civil_from_days(days);
    Ok(Decimal {
        coefficient: i128::from(cobol_integer_of_date(year, month, day)?),
        scale: 0,
    })
}

fn current_year(current_date: &[u8]) -> Result<i128, MachineProblem> {
    let year = current_date
        .get(..4)
        .and_then(|value| std::str::from_utf8(value).ok())
        .and_then(|value| value.parse::<i128>().ok())
        .ok_or(MachineProblem::DataException)?;
    (1_601..=9_999)
        .contains(&year)
        .then_some(year)
        .ok_or(MachineProblem::DataException)
}

fn windowed_year(
    value: i128,
    offset: i128,
    current_year: i128,
    trailing_digits: u32,
) -> Result<CobolValue, MachineProblem> {
    let divisor = 10i128
        .checked_pow(trailing_digits)
        .ok_or(MachineProblem::SizeError)?;
    if value < 0 {
        return Err(MachineProblem::DataException);
    }
    let short_year = value / divisor;
    if !(0..=99).contains(&short_year) {
        return Err(MachineProblem::DataException);
    }
    let ending_year = current_year
        .checked_add(offset)
        .filter(|year| (1_700..=9_999).contains(year))
        .ok_or(MachineProblem::DataException)?;
    let mut year = (ending_year / 100) * 100 + short_year;
    if year > ending_year {
        year -= 100;
    }
    Ok(integer_value(year * divisor + value % divisor))
}

fn test_date_yyyymmdd(value: i128) -> i128 {
    if !(16_010_000..=99_999_999).contains(&value) {
        return 1;
    }
    let year = (value / 10_000) as i32;
    let month_day = value % 10_000;
    if !(100..=1_299).contains(&month_day) {
        return 2;
    }
    let month = (month_day / 100) as u32;
    let day = (month_day % 100) as u32;
    if !valid_date(year, month, day) {
        return 3;
    }
    0
}

fn test_day_yyyyddd(value: i128) -> i128 {
    if !(1_601_000..=9_999_999).contains(&value) {
        return 1;
    }
    let year = (value / 1_000) as i32;
    let day = value % 1_000;
    let maximum = if valid_date(year, 2, 29) { 366 } else { 365 };
    if !(1..=maximum).contains(&day) {
        return 2;
    }
    0
}

fn parse_hhmmss(bytes: &[u8]) -> Result<u32, MachineProblem> {
    if bytes.len() != 6 || !bytes.iter().all(u8::is_ascii_digit) {
        return Err(MachineProblem::DataException);
    }
    let value = std::str::from_utf8(bytes)
        .map_err(|_| MachineProblem::DataException)?
        .parse::<u32>()
        .map_err(|_| MachineProblem::DataException)?;
    let hour = value / 10_000;
    let minute = value / 100 % 100;
    let second = value % 100;
    if hour > 23 || minute > 59 || second > 59 {
        return Err(MachineProblem::DataException);
    }
    Ok(hour * 3_600 + minute * 60 + second)
}

fn render_datetime_format(
    format: &str,
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
) -> Result<Vec<u8>, MachineProblem> {
    if !valid_date(year, month, day) || hour > 23 || minute > 59 || second > 59 {
        return Err(MachineProblem::DataException);
    }
    let mut output = format.to_string();
    for (token, value) in [
        ("YYYY", format!("{year:04}")),
        ("YY", format!("{:02}", year.rem_euclid(100))),
        ("MM", format!("{month:02}")),
        ("DD", format!("{day:02}")),
        ("hh", format!("{hour:02}")),
        ("mm", format!("{minute:02}")),
        ("ss", format!("{second:02}")),
    ] {
        output = output.replace(token, &value);
    }
    Ok(output.into_bytes())
}

fn format_datetime(format: &str, current: &[u8]) -> Result<Vec<u8>, MachineProblem> {
    if current.len() != 21 {
        return Err(MachineProblem::DataException);
    }
    let (year, month, day) = split_yyyymmdd(
        std::str::from_utf8(&current[..8])
            .map_err(|_| MachineProblem::DataException)?
            .parse()
            .map_err(|_| MachineProblem::DataException)?,
    )?;
    let seconds = parse_hhmmss(&current[8..14])?;
    render_datetime_format(
        format,
        year,
        month,
        day,
        seconds / 3_600,
        seconds / 60 % 60,
        seconds % 60,
    )
}

fn formatted_date(format: &str, date: i128) -> Result<Vec<u8>, MachineProblem> {
    let date = i64::try_from(date).map_err(|_| MachineProblem::DataException)?;
    let (year, month, day) = cobol_date_of_integer(date)?;
    render_datetime_format(format, year, month, day, 0, 0, 0)
}

fn formatted_datetime(
    format: &str,
    date: i128,
    time: Decimal,
    _offset: i128,
) -> Result<Vec<u8>, MachineProblem> {
    let date = i64::try_from(date).map_err(|_| MachineProblem::DataException)?;
    let (year, month, day) = cobol_date_of_integer(date)?;
    let seconds = decimal_f64(time)?;
    if !(0.0..86_400.0).contains(&seconds) {
        return Err(MachineProblem::DataException);
    }
    let seconds = seconds as u32;
    render_datetime_format(
        format,
        year,
        month,
        day,
        seconds / 3_600,
        seconds / 60 % 60,
        seconds % 60,
    )
}

fn formatted_time(format: &str, time: Decimal, _offset: i128) -> Result<Vec<u8>, MachineProblem> {
    let seconds = decimal_f64(time)?;
    if !(0.0..86_400.0).contains(&seconds) {
        return Err(MachineProblem::DataException);
    }
    let seconds = seconds as u32;
    render_datetime_format(
        format,
        1_600,
        1,
        1,
        seconds / 3_600,
        seconds / 60 % 60,
        seconds % 60,
    )
}

fn digits_only(text: &str) -> String {
    text.chars().filter(char::is_ascii_digit).collect()
}

fn integer_of_formatted_date(format: &str, value: &str) -> Result<Decimal, MachineProblem> {
    if !format.contains("YYYY") || !format.contains("MM") || !format.contains("DD") {
        return Err(MachineProblem::DataException);
    }
    let digits = digits_only(value);
    if digits.len() != 8 {
        return Err(MachineProblem::DataException);
    }
    let date = digits
        .parse::<i128>()
        .map_err(|_| MachineProblem::DataException)?;
    let (year, month, day) = split_yyyymmdd(date)?;
    Ok(Decimal {
        coefficient: i128::from(cobol_integer_of_date(year, month, day)?),
        scale: 0,
    })
}

fn test_formatted_datetime(format: &str, value: &str) -> usize {
    #[derive(Clone, Copy)]
    enum Field {
        Year,
        ShortYear,
        Month,
        Day,
        Hour,
        Minute,
        Second,
    }

    let format = format.as_bytes();
    let value = value.as_bytes();
    let mut format_at = 0usize;
    let mut value_at = 0usize;
    let mut fields = Vec::new();
    while format_at < format.len() {
        let field = [
            (b"YYYY".as_slice(), 4usize, Field::Year),
            (b"YY".as_slice(), 2, Field::ShortYear),
            (b"MM".as_slice(), 2, Field::Month),
            (b"DD".as_slice(), 2, Field::Day),
            (b"hh".as_slice(), 2, Field::Hour),
            (b"mm".as_slice(), 2, Field::Minute),
            (b"ss".as_slice(), 2, Field::Second),
        ]
        .into_iter()
        .find_map(|(token, width, field)| {
            format[format_at..]
                .starts_with(token)
                .then_some((token.len(), width, field))
        });
        if let Some((token_length, width, field)) = field {
            for offset in 0..width {
                if value
                    .get(value_at + offset)
                    .is_none_or(|byte| !byte.is_ascii_digit())
                {
                    return value_at + offset + 1;
                }
            }
            fields.push((field, value_at, width));
            format_at += token_length;
            value_at += width;
        } else {
            if value.get(value_at) != format.get(format_at) {
                return value_at + 1;
            }
            format_at += 1;
            value_at += 1;
        }
    }
    if value_at != value.len() {
        return value_at + 1;
    }
    let value_of = |wanted: fn(Field) -> bool| {
        fields
            .iter()
            .find(|(field, _, _)| wanted(*field))
            .and_then(|(_, start, width)| value.get(*start..start + width))
            .and_then(|digits| std::str::from_utf8(digits).ok())
            .and_then(|digits| digits.parse::<u32>().ok())
    };
    let year = value_of(|field| matches!(field, Field::Year));
    let month = value_of(|field| matches!(field, Field::Month));
    let day_limit = match (year, month) {
        (Some(year), Some(month)) if (1_601..=9_999).contains(&year) => {
            days_in_month(year as i32, month).unwrap_or(31)
        }
        _ => 31,
    };
    fields
        .into_iter()
        .filter_map(|(field, start, width)| {
            let range = match field {
                Field::Year => Some((1_601, 9_999)),
                Field::ShortYear => None,
                Field::Month => Some((1, 12)),
                Field::Day => Some((1, day_limit)),
                Field::Hour => Some((0, 23)),
                Field::Minute | Field::Second => Some((0, 59)),
            }?;
            first_range_error(&value[start..start + width], start, range.0, range.1)
        })
        .min()
        .unwrap_or(0)
}

fn first_range_error(digits: &[u8], start: usize, minimum: u32, maximum: u32) -> Option<usize> {
    let width = digits.len();
    for consumed in 1..=width {
        let prefix = std::str::from_utf8(&digits[..consumed])
            .ok()?
            .parse::<u32>()
            .ok()?;
        let factor = 10u32.checked_pow((width - consumed) as u32)?;
        let possible_minimum = prefix.checked_mul(factor)?;
        let possible_maximum = possible_minimum.checked_add(factor - 1)?;
        if possible_maximum < minimum || possible_minimum > maximum {
            return Some(start + consumed);
        }
    }
    None
}

fn days_in_month(year: i32, month: u32) -> Option<u32> {
    (1..=12).contains(&month).then(|| match month {
        2 if valid_date(year, 2, 29) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    })
}

fn seconds_from_formatted_time(format: &str, value: &str) -> Result<Decimal, MachineProblem> {
    if !format.contains("hh") {
        return Err(MachineProblem::DataException);
    }
    let digits = digits_only(value);
    let time = digits
        .get(digits.len().saturating_sub(6)..)
        .ok_or(MachineProblem::DataException)?;
    Ok(Decimal {
        coefficient: i128::from(parse_hhmmss(time.as_bytes())?),
        scale: 0,
    })
}

fn accept_clock_value(format: AcceptClockFormat, value: &str) -> Result<Vec<u8>, MachineProblem> {
    match format {
        AcceptClockFormat::Time => {
            if value.len() != 9 || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(MachineProblem::UnexpectedHostResult);
            }
            Ok(value.as_bytes()[..8].to_vec())
        }
        _ => {
            if value.len() != 8 || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(MachineProblem::UnexpectedHostResult);
            }
            let numeric = value
                .parse::<i128>()
                .map_err(|_| MachineProblem::UnexpectedHostResult)?;
            let (year, month, day) = split_yyyymmdd(numeric)?;
            let ordinal = days_from_civil(year, month, day) - days_from_civil(year, 1, 1) + 1;
            Ok(match format {
                AcceptClockFormat::DateYymmdd => value.as_bytes()[2..].to_vec(),
                AcceptClockFormat::DateYyyymmdd => value.as_bytes().to_vec(),
                AcceptClockFormat::DayYyddd => {
                    format!("{:02}{ordinal:03}", year.rem_euclid(100)).into_bytes()
                }
                AcceptClockFormat::DayYyyyddd => format!("{year:04}{ordinal:03}").into_bytes(),
                AcceptClockFormat::DayOfWeek => {
                    let weekday = (days_from_civil(year, month, day) + 3).rem_euclid(7) + 1;
                    weekday.to_string().into_bytes()
                }
                AcceptClockFormat::Time => unreachable!(),
            })
        }
    }
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
    if !matches!(
        payload.schema(),
        "mainframe-env.cobol.call@1" | "mainframe-env.cobol.batch-main@1"
    ) {
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
    fn runtime_condition(&self) -> Option<Condition> {
        let (name, response) = match self {
            Self::SizeError => ("SIZE-ERROR", 1),
            Self::DataException => ("DATA-EXCEPTION", 2),
            Self::SubscriptError => ("SUBSCRIPT-ERROR", 3),
            Self::ReferenceModificationError => ("REFERENCE-MODIFICATION-ERROR", 4),
            _ => return None,
        };
        Some(Condition {
            name: name.into(),
            response,
            response2: 0,
            handled: false,
        })
    }

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

    fn divide_rounding_machine() -> ReferenceMachine {
        let mut machine =
            ReferenceMachine::from_binary(&binary(), invocation(), CodecLimits::default()).unwrap();
        for name in ["R", "SRC", "Q", "REM"] {
            let base = machine.bases.len();
            machine.bases.push(vec![b'0'; 9]);
            machine.views.insert(
                name.into(),
                StorageView {
                    base,
                    offset: 0,
                    length: 9,
                },
            );
            machine.layouts.insert(
                name.into(),
                LayoutMetadata {
                    name: name.into(),
                    simple_name: name.into(),
                    category: LayoutCategory::NumericDisplay,
                    picture: "S9(7)V99".into(),
                    digits: 9,
                    scale: 2,
                    native_binary: false,
                    signed: true,
                    sign_separate: false,
                    justified_right: false,
                    blank_when_zero: false,
                    linkage: false,
                    offset: 0,
                    length: 9,
                    element_length: 9,
                    occurs: 1,
                    occurs_min: 1,
                    unbounded: false,
                    depending_on: None,
                    indexes: Vec::new(),
                    keys: Vec::new(),
                    dynamic: false,
                    dynamic_limit: 0,
                    parent: None,
                    alias_of: None,
                    occurs_clause: false,
                    condition_values: Vec::new(),
                    object_class: None,
                },
            );
        }
        machine
    }

    #[test]
    fn divide_rounded_forms_preserve_guard_digit_and_truncated_remainder() {
        for (case, dividend, divisor, expected, expected_remainder) in [
            ("positive tie", 9585, 2, 4793, 1),
            ("negative tie", -9585, 2, -4793, -1),
            ("negative divisor tie", 9585, -2, -4793, 1),
            ("positive non-tie", 9584, 3, 3195, 2),
            ("negative non-tie", -9584, 3, -3195, -2),
        ] {
            for form in [
                "into",
                "into giving",
                "by giving",
                "into giving remainder",
                "by giving remainder",
                "compute",
            ] {
                let mut machine = divide_rounding_machine();
                let source = Decimal {
                    coefficient: dividend,
                    scale: 2,
                };
                machine.write_decimal("R", source).unwrap();
                machine.write_decimal("SRC", source).unwrap();
                let divisor = divisor.to_string();
                let tokens: Vec<String> = match form {
                    "into" => vec![divisor.as_str(), "INTO", "R", "ROUNDED"],
                    "into giving" => {
                        vec![divisor.as_str(), "INTO", "SRC", "GIVING", "Q", "ROUNDED"]
                    }
                    "by giving" => vec!["SRC", "BY", divisor.as_str(), "GIVING", "Q", "ROUNDED"],
                    "into giving remainder" => vec![
                        divisor.as_str(),
                        "INTO",
                        "SRC",
                        "GIVING",
                        "Q",
                        "ROUNDED",
                        "REMAINDER",
                        "REM",
                    ],
                    "by giving remainder" => vec![
                        "SRC",
                        "BY",
                        divisor.as_str(),
                        "GIVING",
                        "Q",
                        "ROUNDED",
                        "REMAINDER",
                        "REM",
                    ],
                    "compute" => vec!["Q", "ROUNDED", "=", "SRC", "/", divisor.as_str()],
                    _ => unreachable!(),
                }
                .into_iter()
                .map(str::to_string)
                .collect();
                if form == "compute" {
                    machine.arithmetic("compute", &tokens, false).unwrap();
                } else {
                    machine.divide_statement(&tokens, false).unwrap();
                }
                let target = if form == "into" { "R" } else { "Q" };
                assert_eq!(
                    machine.decimal(target).unwrap().coefficient,
                    expected,
                    "{case}: {form} quotient"
                );
                if form.contains("remainder") {
                    assert_eq!(
                        machine.decimal("REM").unwrap().coefficient,
                        expected_remainder,
                        "{case}: {form} remainder"
                    );
                }
            }
        }
    }
    pub(super) fn invocation() -> Invocation {
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
    pub(super) fn binary() -> Vec<u8> {
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
    fn entry_argument_metadata_rejects_legacy_and_preserves_no_using_batch_entry() {
        let mut legacy = invocation();
        legacy.bindings.insert(
            "cobol.call.arguments".into(),
            encode_call_values(&["ARG".into()], &[b"X".to_vec()]).unwrap(),
        );
        assert!(matches!(
            ReferenceMachine::from_binary(&binary(), legacy, CodecLimits::default()),
            Err(MachineProblem::InvalidArtifact(detail)) if detail == "missing entry_formals_v1"
        ));

        let mut builder = ModuleBuilder::new(IrLimits::default());
        let region = builder.add_region().unwrap();
        let block = builder.add_block(region).unwrap();
        builder
            .add_operation(
                block,
                OperationIdentity::new(NAMESPACE, "config", 1).unwrap(),
                Vec::new(),
                0,
                BTreeMap::from([
                    ("arithmetic_mode".into(), Attribute::Text("extended".into())),
                    ("entry_formals_v1".into(), Attribute::Text(String::new())),
                ]),
                Vec::new(),
                Vec::new(),
                None,
            )
            .unwrap();
        builder
            .add_operation(
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
        let encoded =
            mainframe_env_ir::encode_binary(&builder.finish().unwrap(), CodecLimits::default())
                .unwrap();
        let mut batch = invocation();
        let payload = encode_call_values(&["PARM".into()], &[b"2022071800".to_vec()]).unwrap();
        batch.bindings.insert(
            "cobol.call.arguments".into(),
            BoundedPayload::new(
                "mainframe-env.cobol.batch-main@1",
                payload.bytes().to_vec(),
                InvocationLimits::default(),
            )
            .unwrap(),
        );
        assert!(
            ReferenceMachine::from_binary(&binary(), batch.clone(), CodecLimits::default()).is_ok()
        );
        let machine =
            ReferenceMachine::from_binary(&encoded, batch, CodecLimits::default()).unwrap();
        assert!(machine.linkage_values().unwrap().is_empty());
    }
    fn display_fixture() -> ReferenceMachine {
        let mut machine =
            ReferenceMachine::from_binary(&binary(), invocation(), CodecLimits::default()).unwrap();
        fn add(machine: &mut ReferenceMachine, name: &str, bytes: &[u8], occurs: usize) {
            let simple_name = name.split('.').next_back().unwrap().to_string();
            let layout = LayoutMetadata {
                name: name.into(),
                simple_name: simple_name.clone(),
                category: LayoutCategory::NumericDisplay,
                picture: "9(11)".into(),
                digits: 11,
                scale: 0,
                native_binary: false,
                signed: false,
                sign_separate: false,
                justified_right: false,
                blank_when_zero: false,
                linkage: false,
                offset: 0,
                length: bytes.len(),
                element_length: bytes.len() / occurs,
                occurs,
                occurs_min: 1,
                unbounded: false,
                depending_on: None,
                indexes: Vec::new(),
                keys: Vec::new(),
                dynamic: false,
                dynamic_limit: 0,
                parent: name.split_once('.').map(|(parent, _)| parent.into()),
                alias_of: None,
                occurs_clause: occurs > 1,
                condition_values: Vec::new(),
                object_class: None,
            };
            let base = machine.bases.len();
            machine.bases.push(bytes.to_vec());
            machine.views.insert(
                name.into(),
                StorageView {
                    base,
                    offset: 0,
                    length: bytes.len(),
                },
            );
            machine.layouts.insert(name.into(), layout);
            machine
                .simple_layouts
                .entry(simple_name)
                .or_default()
                .push(name.into());
        }
        add(&mut machine, "ACCT-ID", b"00000000042", 1);
        add(&mut machine, "OTHER-ID", b"00000000007", 1);
        add(&mut machine, "REC.ITEM", b"00000000009", 1);
        add(&mut machine, "TABLE-ITEM", b"0000000000100000000002", 2);
        machine
    }

    fn display_operands(machine: &mut ReferenceMachine, operands: &[&str]) -> Vec<u8> {
        let mut operation = machine
            .operations
            .iter()
            .find(|op| op.identity.name() == "display")
            .unwrap()
            .clone();
        let args = operands
            .iter()
            .map(|arg| (*arg).to_string())
            .collect::<Vec<_>>();
        let mut encoded = Vec::new();
        for arg in args {
            encoded.extend_from_slice(&(arg.len() as u64).to_be_bytes());
            encoded.extend_from_slice(arg.as_bytes());
        }
        operation
            .attributes
            .insert("arguments".into(), Attribute::Bytes(encoded));
        machine.output.clear();
        machine.execute(&operation).unwrap();
        machine.output.clone()
    }

    #[test]
    fn display_keeps_leading_literals_before_identifiers() {
        let machine = &mut display_fixture();
        for (tokens, expected) in [
            (
                vec!["'ACCT-ID                 :'", "ACCT-ID"],
                "ACCT-ID                 :00000000042\n",
            ),
            (
                vec!["'LABEL                   :'", "ACCT-ID"],
                "LABEL                   :00000000042\n",
            ),
            (vec!["'ACCT-ID'", "OTHER-ID"], "ACCT-ID00000000007\n"),
            (
                vec!["'ACCT-ID'", "OTHER-ID", "'END'"],
                "ACCT-ID00000000007END\n",
            ),
            (
                vec!["'ACCT-ID OF (: '", "OTHER-ID"],
                "ACCT-ID OF (: 00000000007\n",
            ),
        ] {
            assert_eq!(
                display_operands(machine, &tokens),
                expected.as_bytes(),
                "{tokens:?}"
            );
        }
    }

    #[test]
    fn quoted_literals_never_resolve_as_data_references() {
        let machine = display_fixture();
        for literal in [
            "'ACCT-ID'",
            "\"ACCT-ID\"",
            "'ACCT-ID                 :'",
            "'ACCT-ID.'",
        ] {
            assert!(
                machine.layout_qualified(&[literal.to_string()]).is_none(),
                "{literal} resolved as a data reference"
            );
        }
        assert!(machine.layout_qualified(&["ACCT-ID".to_string()]).is_some());
    }

    #[test]
    fn display_preserves_qualified_modified_and_subscripted_references() {
        let machine = &mut display_fixture();
        assert_eq!(
            display_operands(machine, &["'X'", "ITEM", "OF", "REC"]),
            b"X00000000009\n"
        );
        assert_eq!(
            display_operands(machine, &["'X'", "ITEM", "IN", "REC"]),
            b"X00000000009\n"
        );
        assert_eq!(
            display_operands(machine, &["'X'", "ACCT-ID", "(", "10:2", ")"]),
            b"X42\n"
        );
        assert_eq!(
            display_operands(machine, &["'X'", "TABLE-ITEM", "(", "2", ")"]),
            b"X00000000002\n"
        );
    }

    #[test]
    fn display_keeps_special_operands_and_output_phrases_after_literals() {
        let machine = &mut display_fixture();
        assert_eq!(
            display_operands(machine, &["'X'", "LENGTH", "OF", "ACCT-ID"]),
            b"X11\n"
        );
        assert_eq!(
            display_operands(
                machine,
                &["'X'", "FUNCTION", "UPPER-CASE", "(", "'ab'", ")"]
            ),
            b"XAB\n"
        );
        assert_eq!(
            display_operands(machine, &["'X'", "SPACE", "ZERO", "ALL", "'x'"]),
            b"X 0x\n"
        );
        assert_eq!(
            display_operands(
                machine,
                &[
                    "'X'",
                    "ACCT-ID",
                    "UPON",
                    "SYSOUT",
                    "WITH",
                    "NO",
                    "ADVANCING"
                ]
            ),
            b"X00000000042"
        );
        let address = display_operands(machine, &["'X'", "ADDRESS", "OF", "ACCT-ID"]);
        assert_eq!(address.len(), 10);
        assert_eq!(address[0], b'X');
        assert_eq!(address[9], b'\n');
    }
    #[test]
    fn sequential_empty_browse_maps_to_open_read_close_statuses() {
        use mainframe_env_host_api::{DatasetResult, HostResult, KeyRelation};

        let mut machine =
            ReferenceMachine::from_binary(&binary(), invocation(), CodecLimits::default()).unwrap();
        machine.files.insert(
            "INPUT-FILE".into(),
            FileMetadata {
                assignment: "USER.EMPTY.G0001V00".into(),
                record_name: None,
                record_names: Vec::new(),
                organization: "SEQUENTIAL".into(),
                access_mode: "SEQUENTIAL".into(),
                record_key: None,
                alternate_record_keys: Vec::new(),
                relative_key: None,
                file_status: None,
                sort_merge: false,
                description: String::new(),
                record_min: None,
                record_max: None,
                ccsid: None,
                linage: None,
            },
        );
        let file = "INPUT-FILE".to_string();
        let Step::Effect(open) = machine
            .dataset_effect("open", &["INPUT".into(), file.clone()])
            .unwrap()
        else {
            panic!("OPEN INPUT did not call the dataset provider");
        };
        assert!(matches!(
            open.request,
            HostRequest::Dataset(DatasetRequest::StartBrowse {
                ref key,
                relation: KeyRelation::GreaterOrEqual,
                ..
            }) if key.is_empty()
        ));
        let cursor = "empty-cursor".to_string();
        machine
            .resume_host(EffectResult {
                sequence: open.sequence,
                outcome: Ok(HostResult::Dataset(DatasetResult::Browse {
                    cursor: cursor.clone(),
                    record: None,
                    identity: None,
                    key: None,
                })),
            })
            .unwrap();
        assert_eq!(machine.last_file_status, "00");
        let Step::Effect(read) = machine.dataset_effect("read", &[file.clone()]).unwrap() else {
            panic!("READ did not call the dataset provider");
        };
        assert!(matches!(
            read.request,
            HostRequest::Dataset(DatasetRequest::ReadNext { ref cursor, .. })
                if cursor == "empty-cursor"
        ));
        machine
            .resume_host(EffectResult {
                sequence: read.sequence,
                outcome: Ok(HostResult::Dataset(DatasetResult::Browse {
                    cursor: cursor.clone(),
                    record: None,
                    identity: None,
                    key: None,
                })),
            })
            .unwrap();
        assert_eq!(machine.last_file_status, "10");
        let Step::Effect(close) = machine.dataset_effect("close", &[file]).unwrap() else {
            panic!("CLOSE did not call the dataset provider");
        };
        assert!(matches!(
            close.request,
            HostRequest::Dataset(DatasetRequest::Close { ref cursor, .. })
                if cursor.as_deref() == Some("empty-cursor")
        ));
        machine
            .resume_host(EffectResult {
                sequence: close.sequence,
                outcome: Ok(HostResult::Dataset(DatasetResult::Browse {
                    cursor,
                    record: None,
                    identity: None,
                    key: None,
                })),
            })
            .unwrap();
        assert_eq!(machine.last_file_status, "00");
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
    fn integer_evaluates_parenthesized_arithmetic_argument() {
        let machine =
            ReferenceMachine::from_binary(&binary(), invocation(), CodecLimits::default()).unwrap();
        let tokens = [
            "FUNCTION", "INTEGER", "(", "(", "10", "*", "2", ")", "+", "1", ")",
        ]
        .map(str::to_string);
        assert!(matches!(
            machine.eval_value(&tokens),
            Ok(CobolValue::Decimal(Decimal {
                coefficient: 21,
                scale: 0
            }))
        ));
    }
    #[test]
    fn list_intrinsics_evaluate_arithmetic_arguments() {
        let mut machine =
            ReferenceMachine::from_binary(&binary(), invocation(), CodecLimits::default()).unwrap();
        machine.implicit.insert("A".into(), integer_value(7));
        machine.implicit.insert("B".into(), integer_value(-3));
        for (tokens, expected) in [
            ("FUNCTION MIN ( A , ( B * 4 ) )", -12),
            ("FUNCTION MAX ( ( A - 10 ) , B * 2 , 1 )", 1),
            ("FUNCTION SUM ( A , B * 4 , 2 )", -3),
        ] {
            let tokens = tokens
                .split_whitespace()
                .map(str::to_string)
                .collect::<Vec<_>>();
            assert!(
                matches!(
                    machine.eval_value(&tokens),
                    Ok(CobolValue::Decimal(Decimal { coefficient, scale: 0 })) if coefficient == expected
                ),
                "{tokens:?}"
            );
        }
    }
    #[test]
    fn write_without_from_uses_record_name_and_preserves_fixed_length() {
        let mut builder = ModuleBuilder::new(IrLimits::default());
        for (name, length) in [("REC", 5), ("WS-ITEM", 5), ("FILE-STATUS", 2)] {
            builder.add_storage(name, length, None).unwrap();
        }
        let region = builder.add_region().unwrap();
        let block = builder.add_block(region).unwrap();
        builder
            .add_operation(
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
        let binary =
            mainframe_env_ir::encode_binary(&builder.finish().unwrap(), CodecLimits::default())
                .unwrap();
        let mut machine =
            ReferenceMachine::from_binary(&binary, invocation(), CodecLimits::default()).unwrap();
        machine.files.insert(
            "TESTFILE".into(),
            FileMetadata {
                assignment: "TESTFILE".into(),
                record_name: Some("REC".into()),
                record_names: vec!["REC".into()],
                organization: "SEQUENTIAL".into(),
                access_mode: "SEQUENTIAL".into(),
                record_key: None,
                alternate_record_keys: Vec::new(),
                relative_key: None,
                file_status: Some("FILE-STATUS".into()),
                sort_merge: false,
                description: String::new(),
                record_min: Some(5),
                record_max: Some(5),
                ccsid: None,
                linage: Some(10),
            },
        );
        machine.write_raw("REC", b"REC01").unwrap();
        machine.write_raw("WS-ITEM", b"FROM2").unwrap();
        for (args, expected) in [
            (vec!["REC"], b"REC01".as_slice()),
            (vec!["REC", "FROM", "WS-ITEM"], b"FROM2".as_slice()),
            (
                vec!["REC", "AFTER", "ADVANCING", "1", "LINE"],
                b"REC01".as_slice(),
            ),
            (vec!["REC", "INVALID", "KEY"], b"REC01".as_slice()),
        ] {
            machine.write_raw("REC", b"REC01").unwrap();
            let args = args.into_iter().map(str::to_string).collect::<Vec<_>>();
            let step = machine.dataset_effect("write", &args).unwrap();
            let Step::Effect(effect) = step else {
                panic!("WRITE did not request a dataset effect");
            };
            let HostRequest::Dataset(DatasetRequest::Append { records, .. }) = &effect.request
            else {
                panic!("WRITE did not append a sequential record");
            };
            assert_eq!(records, &vec![expected.to_vec()]);
            if args.iter().any(|arg| arg == "FROM") {
                assert_eq!(machine.resolve("REC").unwrap(), b"FROM2");
            }
            machine
                .resume_host(EffectResult {
                    sequence: effect.sequence,
                    outcome: Ok(HostResult::Dataset(
                        mainframe_env_host_api::DatasetResult::Mutated { version: 1 },
                    )),
                })
                .unwrap();
            assert_eq!(machine.resolve("FILE-STATUS").unwrap(), b"00");
        }
    }
    #[test]
    fn quoted_numeric_literal_move_zero_fills_numeric_display() {
        let mut machine =
            ReferenceMachine::from_binary(&binary(), invocation(), CodecLimits::default()).unwrap();
        let layout = |name: &str, category, length, scale, signed| LayoutMetadata {
            name: name.into(),
            simple_name: name.into(),
            category,
            picture: String::new(),
            digits: length,
            scale,
            native_binary: false,
            signed,
            sign_separate: false,
            justified_right: false,
            blank_when_zero: false,
            linkage: false,
            offset: 0,
            length,
            element_length: length,
            occurs: 1,
            occurs_min: 1,
            unbounded: false,
            depending_on: None,
            indexes: Vec::new(),
            keys: Vec::new(),
            dynamic: false,
            dynamic_limit: 0,
            parent: None,
            alias_of: None,
            occurs_clause: false,
            condition_values: Vec::new(),
            object_class: None,
        };
        for (name, category, length, scale, signed) in [
            ("N4", LayoutCategory::NumericDisplay, 4, 0, false),
            ("SIGNED4", LayoutCategory::NumericDisplay, 4, 0, true),
            (
                "SIGNED4-SEPARATE",
                LayoutCategory::NumericDisplay,
                5,
                0,
                true,
            ),
            ("N5", LayoutCategory::NumericDisplay, 5, 2, false),
            ("SEND-UNSIGNED", LayoutCategory::NumericDisplay, 2, 0, false),
            ("SEND-ALPHA", LayoutCategory::Alphanumeric, 2, 0, false),
        ] {
            let mut view = machine.views["MSG"].clone();
            view.length = length;
            machine.views.insert(name.into(), view);
            let mut item = layout(name, category, length, scale, signed);
            if name == "SIGNED4-SEPARATE" {
                item.digits = 4;
                item.sign_separate = true;
            }
            machine.layouts.insert(name.into(), item);
        }
        let move_to = |machine: &mut ReferenceMachine, source: &str, receiver: &str| {
            machine
                .move_op(&[source.into(), "TO".into(), receiver.into()])
                .unwrap();
            machine.read(receiver).unwrap()
        };
        assert_eq!(move_to(&mut machine, "'05'", "N4"), b"0005");
        assert_eq!(move_to(&mut machine, "'123456'", "N4"), b"3456");
        let numeric_signed = move_to(&mut machine, "5", "SIGNED4");
        assert_eq!(numeric_signed, b"000E");
        assert_eq!(move_to(&mut machine, "'05'", "SIGNED4"), numeric_signed);
        assert_eq!(move_to(&mut machine, "'12'", "N5"), b"01200");
        let source = machine.reference(&["SEND-UNSIGNED".into()]).unwrap();
        machine.write_reference(&source, b"05").unwrap();
        assert_eq!(
            move_to(&mut machine, "SEND-UNSIGNED", "SIGNED4-SEPARATE"),
            b"0005+"
        );
        let source = machine.reference(&["SEND-ALPHA".into()]).unwrap();
        machine.write_reference(&source, b"05").unwrap();
        assert_eq!(move_to(&mut machine, "SEND-ALPHA", "N4"), b"0005");
        machine.write_reference(&source, b"05").unwrap();
        assert_eq!(move_to(&mut machine, "SEND-ALPHA", "SIGNED4"), b"000E");
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
        let test_invocation = invocation();
        let mut first = ReferenceMachine::from_binary(
            &binary(),
            test_invocation.clone(),
            CodecLimits::default(),
        )
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
        first.random_state.set(Some(42));
        let checkpoint = first.checkpoint().unwrap();
        assert_eq!(
            checkpoint.schema(),
            "mainframe-env.reference-machine-checkpoint@12"
        );
        let mut restored =
            ReferenceMachine::from_binary(&binary(), test_invocation, CodecLimits::default())
                .unwrap();
        restored.restore_checkpoint(&checkpoint).unwrap();
        assert_eq!(restored.last_file_status, "10");
        assert_eq!(restored.condition_status, first.condition_status);
        assert_eq!(restored.dataset_cursors, first.dataset_cursors);
        assert_eq!(restored.executed_steps, first.executed_steps);
        assert_eq!(restored.random_state.get(), Some(42));
        let first_done = first.drive(MachineResume::Start, Quantum::new(100, 1024).unwrap());
        let restored_done = restored.drive(MachineResume::Start, Quantum::new(100, 1024).unwrap());
        assert_eq!(first_done, restored_done);

        let mut version_nine = checkpoint.bytes()[..checkpoint.bytes().len() - 37].to_vec();
        version_nine[..8].copy_from_slice(b"MECP0009");
        version_nine[8..12].copy_from_slice(&9u32.to_be_bytes());
        let version_nine = BoundedPayload::new(
            "mainframe-env.reference-machine-checkpoint@9",
            version_nine,
            InvocationLimits::default(),
        )
        .unwrap();
        let mut migrated_nine =
            ReferenceMachine::from_binary(&binary(), invocation(), CodecLimits::default()).unwrap();
        migrated_nine.restore_checkpoint(&version_nine).unwrap();
        assert_eq!(migrated_nine.random_state.get(), None);

        let mut version_eight = version_nine.bytes()[..version_nine.bytes().len() - 8].to_vec();
        version_eight[..8].copy_from_slice(b"MECP0008");
        version_eight[8..12].copy_from_slice(&8u32.to_be_bytes());
        let version_eight = BoundedPayload::new(
            "mainframe-env.reference-machine-checkpoint@8",
            version_eight,
            InvocationLimits::default(),
        )
        .unwrap();
        let mut migrated_eight =
            ReferenceMachine::from_binary(&binary(), invocation(), CodecLimits::default()).unwrap();
        migrated_eight.restore_checkpoint(&version_eight).unwrap();
        assert!(migrated_eight.linkage_addresses.is_empty());
        assert!(migrated_eight.freed_allocations.is_empty());

        let mut damaged = checkpoint.bytes().to_vec();
        damaged.push(0);
        let damaged =
            BoundedPayload::new(checkpoint.schema(), damaged, InvocationLimits::default()).unwrap();
        assert_eq!(
            restored.restore_checkpoint(&damaged),
            Err(MachineProblem::IncompatibleSnapshot)
        );

        let snapshot = first.snapshot();
        let mut legacy = b"MECP0007".to_vec();
        legacy.extend_from_slice(&7u32.to_be_bytes());
        legacy.extend_from_slice(&(snapshot.program_counter as u64).to_be_bytes());
        legacy.extend_from_slice(&snapshot.effect_sequence.to_be_bytes());
        legacy.extend_from_slice(&snapshot.executed_steps.to_be_bytes());
        push_bytes(&mut legacy, &snapshot.output).unwrap();
        legacy.extend_from_slice(&(snapshot.base_storage.len() as u32).to_be_bytes());
        for storage in &snapshot.base_storage {
            push_bytes(&mut legacy, storage).unwrap();
        }
        legacy.extend_from_slice(&(snapshot.perform_stack.len() as u32).to_be_bytes());
        for target in &snapshot.perform_stack {
            legacy.extend_from_slice(&(*target as u64).to_be_bytes());
        }
        legacy.extend_from_slice(&(snapshot.altered_targets.len() as u32).to_be_bytes());
        for (from, to) in &snapshot.altered_targets {
            push_bytes(&mut legacy, from.as_bytes()).unwrap();
            push_bytes(&mut legacy, to.as_bytes()).unwrap();
        }
        legacy.extend_from_slice(&(snapshot.loop_reentry.len() as u32).to_be_bytes());
        for node in &snapshot.loop_reentry {
            legacy.extend_from_slice(&(*node as u64).to_be_bytes());
        }
        legacy.extend_from_slice(&(snapshot.loop_counts.len() as u32).to_be_bytes());
        for (node, count) in &snapshot.loop_counts {
            legacy.extend_from_slice(&(*node as u64).to_be_bytes());
            legacy.extend_from_slice(&count.to_be_bytes());
        }
        push_bytes(&mut legacy, snapshot.last_file_status.as_bytes()).unwrap();
        legacy.extend_from_slice(&(snapshot.dataset_cursors.len() as u32).to_be_bytes());
        for (dataset, cursor) in &snapshot.dataset_cursors {
            push_bytes(&mut legacy, dataset.as_bytes()).unwrap();
            push_bytes(&mut legacy, cursor.as_bytes()).unwrap();
        }
        legacy.push(snapshot.condition_statuses);
        let legacy = BoundedPayload::new(
            "mainframe-env.reference-machine-checkpoint@7",
            legacy,
            InvocationLimits::default(),
        )
        .unwrap();
        let mut migrated =
            ReferenceMachine::from_binary(&binary(), invocation(), CodecLimits::default()).unwrap();
        migrated.restore_checkpoint(&legacy).unwrap();
        assert_eq!(migrated.pc, snapshot.program_counter);
        assert_eq!(migrated.bases, snapshot.base_storage);
        assert!(migrated.sort_workspaces.is_empty());
    }

    #[test]
    fn checkpoint_preserves_disjoint_storage64_identity_and_rejects_cross_width_decode() {
        let limits = CodecLimits::default();
        let binary = binary();
        let mut machine = ReferenceMachine::from_binary(&binary, invocation(), limits).unwrap();
        let attributes = Storage64Attributes {
            location: Storage64Location::AboveBar,
            key: Storage64Key::User,
            shared: false,
            executable: false,
        };
        let address = machine.storage64.allocate("run", 17, attributes).unwrap();
        assert_eq!(
            machine.decode_address(&address.to_be_bytes()),
            Err(MachineProblem::DataException)
        );
        let checkpoint = machine.checkpoint().unwrap();
        assert_eq!(
            checkpoint.schema(),
            "mainframe-env.reference-machine-checkpoint@12"
        );
        let mut previous_bytes = checkpoint.bytes()[..checkpoint.bytes().len() - 4].to_vec();
        previous_bytes[..8].copy_from_slice(b"MECP0011");
        previous_bytes[8..12].copy_from_slice(&11u32.to_be_bytes());
        let previous = BoundedPayload::new(
            "mainframe-env.reference-machine-checkpoint@11",
            previous_bytes,
            InvocationLimits::default(),
        )
        .unwrap();
        let mut migrated = ReferenceMachine::from_binary(&binary, invocation(), limits).unwrap();
        migrated.restore_checkpoint(&previous).unwrap();
        assert_eq!(migrated.storage64.get(address).unwrap().bytes.len(), 17);
        let mut restored = ReferenceMachine::from_binary(&binary, invocation(), limits).unwrap();
        restored.restore_checkpoint(&checkpoint).unwrap();
        assert_eq!(restored.storage64.get(address).unwrap().bytes.len(), 17);
        restored
            .storage64
            .release(address, "run", Storage64Key::User)
            .unwrap();
        assert!(restored.storage64.get(address).is_none());
        let next = restored.storage64.allocate("run", 17, attributes).unwrap();
        assert_ne!(address, next);
        let reopened = restored.checkpoint().unwrap();
        let mut restored_again =
            ReferenceMachine::from_binary(&binary, invocation(), limits).unwrap();
        restored_again.restore_checkpoint(&reopened).unwrap();
        assert!(restored_again.storage64.get(address).is_none());
        assert_eq!(restored_again.storage64.get(next).unwrap().bytes.len(), 17);
    }

    #[test]
    fn storage64_cancellation_and_deadline_discard_private_allocations() {
        let attributes = Storage64Attributes {
            location: Storage64Location::AboveBar,
            key: Storage64Key::User,
            shared: false,
            executable: false,
        };
        for resume in [MachineResume::Cancelled, MachineResume::TimedOut] {
            let mut machine =
                ReferenceMachine::from_binary(&binary(), invocation(), CodecLimits::default())
                    .unwrap();
            let address = machine.storage64.allocate("run", 1, attributes).unwrap();
            assert!(matches!(
                machine.drive(resume, Quantum::new(1, 1024).unwrap()),
                MachineDrive::Failed(_)
            ));
            assert!(machine.storage64.get(address).is_none());
        }
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
        let arguments = typed_cics::legacy_arguments(&tokens).unwrap();
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
            native_binary: false,
            signed: true,
            sign_separate: false,
            justified_right: false,
            blank_when_zero: false,
            linkage: false,
            offset: 0,
            length: 13,
            element_length: 13,
            occurs: 1,
            occurs_min: 1,
            unbounded: false,
            depending_on: None,
            indexes: Vec::new(),
            keys: Vec::new(),
            dynamic: false,
            dynamic_limit: 0,
            parent: None,
            alias_of: None,
            occurs_clause: false,
            condition_values: Vec::new(),
            object_class: None,
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

    #[test]
    fn numeric_edited_suppresses_commas_through_leading_and_floating_positions() {
        let cases = [
            ("+ZZZ,ZZZ,ZZZ.ZZ", 11, 2, 9585, "+         95.85"),
            ("-ZZZ,ZZZ,ZZZ.ZZ", 11, 2, -123456, "-      1,234.56"),
            ("ZZZ,ZZ9.99-", 8, 2, 9585, "     95.85 "),
            ("ZZZ,ZZ9.99-", 8, 2, 700, "      7.00 "),
            ("Z,ZZZ,ZZ9", 7, 0, 42, "       42"),
            ("****,**9.99", 9, 2, 9585, "******95.85"),
            ("-,---,--9.99", 8, 2, -123456, "   -1,234.56"),
            ("-,---,--9.99", 8, 2, 9585, "       95.85"),
            ("++++,++9.99", 9, 2, 9585, "     +95.85"),
            ("+ZZZ,ZZZ,ZZZ.ZZ", 11, 2, 1234567890, "+ 12,345,678.90"),
            // "/" and "0" inside suppression follow GnuCOBOL 3.2 (kept); IBM source pending, #271.
            ("ZZ/ZZ9", 5, 0, 42, "  / 42"),
            ("ZZ0ZZ9", 5, 0, 42, "  0 42"),
            ("ZZBZZ9", 5, 0, 42, "    42"),
            ("ZZ/ZZ9", 5, 0, 0, "  /  0"),
            ("ZZ0ZZ9", 5, 0, 0, "  0  0"),
            ("ZZBZZ9", 5, 0, 0, "     0"),
            ("****,**9.99", 9, 2, 0, "*******0.00"),
            ("-,---,--9.99", 8, 2, 0, "        0.00"),
        ];
        for (picture, digits, scale, coefficient, expected) in cases {
            let layout = edited_test_layout(picture, digits, scale, false);
            assert_eq!(
                encode_edited(&layout, Decimal { coefficient, scale }),
                Ok(expected.as_bytes().to_vec()),
                "picture {picture}, coefficient {coefficient}"
            );
        }
        let layout = edited_test_layout("Z,ZZ9.99", 6, 2, true);
        assert_eq!(
            encode_edited(
                &layout,
                Decimal {
                    coefficient: 0,
                    scale: 2
                }
            ),
            Ok(vec![b' '; layout.length])
        );
    }

    #[test]
    fn zero_suppression_blanks_every_all_z_position() {
        for (picture, digits, expected) in [
            ("-ZZZ,ZZZ,ZZZ.ZZ", 11, "               "),
            ("+ZZZ,ZZZ,ZZZ.ZZ", 11, "               "),
            ("ZZZ.ZZ", 5, "      "),
            ("$ZZZ.ZZ", 5, "       "),
        ] {
            let layout = edited_test_layout(picture, digits, 2, false);
            assert_eq!(
                encode_edited(
                    &layout,
                    Decimal {
                        coefficient: 0,
                        scale: 2
                    }
                ),
                Ok(expected.as_bytes().to_vec()),
                "picture {picture}"
            );
        }
    }

    #[test]
    fn zero_suppression_keeps_asterisks_and_decimal_point() {
        for (picture, digits, expected) in [
            ("***.**", 5, "***.**"),
            ("+***.**", 5, "****.**"),
            ("***,***.**", 8, "*******.**"),
        ] {
            let layout = edited_test_layout(picture, digits, 2, false);
            assert_eq!(
                encode_edited(
                    &layout,
                    Decimal {
                        coefficient: 0,
                        scale: 2
                    }
                ),
                Ok(expected.as_bytes().to_vec()),
                "picture {picture}"
            );
        }
    }

    #[test]
    fn zero_suppression_blanks_all_floating_insertion_pictures() {
        // GnuCOBOL 3.2 -std=ibm prints seven, seven, and six spaces respectively.
        for (picture, digits) in [("----.--", 6), ("++++.++", 6), ("$$$.$$", 5)] {
            let layout = edited_test_layout(picture, digits, 2, false);
            assert_eq!(
                encode_edited(
                    &layout,
                    Decimal {
                        coefficient: 0,
                        scale: 2
                    }
                ),
                Ok(vec![b' '; layout.length]),
                "picture {picture}"
            );
        }
    }

    #[test]
    fn zero_suppression_preserves_nine_fraction_and_blank_when_zero_controls() {
        for (picture, digits, coefficient, expected) in
            [("ZZ9.99", 5, 0, "  0.00"), ("ZZZ.ZZ", 5, 5, "   .05")]
        {
            let layout = edited_test_layout(picture, digits, 2, false);
            assert_eq!(
                encode_edited(
                    &layout,
                    Decimal {
                        coefficient,
                        scale: 2
                    }
                ),
                Ok(expected.as_bytes().to_vec()),
                "picture {picture}, coefficient {coefficient}"
            );
        }
        let layout = edited_test_layout("ZZ9.99", 5, 2, true);
        assert_eq!(
            encode_edited(
                &layout,
                Decimal {
                    coefficient: 0,
                    scale: 2
                }
            ),
            Ok(vec![b' '; layout.length])
        );
    }

    #[test]
    fn floating_currency_places_symbol_before_significant_digits() {
        // Expected bytes from GnuCOBOL 3.2 with -std=ibm.
        let cases = [
            ("$$$,$$9.99", 7, 9585, "    $95.85"),
            ("$$$,$$9.99", 7, 0, "     $0.00"),
            ("$$$,$$9.99", 7, 85, "     $0.85"),
            ("$$$,$$9.99", 7, 1234567, "$12,345.67"),
            ("$$,$$$,$$9.99", 9, 9585, "       $95.85"),
            ("$$,$$$,$$9.99", 9, 1234567, "   $12,345.67"),
            ("$$$.99", 4, 85, "  $.85"),
            ("$$$.99", 4, 0, "  $.00"),
            ("$ZZ,ZZ9.99", 7, 9585, "$    95.85"),
            ("$$$,$$9.99-", 7, -9585, "    $95.85-"),
            ("$$$,$$9.99CR", 7, -9585, "    $95.85CR"),
            ("$$$,$$9.99", 7, 123450, " $1,234.50"),
            ("$$$,$$9.99", 7, 23450, "   $234.50"),
            ("$$$.99", 4, 1234, "$12.34"),
            ("$$$.99", 4, 150, " $1.50"),
            ("$$,$$$,$$9.99", 9, 100000000, "$1,000,000.00"),
            ("$$,$$$,$$9.99", 9, 10000000, "  $100,000.00"),
        ];
        for (picture, digits, coefficient, expected) in cases {
            let layout = edited_test_layout(picture, digits, 2, false);
            assert_eq!(
                encode_edited(
                    &layout,
                    Decimal {
                        coefficient,
                        scale: 2
                    }
                ),
                Ok(expected.as_bytes().to_vec()),
                "picture {picture}, coefficient {coefficient}"
            );
        }
        let layout = edited_test_layout("$$$,$$9.99", 7, 2, true);
        assert_eq!(
            encode_edited(
                &layout,
                Decimal {
                    coefficient: 0,
                    scale: 2
                }
            ),
            Ok(vec![b' '; layout.length])
        );
    }

    #[test]
    fn floating_insertion_replaces_comma_before_first_significant_digit() {
        // Expected bytes from GnuCOBOL 3.2 with -std=ibm: the floating symbol takes
        // the position immediately left of the first significant digit, even when
        // that position is an insertion comma.
        let cases = [
            ("---,--9.99", 7, -23450, "   -234.50"),
            ("---,--9.99", 7, -123450, " -1,234.50"),
            ("+++,++9.99", 7, 23450, "   +234.50"),
            ("$$$,$$9.99", 7, 23450, "   $234.50"),
            ("$$,$$$.99", 6, 23450, "  $234.50"),
            ("$$,$$$.99", 6, 50, "     $.50"),
        ];
        for (picture, digits, coefficient, expected) in cases {
            let layout = edited_test_layout(picture, digits, 2, false);
            assert_eq!(
                encode_edited(
                    &layout,
                    Decimal {
                        coefficient,
                        scale: 2
                    }
                ),
                Ok(expected.as_bytes().to_vec()),
                "picture {picture}, coefficient {coefficient}"
            );
        }
    }

    #[test]
    fn numeric_edited_cr_db_suffix_follows_value_sign() {
        for (picture, coefficient, expected) in [
            ("99999.99CR", 9585, "00095.85  "),
            ("99999.99CR", -9585, "00095.85CR"),
            ("99999.99DB", 9585, "00095.85  "),
            ("99999.99DB", -9585, "00095.85DB"),
        ] {
            let layout = edited_test_layout(picture, 7, 2, false);
            assert_eq!(
                encode_edited(
                    &layout,
                    Decimal {
                        coefficient,
                        scale: 2
                    }
                ),
                Ok(expected.as_bytes().to_vec()),
                "picture {picture}, coefficient {coefficient}"
            );
        }
    }

    fn edited_test_layout(
        picture: &str,
        digits: usize,
        scale: u32,
        blank_when_zero: bool,
    ) -> LayoutMetadata {
        LayoutMetadata {
            name: "EDITED".into(),
            simple_name: "EDITED".into(),
            category: LayoutCategory::NumericEdited,
            picture: picture.into(),
            digits,
            scale,
            native_binary: false,
            signed: true,
            sign_separate: false,
            justified_right: false,
            blank_when_zero,
            linkage: false,
            offset: 0,
            length: picture.len(),
            element_length: picture.len(),
            occurs: 1,
            occurs_min: 1,
            unbounded: false,
            depending_on: None,
            indexes: Vec::new(),
            keys: Vec::new(),
            dynamic: false,
            dynamic_limit: 0,
            parent: None,
            alias_of: None,
            occurs_clause: false,
            condition_values: Vec::new(),
            object_class: None,
        }
    }

    #[test]
    fn floating_minus_reserves_one_insertion_position_before_numeric_digits() {
        let layout = LayoutMetadata {
            name: "EDITED".into(),
            simple_name: "EDITED".into(),
            category: LayoutCategory::NumericEdited,
            picture: "----9".into(),
            digits: 5,
            scale: 0,
            native_binary: false,
            signed: true,
            sign_separate: false,
            justified_right: false,
            blank_when_zero: false,
            linkage: false,
            offset: 0,
            length: 5,
            element_length: 5,
            occurs: 1,
            occurs_min: 1,
            unbounded: false,
            depending_on: None,
            indexes: Vec::new(),
            keys: Vec::new(),
            dynamic: false,
            dynamic_limit: 0,
            parent: None,
            alias_of: None,
            occurs_clause: false,
            condition_values: Vec::new(),
            object_class: None,
        };
        assert_eq!(
            encode_edited(
                &layout,
                Decimal {
                    coefficient: -911,
                    scale: 0,
                }
            ),
            Ok(b" -911".to_vec())
        );
        assert_eq!(
            encode_edited(
                &layout,
                Decimal {
                    coefficient: 99_999,
                    scale: 0,
                }
            ),
            Ok(b" 9999".to_vec())
        );
        assert_eq!(
            encode_decimal(
                &layout,
                Decimal {
                    coefficient: 999_999,
                    scale: 0,
                }
            ),
            Ok(b" 9999".to_vec())
        );
    }

    #[test]
    fn numval_currency_and_error_positions_are_not_boolean_shortcuts() {
        assert_eq!(test_numval(b"12A3", false, None), 3);
        assert_eq!(
            test_numval("€12X".as_bytes(), false, Some("€".as_bytes())),
            4
        );
        assert_eq!(test_numval(b"123.45", false, None), 0);
        assert!(parse_numval(b"$12.5", false, None).is_err());
        assert_eq!(
            parse_numval("€12.5".as_bytes(), false, Some("€".as_bytes())).unwrap(),
            Decimal {
                coefficient: 125,
                scale: 1
            }
        );
    }
    #[test]
    fn varying_record_depending_accepts_optional_is_and_on() {
        for description in [
            "FD VBRC-FILE RECORDING MODE IS V RECORD IS VARYING IN SIZE FROM 10 TO 80 DEPENDING ON WS-RECD-LEN",
            "FD VBRC-FILE RECORD VARYING IN SIZE FROM 10 TO 80 DEPENDING ON WS-RECD-LEN",
            "FD VBRC-FILE RECORD IS VARYING FROM 10 TO 80 DEPENDING WS-RECD-LEN",
        ] {
            assert_eq!(
                file_record_depending(description).as_deref(),
                Some("WS-RECD-LEN"),
                "{description}"
            );
        }
        assert_eq!(
            file_record_depending("FD F RECORD CONTAINS 80 CHARACTERS"),
            None
        );
        assert_eq!(
            file_record_depending("FD F RECORD IS VARYING IN SIZE FROM 10 TO 80"),
            None
        );
    }

    fn varying_file_machine() -> ReferenceMachine {
        let mut m =
            ReferenceMachine::from_binary(&binary(), invocation(), CodecLimits::default()).unwrap();
        m.files.insert("VBFILE".into(), FileMetadata {
            assignment: "VBPS".into(), record_name: Some("VBR-REC".into()),
            record_names: vec!["VBR-REC".into()],
            organization: "SEQUENTIAL".into(), access_mode: "SEQUENTIAL".into(),
            record_key: None, alternate_record_keys: Vec::new(), relative_key: None,
            file_status: None, sort_merge: false,
            description: "FD VBFILE RECORD IS VARYING IN SIZE FROM 10 TO 80 CHARACTERS DEPENDING ON WS-RECD-LEN".into(),
            record_min: Some(10), record_max: Some(80), ccsid: None, linage: None,
        });
        let base = m.bases.len();
        m.bases.push(vec![b'A'; 80]);
        m.views.insert(
            "VBR-REC".into(),
            StorageView {
                base,
                offset: 0,
                length: 80,
            },
        );
        let base = m.bases.len();
        m.bases.push(b"0012".to_vec());
        m.views.insert(
            "WS-RECD-LEN".into(),
            StorageView {
                base,
                offset: 0,
                length: 4,
            },
        );
        m.layouts.insert(
            "WS-RECD-LEN".into(),
            LayoutMetadata {
                name: "WS-RECD-LEN".into(),
                simple_name: "WS-RECD-LEN".into(),
                category: LayoutCategory::NumericDisplay,
                picture: "9(4)".into(),
                digits: 4,
                scale: 0,
                signed: false,
                sign_separate: false,
                justified_right: false,
                blank_when_zero: false,
                linkage: false,
                offset: 0,
                length: 4,
                element_length: 4,
                occurs: 1,
                occurs_min: 1,
                unbounded: false,
                depending_on: None,
                indexes: Vec::new(),
                keys: Vec::new(),
                dynamic: false,
                dynamic_limit: 0,
                parent: None,
                alias_of: None,
                occurs_clause: false,
                condition_values: Vec::new(),
                object_class: None,
            },
        );
        m
    }

    #[test]
    fn varying_write_uses_depending_length_and_read_restores_it() {
        let mut m = varying_file_machine();
        for (length, fill) in [(12, b'A'), (39, b'B')] {
            m.write("WS-RECD-LEN", format!("{length:04}").as_bytes())
                .unwrap();
            m.write("VBR-REC", &[fill; 80]).unwrap();
            let Step::Effect(effect) = m.dataset_effect("write", &["VBR-REC".into()]).unwrap()
            else {
                panic!("expected effect")
            };
            let HostRequest::Dataset(DatasetRequest::Append { records, .. }) = effect.request
            else {
                panic!("expected append")
            };
            assert_eq!(records[0], vec![fill; length]);
            m.pending = None;
        }
        m.write("WS-RECD-LEN", b"0080").unwrap();
        let Step::Effect(effect) = m.dataset_effect("read", &["VBFILE".into()]).unwrap() else {
            panic!("expected effect")
        };
        m.resume_host(EffectResult {
            sequence: effect.sequence,
            outcome: Ok(HostResult::Dataset(
                mainframe_env_host_api::DatasetResult::Records {
                    records: vec![vec![b'A'; 12]],
                    identities: vec![b"1".to_vec()],
                    version: 1,
                },
            )),
        })
        .unwrap();
        assert_eq!(m.read("WS-RECD-LEN").unwrap(), b"0012");
        assert_eq!(&m.read("VBR-REC").unwrap()[..12], &[b'A'; 12]);
        let base = m.bases.len();
        m.bases.push(vec![b' '; 80]);
        m.views.insert(
            "INTO-REC".into(),
            StorageView {
                base,
                offset: 0,
                length: 80,
            },
        );
        let Step::Effect(effect) = m
            .dataset_effect("read", &["VBFILE".into(), "INTO".into(), "INTO-REC".into()])
            .unwrap()
        else {
            panic!("expected effect")
        };
        m.resume_host(EffectResult {
            sequence: effect.sequence,
            outcome: Ok(HostResult::Dataset(
                mainframe_env_host_api::DatasetResult::Records {
                    records: vec![vec![b'B'; 39]],
                    identities: vec![b"2".to_vec()],
                    version: 2,
                },
            )),
        })
        .unwrap();
        assert_eq!(m.read("WS-RECD-LEN").unwrap(), b"0039");
        assert_eq!(&m.read("INTO-REC").unwrap()[..39], &[b'B'; 39]);
    }

    #[test]
    fn varying_write_from_uses_fd_length_and_rejects_out_of_bounds() {
        let mut m = varying_file_machine();
        m.implicit
            .insert("SOURCE".into(), CobolValue::Bytes(vec![b'C'; 80]));
        let Step::Effect(effect) = m
            .dataset_effect("write", &["VBR-REC".into(), "FROM".into(), "SOURCE".into()])
            .unwrap()
        else {
            panic!("expected effect")
        };
        let HostRequest::Dataset(DatasetRequest::Append { records, .. }) = effect.request else {
            panic!("expected append")
        };
        assert_eq!(records[0], vec![b'C'; 12]);
        m.pending = None;
        m.implicit
            .insert("SOURCE".into(), CobolValue::Bytes(b"SHORT!".to_vec()));
        let Step::Effect(effect) = m
            .dataset_effect("write", &["VBR-REC".into(), "FROM".into(), "SOURCE".into()])
            .unwrap()
        else {
            panic!("expected effect")
        };
        let HostRequest::Dataset(DatasetRequest::Append { records, .. }) = effect.request else {
            panic!("expected append")
        };
        assert_eq!(records[0], b"SHORT!      ");
        m.pending = None;
        for length in [9, 81] {
            m.write("WS-RECD-LEN", format!("{length:04}").as_bytes())
                .unwrap();
            assert!(matches!(
                m.dataset_effect("write", &["VBR-REC".into()]),
                Err(MachineProblem::SizeError)
            ));
        }
    }

    #[test]
    fn varying_without_depending_uses_named_fd_record_size() {
        let mut m = varying_file_machine();
        let file = m.files.get_mut("VBFILE").unwrap();
        file.record_name = Some("SHORT-REC".into());
        file.record_names = vec!["SHORT-REC".into(), "LONG-REC".into()];
        file.description = "FD VBFILE RECORD IS VARYING IN SIZE FROM 10 TO 80 CHARACTERS".into();
        let base = m.bases.len();
        m.bases.push(vec![b'Z'; 39]);
        m.views.insert(
            "LONG-REC".into(),
            StorageView {
                base,
                offset: 0,
                length: 39,
            },
        );
        let Step::Effect(effect) = m.dataset_effect("write", &["LONG-REC".into()]).unwrap() else {
            panic!("expected effect")
        };
        let HostRequest::Dataset(DatasetRequest::Append {
            dataset, records, ..
        }) = effect.request
        else {
            panic!("expected append")
        };
        assert_eq!(dataset.as_str(), "VBPS");
        assert_eq!(records[0], vec![b'Z'; 39]);
    }

    #[test]
    fn varying_rewrite_uses_depending_length() {
        let mut m = varying_file_machine();
        m.files.get_mut("VBFILE").unwrap().record_key = Some("KEY".into());
        m.implicit
            .insert("KEY".into(), CobolValue::Bytes(b"K".to_vec()));
        m.write("WS-RECD-LEN", b"0039").unwrap();
        let Step::Effect(effect) = m.dataset_effect("rewrite", &["VBR-REC".into()]).unwrap() else {
            panic!("expected effect")
        };
        let HostRequest::Dataset(DatasetRequest::RewriteRecord { record, .. }) = effect.request
        else {
            panic!("expected rewrite")
        };
        assert_eq!(record, vec![b'A'; 39]);
    }
}

mod instance;
