//! Catalog-bound recognition of top-level EXEC CICS command clauses.
use super::{
    HirCicsConditionPolicy, HirCicsNamedOperand, HirCicsOperandName, HirCicsOperation,
    HirCicsOption, HirCicsOutputBinding, HirCicsOutputName, HirCicsStatement, HirCicsValue,
    Resolution, ResolutionFailure, numeric_literal, require_numeric, require_writable,
};
use crate::{CobolUsage, SemanticModel};
use mainframe_env_ir::{
    CICS_APPLICATION_AID_NAMES, CicsApplicationCobolApplicability,
    CicsApplicationConditionLabelOperand, CicsApplicationConstraintStatus,
    CicsApplicationHandlerReadiness, CicsApplicationOptionValueShape,
    CicsApplicationRegistryDescriptor, cics_application_registry_candidates_for_tokens,
};
use std::collections::{BTreeMap, BTreeSet};
type Clauses = BTreeMap<String, Vec<String>>;
mod abend;
mod address;
mod assign_validation;
mod bts_child_link;
mod builtin_function;
mod candidate_validation;
mod certificate_control;
mod clause_parser;
mod command_recognition;
mod conversation_control;
mod conversation_data;
mod conversation_open;
mod convert_time;
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
use operation::is_condition_name;
mod operator_control;
mod output_bindings;
mod program_control;
mod program_name;
mod queue_control;
mod route;
mod security_control;
mod shape;
mod spool_control;
mod storage_control;
mod task_wait;
mod terminal_control;
mod transaction_name;
mod transform_control;
mod value;
mod web_control;
mod web_service_control;

use candidate_validation::{keep_best_failure, validate_candidate};
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
        let (clauses, mut options) = match clauses(&tokens, Some(candidate.descriptor)) {
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
        if candidate.descriptor.label_tokens == ["WEB", "STARTBROWSE"]
            && candidate.head_tokens.len() == 3
            && let Some(kind) = candidate.head_tokens.last()
            && *kind != "HTTPHEADER"
            && !clauses.contains_key(*kind)
            && !options.iter().any(|option| option == *kind)
        {
            options.push((*kind).into());
        }
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

        if candidate.descriptor.label_tokens == ["WEB", "STARTBROWSE"]
            && candidate.head_tokens.last() == Some(&"HTTPHEADER")
        {
            options.push("HTTPHEADER".into());
        }
        if candidate.descriptor.label_tokens == ["WEB", "ENDBROWSE"]
            && candidate.head_tokens.len() == 3
            && let Some(kind) = candidate.head_tokens.last()
        {
            options.push((*kind).into());
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

fn option_is_known(descriptor: &CicsApplicationRegistryDescriptor, name: &str) -> bool {
    command_recognition::option_value_shape(descriptor, name).is_some()
        || (descriptor.condition_clauses.is_some() && is_condition_name(name))
        || (descriptor.label_tokens == ["HANDLE", "AID"] && is_aid_name(name))
}

fn is_aid_name(name: &str) -> bool {
    CICS_APPLICATION_AID_NAMES.binary_search(&name).is_ok()
}

fn is_single_condition_label(tokens: &[String]) -> bool {
    matches!(tokens, [label] if !label.is_empty() && label.chars().all(|character| {
        character.is_ascii_alphanumeric() || character == '-'
    }))
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
    let bts_shape = bts_child_link::shape(operation);
    let command_shape = transform_shape
        .as_ref()
        .or(event_shape.as_ref())
        .or(bts_shape.as_ref());
    let allowed_clauses: &[&str] = match operation {
        op if conversation_control::is_operation(op) => conversation_control::allowed_clauses(op),
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
        HirCicsOperation::ConvertTime => &["DATESTRING", "ABSTIME", "RESP", "RESP2"],
        HirCicsOperation::BifDeedit => &["FIELD", "LENGTH", "RESP", "RESP2"],
        HirCicsOperation::BifDigest => &[
            "RECORD",
            "RECORDLEN",
            "DIGESTTYPE",
            "RESULT",
            "RESP",
            "RESP2",
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
        HirCicsOperation::WebParseUrl => web_control::PARSE_URL_CLAUSES,
        HirCicsOperation::WebOpen => web_control::OPEN_CLAUSES,
        HirCicsOperation::WebClose => web_control::CLOSE_CLAUSES,
        HirCicsOperation::WebExtract | HirCicsOperation::ExtractWeb => web_control::EXTRACT_CLAUSES,
        HirCicsOperation::WebRead => web_control::READ_CLAUSES,
        HirCicsOperation::WebStartBrowse => web_control::START_BROWSE_CLAUSES,
        HirCicsOperation::WebReadNext => web_control::READ_NEXT_CLAUSES,
        HirCicsOperation::WebEndBrowse => web_control::END_BROWSE_CLAUSES,
        HirCicsOperation::WebWrite => web_control::WRITE_CLAUSES,
        HirCicsOperation::WebSend => web_control::SEND_CLAUSES,
        HirCicsOperation::WebRetrieve => web_control::RETRIEVE_CLAUSES,
        HirCicsOperation::WebReceive => web_control::RECEIVE_CLAUSES,
        HirCicsOperation::WebConverse => web_control::CONVERSE_CLAUSES,
        operation if conversation_open::is_conversation(operation) => {
            conversation_open::allowed_clauses(operation)
        }
        operation if conversation_data::is_data_wait(operation) => {
            conversation_data::allowed_clauses(operation)
        }
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
        HirCicsOperation::Post => &[
            "INTERVAL", "TIME", "HOURS", "MINUTES", "SECONDS", "SET", "REQID", "RESP", "RESP2",
        ],
        HirCicsOperation::WriteOperator => operator_control::ALLOWED_CLAUSES,
        HirCicsOperation::ExtractCertificate => certificate_control::ALLOWED_CLAUSES,
        HirCicsOperation::ExtractTcpip => certificate_control::tcpip_control::ALLOWED_CLAUSES,
        HirCicsOperation::PurgeMessage => &["RESP", "RESP2"],
        HirCicsOperation::QuerySecurity => security_control::QUERY_CLAUSES,
        HirCicsOperation::VerifyPassword => security_control::VERIFY_PASSWORD_CLAUSES,
        HirCicsOperation::ChangePassword => security_control::CHANGE_PASSWORD_CLAUSES,
        HirCicsOperation::ChangePhrase => security_control::CHANGE_PHRASE_CLAUSES,
        HirCicsOperation::RequestPassTicket => security_control::PASSTICKET_CLAUSES,
        HirCicsOperation::RequestEncryptPassTicket => security_control::ENCRYPTPTKT_CLAUSES,
        HirCicsOperation::VerifyToken => security_control::VERIFY_TOKEN_CLAUSES,
        HirCicsOperation::Signon => security_control::SIGNON_CLAUSES,
        HirCicsOperation::Signoff => &["RESP", "RESP2"],
        HirCicsOperation::VerifyPhrase => security_control::VERIFY_PHRASE_CLAUSES,
        HirCicsOperation::SetAssociationUserCorrData => &["USERCORRDATA", "RESP", "RESP2"],
        HirCicsOperation::Syncpoint => &["RESP", "RESP2"],
        HirCicsOperation::Suspend => &["RESP", "RESP2"],
        op @ (HirCicsOperation::WaitEvent
        | HirCicsOperation::WaitExternal
        | HirCicsOperation::WaitCics) => task_wait::names(op),
        HirCicsOperation::Start => interval_control::START_CLAUSES,
        HirCicsOperation::StartAttach => &["TRANSID", "RESP", "RESP2"],
        HirCicsOperation::StartBrexit => interval_control::BREXIT_CLAUSES,
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
        HirCicsOperation::Dump => diagnostics::allowed_clauses(operation),
        HirCicsOperation::Trace => diagnostics::allowed_clauses(operation),
        HirCicsOperation::EnterTraceId => diagnostics::allowed_clauses(operation),
        _ => {
            command_shape
                .as_ref()
                .ok_or(ResolutionFailure::Unsupported)?
                .clauses
        }
    };
    let allowed_options: &[&str] = match operation {
        op if conversation_control::is_operation(op) => &["NOHANDLE"],
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
        | HirCicsOperation::QuerySecurity
        | HirCicsOperation::VerifyPassword
        | HirCicsOperation::ChangePassword
        | HirCicsOperation::ChangePhrase
        | HirCicsOperation::RequestPassTicket
        | HirCicsOperation::RequestEncryptPassTicket
        | HirCicsOperation::Signon
        | HirCicsOperation::Signoff
        | HirCicsOperation::VerifyPhrase
        | HirCicsOperation::PopHandle
        | HirCicsOperation::PushHandle
        | HirCicsOperation::SetAssociationUserCorrData
        | HirCicsOperation::Suspend
        | HirCicsOperation::WaitEvent => &["NOHANDLE"],
        HirCicsOperation::VerifyToken => &["NOHANDLE"],
        HirCicsOperation::WaitExternal | HirCicsOperation::WaitCics => {
            task_wait::WAIT_EXTERNAL_OPTIONS
        }
        HirCicsOperation::ReadTemporaryStorage => &["NEXT", "NOHANDLE"],
        HirCicsOperation::WriteTemporaryStorage => {
            &["AUXILIARY", "MAIN", "NOSUSPEND", "REWRITE", "NOHANDLE"]
        }
        HirCicsOperation::DocumentCreate => document_control::ALLOWED_OPTIONS,
        HirCicsOperation::DocumentRetrieve => document_control::RETRIEVE_OPTIONS,
        HirCicsOperation::DocumentSet => document_control::ALLOWED_OPTIONS,
        HirCicsOperation::WebParseUrl => &["NOHANDLE"],
        HirCicsOperation::WebOpen => &["NOHANDLE"],
        HirCicsOperation::WebClose => &["NOHANDLE"],
        HirCicsOperation::WebExtract | HirCicsOperation::ExtractWeb => &["NOHANDLE"],
        HirCicsOperation::WebRead => &["NOHANDLE"],
        HirCicsOperation::WebStartBrowse => web_control::START_BROWSE_OPTIONS,
        HirCicsOperation::WebReadNext => &["NOHANDLE"],
        HirCicsOperation::WebEndBrowse => web_control::END_BROWSE_OPTIONS,
        HirCicsOperation::WebWrite => &["NOHANDLE"],
        HirCicsOperation::WebSend => &["NOHANDLE"],
        HirCicsOperation::WebRetrieve => &["NOHANDLE"],
        HirCicsOperation::WebReceive => &["NOTRUNCATE", "NOHANDLE"],
        HirCicsOperation::WebConverse => &["NOTRUNCATE", "NOHANDLE"],
        operation if conversation_open::is_conversation(operation) => {
            conversation_open::allowed_options(operation)
        }
        operation if conversation_data::is_data_wait(operation) => {
            conversation_data::allowed_options(operation)
        }
        HirCicsOperation::Start => &["AFTER", "AT", "FMH", "PROTECT", "NOCHECK", "NOHANDLE"],
        HirCicsOperation::StartAttach => &["NOHANDLE"],
        HirCicsOperation::StartBrexit => &["BREXIT", "NOHANDLE"],
        HirCicsOperation::Cancel => &["NOHANDLE"],
        HirCicsOperation::Delay => &["FOR", "UNTIL", "NOHANDLE"],
        HirCicsOperation::Post => &["AFTER", "AT", "NOHANDLE"],
        HirCicsOperation::WriteOperator => &["IMMEDIATE", "EVENTUAL", "CRITICAL", "NOHANDLE"],
        HirCicsOperation::ExtractCertificate => &["OWNER", "ISSUER", "NOHANDLE"],
        HirCicsOperation::ExtractTcpip => &["NOHANDLE"],
        HirCicsOperation::Retrieve => &["WAIT", "NOHANDLE"],
        HirCicsOperation::FormatTime => &["DATESEP", "TIMESEP", "NOHANDLE"],
        HirCicsOperation::ConvertTime => &["NOHANDLE"],
        HirCicsOperation::BifDeedit => &["NOHANDLE"],
        HirCicsOperation::BifDigest => &["HEX", "BINARY", "BASE64", "NOHANDLE"],
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
        HirCicsOperation::Dump => diagnostics::allowed_options(operation),
        HirCicsOperation::Trace => diagnostics::allowed_options(operation),
        HirCicsOperation::EnterTraceId => diagnostics::allowed_options(operation),
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
    conversation_open::validate(&clauses, &raw_options, operation)?;
    conversation_data::validate(&clauses, &raw_options, operation)?;
    file_operands::validate_constraints(&clauses, &raw_options, operation)?;
    queue_control::validate_constraints(&clauses, &raw_options, operation)?;
    storage_control::validate_constraints(&clauses, operation, semantic)?;
    route::validate_constraints(&clauses, &raw_options, operation)?;
    security_control::validate(&clauses, operation, semantic)?;
    conversation_control::validate(&clauses, operation)?;
    outboard::validate_constraints(&clauses, &raw_options, operation)?;
    terminal_control::validate_constraints(&clauses, &raw_options, operation)?;
    interval_control::validate_constraints(&clauses, &raw_options, operation)?;
    document_control::validate_constraints(&clauses, &raw_options, operation, semantic)?;
    web_control::validate(&clauses, &raw_options, operation, semantic)?;
    let mut operands = task_wait::resolve(&clauses, &raw_options, operation, semantic)?;
    transform_control::validate_constraints(&clauses, operation)?;
    event_control::validate_constraints(operation, &raw_options)?;
    bts_child_link::validate(&clauses, &raw_options, operation)?;
    web_service_control::validate(&clauses, operation)?;
    for required in match operation {
        op if conversation_control::is_operation(op) => &[][..],
        HirCicsOperation::Address => &["COMMAREA"][..],
        HirCicsOperation::AddressSet => &["SET", "USING"][..],
        HirCicsOperation::Asktime => &["ABSTIME"][..],
        HirCicsOperation::FormatTime => &["ABSTIME"][..],
        HirCicsOperation::ConvertTime => &["DATESTRING", "ABSTIME"][..],
        HirCicsOperation::BifDeedit => &["FIELD"][..],
        HirCicsOperation::BifDigest => &["RECORD", "RECORDLEN", "RESULT"][..],
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
        | HirCicsOperation::QuerySecurity
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
        HirCicsOperation::WaitEvent
        | HirCicsOperation::WaitExternal
        | HirCicsOperation::WaitCics => &[][..],
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
        HirCicsOperation::WebParseUrl => &["URL", "URLLENGTH"][..],
        HirCicsOperation::WebOpen => &["SESSTOKEN"][..],
        HirCicsOperation::WebClose => &["SESSTOKEN"][..],
        HirCicsOperation::WebExtract | HirCicsOperation::ExtractWeb => &[][..],
        HirCicsOperation::WebRead => &["NAMELENGTH", "VALUE", "VALUELENGTH"][..],
        HirCicsOperation::WebStartBrowse => &[][..],
        HirCicsOperation::WebReadNext => &["NAMELENGTH", "VALUE", "VALUELENGTH"][..],
        HirCicsOperation::WebEndBrowse => &[][..],
        HirCicsOperation::WebWrite => &["HTTPHEADER", "NAMELENGTH", "VALUE", "VALUELENGTH"][..],
        HirCicsOperation::WebSend => &[][..],
        HirCicsOperation::WebRetrieve => &["DOCTOKEN"][..],
        HirCicsOperation::WebReceive => &["INTO", "LENGTH", "MAXLENGTH"][..],
        HirCicsOperation::WebConverse => {
            &["SESSTOKEN", "METHOD", "INTO", "TOLENGTH", "MAXLENGTH"][..]
        }
        HirCicsOperation::Cancel => &["REQID"][..],
        HirCicsOperation::Start | HirCicsOperation::StartAttach | HirCicsOperation::StartBrexit => {
            &["TRANSID"][..]
        }
        HirCicsOperation::Post => &["SET"][..],
        HirCicsOperation::WriteOperator => &["TEXT"][..],
        HirCicsOperation::ExtractCertificate => &["CERTIFICATE"][..],
        HirCicsOperation::ExtractTcpip => &[][..],
        HirCicsOperation::Retrieve => &[][..],
        HirCicsOperation::Deq | HirCicsOperation::Enq => &["RESOURCE"][..],
        HirCicsOperation::Link | HirCicsOperation::Xctl => &["PROGRAM"][..],
        HirCicsOperation::SetAssociationUserCorrData => &["USERCORRDATA"][..],
        HirCicsOperation::VerifyPassword => &["PASSWORD", "USERID"][..],
        HirCicsOperation::ChangePassword => &["PASSWORD", "NEWPASSWORD", "USERID"][..],
        HirCicsOperation::ChangePhrase => {
            &["PHRASE", "PHRASELEN", "NEWPHRASE", "NEWPHRASELEN", "USERID"][..]
        }
        HirCicsOperation::RequestPassTicket => &["PASSTICKET", "ESMAPPNAME"][..],
        HirCicsOperation::RequestEncryptPassTicket => {
            &["ENCRYPTKEY", "ENCRYPTPTKT", "FLENGTH", "ESMAPPNAME"][..]
        }
        HirCicsOperation::VerifyToken => &["TOKEN", "TOKENLEN"][..],
        HirCicsOperation::Signon => &["USERID"][..],
        HirCicsOperation::Signoff => &[][..],
        operation if conversation_open::is_conversation(operation) => {
            conversation_open::required_clauses(operation)
        }
        operation if conversation_data::is_data_wait(operation) => {
            conversation_data::required_clauses(operation)
        }
        HirCicsOperation::VerifyPhrase => &["PHRASE", "PHRASELEN", "USERID"][..],
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
        HirCicsOperation::Dump => diagnostics::required(operation),
        HirCicsOperation::Trace => diagnostics::required(operation),
        HirCicsOperation::EnterTraceId => diagnostics::required(operation),
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
    operands.extend(diagnostics::operands(
        &clauses,
        &raw_options,
        operation,
        semantic,
    )?);
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
    if operation == HirCicsOperation::WriteOperator {
        operator_control::validate(&clauses, &raw_options)?;
        operands.extend(operator_control::operands(&clauses, semantic)?);
    }
    operands.extend(document_control::operands(&clauses, operation, semantic)?);
    operands.extend(transform_control::operands(&clauses, operation, semantic)?);
    operands.extend(event_control::operands(&clauses, operation, semantic)?);
    operands.extend(bts_child_link::operands(&clauses, operation, semantic)?);
    operands.extend(web_service_control::operands(
        &clauses, operation, semantic,
    )?);
    operands.extend(journal_control::operands(&clauses, operation, semantic)?);
    operands.extend(counter_control::operands(&clauses, operation, semantic)?);
    operands.extend(web_control::operands(&clauses, operation, semantic)?);
    operands.extend(conversation_control::operands(
        &clauses, operation, semantic,
    )?);
    operands.extend(conversation_data::operands(&clauses, operation, semantic)?);
    operands.extend(security_control::operands(&clauses, operation, semantic)?);
    operands.extend(conversation_open::operands(&clauses, operation, semantic)?);
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
    if operation == HirCicsOperation::ConvertTime {
        operands.extend(convert_time::operands(&clauses, semantic)?);
    }
    if operation == HirCicsOperation::BifDeedit {
        operands.extend(builtin_function::deedit_operands(&clauses, semantic)?);
    }
    if operation == HirCicsOperation::BifDigest {
        operands.extend(builtin_function::digest_operands(&clauses, semantic)?);
    }
    let mut outputs = output_bindings::resolve(&clauses, &raw_options, operation, semantic)?;
    if operation == HirCicsOperation::BifDeedit {
        outputs.push(builtin_function::deedit_output(&clauses, semantic)?);
    }
    if operation == HirCicsOperation::BifDigest {
        outputs.push(builtin_function::digest_output(
            &clauses,
            &raw_options,
            semantic,
        )?);
    }
    outputs.extend(queue_control::outputs(&clauses, operation, semantic)?);
    if operation == HirCicsOperation::WriteOperator {
        outputs.extend(operator_control::outputs(&clauses, semantic)?);
    }
    outputs.extend(certificate_control::outputs(&clauses, operation, semantic)?);
    outputs.extend(document_control::outputs(&clauses, operation, semantic)?);
    outputs.extend(transform_control::outputs(&clauses, operation, semantic)?);
    outputs.extend(web_service_control::outputs(&clauses, operation, semantic)?);
    outputs.extend(counter_control::outputs(&clauses, operation, semantic)?);
    outputs.extend(diagnostics::outputs(&clauses, operation, semantic)?);
    outputs.extend(web_control::outputs(&clauses, operation, semantic)?);
    outputs.extend(conversation_control::outputs(
        &clauses, operation, semantic,
    )?);
    outputs.extend(conversation_data::outputs(&clauses, operation, semantic)?);
    outputs.extend(security_control::outputs(&clauses, operation, semantic)?);
    outputs.extend(bts_child_link::outputs(&clauses, operation, semantic)?);
    outputs.extend(conversation_open::outputs(&clauses, operation, semantic)?);
    if operation == HirCicsOperation::Retrieve
        && let Some(length) = clauses.get("LENGTH")
    {
        let target = complete_data_reference(length, semantic)?;
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
                && !(operation == HirCicsOperation::StartBrexit && option.as_str() == "BREXIT")
        })
        .map(|option| {
            conversation_open::option(operation, option)
                .or_else(|| conversation_data::option(operation, option))
                .or_else(|| bts_child_link::option(operation, option))
                .or_else(|| counter_control::option(operation, option))
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
    if matches!(
        operation,
        HirCicsOperation::WebReceive | HirCicsOperation::WebConverse
    ) {
        options.extend(web_control::receive_options(&clauses)?);
    }
    if matches!(
        operation,
        HirCicsOperation::WebStartBrowse | HirCicsOperation::WebReadNext
    ) {
        if operation == HirCicsOperation::WebReadNext && clauses.contains_key("HTTPHEADER") {
            options.insert(HirCicsOption::WebBrowseHttpHeader);
        }
        if clauses.contains_key("FORMFIELD") {
            options.insert(HirCicsOption::WebBrowseFormField);
        }
        if clauses.contains_key("QUERYPARM") {
            options.insert(HirCicsOption::WebBrowseQueryParm);
        }
    }
    if operation == HirCicsOperation::VerifyToken {
        let token_type = security_control::token_cvda(&clauses, "TOKENTYPE")?;
        options.insert(operation::resolve_option(&token_type, operation));
        if clauses.contains_key("DATATYPE") {
            let datatype = security_control::token_cvda(&clauses, "DATATYPE")?;
            options.insert(operation::resolve_option(&datatype, operation));
        }
    }
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
