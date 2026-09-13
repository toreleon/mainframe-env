//! Executable descriptors for the bounded typed CICS dialect.

use crate::{CicsPlanOperation, Effect, OperationIdentity};

/// Runtime import required by every executable operation in this dialect.
pub const CICS_RUNTIME_IMPORT: &str = "host.cics";

const READ_EFFECTS: &[Effect] = &[
    Effect::DatasetRead,
    Effect::MemoryRead,
    Effect::MemoryWrite,
    Effect::Security,
    Effect::Audit,
    Effect::Condition,
    Effect::Transaction,
];
const REWRITE_EFFECTS: &[Effect] = &[
    Effect::DatasetWrite,
    Effect::MemoryRead,
    Effect::MemoryWrite,
    Effect::Security,
    Effect::Audit,
    Effect::Condition,
    Effect::Transaction,
];
const SYNCPOINT_EFFECTS: &[Effect] = &[
    Effect::MemoryWrite,
    Effect::Security,
    Effect::Audit,
    Effect::Condition,
    Effect::Transaction,
];
const DEQ_EFFECTS: &[Effect] = &[
    Effect::MemoryRead,
    Effect::MemoryWrite,
    Effect::Security,
    Effect::Audit,
    Effect::Condition,
    Effect::Transaction,
];
const ENQ_EFFECTS: &[Effect] = &[
    Effect::MemoryRead,
    Effect::MemoryWrite,
    Effect::Security,
    Effect::Audit,
    Effect::Suspension,
    Effect::Condition,
    Effect::Transaction,
];
const CHANGE_TASK_EFFECTS: &[Effect] = &[
    Effect::MemoryRead,
    Effect::MemoryWrite,
    Effect::Security,
    Effect::Audit,
    Effect::Suspension,
    Effect::Condition,
];
const SUSPEND_EFFECTS: &[Effect] = &[
    Effect::MemoryWrite,
    Effect::Security,
    Effect::Audit,
    Effect::Suspension,
    Effect::Condition,
];
const SET_ASSOCIATION_EFFECTS: &[Effect] = &[
    Effect::MemoryRead,
    Effect::MemoryWrite,
    Effect::Security,
    Effect::Audit,
    Effect::Condition,
];
const ADDRESS_SET_EFFECTS: &[Effect] = &[
    Effect::MemoryRead,
    Effect::MemoryWrite,
    Effect::Security,
    Effect::Audit,
    Effect::Condition,
];
const HANDLE_STACK_EFFECTS: &[Effect] = &[
    Effect::MemoryWrite,
    Effect::Security,
    Effect::Audit,
    Effect::Condition,
];
const IGNORE_CONDITION_EFFECTS: &[Effect] = &[
    Effect::MemoryRead,
    Effect::MemoryWrite,
    Effect::Security,
    Effect::Audit,
    Effect::Condition,
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

include!("generated/cics_application_registry.rs");

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

/// Resolves the unique advertised application row for a host runtime operation.
///
/// Internal-only operations and the separate SPI compatibility route have no
/// application row and therefore return `None`.
#[must_use]
pub fn cics_application_registry_for_runtime_operation(
    runtime_operation: &str,
) -> Option<&'static CicsApplicationRegistryDescriptor> {
    let mut matches = CICS_APPLICATION_REGISTRY.iter().filter(|descriptor| {
        descriptor.advertised && descriptor.runtime_operation == Some(runtime_operation)
    });
    let descriptor = matches.next()?;
    matches.next().is_none().then_some(descriptor)
}

/// Complete registry for the bounded typed CICS executable pilot.
pub const CICS_EXECUTABLE_DESCRIPTORS: [CicsExecutableDescriptor; 12] = [
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
];

/// Resolves the executable descriptor for a decoded CICS plan operation.
#[must_use]
pub const fn cics_executable_descriptor(
    operation: CicsPlanOperation,
) -> &'static CicsExecutableDescriptor {
    match operation {
        CicsPlanOperation::Deq => &CICS_EXECUTABLE_DESCRIPTORS[0],
        CicsPlanOperation::Enq => &CICS_EXECUTABLE_DESCRIPTORS[1],
        CicsPlanOperation::Read => &CICS_EXECUTABLE_DESCRIPTORS[2],
        CicsPlanOperation::Rewrite => &CICS_EXECUTABLE_DESCRIPTORS[3],
        CicsPlanOperation::Syncpoint => &CICS_EXECUTABLE_DESCRIPTORS[4],
        CicsPlanOperation::ChangeTask => &CICS_EXECUTABLE_DESCRIPTORS[5],
        CicsPlanOperation::Suspend => &CICS_EXECUTABLE_DESCRIPTORS[6],
        CicsPlanOperation::SetAssociationUserCorrData => &CICS_EXECUTABLE_DESCRIPTORS[7],
        CicsPlanOperation::AddressSet => &CICS_EXECUTABLE_DESCRIPTORS[8],
        CicsPlanOperation::PopHandle => &CICS_EXECUTABLE_DESCRIPTORS[9],
        CicsPlanOperation::PushHandle => &CICS_EXECUTABLE_DESCRIPTORS[10],
        CicsPlanOperation::IgnoreCondition => &CICS_EXECUTABLE_DESCRIPTORS[11],
    }
}

/// Resolves an executable descriptor without accepting adjacent legacy CICS
/// operation identities.
#[must_use]
pub fn cics_executable_descriptor_for_identity(
    identity: &OperationIdentity,
) -> Option<&'static CicsExecutableDescriptor> {
    CICS_EXECUTABLE_DESCRIPTORS
        .iter()
        .find(|descriptor| descriptor.matches_identity(identity))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn application_registry_is_complete_unique_and_fail_closed() {
        assert_eq!(CICS_APPLICATION_REGISTRY.len(), 263);
        assert_eq!(
            CICS_APPLICATION_REGISTRY
                .iter()
                .map(|descriptor| descriptor.official_row)
                .collect::<BTreeSet<_>>()
                .len(),
            263
        );
        assert_eq!(
            CICS_APPLICATION_REGISTRY
                .iter()
                .map(|descriptor| descriptor.handler_id)
                .collect::<BTreeSet<_>>()
                .len(),
            263
        );
        assert!(CICS_APPLICATION_REGISTRY.iter().all(|descriptor| {
            descriptor.official_row.contains(":api-commands:")
                && !descriptor.official_row.contains(":spi-")
                && !descriptor.official_row.contains(":fepi-")
                && !descriptor.label_tokens.is_empty()
                && descriptor.handler_sha256.starts_with("sha256:")
                && !descriptor.recognition_heads.is_empty()
                && descriptor
                    .options
                    .windows(2)
                    .all(|pair| pair[0].name < pair[1].name)
                && descriptor.options.iter().all(|option| {
                    option.source_max_value_bytes.is_none()
                        || option.value_shape == CicsApplicationOptionValueShape::Value
                })
        }));
        let typed = CICS_APPLICATION_REGISTRY
            .iter()
            .filter(|descriptor| {
                descriptor.readiness == CicsApplicationHandlerReadiness::TypedRuntime
            })
            .collect::<Vec<_>>();
        assert_eq!(typed.len(), 12);
        assert!(typed.iter().all(|descriptor| descriptor.advertised
            && descriptor.runtime_operation.is_some()
            && descriptor.legacy_execution_options.is_empty()));
        let legacy = CICS_APPLICATION_REGISTRY
            .iter()
            .filter(|descriptor| {
                descriptor.readiness == CicsApplicationHandlerReadiness::LegacyCompatibility
            })
            .collect::<Vec<_>>();
        assert_eq!(legacy.len(), 20);
        assert!(legacy.iter().all(|descriptor| descriptor.advertised
            && descriptor.runtime_operation.is_some()
            && !descriptor.legacy_execution_options.is_empty()));
        let unready = CICS_APPLICATION_REGISTRY
            .iter()
            .filter(|descriptor| descriptor.readiness == CicsApplicationHandlerReadiness::Unready)
            .collect::<Vec<_>>();
        assert_eq!(unready.len(), 231);
        assert!(unready.iter().all(|descriptor| !descriptor.advertised
            && descriptor.runtime_operation.is_none()
            && descriptor.legacy_execution_options.is_empty()));
        for descriptor in typed.into_iter().chain(legacy) {
            assert_eq!(
                cics_application_registry_for_runtime_operation(
                    descriptor
                        .runtime_operation
                        .expect("ready runtime operation")
                ),
                Some(descriptor)
            );
        }
        assert_eq!(
            cics_application_registry_for_runtime_operation("Inquire"),
            None
        );
    }

    #[test]
    fn application_registry_lookup_prefers_the_longest_command_label() {
        let asktime = cics_application_registry_for_tokens(&["exec", "cics"]);
        assert_eq!(asktime, None);

        let asktime = cics_application_registry_for_tokens(&["asktime"])
            .expect("ASKTIME must be catalog-known");
        assert_eq!(asktime.label_tokens, ["ASKTIME"]);
        assert_eq!(asktime.readiness, CicsApplicationHandlerReadiness::Unready);
        assert!(!asktime.advertised);

        let absolute = cics_application_registry_for_tokens(&["asktime", "abstime", "target"])
            .expect("ASKTIME ABSTIME must be catalog-known");
        assert_eq!(absolute.label_tokens, ["ASKTIME", "ABSTIME"]);
        assert_eq!(
            absolute.readiness,
            CicsApplicationHandlerReadiness::LegacyCompatibility
        );
        assert!(absolute.advertised);
        assert_eq!(absolute.runtime_operation, Some("Asktime"));
    }

    #[test]
    fn application_registry_exposes_option_shapes_constraints_and_source_heads() {
        let read = CICS_APPLICATION_REGISTRY
            .iter()
            .find(|descriptor| descriptor.label_tokens == ["READ"])
            .expect("READ registry row");
        let file = read
            .options
            .iter()
            .find(|option| option.name == "FILE")
            .expect("READ FILE option");
        assert_eq!(file.value_shape, CicsApplicationOptionValueShape::Value);
        let nohandle = read
            .options
            .iter()
            .find(|option| option.name == "NOHANDLE")
            .expect("common NOHANDLE option");
        assert_eq!(nohandle.value_shape, CicsApplicationOptionValueShape::Flag);
        assert!(
            read.dependencies.iter().any(|dependency| {
                dependency.option == "RESP2" && dependency.requires == ["RESP"]
            })
        );

        let wait = CICS_APPLICATION_REGISTRY
            .iter()
            .find(|descriptor| descriptor.label_tokens == ["WAIT"])
            .expect("WAIT registry row");
        assert_eq!(wait.recognition_heads, [&["GDS", "WAIT"] as &[&str]]);
        assert_eq!(cics_application_registry_for_tokens(&["WAIT"]), None);
        assert!(
            cics_application_registry_candidates_for_tokens(&["WAIT", "CONVID"])
                .all(|candidate| candidate.descriptor.label_tokens != ["WAIT"])
        );
        assert!(
            cics_application_registry_candidates_for_tokens(&["GDS", "WAIT", "CONVID"])
                .any(|candidate| candidate.descriptor.label_tokens == ["WAIT"])
        );

        let acquire = CICS_APPLICATION_REGISTRY
            .iter()
            .find(|descriptor| descriptor.label_tokens == ["ACQUIRE", "ACTIVITYID"])
            .expect("ACQUIRE ACTIVITYID registry row");
        assert_eq!(acquire.recognition_heads, [&["ACQUIRE"] as &[&str]]);
        assert!(acquire.discriminator_options.contains(&"ACTIVITYID"));

        let passticket = cics_application_registry_for_tokens(&[
            "REQUEST",
            "PASSTICKET",
            "(",
            "TARGET",
            ")",
            "ESMAPPNAME",
            "(",
            "APP",
            ")",
        ])
        .expect("REQUEST PASSTICKET valued discriminator must resolve");
        assert_eq!(passticket.label_tokens, ["REQUEST", "PASSTICKET"]);
    }

    #[test]
    fn dynamic_condition_clauses_use_one_eibresp_name_authority() {
        assert_eq!(CICS_APPLICATION_CONDITION_NAMES.len(), 121);
        assert!(
            CICS_APPLICATION_CONDITION_NAMES
                .windows(2)
                .all(|pair| pair[0] < pair[1])
        );
        assert!(CICS_APPLICATION_CONDITION_NAMES.contains(&"NORMAL"));
        assert!(CICS_APPLICATION_CONDITION_NAMES.contains(&"ERROR"));
        assert!(CICS_APPLICATION_CONDITION_NAMES.contains(&"BUSY"));

        for (label, operand) in [
            (
                &["HANDLE", "CONDITION"] as &[&str],
                CicsApplicationConditionLabelOperand::Optional,
            ),
            (
                &["IGNORE", "CONDITION"] as &[&str],
                CicsApplicationConditionLabelOperand::Forbidden,
            ),
        ] {
            let descriptor = CICS_APPLICATION_REGISTRY
                .iter()
                .find(|descriptor| descriptor.label_tokens == label)
                .expect("dynamic condition registry row");
            assert_eq!(descriptor.recognition_heads, &[label]);
            assert!(!descriptor.top_level_options.contains(&"CONDITION-NAME"));
            assert!(
                !descriptor
                    .options
                    .iter()
                    .any(|option| option.name == "CONDITION-NAME")
            );
            let clauses = descriptor
                .condition_clauses
                .expect("condition clause profile");
            assert_eq!(clauses.name_authority, "cics-eibresp-condition-name@1");
            assert_eq!(
                clauses.name_authority_sha256,
                CICS_APPLICATION_CONDITION_AUTHORITY_SHA256
            );
            assert_eq!(
                (clauses.minimum_occurrences, clauses.maximum_occurrences),
                (1, 16)
            );
            assert_eq!(clauses.label_operand, operand);
        }
    }

    #[test]
    fn executable_registry_is_complete_unique_and_round_trips() {
        assert_eq!(
            CICS_EXECUTABLE_DESCRIPTORS
                .iter()
                .map(|descriptor| descriptor.operation)
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([
                CicsPlanOperation::AddressSet,
                CicsPlanOperation::ChangeTask,
                CicsPlanOperation::Deq,
                CicsPlanOperation::Enq,
                CicsPlanOperation::IgnoreCondition,
                CicsPlanOperation::PopHandle,
                CicsPlanOperation::PushHandle,
                CicsPlanOperation::Read,
                CicsPlanOperation::Rewrite,
                CicsPlanOperation::Syncpoint,
                CicsPlanOperation::SetAssociationUserCorrData,
                CicsPlanOperation::Suspend,
            ])
        );
        assert_eq!(
            CICS_EXECUTABLE_DESCRIPTORS
                .iter()
                .map(|descriptor| descriptor.identity())
                .collect::<BTreeSet<_>>()
                .len(),
            CICS_EXECUTABLE_DESCRIPTORS.len()
        );
        for descriptor in CICS_EXECUTABLE_DESCRIPTORS {
            assert_eq!(
                cics_executable_descriptor(descriptor.operation),
                &descriptor
            );
            assert_eq!(
                cics_executable_descriptor_for_identity(&descriptor.identity()),
                Some(&descriptor)
            );
            assert!(!descriptor.effects.is_empty());
            assert_eq!(descriptor.runtime_import, CICS_RUNTIME_IMPORT);
        }
    }
}
