//! Catalog-bound recognition of top-level EXEC CICS command clauses.
use super::{
    HirCicsConditionPolicy, HirCicsNamedOperand, HirCicsOperandName, HirCicsOperation,
    HirCicsOption, HirCicsOutputBinding, HirCicsOutputName, HirCicsStatement, HirCicsValue,
    Resolution, ResolutionFailure, numeric_literal, require_numeric, require_writable,
};
use crate::{CobolUsage, SemanticModel};
use mainframe_env_ir::{
    CICS_APPLICATION_AID_NAMES, CICS_APPLICATION_CONDITION_NAMES,
    CicsApplicationCobolApplicability, CicsApplicationConditionLabelOperand,
    CicsApplicationConstraintStatus, CicsApplicationHandlerReadiness,
    CicsApplicationOptionValueShape, CicsApplicationRegistryDescriptor,
    cics_application_registry_candidates_for_tokens,
};
use std::collections::{BTreeMap, BTreeSet};
type Clauses = BTreeMap<String, Vec<String>>;
mod abend;
mod address;
mod assign_validation;
mod clause_parser;
mod command_recognition;
mod counter_control;
mod diagnostics;
mod document_control;
mod event_control;
mod file_operands;
mod format_time;
mod handle_abend;
mod interval_control;
mod journal_control;
mod legacy_compatibility;
mod numeric_value;
mod operation;
mod outboard;
mod output_bindings;
mod program_control;
mod program_name;
mod queue_control;
mod route;
mod shape;
mod spool_control;
mod storage_control;
mod task_wait;
mod terminal_control;
mod transaction_name;
mod transform_control;
mod value;
mod web_service_control;

use clause_parser::{clauses, matching_close};
use numeric_value::{cics_cvda_value, cics_integer_value};
use value::{cics_address_value, cics_value, complete_data_reference, output};

struct ValidatedCandidate {
    descriptor: &'static CicsApplicationRegistryDescriptor,
    clauses: Clauses,
    options: Vec<String>,
    head_len: usize,
}
struct CandidateFailure {
    score: (usize, usize, usize),
    detail: String,
}
pub(super) fn validated_command(
    body: &[String],
    semantic: &SemanticModel,
) -> Resolution<(
    &'static CicsApplicationRegistryDescriptor,
    Clauses,
    Vec<String>,
)> {
    let candidates = cics_application_registry_candidates_for_tokens(body).collect::<Vec<_>>();
    if candidates.is_empty() {
        return Err(ResolutionFailure::Invalid(format!(
            "unknown CICS application command: {}",
            body.first().map_or("<empty>", String::as_str)
        )));
    }

    let mut valid = BTreeMap::<&'static str, ValidatedCandidate>::new();
    let mut best_failure: Option<CandidateFailure> = None;
    for candidate in candidates {
        let tokens =
            command_recognition::clause_tokens(body, candidate.head_tokens, candidate.descriptor);
        let (clauses, options) = match clauses(&tokens, Some(candidate.descriptor)) {
            Ok(parsed) => parsed,
            Err(ResolutionFailure::Invalid(detail)) => {
                keep_best_failure(
                    &mut best_failure,
                    CandidateFailure {
                        score: (candidate.head_tokens.len(), 0, 0),
                        detail,
                    },
                );
                continue;
            }
            Err(ResolutionFailure::Unsupported) => unreachable!("clause parser is fail-closed"),
        };
        let present = clauses
            .keys()
            .chain(options.iter())
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        let discriminator_matches = candidate
            .descriptor
            .discriminator_options
            .iter()
            .filter(|name| present.contains(**name))
            .count();
        let recognized_options = present
            .iter()
            .filter(|name| option_is_known(candidate.descriptor, name))
            .count();
        if let Err(detail) =
            validate_candidate(candidate.descriptor, &clauses, &options, &present, semantic)
        {
            keep_best_failure(
                &mut best_failure,
                CandidateFailure {
                    score: (
                        candidate.head_tokens.len(),
                        discriminator_matches,
                        recognized_options,
                    ),
                    detail,
                },
            );
            continue;
        }
        if candidate.descriptor.readiness == CicsApplicationHandlerReadiness::LegacyCompatibility
            && let Err(detail) = validate_legacy_execution_subset(candidate.descriptor, &present)
        {
            keep_best_failure(
                &mut best_failure,
                CandidateFailure {
                    score: (
                        candidate.head_tokens.len(),
                        discriminator_matches,
                        recognized_options,
                    ),
                    detail,
                },
            );
            continue;
        }

        let validated = ValidatedCandidate {
            descriptor: candidate.descriptor,
            clauses,
            options,
            head_len: candidate.head_tokens.len(),
        };
        match valid.get(candidate.descriptor.official_row) {
            Some(existing) if existing.head_len >= validated.head_len => {}
            _ => {
                valid.insert(candidate.descriptor.official_row, validated);
            }
        }
    }
    match valid.len() {
        1 => {
            let candidate = valid.into_values().next().expect("one validated candidate");
            Ok((candidate.descriptor, candidate.clauses, candidate.options))
        }
        0 => Err(ResolutionFailure::Invalid(best_failure.map_or_else(
            || "CICS command does not match a source-reviewed application form".into(),
            |failure| failure.detail,
        ))),
        _ => Err(ResolutionFailure::Invalid(format!(
            "CICS command is ambiguous across source-reviewed forms: {}",
            valid
                .values()
                .map(|candidate| candidate.descriptor.label_tokens.join(" "))
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

fn keep_best_failure(best: &mut Option<CandidateFailure>, candidate: CandidateFailure) {
    if best
        .as_ref()
        .is_none_or(|current| candidate.score > current.score)
    {
        *best = Some(candidate);
    }
}

fn validate_candidate(
    descriptor: &CicsApplicationRegistryDescriptor,
    clauses: &Clauses,
    options: &[String],
    present: &BTreeSet<&str>,
    semantic: &SemanticModel,
) -> Result<(), String> {
    if descriptor.recognition_status == CicsApplicationConstraintStatus::Pending
        || descriptor.constraint_status == CicsApplicationConstraintStatus::Pending
    {
        return Err(format!(
            "CICS {} has an unfrozen source contract",
            operation::command_label(descriptor)
        ));
    }

    let mut canonical_spellings = BTreeMap::<&str, &str>::new();
    for name in present {
        let canonical = compatibility_alias_target(descriptor, name).unwrap_or(name);
        if let Some(existing) = canonical_spellings.insert(canonical, name) {
            return Err(format!(
                "CICS {} options {existing} and {name} are aliases and mutually exclusive",
                operation::command_label(descriptor)
            ));
        }
    }

    let mut condition_clause_count = 0usize;
    let mut aid_clause_count = 0usize;
    for name in present {
        let Some(shape) = option_value_shape(descriptor, name) else {
            if let Some(condition_clauses) = descriptor.condition_clauses
                && is_condition_name(name)
            {
                condition_clause_count += 1;
                let value = clauses.get(*name);
                match (condition_clauses.label_operand, value) {
                    (CicsApplicationConditionLabelOperand::Optional, Some(tokens))
                        if !is_single_condition_label(tokens) =>
                    {
                        return Err(format!(
                            "CICS {} condition {name} requires one label operand",
                            operation::command_label(descriptor)
                        ));
                    }
                    (CicsApplicationConditionLabelOperand::Optional, _) => {}
                    (CicsApplicationConditionLabelOperand::Forbidden, Some(_)) => {
                        return Err(format!(
                            "CICS {} condition {name} forbids a label operand",
                            operation::command_label(descriptor)
                        ));
                    }
                    (CicsApplicationConditionLabelOperand::Forbidden, None) => {}
                }
                continue;
            }
            if descriptor.label_tokens == ["HANDLE", "AID"] && is_aid_name(name) {
                aid_clause_count += 1;
                if clauses
                    .get(*name)
                    .is_some_and(|tokens| !is_single_condition_label(tokens))
                {
                    return Err(format!(
                        "CICS HANDLE AID option {name} requires one label operand"
                    ));
                }
                continue;
            }
            return Err(format!(
                "CICS {} has unknown or unreviewed top-level option {name}",
                operation::command_label(descriptor)
            ));
        };
        let has_value = clauses.contains_key(*name);
        if descriptor.label_tokens == ["DUMP", "TRANSACTION"] && *name == "DUMPID" && has_value {
            continue;
        }
        match (shape, has_value) {
            (CicsApplicationOptionValueShape::Flag, true) => {
                return Err(format!(
                    "CICS {} option {name} is a flag and rejects a parenthesized operand",
                    operation::command_label(descriptor)
                ));
            }
            (CicsApplicationOptionValueShape::Value, false) => {
                return Err(format!(
                    "CICS {} option {name} requires a parenthesized operand",
                    operation::command_label(descriptor)
                ));
            }
            (CicsApplicationOptionValueShape::BoundedAmbiguity, _) => {
                return Err(format!(
                    "CICS {} option {name} has a source-bounded operand shape",
                    operation::command_label(descriptor)
                ));
            }
            _ => {}
        }
    }
    if let Some(condition_clauses) = descriptor.condition_clauses
        && !(condition_clauses.minimum_occurrences..=condition_clauses.maximum_occurrences)
            .contains(&condition_clause_count)
    {
        return Err(format!(
            "CICS {} requires {}..={} EIBRESP condition clauses, found {condition_clause_count}",
            operation::command_label(descriptor),
            condition_clauses.minimum_occurrences,
            condition_clauses.maximum_occurrences,
        ));
    }
    if descriptor.label_tokens == ["HANDLE", "AID"] && aid_clause_count > 16 {
        return Err(format!(
            "CICS HANDLE AID permits at most 16 AID clauses, found {aid_clause_count}"
        ));
    }

    if !descriptor
        .required_discriminator_options
        .iter()
        .all(|name| option_is_present(descriptor, present, name))
    {
        return Err(format!(
            "CICS {} is missing a required command discriminator",
            operation::command_label(descriptor)
        ));
    }
    if let Some(name) = descriptor
        .forbidden_discriminator_options
        .iter()
        .find(|name| option_is_present(descriptor, present, name))
    {
        return Err(format!(
            "CICS {} forbids discriminator {name}",
            operation::command_label(descriptor)
        ));
    }
    if descriptor.required_discriminator_options.is_empty()
        && descriptor.forbidden_discriminator_options.is_empty()
        && !descriptor.discriminator_options.is_empty()
        && !descriptor
            .discriminator_options
            .iter()
            .any(|name| option_is_present(descriptor, present, name))
    {
        return Err(format!(
            "CICS {} is missing a source-reviewed command discriminator",
            operation::command_label(descriptor)
        ));
    }

    match descriptor.cobol_applicability {
        CicsApplicationCobolApplicability::Allowed => {}
        CicsApplicationCobolApplicability::NotApplicable => {
            return Err(format!(
                "CICS {} is not applicable to COBOL",
                operation::command_label(descriptor)
            ));
        }
        CicsApplicationCobolApplicability::Conditional
        | CicsApplicationCobolApplicability::BoundedAmbiguity => {
            return Err(format!(
                "CICS {} has no unconditional source-reviewed COBOL form",
                operation::command_label(descriptor)
            ));
        }
    }

    if descriptor.runtime_operation == Some("Assign") {
        assign_validation::validate(clauses, present, semantic)?;
    }
    journal_control::validate_candidate(descriptor, clauses)?;

    if let Some(name) = descriptor
        .required_options
        .iter()
        .find(|name| !option_is_present(descriptor, present, name))
    {
        return Err(format!(
            "CICS {} requires option {name}",
            operation::command_label(descriptor)
        ));
    }
    for alternative in descriptor.alternative_groups {
        let count = alternative
            .members
            .iter()
            .filter(|name| option_is_present(descriptor, present, name))
            .count();
        if alternative.required && count == 0 {
            return Err(format!(
                "CICS {} requires one of {}",
                operation::command_label(descriptor),
                alternative.members.join(", ")
            ));
        }
    }
    for dependency in descriptor.dependencies {
        if option_is_present(descriptor, present, dependency.option)
            && let Some(required) = dependency
                .requires
                .iter()
                .find(|required| !option_is_present(descriptor, present, required))
        {
            return Err(format!(
                "CICS {} option {} requires {required}",
                operation::command_label(descriptor),
                dependency.option
            ));
        }
    }
    for group in descriptor.mutual_exclusion_groups {
        let selected = group
            .iter()
            .filter(|name| option_is_present(descriptor, present, name))
            .copied()
            .collect::<Vec<_>>();
        if selected.len() > 1 {
            return Err(format!(
                "CICS {} options {} are mutually exclusive",
                operation::command_label(descriptor),
                selected.join(", ")
            ));
        }
    }

    for (name, value) in clauses {
        let Some(limit) = descriptor
            .options
            .iter()
            .find(|option| option.name == name)
            .and_then(|option| option.source_max_value_bytes)
        else {
            continue;
        };
        if let Some(bytes) = statically_known_value_bytes(value, semantic)
            && bytes > limit
        {
            return Err(format!(
                "CICS {} option {name} exceeds its source maximum of {limit} bytes",
                operation::command_label(descriptor)
            ));
        }
    }

    // Parsing keeps valued and flag options separate; use both here so future
    // callers cannot accidentally validate only one representation.
    debug_assert_eq!(present.len(), clauses.len() + options.len());
    Ok(())
}

fn option_is_known(descriptor: &CicsApplicationRegistryDescriptor, name: &str) -> bool {
    option_value_shape(descriptor, name).is_some()
        || (descriptor.condition_clauses.is_some() && is_condition_name(name))
        || (descriptor.label_tokens == ["HANDLE", "AID"] && is_aid_name(name))
}

fn is_condition_name(name: &str) -> bool {
    CICS_APPLICATION_CONDITION_NAMES
        .binary_search(&name)
        .is_ok()
}

fn is_aid_name(name: &str) -> bool {
    CICS_APPLICATION_AID_NAMES.binary_search(&name).is_ok()
}

fn is_single_condition_label(tokens: &[String]) -> bool {
    matches!(tokens, [label] if !label.is_empty() && label.chars().all(|character| {
        character.is_ascii_alphanumeric() || character == '-'
    }))
}

fn option_value_shape(
    descriptor: &CicsApplicationRegistryDescriptor,
    name: &str,
) -> Option<CicsApplicationOptionValueShape> {
    if matches!(
        descriptor.label_tokens,
        ["ISSUE", "ABORT" | "END" | "SEND" | "WAIT"]
    ) && matches!(name, "WPMEDIA2" | "WPMEDIA3" | "WPMEDIA4")
    {
        // The pinned prose explicitly lists all four media; the projected
        // syntax diagram currently materializes only WPMEDIA1.
        return Some(CicsApplicationOptionValueShape::Flag);
    }
    journal_control::option_value_shape(descriptor, name).or_else(|| {
        descriptor
            .options
            .iter()
            .find(|option| option.name == name)
            .map(|option| option.value_shape)
            .or_else(|| {
                compatibility_alias_target(descriptor, name).and_then(|canonical| {
                    descriptor
                        .options
                        .iter()
                        .find(|option| option.name == canonical)
                        .map(|option| option.value_shape)
                })
            })
    })
}

fn compatibility_alias_target(
    descriptor: &CicsApplicationRegistryDescriptor,
    name: &str,
) -> Option<&'static str> {
    (name == "DATASET"
        && descriptor.family == "file-control"
        && descriptor
            .options
            .iter()
            .any(|option| option.name == "FILE")
        && descriptor
            .options
            .iter()
            .all(|option| option.name != "DATASET"))
    .then_some("FILE")
}

fn option_is_present(
    descriptor: &CicsApplicationRegistryDescriptor,
    present: &BTreeSet<&str>,
    name: &str,
) -> bool {
    present.contains(name)
        || present
            .iter()
            .any(|candidate| compatibility_alias_target(descriptor, candidate) == Some(name))
}

fn validate_legacy_execution_subset(
    descriptor: &CicsApplicationRegistryDescriptor,
    present: &BTreeSet<&str>,
) -> Result<(), String> {
    if descriptor.legacy_execution_options.is_empty() {
        return Err(format!(
            "CICS {} has no frozen legacy execution option subset",
            operation::command_label(descriptor)
        ));
    }
    let unready = present
        .iter()
        .filter(|name| {
            let canonical = compatibility_alias_target(descriptor, name).unwrap_or(name);
            !descriptor.legacy_execution_options.contains(&canonical)
                && !(descriptor.condition_clauses.is_some() && is_condition_name(name))
        })
        .copied()
        .collect::<Vec<_>>();
    if !unready.is_empty() {
        return Err(format!(
            "CICS {} is catalog-known but legacy execution is unready for {}",
            operation::command_label(descriptor),
            unready.join(", ")
        ));
    }
    Ok(())
}

fn statically_known_value_bytes(tokens: &[String], semantic: &SemanticModel) -> Option<usize> {
    if let [literal] = tokens
        && literal.len() >= 2
        && let Some(quote) = literal.chars().next()
        && matches!(quote, '\'' | '"')
        && literal.ends_with(quote)
    {
        let contents = &literal[quote.len_utf8()..literal.len() - quote.len_utf8()];
        let escaped = format!("{quote}{quote}");
        return Some(contents.replace(&escaped, &quote.to_string()).len());
    }
    semantic
        .resolve(&tokens.join(" "))
        .ok()
        .map(|layout| layout.length)
}

pub(super) fn resolve(tokens: &[String], semantic: &SemanticModel) -> Resolution<HirCicsStatement> {
    let mut body = tokens;
    if body
        .first()
        .is_some_and(|token| token.eq_ignore_ascii_case("CICS"))
    {
        body = &body[1..];
    }
    if body
        .last()
        .is_some_and(|token| token.eq_ignore_ascii_case("END-EXEC"))
    {
        body = &body[..body.len() - 1];
    }
    if legacy_compatibility::validated(body)?.is_some() {
        return Err(ResolutionFailure::Unsupported);
    }
    let (descriptor, clauses, raw_options) = validated_command(body, semantic)?;
    match descriptor.readiness {
        CicsApplicationHandlerReadiness::TypedRuntime => {}
        CicsApplicationHandlerReadiness::LegacyCompatibility => {
            return Err(ResolutionFailure::Unsupported);
        }
        CicsApplicationHandlerReadiness::Unready => {
            return Err(ResolutionFailure::Invalid(format!(
                "CICS application command {} is catalog-known but its handler is unready",
                descriptor.label_tokens.join(" ")
            )));
        }
    }
    let operation = operation::resolve(descriptor)?;
    let transform_shape = transform_control::shape(operation);
    let web_shape = web_service_control::shape(operation);
    let event_shape = event_control::shape(operation);
    let command_shape = transform_shape.as_ref().or(event_shape.as_ref());
    let allowed_clauses: &[&str] = match operation {
        HirCicsOperation::Abend => &["ABCODE", "RESP", "RESP2"],
        HirCicsOperation::Address => &["COMMAREA", "RESP", "RESP2"],
        HirCicsOperation::AddressSet => &["SET", "USING", "RESP", "RESP2"],
        HirCicsOperation::Asktime => &["ABSTIME", "RESP", "RESP2"],
        HirCicsOperation::AsktimeEib => &["RESP", "RESP2"],
        HirCicsOperation::FormatTime => &[
            "ABSTIME",
            "DATESEP",
            "MILLISECONDS",
            "MMDDYY",
            "MMDDYYYY",
            "RESP",
            "RESP2",
            "TIME",
            "TIMESEP",
            "YYDDD",
            "YYMMDD",
            "YYYYMMDD",
        ],
        HirCicsOperation::ChangeTask => &["PRIORITY", "RESP", "RESP2"],
        HirCicsOperation::Deq | HirCicsOperation::Enq => {
            &["RESOURCE", "LENGTH", "MAXLIFETIME", "RESP", "RESP2"]
        }
        HirCicsOperation::HandleAbend => &["LABEL", "PROGRAM", "RESP", "RESP2"],
        HirCicsOperation::HandleAid
        | HirCicsOperation::HandleCondition
        | HirCicsOperation::IgnoreCondition
        | HirCicsOperation::PopHandle
        | HirCicsOperation::PushHandle => &["RESP", "RESP2"],
        HirCicsOperation::InvokeApplication => program_control::INVOKE_CLAUSES,
        op @ (HirCicsOperation::InvokeService
        | HirCicsOperation::SoapFaultAdd
        | HirCicsOperation::SoapFaultCreate
        | HirCicsOperation::SoapFaultDelete
        | HirCicsOperation::WsaContextBuild
        | HirCicsOperation::WsaContextDelete
        | HirCicsOperation::WsaContextGet
        | HirCicsOperation::WsaEprCreate) => {
            web_service_control::shape(op).expect("web shape").clauses
        }
        HirCicsOperation::Route => route::ALLOWED_CLAUSES,
        HirCicsOperation::Load => program_control::LOAD_CLAUSES,
        HirCicsOperation::Release => program_control::RELEASE_CLAUSES,
        HirCicsOperation::Link | HirCicsOperation::Xctl => &[
            "PROGRAM",
            "COMMAREA",
            "LENGTH",
            "DATALENGTH",
            "RESP",
            "RESP2",
        ],
        HirCicsOperation::Return => &["TRANSID", "COMMAREA", "LENGTH", "RESP", "RESP2"],
        op @ (HirCicsOperation::StartBrowse
        | HirCicsOperation::ResetBrowse
        | HirCicsOperation::ReadNext
        | HirCicsOperation::ReadPrev
        | HirCicsOperation::EndBrowse
        | HirCicsOperation::Delete
        | HirCicsOperation::Unlock
        | HirCicsOperation::Write
        | HirCicsOperation::Read
        | HirCicsOperation::Rewrite) => file_operands::allowed_clauses(op),
        HirCicsOperation::WriteTransientData => {
            &["QUEUE", "FROM", "LENGTH", "SYSID", "RESP", "RESP2"]
        }
        HirCicsOperation::ReadTransientData => {
            &["QUEUE", "INTO", "SET", "LENGTH", "SYSID", "RESP", "RESP2"]
        }
        HirCicsOperation::DeleteTransientData => &["QUEUE", "SYSID", "RESP", "RESP2"],
        HirCicsOperation::DeleteTemporaryStorage => &["QNAME", "QUEUE", "SYSID", "RESP", "RESP2"],
        HirCicsOperation::ReadTemporaryStorage => &[
            "QNAME", "QUEUE", "INTO", "SET", "LENGTH", "NUMITEMS", "ITEM", "SYSID", "RESP", "RESP2",
        ],
        HirCicsOperation::WriteTemporaryStorage => &[
            "QNAME", "QUEUE", "FROM", "LENGTH", "NUMITEMS", "ITEM", "SYSID", "RESP", "RESP2",
        ],
        HirCicsOperation::DocumentCreate => document_control::ALLOWED_CLAUSES,
        HirCicsOperation::DocumentDelete => document_control::DELETE_CLAUSES,
        HirCicsOperation::DocumentInsert => document_control::INSERT_CLAUSES,
        HirCicsOperation::DocumentRetrieve => document_control::RETRIEVE_CLAUSES,
        HirCicsOperation::DocumentSet => document_control::SET_CLAUSES,
        HirCicsOperation::Freemain => &["DATA", "DATAPOINTER", "RESP", "RESP2"],
        HirCicsOperation::Getmain => &["FLENGTH", "LENGTH", "INITIMG", "SET", "RESP", "RESP2"],
        HirCicsOperation::ReceiveMap => {
            &["MAP", "MAPSET", "FROM", "INTO", "LENGTH", "RESP", "RESP2"]
        }
        HirCicsOperation::SendMap => &["MAP", "MAPSET", "FROM", "LENGTH", "RESP", "RESP2"],
        HirCicsOperation::SendText => &["FROM", "LENGTH", "RESP", "RESP2"],
        HirCicsOperation::SendPartnset => &["PARTNSET", "RESP", "RESP2"],
        HirCicsOperation::ReceivePartn => &["PARTN", "INTO", "SET", "LENGTH", "RESP", "RESP2"],
        HirCicsOperation::SendControl => &[
            "CURSOR", "MSR", "OUTPARTN", "ACTPARTN", "LDC", "REQID", "SET", "RESP", "RESP2",
        ],
        HirCicsOperation::SendPage => &["TRANSID", "TRAILER", "SET", "FMHPARM", "RESP", "RESP2"],
        HirCicsOperation::Assign => &["RESP", "RESP2"],
        HirCicsOperation::Cancel => &["REQID", "TRANSID", "RESP", "RESP2"],
        HirCicsOperation::Delay => interval_control::DELAY_CLAUSES,
        HirCicsOperation::PurgeMessage => &["RESP", "RESP2"],
        HirCicsOperation::SetAssociationUserCorrData => &["USERCORRDATA", "RESP", "RESP2"],
        HirCicsOperation::Syncpoint => &["RESP", "RESP2"],
        HirCicsOperation::Suspend => &["RESP", "RESP2"],
        op @ (HirCicsOperation::WaitEvent | HirCicsOperation::WaitExternal) => task_wait::names(op),
        HirCicsOperation::Start => interval_control::START_CLAUSES,
        HirCicsOperation::Retrieve => interval_control::RETRIEVE_CLAUSES,
        HirCicsOperation::WaitJournalName
        | HirCicsOperation::WaitJournalNum
        | HirCicsOperation::WriteJournalName
        | HirCicsOperation::WriteJournalNum => journal_control::allowed_clauses(operation),
        operation if counter_control::is_counter(operation) => {
            counter_control::allowed_clauses(operation)
        }
        op if outboard::is_issue(op) => outboard::allowed_clauses(op),
        HirCicsOperation::SpoolClose
        | HirCicsOperation::SpoolOpenInput
        | HirCicsOperation::SpoolOpenOutput
        | HirCicsOperation::SpoolRead
        | HirCicsOperation::SpoolWrite => spool_control::allowed_clauses(operation),
        HirCicsOperation::EnterTraceNum => diagnostics::allowed_clauses(operation),
        HirCicsOperation::Monitor => diagnostics::allowed_clauses(operation),
        HirCicsOperation::DumpTransaction => diagnostics::allowed_clauses(operation),
        _ => {
            command_shape
                .as_ref()
                .ok_or(ResolutionFailure::Unsupported)?
                .clauses
        }
    };
    let allowed_options: &[&str] = match operation {
        HirCicsOperation::Abend => &["CANCEL", "NODUMP", "NOHANDLE"],
        HirCicsOperation::HandleAbend => &["CANCEL", "RESET", "NOHANDLE"],
        HirCicsOperation::InvokeApplication => &["EXACTMATCH", "MINIMUM", "NOHANDLE"],
        HirCicsOperation::InvokeService
        | HirCicsOperation::SoapFaultAdd
        | HirCicsOperation::SoapFaultCreate
        | HirCicsOperation::SoapFaultDelete
        | HirCicsOperation::WsaContextBuild
        | HirCicsOperation::WsaContextDelete
        | HirCicsOperation::WsaContextGet
        | HirCicsOperation::WsaEprCreate => &["NOHANDLE"],
        HirCicsOperation::Route => route::ALLOWED_OPTIONS,
        HirCicsOperation::Load => &["HOLD", "NOHANDLE"],
        HirCicsOperation::Release => &["NOHANDLE"],
        HirCicsOperation::Address
        | HirCicsOperation::AddressSet
        | HirCicsOperation::Asktime
        | HirCicsOperation::AsktimeEib
        | HirCicsOperation::ChangeTask
        | HirCicsOperation::HandleAid
        | HirCicsOperation::HandleCondition
        | HirCicsOperation::IgnoreCondition
        | HirCicsOperation::Link
        | HirCicsOperation::Xctl
        | HirCicsOperation::Return
        | HirCicsOperation::ReadNext
        | HirCicsOperation::ReadPrev
        | HirCicsOperation::EndBrowse
        | HirCicsOperation::Delete
        | HirCicsOperation::Unlock
        | HirCicsOperation::Write
        | HirCicsOperation::WriteTransientData
        | HirCicsOperation::ReadTransientData
        | HirCicsOperation::DeleteTransientData
        | HirCicsOperation::DeleteTemporaryStorage
        | HirCicsOperation::DocumentDelete
        | HirCicsOperation::DocumentInsert
        | HirCicsOperation::Freemain
        | HirCicsOperation::Assign
        | HirCicsOperation::PurgeMessage
        | HirCicsOperation::PopHandle
        | HirCicsOperation::PushHandle
        | HirCicsOperation::SetAssociationUserCorrData
        | HirCicsOperation::Suspend
        | HirCicsOperation::WaitEvent => &["NOHANDLE"],
        HirCicsOperation::WaitExternal => task_wait::WAIT_EXTERNAL_OPTIONS,
        HirCicsOperation::ReadTemporaryStorage => &["NEXT", "NOHANDLE"],
        HirCicsOperation::WriteTemporaryStorage => {
            &["AUXILIARY", "MAIN", "NOSUSPEND", "REWRITE", "NOHANDLE"]
        }
        HirCicsOperation::DocumentCreate => document_control::ALLOWED_OPTIONS,
        HirCicsOperation::DocumentRetrieve => document_control::RETRIEVE_OPTIONS,
        HirCicsOperation::DocumentSet => document_control::ALLOWED_OPTIONS,
        HirCicsOperation::Start => &["AFTER", "AT", "FMH", "PROTECT", "NOCHECK", "NOHANDLE"],
        HirCicsOperation::Cancel => &["NOHANDLE"],
        HirCicsOperation::Delay => &["FOR", "UNTIL", "NOHANDLE"],
        HirCicsOperation::Retrieve => &["WAIT", "NOHANDLE"],
        HirCicsOperation::FormatTime => &["DATESEP", "TIMESEP", "NOHANDLE"],
        HirCicsOperation::ReceiveMap => &["TERMINAL", "NOHANDLE"],
        HirCicsOperation::SendMap => &[
            "DATAONLY", "ERASE", "CURSOR", "FREEKB", "MAPONLY", "NOHANDLE",
        ],
        HirCicsOperation::SendText => &["ERASE", "FREEKB", "NOHANDLE"],
        HirCicsOperation::SendPartnset => &["NOHANDLE"],
        HirCicsOperation::ReceivePartn => &["ASIS", "NOHANDLE"],
        HirCicsOperation::SendControl => terminal_control::SEND_CONTROL_OPTIONS,
        HirCicsOperation::SendPage => terminal_control::SEND_PAGE_OPTIONS,
        HirCicsOperation::StartBrowse => &["EQUAL", "GENERIC", "GTEQ", "NOHANDLE"],
        HirCicsOperation::ResetBrowse => &["EQUAL", "GENERIC", "GTEQ", "NOHANDLE"],
        HirCicsOperation::Deq => &["UOW", "TASK", "NOHANDLE"],
        HirCicsOperation::Enq => &["UOW", "TASK", "NOSUSPEND", "NOHANDLE"],
        HirCicsOperation::Getmain => &["NOSUSPEND", "NOHANDLE"],
        HirCicsOperation::Read => &["EQUAL", "GENERIC", "GTEQ", "UPDATE", "NOHANDLE"],
        HirCicsOperation::Rewrite => &["NOHANDLE"],
        HirCicsOperation::Syncpoint => &["ROLLBACK", "NOHANDLE"],
        HirCicsOperation::WaitJournalName
        | HirCicsOperation::WaitJournalNum
        | HirCicsOperation::WriteJournalName
        | HirCicsOperation::WriteJournalNum => journal_control::allowed_options(operation),
        operation if counter_control::is_counter(operation) => {
            counter_control::allowed_options(operation)
        }
        op if outboard::is_issue(op) => outboard::allowed_options(op),
        HirCicsOperation::SpoolClose
        | HirCicsOperation::SpoolOpenInput
        | HirCicsOperation::SpoolOpenOutput
        | HirCicsOperation::SpoolRead
        | HirCicsOperation::SpoolWrite => spool_control::allowed_options(operation),
        HirCicsOperation::EnterTraceNum => diagnostics::allowed_options(operation),
        HirCicsOperation::Monitor => diagnostics::allowed_options(operation),
        HirCicsOperation::DumpTransaction => diagnostics::allowed_options(operation),
        _ => {
            command_shape
                .as_ref()
                .ok_or(ResolutionFailure::Unsupported)?
                .options
        }
    };
    let unready_clauses = clauses
        .keys()
        .filter(|name| {
            !allowed_clauses.contains(&name.as_str())
                && !(operation == HirCicsOperation::Assign
                    && mainframe_env_ir::CicsAssignOutput::from_name(name).is_some())
                && !(operation == HirCicsOperation::HandleCondition && is_condition_name(name))
                && !(operation == HirCicsOperation::HandleAid && is_aid_name(name))
        })
        .cloned()
        .collect::<Vec<_>>();
    let unready_options = raw_options
        .iter()
        .filter(|name| {
            !allowed_options.contains(&name.as_str())
                && !(matches!(
                    operation,
                    HirCicsOperation::HandleCondition | HirCicsOperation::IgnoreCondition
                ) && is_condition_name(name))
                && !(operation == HirCicsOperation::HandleAid && is_aid_name(name))
        })
        .cloned()
        .collect::<Vec<_>>();
    if !unready_clauses.is_empty() || !unready_options.is_empty() {
        let names = unready_clauses
            .into_iter()
            .chain(unready_options)
            .collect::<Vec<_>>()
            .join(", ");
        return Err(ResolutionFailure::Invalid(format!(
            "CICS {} is catalog-known but typed lowering is unready for {names}",
            descriptor.label_tokens.join(" ")
        )));
    }
    program_control::validate(operation, &clauses, &raw_options)?;
    file_operands::validate_constraints(&clauses, &raw_options, operation)?;
    queue_control::validate_constraints(&clauses, &raw_options, operation)?;
    storage_control::validate_constraints(&clauses, operation, semantic)?;
    route::validate_constraints(&clauses, &raw_options, operation)?;
    outboard::validate_constraints(&clauses, &raw_options, operation)?;
    terminal_control::validate_constraints(&clauses, &raw_options, operation)?;
    interval_control::validate_constraints(&clauses, &raw_options, operation)?;
    document_control::validate_constraints(&clauses, &raw_options, operation, semantic)?;
    let mut operands = task_wait::resolve(&clauses, &raw_options, operation, semantic)?;
    transform_control::validate_constraints(&clauses, operation)?;
    event_control::validate_constraints(operation, &raw_options)?;
    web_service_control::validate(&clauses, operation)?;
    for required in match operation {
        HirCicsOperation::Address => &["COMMAREA"][..],
        HirCicsOperation::AddressSet => &["SET", "USING"][..],
        HirCicsOperation::Asktime => &["ABSTIME"][..],
        HirCicsOperation::FormatTime => &["ABSTIME"][..],
        HirCicsOperation::Abend
        | HirCicsOperation::AsktimeEib
        | HirCicsOperation::ChangeTask
        | HirCicsOperation::HandleAid
        | HirCicsOperation::HandleAbend
        | HirCicsOperation::HandleCondition
        | HirCicsOperation::IgnoreCondition
        | HirCicsOperation::PopHandle
        | HirCicsOperation::PushHandle
        | HirCicsOperation::Return
        | HirCicsOperation::StartBrowse
        | HirCicsOperation::ResetBrowse
        | HirCicsOperation::ReadNext
        | HirCicsOperation::ReadPrev
        | HirCicsOperation::EndBrowse
        | HirCicsOperation::Delete
        | HirCicsOperation::Unlock
        | HirCicsOperation::Write
        | HirCicsOperation::Read
        | HirCicsOperation::Rewrite
        | HirCicsOperation::WriteTransientData
        | HirCicsOperation::ReadTransientData
        | HirCicsOperation::DeleteTransientData
        | HirCicsOperation::DeleteTemporaryStorage
        | HirCicsOperation::ReadTemporaryStorage
        | HirCicsOperation::WriteTemporaryStorage
        | HirCicsOperation::Freemain
        | HirCicsOperation::Getmain
        | HirCicsOperation::ReceiveMap
        | HirCicsOperation::SendMap
        | HirCicsOperation::SendText
        | HirCicsOperation::SendPartnset
        | HirCicsOperation::ReceivePartn
        | HirCicsOperation::SendControl
        | HirCicsOperation::SendPage
        | HirCicsOperation::Assign
        | HirCicsOperation::Delay
        | HirCicsOperation::PurgeMessage
        | HirCicsOperation::Suspend
        | HirCicsOperation::InvokeApplication
        | HirCicsOperation::IssueAbort
        | HirCicsOperation::IssueAdd
        | HirCicsOperation::IssueEnd
        | HirCicsOperation::IssueErase
        | HirCicsOperation::IssueNote
        | HirCicsOperation::IssueQuery
        | HirCicsOperation::IssueReceive
        | HirCicsOperation::IssueReplace
        | HirCicsOperation::IssueSend
        | HirCicsOperation::IssueWait
        | HirCicsOperation::Route => &[][..],
        HirCicsOperation::WaitEvent | HirCicsOperation::WaitExternal => &[][..],
        HirCicsOperation::InvokeService
        | HirCicsOperation::SoapFaultAdd
        | HirCicsOperation::SoapFaultCreate
        | HirCicsOperation::SoapFaultDelete
        | HirCicsOperation::WsaContextBuild
        | HirCicsOperation::WsaContextDelete
        | HirCicsOperation::WsaContextGet
        | HirCicsOperation::WsaEprCreate => web_shape.as_ref().expect("web shape").required,
        HirCicsOperation::Load => &["PROGRAM"][..],
        HirCicsOperation::Release => &["PROGRAM"][..],
        HirCicsOperation::DocumentCreate => &["DOCTOKEN"][..],
        HirCicsOperation::DocumentDelete => &["DOCTOKEN"][..],
        HirCicsOperation::DocumentInsert => &["DOCTOKEN"][..],
        HirCicsOperation::DocumentRetrieve => &["DOCTOKEN", "INTO", "LENGTH"][..],
        HirCicsOperation::DocumentSet => &["DOCTOKEN", "LENGTH"][..],
        HirCicsOperation::Cancel => &["REQID"][..],
        HirCicsOperation::Start => &["TRANSID"][..],
        HirCicsOperation::Retrieve => &["LENGTH"][..],
        HirCicsOperation::Deq | HirCicsOperation::Enq => &["RESOURCE"][..],
        HirCicsOperation::Link | HirCicsOperation::Xctl => &["PROGRAM"][..],
        HirCicsOperation::SetAssociationUserCorrData => &["USERCORRDATA"][..],
        HirCicsOperation::Syncpoint => &[][..],
        HirCicsOperation::WaitJournalName
        | HirCicsOperation::WaitJournalNum
        | HirCicsOperation::WriteJournalName
        | HirCicsOperation::WriteJournalNum => journal_control::required_clauses(operation),
        operation if counter_control::is_counter(operation) => counter_control::required(operation),
        HirCicsOperation::SpoolClose
        | HirCicsOperation::SpoolOpenInput
        | HirCicsOperation::SpoolOpenOutput
        | HirCicsOperation::SpoolRead
        | HirCicsOperation::SpoolWrite => spool_control::required(operation),
        HirCicsOperation::EnterTraceNum => diagnostics::required(operation),
        HirCicsOperation::Monitor => diagnostics::required(operation),
        HirCicsOperation::DumpTransaction => diagnostics::required(operation),
        _ => {
            command_shape
                .as_ref()
                .ok_or(ResolutionFailure::Unsupported)?
                .required
        }
    } {
        if !clauses.contains_key(*required) {
            return Err(ResolutionFailure::Invalid(format!(
                "CICS {operation:?} requires {required}"
            )));
        }
    }
    operands.extend(spool_control::operands(
        &clauses,
        &raw_options,
        operation,
        semantic,
    )?);
    operands.extend(diagnostics::operands(&clauses, operation, semantic)?);
    if operation == HirCicsOperation::Abend
        && let Some(operand) = abend::operand(&clauses, semantic)?
    {
        operands.push(operand);
    }
    if operation == HirCicsOperation::HandleAbend {
        operands.extend(handle_abend::operands(&clauses, &raw_options, semantic)?);
    }
    operands.extend(program_control::operands(operation, &clauses, semantic)?);
    if operation == HirCicsOperation::Address {
        operands.extend(address::operands(&clauses, semantic)?);
    }
    if operation == HirCicsOperation::AddressSet {
        let (set_is_address, set) = cics_address_value(&clauses["SET"], semantic)?;
        let (using_is_address, using) = cics_address_value(&clauses["USING"], semantic)?;
        if set_is_address == using_is_address {
            return Err(ResolutionFailure::Invalid(
                "CICS ADDRESS SET requires one pointer reference and one ADDRESS OF data area"
                    .into(),
            ));
        }
        require_writable(&set)?;
        let pointer = if set_is_address { &using } else { &set };
        if !matches!(pointer.usage, CobolUsage::Pointer | CobolUsage::Pointer32) {
            return Err(ResolutionFailure::Invalid(
                "CICS ADDRESS SET pointer operand must use POINTER or POINTER-32".into(),
            ));
        }
        operands.extend([
            HirCicsNamedOperand {
                name: if set_is_address {
                    HirCicsOperandName::SetAddress
                } else {
                    HirCicsOperandName::SetPointer
                },
                value: HirCicsValue::Data(set),
            },
            HirCicsNamedOperand {
                name: if using_is_address {
                    HirCicsOperandName::UsingAddress
                } else {
                    HirCicsOperandName::UsingPointer
                },
                value: HirCicsValue::Data(using),
            },
        ]);
    }
    if operation == HirCicsOperation::HandleAid {
        let mut handlers = clauses
            .iter()
            .filter(|(name, _)| is_aid_name(name))
            .map(|(name, value)| (name.clone(), value[0].clone()))
            .collect::<BTreeMap<_, _>>();
        handlers.extend(
            raw_options
                .iter()
                .filter(|name| is_aid_name(name))
                .map(|name| (name.clone(), String::new())),
        );
        operands.push(HirCicsNamedOperand {
            name: HirCicsOperandName::Aids,
            value: HirCicsValue::Literal(
                handlers
                    .iter()
                    .map(|(name, label)| format!("{name}\t{label}"))
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
        });
    } else if operation == HirCicsOperation::HandleCondition {
        let mut handlers = clauses
            .iter()
            .filter(|(name, _)| is_condition_name(name))
            .map(|(name, value)| (name.clone(), value[0].clone()))
            .collect::<BTreeMap<_, _>>();
        handlers.extend(
            raw_options
                .iter()
                .filter(|name| is_condition_name(name))
                .map(|name| (name.clone(), String::new())),
        );
        operands.push(HirCicsNamedOperand {
            name: HirCicsOperandName::Conditions,
            value: HirCicsValue::Literal(
                handlers
                    .iter()
                    .map(|(name, label)| format!("{name}\t{label}"))
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
        });
    } else if operation == HirCicsOperation::IgnoreCondition {
        let names = raw_options
            .iter()
            .filter(|name| is_condition_name(name))
            .cloned()
            .collect::<Vec<_>>();
        operands.push(HirCicsNamedOperand {
            name: HirCicsOperandName::Conditions,
            value: HirCicsValue::Literal(names.join("\n")),
        });
    }
    operands.extend(file_operands::resolve(&clauses, operation, semantic)?);
    operands.extend(queue_control::operands(&clauses, operation, semantic)?);
    operands.extend(storage_control::operands(&clauses, operation, semantic)?);
    operands.extend(route::operands(&clauses, operation, semantic)?);
    operands.extend(outboard::operands(&clauses, operation, semantic)?);
    operands.extend(terminal_control::operands(&clauses, operation, semantic)?);
    operands.extend(interval_control::operands(&clauses, operation, semantic)?);
    operands.extend(document_control::operands(&clauses, operation, semantic)?);
    operands.extend(transform_control::operands(&clauses, operation, semantic)?);
    operands.extend(event_control::operands(&clauses, operation, semantic)?);
    operands.extend(web_service_control::operands(
        &clauses, operation, semantic,
    )?);
    operands.extend(journal_control::operands(&clauses, operation, semantic)?);
    operands.extend(counter_control::operands(&clauses, operation, semantic)?);
    if matches!(operation, HirCicsOperation::Deq | HirCicsOperation::Enq) {
        let resource = complete_data_reference(&clauses["RESOURCE"], semantic)?;
        operands.push(HirCicsNamedOperand {
            name: HirCicsOperandName::Resource,
            value: HirCicsValue::Data(resource),
        });
        if let Some(value) = clauses.get("LENGTH") {
            operands.push(HirCicsNamedOperand {
                name: HirCicsOperandName::Length,
                value: cics_integer_value(value, semantic)?,
            });
        }
        if let Some(value) = clauses.get("MAXLIFETIME") {
            operands.push(HirCicsNamedOperand {
                name: HirCicsOperandName::MaxLifetime,
                value: cics_cvda_value(value, semantic)?,
            });
        }
    }
    if operation == HirCicsOperation::ChangeTask
        && let Some(value) = clauses.get("PRIORITY")
    {
        operands.push(HirCicsNamedOperand {
            name: HirCicsOperandName::Priority,
            value: cics_integer_value(value, semantic)?,
        });
    }
    if operation == HirCicsOperation::SetAssociationUserCorrData {
        operands.push(HirCicsNamedOperand {
            name: HirCicsOperandName::UserCorrData,
            value: cics_value(&clauses["USERCORRDATA"], semantic)?,
        });
    }
    if operation == HirCicsOperation::FormatTime {
        operands.extend(format_time::operands(&clauses, semantic)?);
    }
    let mut outputs = output_bindings::resolve(&clauses, &raw_options, operation, semantic)?;
    outputs.extend(queue_control::outputs(&clauses, operation, semantic)?);
    outputs.extend(document_control::outputs(&clauses, operation, semantic)?);
    outputs.extend(transform_control::outputs(&clauses, operation, semantic)?);
    outputs.extend(web_service_control::outputs(&clauses, operation, semantic)?);
    outputs.extend(counter_control::outputs(&clauses, operation, semantic)?);
    outputs.extend(diagnostics::outputs(&clauses, operation, semantic)?);
    if operation == HirCicsOperation::Retrieve {
        let target = complete_data_reference(&clauses["LENGTH"], semantic)?;
        require_writable(&target)?;
        outputs.push(HirCicsOutputBinding {
            name: HirCicsOutputName::Length,
            target,
        });
    } else if let Some(target) = output_bindings::inout_length(&operands, operation) {
        require_writable(target)?;
        outputs.push(HirCicsOutputBinding {
            name: HirCicsOutputName::Length,
            target: target.clone(),
        });
    }
    let mut options = raw_options
        .iter()
        .filter(|option| {
            !(matches!(
                operation,
                HirCicsOperation::HandleCondition | HirCicsOperation::IgnoreCondition
            ) && is_condition_name(option))
                && !(operation == HirCicsOperation::HandleAid && is_aid_name(option))
        })
        .map(|option| {
            counter_control::option(operation, option)
                .or_else(|| event_control::option(operation, option))
                .or_else(|| diagnostics::option(operation, option))
                .or_else(|| {
                    matches!(
                        operation,
                        HirCicsOperation::SpoolClose
                            | HirCicsOperation::SpoolOpenInput
                            | HirCicsOperation::SpoolOpenOutput
                            | HirCicsOperation::SpoolRead
                            | HirCicsOperation::SpoolWrite
                    )
                    .then(|| spool_control::option(option))
                    .flatten()
                })
                .unwrap_or_else(|| operation::resolve_option(option, operation))
        })
        .collect::<BTreeSet<_>>();
    let response = output(&outputs, HirCicsOutputName::Resp).cloned();
    let response2 = output(&outputs, HirCicsOutputName::Resp2).cloned();
    if response.is_none() && response2.is_some() {
        return Err(ResolutionFailure::Invalid(
            "CICS RESP2 requires RESP".into(),
        ));
    }
    let no_handle = options.contains(&HirCicsOption::NoHandle);
    let condition_policy = if let Some(response) = response {
        // RESP implies NOHANDLE while retaining the response-area update.
        options.remove(&HirCicsOption::NoHandle);
        HirCicsConditionPolicy::Respond {
            response,
            response2,
        }
    } else if no_handle {
        HirCicsConditionPolicy::NoHandle
    } else {
        HirCicsConditionPolicy::Default
    };
    Ok(HirCicsStatement {
        operation,
        operands,
        options,
        outputs,
        condition_policy,
    })
}
