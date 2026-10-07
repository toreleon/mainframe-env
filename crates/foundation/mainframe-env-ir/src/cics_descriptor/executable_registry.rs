use super::*;

/// Static operation facts owned by the typed CICS dialect.
///
/// Option direction and operation-specific plan shape remain owned by the
/// CICS plan codec validator. Host request mapping and provider transitions
/// deliberately do not belong in this descriptor.
/// Reserved identities become executable only when [`Self::is_registered`] is true.
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
    /// Reports whether the generated application registry admits this plan.
    ///
    /// Reserved codec identities alone do not grant execution readiness.
    #[must_use]
    pub fn is_registered(self) -> bool {
        CICS_REGISTERED_PLAN_OPERATIONS.contains(&self.operation)
    }

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

/// Source-reviewed symbolic values for one scoped CVDA operand.
///
/// This does not supply numeric codes, implied flag aliases, context predicates,
/// completeness, or compiler admission. Its enclosing contract owns readiness.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CicsApplicationCvdaDomain {
    /// Declared valued operand whose CVDA symbols are reviewed.
    pub option: &'static str,
    /// Sorted unique source symbols; numeric encoding is a separate authority.
    pub values: &'static [&'static str],
}

/// One explicitly source-reviewed fullword CVDA representation.
///
/// Equal numbers do not establish equivalent commands, operands or flag aliases.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CicsApplicationCvdaNumericValue {
    /// Exact source symbol, retained independently from other equal encodings.
    pub symbol: &'static str,
    /// Reference numeric value, represented as a signed fullword.
    pub number: i32,
}

/// Reviewed numeric representations for a subset of a scoped symbolic domain.
///
/// Missing symbols remain unresolved. This is immutable source metadata, not a
/// value mapper, ABI encoder, complete domain or compiler-admission authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CicsApplicationCvdaNumericDomain {
    /// Existing valued fullword operand whose symbolic domain owns these names.
    pub option: &'static str,
    /// Exact baseline of the numeric reference, separate from the command body.
    pub source_baseline: &'static str,
    /// Pinned numeric-reference topic path.
    pub source_topic: &'static str,
    /// Exact numeric-reference SHA-256, with the sha256 prefix.
    pub source_sha256: &'static str,
    /// Sorted unique symbols with explicit source representations.
    pub values: &'static [CicsApplicationCvdaNumericValue],
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
