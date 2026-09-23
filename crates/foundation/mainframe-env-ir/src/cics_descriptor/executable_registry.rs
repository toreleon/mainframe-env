use super::*;

const DIAGNOSTIC_EFFECTS: &[Effect] = DOCUMENT_EFFECTS;
const MONITOR_EFFECTS: &[Effect] = &[
    Effect::MemoryRead,
    Effect::MemoryWrite,
    Effect::Security,
    Effect::Audit,
    Effect::Clock,
    Effect::Condition,
    Effect::Transaction,
];

/// Static executable facts owned by the typed CICS dialect.
///
/// Option direction and operation-specific plan shape remain owned by the
/// CICS plan codec validator. Host request mapping and provider transitions
/// deliberately do not belong in this descriptor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CicsExecutableDescriptor {
    /// Plan operation represented by this executable identity.
    pub operation: CicsPlanOperation,
    /// Executable operation namespace.
    pub namespace: &'static str,
    /// Executable operation name.
    pub name: &'static str,
    /// Executable operation semantic major.
    pub major: u16,
    /// Exact declared effect sequence.
    pub effects: &'static [Effect],
    /// Runtime import required during legalization.
    pub runtime_import: &'static str,
}

impl CicsExecutableDescriptor {
    /// Builds the checked generic IR identity for this descriptor.
    #[must_use]
    pub fn identity(self) -> OperationIdentity {
        OperationIdentity::new(self.namespace, self.name, self.major)
            .expect("dialect-owned typed CICS identity")
    }

    /// Reports whether this descriptor owns an already-decoded identity.
    #[must_use]
    pub fn matches_identity(self, identity: &OperationIdentity) -> bool {
        identity.namespace() == self.namespace
            && identity.name() == self.name
            && identity.major() == self.major
    }
}

/// Handler readiness carried by the frozen application-command registry shape.
///
/// `Unready` rows are recognized contract identities but are not executable or
/// advertised. They must fail explicitly until a later family slice seals a
/// semantic handler.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CicsApplicationHandlerReadiness {
    /// A typed HIR lowering and runtime handler are both sealed.
    TypedRuntime,
    /// An advertised raw compatibility route exists without typed HIR lowering.
    LegacyCompatibility,
    /// No semantic handler has been sealed for this command.
    Unready,
}

/// Source-projected operand shape for one top-level command option.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CicsApplicationOptionValueShape {
    /// The option is a keyword flag and rejects a parenthesized operand.
    Flag,
    /// The option requires a parenthesized operand.
    Value,
    /// The pinned syntax diagram draws the parenthesized operand as an
    /// independently optional nested group: both the bare keyword and the
    /// keyword with its parenthesized operand are well-formed.
    OptionalValue,
    /// Pinned source facts do not establish one safe shape.
    BoundedAmbiguity,
}

/// Source-projected data flow for one top-level command option.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CicsApplicationOptionDirection {
    /// Operand data flows into the command.
    Input,
    /// Operand data is returned by the command.
    Output,
    /// Operand data can flow in both directions.
    InputOutput,
    /// The flag has no operand.
    None,
    /// Pinned source facts do not establish one safe direction.
    BoundedAmbiguity,
}

/// Completeness of the compact option-constraint projection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CicsApplicationConstraintStatus {
    /// All applicable constraints are source-resolved.
    Resolved,
    /// Emitted constraints are exact, but the complete prose rule set is bounded.
    BoundedAmbiguity,
    /// The internal-only command has no application option contract.
    NotApplicable,
    /// Source review has not frozen this row yet.
    Pending,
}

/// COBOL applicability projected from IBM's command language restrictions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CicsApplicationCobolApplicability {
    /// The command is available to COBOL applications.
    Allowed,
    /// The command is restricted to other source languages or is internal-only.
    NotApplicable,
    /// Availability depends on a source-reviewed command context.
    Conditional,
    /// Pinned source facts do not establish COBOL applicability.
    BoundedAmbiguity,
}

/// Compact compiler-facing shape for one top-level command option.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CicsApplicationOptionDescriptor {
    /// Source option name.
    pub name: &'static str,
    /// Whether the option is a flag, valued, or bounded.
    pub value_shape: CicsApplicationOptionValueShape,
    /// Reviewed operand direction.
    pub direction: CicsApplicationOptionDirection,
    /// Exact IBM maximum in bytes when pinned source states one.
    ///
    /// Host safety ceilings are intentionally not exposed as IBM semantics.
    pub source_max_value_bytes: Option<usize>,
}

/// An alternative option group; mutual exclusion is carried separately.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CicsApplicationOptionAlternative {
    /// Options participating in the alternative.
    pub members: &'static [&'static str],
    /// Whether at least one member is required.
    pub required: bool,
}

/// A one-way option dependency.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CicsApplicationOptionDependency {
    /// Option that activates the dependency.
    pub option: &'static str,
    /// Options that must also be present.
    pub requires: &'static [&'static str],
}

/// Whether a dynamic HANDLE/IGNORE CONDITION clause accepts a label operand.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CicsApplicationConditionLabelOperand {
    /// A parenthesized COBOL label is optional.
    Optional,
    /// A parenthesized operand is forbidden.
    Forbidden,
}

/// Source-backed wildcard clause whose name comes from the EIBRESP table.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CicsApplicationConditionClauseDescriptor {
    /// Stable condition-name authority profile.
    pub name_authority: &'static str,
    /// Digest of the normalized name/code authority.
    pub name_authority_sha256: &'static str,
    /// Minimum clauses required by the command syntax.
    pub minimum_occurrences: usize,
    /// Maximum clauses accepted by one command.
    pub maximum_occurrences: usize,
    /// Whether each condition name may carry a label operand.
    pub label_operand: CicsApplicationConditionLabelOperand,
}

/// Compact compiler-facing shape for one pinned application command.
///
/// Full source facts and semantic policies stay in the generated conformance
/// contract. This structure contains only the identity and lookup data needed
/// to recognize catalog commands and reject unready routes without a fallback.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CicsApplicationRegistryDescriptor {
    /// Stable official application-command row identity.
    pub official_row: &'static str,
    /// Command words before any option or operand.
    pub label_tokens: &'static [&'static str],
    /// Syntax-derived heads and bounded catalog aliases used for recognition.
    pub recognition_heads: &'static [&'static [&'static str]],
    /// Catalog or syntax option names that distinguish a shared command head.
    pub discriminator_options: &'static [&'static str],
    /// Discriminator options whose presence is source-required for this row.
    pub required_discriminator_options: &'static [&'static str],
    /// Discriminator options whose presence selects a different catalog row.
    pub forbidden_discriminator_options: &'static [&'static str],
    /// Completeness of syntax-head recognition for this row.
    pub recognition_status: CicsApplicationConstraintStatus,
    /// Whether the source command applies to COBOL compilation.
    pub cobol_applicability: CicsApplicationCobolApplicability,
    /// Source-reviewed top-level option shapes.
    pub options: &'static [CicsApplicationOptionDescriptor],
    /// Dynamic condition-name clause contract, when the command defines one.
    pub condition_clauses: Option<CicsApplicationConditionClauseDescriptor>,
    /// Backward-compatible name view of `options` for legacy compiler routes.
    pub top_level_options: &'static [&'static str],
    /// Options required outside an alternative group.
    pub required_options: &'static [&'static str],
    /// Required or optional alternative groups.
    pub alternative_groups: &'static [CicsApplicationOptionAlternative],
    /// One-way option dependencies.
    pub dependencies: &'static [CicsApplicationOptionDependency],
    /// Groups in which no more than one option may occur.
    pub mutual_exclusion_groups: &'static [&'static [&'static str]],
    /// Completeness of the emitted compact constraints.
    pub constraint_status: CicsApplicationConstraintStatus,
    /// Two EIB function-code bytes.
    pub eibfn: [u8; 2],
    /// Stable semantic-family owner.
    pub family: &'static str,
    /// Stable future handler identity; identity does not imply readiness.
    pub handler_id: &'static str,
    /// Digest of handler identity, readiness, advertisement, and route binding.
    pub handler_sha256: &'static str,
    /// Whether a real handler is already present.
    pub readiness: CicsApplicationHandlerReadiness,
    /// Whether the current public profile advertises this command.
    pub advertised: bool,
    /// Existing host API operation name when the handler is ready.
    pub runtime_operation: Option<&'static str>,
    /// Source-reviewed options fully implemented by the raw compatibility route.
    ///
    /// Typed and unready rows keep this empty; their option admission is owned by
    /// typed lowering or the explicit unsupported path respectively.
    pub legacy_execution_options: &'static [&'static str],
}

include!("../generated/cics_application_registry.rs");

/// One syntax-head match for a catalog command candidate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CicsApplicationRegistryMatch {
    /// Candidate command descriptor.
    pub descriptor: &'static CicsApplicationRegistryDescriptor,
    /// Exact recognition head consumed from the input.
    pub head_tokens: &'static [&'static str],
}

/// Returns every syntax-derived command candidate whose head matches the input.
///
/// Shared heads such as `ACQUIRE` and option-valued command selectors such as
/// `WRITE FILE(...)` intentionally yield more than one candidate. The compiler
/// must validate each candidate's option shape and require one unique result.
pub fn cics_application_registry_candidates_for_tokens(
    tokens: &[impl AsRef<str>],
) -> impl Iterator<Item = CicsApplicationRegistryMatch> + '_ {
    CICS_APPLICATION_REGISTRY
        .iter()
        .flat_map(move |descriptor| {
            descriptor
                .recognition_heads
                .iter()
                .copied()
                .filter(move |head| {
                    head.len() <= tokens.len()
                        && head.iter().zip(tokens).all(|(expected, actual)| {
                            expected.eq_ignore_ascii_case(actual.as_ref())
                        })
                })
                .map(move |head_tokens| CicsApplicationRegistryMatch {
                    descriptor,
                    head_tokens,
                })
        })
}

/// Resolves one unambiguous syntax head and root option discriminator.
///
/// The lookup is ASCII case-insensitive so callers may use normalized or
/// source-case tokens. Shared heads that need full grammar validation return
/// `None`; callers should use the candidate API below. A resolved result may be
/// unready, so callers must still inspect `readiness`.
#[must_use]
pub fn cics_application_registry_for_tokens(
    tokens: &[impl AsRef<str>],
) -> Option<&'static CicsApplicationRegistryDescriptor> {
    let mut best = None;
    let mut ambiguous = false;
    for candidate in cics_application_registry_candidates_for_tokens(tokens) {
        let remainder = &tokens[candidate.head_tokens.len()..];
        let contains = |expected: &str| {
            remainder
                .iter()
                .any(|actual| expected.eq_ignore_ascii_case(actual.as_ref()))
        };
        if !candidate
            .descriptor
            .required_discriminator_options
            .iter()
            .all(|expected| contains(expected))
            || candidate
                .descriptor
                .forbidden_discriminator_options
                .iter()
                .any(|expected| contains(expected))
        {
            continue;
        }
        let discriminator_matches = candidate
            .descriptor
            .discriminator_options
            .iter()
            .filter(|expected| contains(expected))
            .count();
        if candidate
            .descriptor
            .required_discriminator_options
            .is_empty()
            && candidate
                .descriptor
                .forbidden_discriminator_options
                .is_empty()
            && !candidate.descriptor.discriminator_options.is_empty()
            && discriminator_matches == 0
        {
            continue;
        }
        let score = (candidate.head_tokens.len(), discriminator_matches);
        match best {
            None => {
                best = Some((candidate.descriptor, score));
                ambiguous = false;
            }
            Some((_descriptor, best_score)) if score > best_score => {
                best = Some((candidate.descriptor, score));
                ambiguous = false;
            }
            Some((descriptor, best_score))
                if score == best_score && descriptor != candidate.descriptor =>
            {
                ambiguous = true;
            }
            _ => {}
        }
    }
    best.and_then(|(descriptor, _)| (!ambiguous).then_some(descriptor))
}

/// Complete registry for the bounded typed CICS executable pilot.
pub const CICS_EXECUTABLE_DESCRIPTORS: [CicsExecutableDescriptor; 123] = [
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::Deq,
        namespace: "cics.task",
        name: "deq",
        major: 1,
        effects: DEQ_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::Enq,
        namespace: "cics.task",
        name: "enq",
        major: 1,
        effects: ENQ_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::Read,
        namespace: "cics.file",
        name: "read",
        major: 1,
        effects: READ_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::Rewrite,
        namespace: "cics.file",
        name: "rewrite",
        major: 1,
        effects: REWRITE_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::Syncpoint,
        namespace: "cics.recovery",
        name: "syncpoint",
        major: 1,
        effects: SYNCPOINT_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::ChangeTask,
        namespace: "cics.task",
        name: "change-task",
        major: 1,
        effects: CHANGE_TASK_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::Suspend,
        namespace: "cics.task",
        name: "suspend",
        major: 1,
        effects: SUSPEND_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::SetAssociationUserCorrData,
        namespace: "cics.task",
        name: "set-association-usercorrdata",
        major: 1,
        effects: SET_ASSOCIATION_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::AddressSet,
        namespace: "cics.task",
        name: "address-set",
        major: 1,
        effects: ADDRESS_SET_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::PopHandle,
        namespace: "cics.task",
        name: "pop-handle",
        major: 1,
        effects: HANDLE_STACK_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::PushHandle,
        namespace: "cics.task",
        name: "push-handle",
        major: 1,
        effects: HANDLE_STACK_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::IgnoreCondition,
        namespace: "cics.task",
        name: "ignore-condition",
        major: 1,
        effects: IGNORE_CONDITION_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::HandleCondition,
        namespace: "cics.task",
        name: "handle-condition",
        major: 1,
        effects: IGNORE_CONDITION_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::HandleAid,
        namespace: "cics.task",
        name: "handle-aid",
        major: 1,
        effects: IGNORE_CONDITION_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::AsktimeEib,
        namespace: "cics.time",
        name: "asktime-eib",
        major: 1,
        effects: ASKTIME_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::Asktime,
        namespace: "cics.time",
        name: "asktime",
        major: 1,
        effects: ASKTIME_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::FormatTime,
        namespace: "cics.time",
        name: "format-time",
        major: 1,
        effects: FORMAT_TIME_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::Abend,
        namespace: "cics.task",
        name: "abend",
        major: 1,
        effects: ABEND_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::HandleAbend,
        namespace: "cics.task",
        name: "handle-abend",
        major: 1,
        effects: IGNORE_CONDITION_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::Link,
        namespace: "cics.program",
        name: "link",
        major: 1,
        effects: CONTROL_TRANSFER_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::Xctl,
        namespace: "cics.program",
        name: "xctl",
        major: 1,
        effects: CONTROL_TRANSFER_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::Return,
        namespace: "cics.task",
        name: "return",
        major: 1,
        effects: CONTROL_TRANSFER_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::StartBrowse,
        namespace: "cics.file",
        name: "start-browse",
        major: 1,
        effects: READ_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::ReadNext,
        namespace: "cics.file",
        name: "read-next",
        major: 1,
        effects: READ_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::ReadPrev,
        namespace: "cics.file",
        name: "read-prev",
        major: 1,
        effects: READ_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::EndBrowse,
        namespace: "cics.file",
        name: "end-browse",
        major: 1,
        effects: READ_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::Delete,
        namespace: "cics.file",
        name: "delete",
        major: 1,
        effects: REWRITE_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::Write,
        namespace: "cics.file",
        name: "write",
        major: 1,
        effects: REWRITE_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::WriteTransientData,
        namespace: "cics.queue",
        name: "write-transient-data",
        major: 1,
        effects: QUEUE_WRITE_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::ReceiveMap,
        namespace: "cics.terminal",
        name: "receive-map",
        major: 1,
        effects: TERMINAL_RECEIVE_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::SendMap,
        namespace: "cics.terminal",
        name: "send-map",
        major: 1,
        effects: TERMINAL_SEND_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::SendText,
        namespace: "cics.terminal",
        name: "send-text",
        major: 1,
        effects: TERMINAL_SEND_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::Assign,
        namespace: "cics.task",
        name: "assign",
        major: 1,
        effects: ASSIGN_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::PurgeMessage,
        namespace: "cics.terminal",
        name: "purge-message",
        major: 1,
        effects: PURGE_MESSAGE_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::Start,
        namespace: "cics.interval",
        name: "start",
        major: 1,
        effects: START_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::Retrieve,
        namespace: "cics.task",
        name: "retrieve",
        major: 1,
        effects: RETRIEVE_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::Cancel,
        namespace: "cics.interval",
        name: "cancel",
        major: 1,
        effects: CANCEL_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::Delay,
        namespace: "cics.interval",
        name: "delay",
        major: 1,
        effects: DELAY_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::DeleteTransientData,
        namespace: "cics.queue",
        name: "delete-transient-data",
        major: 1,
        effects: QUEUE_WRITE_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::Getmain,
        namespace: "cics.storage",
        name: "getmain",
        major: 1,
        effects: STORAGE_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::Freemain,
        namespace: "cics.storage",
        name: "freemain",
        major: 1,
        effects: STORAGE_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::DeleteTemporaryStorage,
        namespace: "cics.queue",
        name: "delete-temporary-storage",
        major: 1,
        effects: QUEUE_WRITE_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::Address,
        namespace: "cics.task",
        name: "address",
        major: 1,
        effects: ADDRESS_SET_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::ReadTransientData,
        namespace: "cics.queue",
        name: "read-transient-data",
        major: 1,
        effects: QUEUE_WRITE_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::ReadTemporaryStorage,
        namespace: "cics.queue",
        name: "read-temporary-storage",
        major: 1,
        effects: QUEUE_WRITE_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::WaitEvent,
        namespace: "cics.task",
        name: "wait-event",
        major: 1,
        effects: WAIT_EVENT_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::WaitExternal,
        namespace: "cics.task",
        name: "wait-external",
        major: 1,
        effects: WAIT_EVENT_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::WriteTemporaryStorage,
        namespace: "cics.queue",
        name: "write-temporary-storage",
        major: 1,
        effects: TEMPORARY_QUEUE_WRITE_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::InvokeApplication,
        namespace: "cics.program",
        name: "invoke-application",
        major: 1,
        effects: CONTROL_TRANSFER_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::Load,
        namespace: "cics.program",
        name: "load",
        major: 1,
        effects: CONTROL_TRANSFER_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::Release,
        namespace: "cics.program",
        name: "release",
        major: 1,
        effects: CONTROL_TRANSFER_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::DocumentCreate,
        namespace: "cics.document",
        name: "create",
        major: 1,
        effects: DOCUMENT_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::DocumentDelete,
        namespace: "cics.document",
        name: "delete",
        major: 1,
        effects: DOCUMENT_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::DocumentInsert,
        namespace: "cics.document",
        name: "insert",
        major: 1,
        effects: DOCUMENT_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::DocumentRetrieve,
        namespace: "cics.document",
        name: "retrieve",
        major: 1,
        effects: DOCUMENT_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::DocumentSet,
        namespace: "cics.document",
        name: "set",
        major: 1,
        effects: DOCUMENT_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::TransformDataToJson,
        namespace: "cics.transform",
        name: "data-to-json",
        major: 1,
        effects: TRANSFORM_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::TransformDataToXml,
        namespace: "cics.transform",
        name: "data-to-xml",
        major: 1,
        effects: TRANSFORM_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::TransformJsonToData,
        namespace: "cics.transform",
        name: "json-to-data",
        major: 1,
        effects: TRANSFORM_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::TransformXmlToData,
        namespace: "cics.transform",
        name: "xml-to-data",
        major: 1,
        effects: TRANSFORM_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::WaitJournalName,
        namespace: "cics.journal",
        name: "wait-journal-name",
        major: 1,
        effects: JOURNAL_WAIT_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::WaitJournalNum,
        namespace: "cics.journal",
        name: "wait-journal-num",
        major: 1,
        effects: JOURNAL_WAIT_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::WriteJournalName,
        namespace: "cics.journal",
        name: "write-journal-name",
        major: 1,
        effects: JOURNAL_WAIT_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::WriteJournalNum,
        namespace: "cics.journal",
        name: "write-journal-num",
        major: 1,
        effects: JOURNAL_WAIT_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::ResetBrowse,
        namespace: "cics.file",
        name: "reset-browse",
        major: 1,
        effects: READ_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::Unlock,
        namespace: "cics.file",
        name: "unlock",
        major: 1,
        effects: READ_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::Getmain64,
        namespace: "cics.storage",
        name: "getmain64",
        major: 1,
        effects: STORAGE_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::Freemain64,
        namespace: "cics.storage",
        name: "freemain64",
        major: 1,
        effects: STORAGE_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::SpoolClose,
        namespace: "cics.spool",
        name: "close",
        major: 1,
        effects: SPOOL_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::SpoolOpenInput,
        namespace: "cics.spool",
        name: "open-input",
        major: 1,
        effects: SPOOL_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::SpoolOpenOutput,
        namespace: "cics.spool",
        name: "open-output",
        major: 1,
        effects: SPOOL_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::SpoolRead,
        namespace: "cics.spool",
        name: "read",
        major: 1,
        effects: SPOOL_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::SpoolWrite,
        namespace: "cics.spool",
        name: "write",
        major: 1,
        effects: SPOOL_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::InvokeService,
        namespace: "cics.web-service",
        name: "invokeservice",
        major: 1,
        effects: WEB_SERVICE_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::SoapFaultAdd,
        namespace: "cics.web-service",
        name: "soapfaultadd",
        major: 1,
        effects: WEB_SERVICE_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::SoapFaultCreate,
        namespace: "cics.web-service",
        name: "soapfaultcreate",
        major: 1,
        effects: WEB_SERVICE_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::SoapFaultDelete,
        namespace: "cics.web-service",
        name: "soapfaultdelete",
        major: 1,
        effects: WEB_SERVICE_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::WsaContextBuild,
        namespace: "cics.web-service",
        name: "wsacontextbuild",
        major: 1,
        effects: WEB_SERVICE_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::WsaContextDelete,
        namespace: "cics.web-service",
        name: "wsacontextdelete",
        major: 1,
        effects: WEB_SERVICE_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::WsaContextGet,
        namespace: "cics.web-service",
        name: "wsacontextget",
        major: 1,
        effects: WEB_SERVICE_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::WsaEprCreate,
        namespace: "cics.web-service",
        name: "wsaeprcreate",
        major: 1,
        effects: WEB_SERVICE_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::DefineCounter,
        namespace: "cics.counter",
        name: "define-counter",
        major: 1,
        effects: COUNTER_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::DefineDCounter,
        namespace: "cics.counter",
        name: "define-dcounter",
        major: 1,
        effects: COUNTER_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::DeleteCounter,
        namespace: "cics.counter",
        name: "delete-counter",
        major: 1,
        effects: COUNTER_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::DeleteDCounter,
        namespace: "cics.counter",
        name: "delete-dcounter",
        major: 1,
        effects: COUNTER_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::GetCounter,
        namespace: "cics.counter",
        name: "get-counter",
        major: 1,
        effects: COUNTER_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::GetDCounter,
        namespace: "cics.counter",
        name: "get-dcounter",
        major: 1,
        effects: COUNTER_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::QueryCounter,
        namespace: "cics.counter",
        name: "query-counter",
        major: 1,
        effects: COUNTER_QUERY_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::QueryDCounter,
        namespace: "cics.counter",
        name: "query-dcounter",
        major: 1,
        effects: COUNTER_QUERY_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::RewindCounter,
        namespace: "cics.counter",
        name: "rewind-counter",
        major: 1,
        effects: COUNTER_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::RewindDCounter,
        namespace: "cics.counter",
        name: "rewind-dcounter",
        major: 1,
        effects: COUNTER_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::UpdateCounter,
        namespace: "cics.counter",
        name: "update-counter",
        major: 1,
        effects: COUNTER_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::UpdateDCounter,
        namespace: "cics.counter",
        name: "update-dcounter",
        major: 1,
        effects: COUNTER_EFFECTS,
        operation: CicsPlanOperation::EnterTraceNum,
        namespace: "cics.diagnostics",
        name: "enter-tracenum",
        major: 1,
        effects: DIAGNOSTIC_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::DefineInputEvent,
        namespace: "cics.event",
        name: "define-input-event",
        major: 1,
        effects: EVENT_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::DefineCompositeEvent,
        namespace: "cics.event",
        name: "define-composite-event",
        major: 1,
        effects: EVENT_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::AddSubevent,
        namespace: "cics.event",
        name: "add-subevent",
        major: 1,
        effects: EVENT_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::RemoveSubevent,
        namespace: "cics.event",
        name: "remove-subevent",
        major: 1,
        effects: EVENT_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::DeleteEvent,
        namespace: "cics.event",
        name: "delete-event",
        major: 1,
        effects: EVENT_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::CheckTimer,
        namespace: "cics.event",
        name: "check-timer",
        major: 1,
        effects: EVENT_TIMER_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::DefineTimer,
        namespace: "cics.event",
        name: "define-timer",
        major: 1,
        effects: EVENT_TIMER_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::DeleteTimer,
        namespace: "cics.event",
        name: "delete-timer",
        major: 1,
        effects: EVENT_TIMER_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::ForceTimer,
        namespace: "cics.event",
        name: "force-timer",
        major: 1,
        effects: EVENT_TIMER_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::RetrieveReattachEvent,
        namespace: "cics.event",
        name: "retrieve-reattach-event",
        major: 1,
        effects: EVENT_TIMER_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::RetrieveSubevent,
        namespace: "cics.event",
        name: "retrieve-subevent",
        major: 1,
        effects: EVENT_TIMER_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::TestEvent,
        namespace: "cics.event",
        name: "test-event",
        major: 1,
        effects: EVENT_TIMER_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::SignalEvent,
        namespace: "cics.event",
        name: "signal-event",
        major: 1,
        effects: EVENT_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::SendPartnset,
        namespace: "cics.terminal",
        name: "send-partnset",
        major: 1,
        effects: TERMINAL_SEND_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::ReceivePartn,
        namespace: "cics.terminal",
        name: "receive-partn",
        major: 1,
        effects: TERMINAL_RECEIVE_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::SendControl,
        namespace: "cics.terminal",
        name: "send-control",
        major: 1,
        effects: TERMINAL_SEND_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::SendPage,
        namespace: "cics.terminal",
        name: "send-page",
        major: 1,
        effects: TERMINAL_SEND_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::IssueAbort,
        namespace: "cics.terminal",
        name: "issue-abort",
        major: 1,
        effects: OUTBOARD_WRITE_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::IssueAdd,
        namespace: "cics.terminal",
        name: "issue-add",
        major: 1,
        effects: OUTBOARD_WRITE_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::IssueEnd,
        namespace: "cics.terminal",
        name: "issue-end",
        major: 1,
        effects: OUTBOARD_WRITE_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::IssueErase,
        namespace: "cics.terminal",
        name: "issue-erase",
        major: 1,
        effects: OUTBOARD_WRITE_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::IssueNote,
        namespace: "cics.terminal",
        name: "issue-note",
        major: 1,
        effects: OUTBOARD_READ_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::IssueQuery,
        namespace: "cics.terminal",
        name: "issue-query",
        major: 1,
        effects: OUTBOARD_READ_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::IssueReceive,
        namespace: "cics.terminal",
        name: "issue-receive",
        major: 1,
        effects: TERMINAL_RECEIVE_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::IssueReplace,
        namespace: "cics.terminal",
        name: "issue-replace",
        major: 1,
        effects: OUTBOARD_WRITE_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::IssueSend,
        namespace: "cics.terminal",
        name: "issue-send",
        major: 1,
        effects: OUTBOARD_WRITE_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::IssueWait,
        namespace: "cics.terminal",
        name: "issue-wait",
        major: 1,
        effects: OUTBOARD_WAIT_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::Route,
        namespace: "cics.terminal",
        name: "route",
        major: 1,
        effects: ROUTE_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
    CicsExecutableDescriptor {
        operation: CicsPlanOperation::Monitor,
        namespace: "cics.diagnostics",
        name: "monitor",
        major: 1,
        effects: MONITOR_EFFECTS,
        runtime_import: CICS_RUNTIME_IMPORT,
    },
];
