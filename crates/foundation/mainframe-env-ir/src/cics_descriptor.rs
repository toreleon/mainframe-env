//! Executable descriptors for the bounded typed CICS dialect.

use crate::{CicsPlanOperation, Effect, OperationIdentity};

mod effects;
mod executable_entries;
mod executable_lookup;
mod executable_registry;
mod registry_lookup;
mod terminal_effects;
use effects::*;
pub use executable_entries::CICS_EXECUTABLE_DESCRIPTORS;
pub use executable_lookup::cics_executable_descriptor;
pub use executable_registry::*;
pub use registry_lookup::cics_application_registry_for_runtime_operation;
use terminal_effects::{
    OUTBOARD_READ_EFFECTS, OUTBOARD_WAIT_EFFECTS, OUTBOARD_WRITE_EFFECTS, ROUTE_EFFECTS,
};

/// Runtime import required by every executable operation in this dialect.
pub const CICS_RUNTIME_IMPORT: &str = "host.cics";

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
                        || option.value_shape == CicsApplicationOptionValueShape::OptionalValue
                })
        }));
        let typed = CICS_APPLICATION_REGISTRY
            .iter()
            .filter(|descriptor| {
                descriptor.readiness == CicsApplicationHandlerReadiness::TypedRuntime
            })
            .collect::<Vec<_>>();
        assert_eq!(typed.len(), 152);
        assert!(typed.iter().all(|descriptor| descriptor.advertised
            && descriptor.runtime_operation.is_some()
            && descriptor.legacy_execution_options.is_empty()));
        let legacy = CICS_APPLICATION_REGISTRY
            .iter()
            .filter(|descriptor| {
                descriptor.readiness == CicsApplicationHandlerReadiness::LegacyCompatibility
            })
            .collect::<Vec<_>>();
        assert_eq!(legacy.len(), 0);
        assert!(legacy.iter().all(|descriptor| descriptor.advertised
            && descriptor.runtime_operation.is_some()
            && !descriptor.legacy_execution_options.is_empty()));
        let unready = CICS_APPLICATION_REGISTRY
            .iter()
            .filter(|descriptor| descriptor.readiness == CicsApplicationHandlerReadiness::Unready)
            .collect::<Vec<_>>();
        assert_eq!(unready.len(), 111);
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
        assert_eq!(
            asktime.readiness,
            CicsApplicationHandlerReadiness::TypedRuntime
        );
        assert!(asktime.advertised);
        assert_eq!(asktime.runtime_operation, Some("AsktimeEib"));

        let absolute = cics_application_registry_for_tokens(&["asktime", "abstime", "target"])
            .expect("ASKTIME ABSTIME must be catalog-known");
        assert_eq!(absolute.label_tokens, ["ASKTIME", "ABSTIME"]);
        assert_eq!(
            absolute.readiness,
            CicsApplicationHandlerReadiness::TypedRuntime
        );
        assert!(absolute.advertised);
        assert_eq!(absolute.runtime_operation, Some("Asktime"));
        assert!(absolute.legacy_execution_options.is_empty());
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

        // SEND MAP CURSOR: the pinned syntax diagram draws the
        // parenthesized data-value as an independently optional nested
        // group, and the IBM option prose confirms bare CURSOR means
        // symbolic cursor positioning. See dfhp4_sendmap.html, Options,
        // CURSOR(data-value).
        let send_map = CICS_APPLICATION_REGISTRY
            .iter()
            .find(|descriptor| descriptor.label_tokens == ["SEND", "MAP"])
            .expect("SEND MAP registry row");
        let cursor = send_map
            .options
            .iter()
            .find(|option| option.name == "CURSOR")
            .expect("SEND MAP CURSOR option");
        assert_eq!(
            cursor.value_shape,
            CicsApplicationOptionValueShape::OptionalValue
        );

        let formattime = CICS_APPLICATION_REGISTRY
            .iter()
            .find(|descriptor| descriptor.label_tokens == ["FORMATTIME"])
            .expect("FORMATTIME registry row");
        for name in ["DATESEP", "TIMESEP"] {
            let option = formattime
                .options
                .iter()
                .find(|option| option.name == name)
                .unwrap_or_else(|| panic!("FORMATTIME {name} option"));
            assert_eq!(
                option.value_shape,
                CicsApplicationOptionValueShape::OptionalValue
            );
        }

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
                CicsPlanOperation::AllocateConversation,
                CicsPlanOperation::GdsAllocateConversation,
                CicsPlanOperation::GdsAssignConversation,
                CicsPlanOperation::BuildAttach,
                CicsPlanOperation::ConnectProcess,
                CicsPlanOperation::GdsConnectProcess,
                CicsPlanOperation::Converse,
                CicsPlanOperation::FreeConversation,
                CicsPlanOperation::GdsFreeConversation,
                CicsPlanOperation::ChangePassword,
                CicsPlanOperation::ChangePhrase,
                CicsPlanOperation::QuerySecurity,
                CicsPlanOperation::RequestPassTicket,
                CicsPlanOperation::RequestEncryptPassTicket,
                CicsPlanOperation::Signoff,
                CicsPlanOperation::Signon,
                CicsPlanOperation::VerifyPassword,
                CicsPlanOperation::VerifyPhrase,
                CicsPlanOperation::VerifyToken,
                CicsPlanOperation::Abend,
                CicsPlanOperation::AddressSet,
                CicsPlanOperation::Address,
                CicsPlanOperation::Asktime,
                CicsPlanOperation::AsktimeEib,
                CicsPlanOperation::FormatTime,
                CicsPlanOperation::ChangeTask,
                CicsPlanOperation::Deq,
                CicsPlanOperation::Enq,
                CicsPlanOperation::HandleAid,
                CicsPlanOperation::HandleAbend,
                CicsPlanOperation::HandleCondition,
                CicsPlanOperation::IgnoreCondition,
                CicsPlanOperation::Link,
                CicsPlanOperation::InvokeApplication,
                CicsPlanOperation::Load,
                CicsPlanOperation::Release,
                CicsPlanOperation::Getmain64,
                CicsPlanOperation::SpoolOpenInput,
                CicsPlanOperation::SpoolOpenOutput,
                CicsPlanOperation::SpoolRead,
                CicsPlanOperation::SpoolWrite,
                CicsPlanOperation::WebParseUrl,
                CicsPlanOperation::WebConverse,
                CicsPlanOperation::WebReceive,
                CicsPlanOperation::WebRetrieve,
                CicsPlanOperation::WebSend,
                CicsPlanOperation::WebWrite,
                CicsPlanOperation::WebEndBrowse,
                CicsPlanOperation::WebReadNext,
                CicsPlanOperation::WebStartBrowse,
                CicsPlanOperation::WebRead,
                CicsPlanOperation::ExtractWeb,
                CicsPlanOperation::WebExtract,
                CicsPlanOperation::WebOpen,
                CicsPlanOperation::WebClose,
                CicsPlanOperation::SendPartnset,
                CicsPlanOperation::ReceivePartn,
                CicsPlanOperation::SendControl,
                CicsPlanOperation::SendPage,
                CicsPlanOperation::IssueAbort,
                CicsPlanOperation::IssueAdd,
                CicsPlanOperation::IssueEnd,
                CicsPlanOperation::IssueErase,
                CicsPlanOperation::IssueNote,
                CicsPlanOperation::IssueQuery,
                CicsPlanOperation::IssueReceive,
                CicsPlanOperation::IssueReplace,
                CicsPlanOperation::IssueSend,
                CicsPlanOperation::IssueWait,
                CicsPlanOperation::Route,
                CicsPlanOperation::DefineCounter,
                CicsPlanOperation::DefineDCounter,
                CicsPlanOperation::DeleteCounter,
                CicsPlanOperation::DeleteDCounter,
                CicsPlanOperation::GetCounter,
                CicsPlanOperation::GetDCounter,
                CicsPlanOperation::QueryCounter,
                CicsPlanOperation::QueryDCounter,
                CicsPlanOperation::RewindCounter,
                CicsPlanOperation::RewindDCounter,
                CicsPlanOperation::UpdateCounter,
                CicsPlanOperation::UpdateDCounter,
                CicsPlanOperation::EnterTraceNum,
                CicsPlanOperation::Monitor,
                CicsPlanOperation::DumpTransaction,
                CicsPlanOperation::Dump,
                CicsPlanOperation::Trace,
                CicsPlanOperation::EnterTraceId,
                CicsPlanOperation::AddSubevent,
                CicsPlanOperation::CheckTimer,
                CicsPlanOperation::DefineCompositeEvent,
                CicsPlanOperation::DefineInputEvent,
                CicsPlanOperation::DefineTimer,
                CicsPlanOperation::DeleteEvent,
                CicsPlanOperation::DeleteTimer,
                CicsPlanOperation::ForceTimer,
                CicsPlanOperation::RemoveSubevent,
                CicsPlanOperation::RetrieveReattachEvent,
                CicsPlanOperation::RetrieveSubevent,
                CicsPlanOperation::SignalEvent,
                CicsPlanOperation::TestEvent,
                CicsPlanOperation::Xctl,
                CicsPlanOperation::Return,
                CicsPlanOperation::StartBrowse,
                CicsPlanOperation::ResetBrowse,
                CicsPlanOperation::Unlock,
                CicsPlanOperation::ReadNext,
                CicsPlanOperation::ReadPrev,
                CicsPlanOperation::ReadTransientData,
                CicsPlanOperation::EndBrowse,
                CicsPlanOperation::Delete,
                CicsPlanOperation::Write,
                CicsPlanOperation::WriteTransientData,
                CicsPlanOperation::DeleteTransientData,
                CicsPlanOperation::DeleteTemporaryStorage,
                CicsPlanOperation::ReadTemporaryStorage,
                CicsPlanOperation::WriteTemporaryStorage,
                CicsPlanOperation::Getmain,
                CicsPlanOperation::Getmain64,
                CicsPlanOperation::Freemain,
                CicsPlanOperation::Freemain64,
                CicsPlanOperation::SpoolClose,
                CicsPlanOperation::ReceiveMap,
                CicsPlanOperation::SendMap,
                CicsPlanOperation::SendText,
                CicsPlanOperation::Assign,
                CicsPlanOperation::PurgeMessage,
                CicsPlanOperation::PopHandle,
                CicsPlanOperation::PushHandle,
                CicsPlanOperation::Read,
                CicsPlanOperation::Rewrite,
                CicsPlanOperation::Syncpoint,
                CicsPlanOperation::SetAssociationUserCorrData,
                CicsPlanOperation::Suspend,
                CicsPlanOperation::WaitEvent,
                CicsPlanOperation::WaitExternal,
                CicsPlanOperation::WaitJournalName,
                CicsPlanOperation::WaitJournalNum,
                CicsPlanOperation::DocumentCreate,
                CicsPlanOperation::DocumentDelete,
                CicsPlanOperation::DocumentInsert,
                CicsPlanOperation::DocumentRetrieve,
                CicsPlanOperation::DocumentSet,
                CicsPlanOperation::Start,
                CicsPlanOperation::Retrieve,
                CicsPlanOperation::Cancel,
                CicsPlanOperation::Delay,
                CicsPlanOperation::TransformDataToJson,
                CicsPlanOperation::TransformDataToXml,
                CicsPlanOperation::TransformJsonToData,
                CicsPlanOperation::TransformXmlToData,
                CicsPlanOperation::WriteJournalName,
                CicsPlanOperation::WriteJournalNum,
                CicsPlanOperation::InvokeService,
                CicsPlanOperation::SoapFaultAdd,
                CicsPlanOperation::SoapFaultCreate,
                CicsPlanOperation::SoapFaultDelete,
                CicsPlanOperation::WsaContextBuild,
                CicsPlanOperation::WsaContextDelete,
                CicsPlanOperation::WsaContextGet,
                CicsPlanOperation::WsaEprCreate,
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
