//! Private preparation; no command route or policy/configuration owner binds this module yet.
#![allow(dead_code)]

use super::{CicsService, normalize_program_name};
use crate::service::Run;
use mainframe_env_host_api::{
    AccessIntent, ClassName, HostLimits, HostProblem, HostRequest, HostResult, ResourceName,
    SecurityDecision, SecurityRequest,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CheckBoundary {
    Command,
    Resource,
}

/// Fully resolved host configuration, never an operand or invocation binding.
/// Class, profile and access have no default. These are product host-name bounds,
/// not a deployed RACF namespace or proof that an IBM example establishes policy.
#[derive(Clone, Debug)]
pub(super) struct ConfiguredCheck {
    pub(super) active: bool,
    pub(super) class: String,
    pub(super) profile: String,
    pub(super) intent: AccessIntent,
}

#[derive(Clone, Debug)]
pub(super) struct NamedInquirySecurityPlan {
    pub(super) program: String,
    pub(super) command: Option<ConfiguredCheck>,
    pub(super) resource: Option<ConfiguredCheck>,
}

/// Keep product failures and unexpected decisions outside source-condition handling.
/// Only `Denied` carries a source denial for the existing condition owner to consume.
#[derive(Debug, Eq, PartialEq)]
pub(super) enum InquirySecurityFailure {
    MissingPlan,
    MissingCheck(CheckBoundary),
    InactiveCheck(CheckBoundary),
    ProgramBindingMismatch,
    InvalidConfiguration(CheckBoundary),
    Product(HostProblem),
    UnexpectedDecision {
        boundary: CheckBoundary,
        decision: SecurityDecision,
    },
    Denied {
        boundary: CheckBoundary,
        condition: HostProblem,
    },
}

impl ConfiguredCheck {
    fn request(
        &self,
        run: &Run,
        boundary: CheckBoundary,
    ) -> Result<HostRequest, InquirySecurityFailure> {
        if !self.active {
            return Err(InquirySecurityFailure::InactiveCheck(boundary));
        }
        let limits = HostLimits::default();
        let invalid = || InquirySecurityFailure::InvalidConfiguration(boundary);
        let class = ClassName::new(&self.class, limits.max_name_bytes).map_err(|_| invalid())?;
        let resource =
            ResourceName::new(&self.profile, limits.max_name_bytes).map_err(|_| invalid())?;
        let request = HostRequest::Security(SecurityRequest::Authorize {
            // The actor used by nested dispatch owns this principal; caller bindings cannot
            // replace it. Transaction admission and trusted issuer/cohort checks remain callers'
            // prerequisites, not conclusions of this private check.
            principal: run.current_program.effect_invocation.principal.id().clone(),
            class: class.as_str().into(),
            resource,
            intent: self.intent,
        });
        request
            .validate(limits)
            .map_err(InquirySecurityFailure::Product)?;
        Ok(request)
    }
}

/// Check the selected private cohort's command boundary, then its resource boundary.
/// This order is a product plan, not IBM precedence for simultaneous invalid states.
/// INQUIRE PROGRAM e3d8ed4c, parser 605–612 distinguishes actual denials. Command
/// resource PROGRAM/prefix roles are pinned by 57539dc4, parser 3–16, 306–310;
/// deployment activation/class/access resolution remains a future owner binding.
pub(super) fn check_named_inquiry(
    service: &CicsService,
    run: &mut Run,
    program: &str,
    plan: Option<&NamedInquirySecurityPlan>,
) -> Result<(), InquirySecurityFailure> {
    let name = normalize_program_name(program).map_err(InquirySecurityFailure::Product)?;
    let plan = plan.ok_or(InquirySecurityFailure::MissingPlan)?;
    let configured =
        normalize_program_name(&plan.program).map_err(InquirySecurityFailure::Product)?;
    if name != configured {
        return Err(InquirySecurityFailure::ProgramBindingMismatch);
    }
    // Preflight both configurations before exposing either check to a provider.
    let command = plan
        .command
        .as_ref()
        .ok_or(InquirySecurityFailure::MissingCheck(CheckBoundary::Command))?
        .request(run, CheckBoundary::Command)?;
    let resource = plan
        .resource
        .as_ref()
        .ok_or(InquirySecurityFailure::MissingCheck(
            CheckBoundary::Resource,
        ))?
        .request(run, CheckBoundary::Resource)?;
    for (boundary, request) in [
        (CheckBoundary::Command, command),
        (CheckBoundary::Resource, resource),
    ] {
        match service
            .nested(run, request)
            .map_err(InquirySecurityFailure::Product)?
        {
            HostResult::Security(SecurityDecision::Allow) => {}
            HostResult::Security(SecurityDecision::Deny) => {
                return Err(InquirySecurityFailure::Denied {
                    boundary,
                    condition: HostProblem::Condition {
                        name: "NOTAUTH".into(),
                        response: 70,
                        response2: match boundary {
                            CheckBoundary::Command => 100,
                            CheckBoundary::Resource => 101,
                        },
                    },
                });
            }
            HostResult::Security(decision) => {
                return Err(InquirySecurityFailure::UnexpectedDecision { boundary, decision });
            }
            _ => {
                return Err(InquirySecurityFailure::Product(
                    HostProblem::ProviderFailure,
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::handlers::{CicsApplicationEntryDefinition, CicsJavaStatus};
    use crate::service::{CicsLimits, DatasetUndo, UowRecord, encode_uow};
    use mainframe_env_execution_api::{
        ArtifactRef, AuditDecision, BoundedPayload, CancellationProbe, CapabilityId,
        IdempotencyKey, Invocation, InvocationLimits, Principal,
    };
    use mainframe_env_host_api::{
        CapabilityDescriptor, CicsConditionPolicy, CicsOperation, CicsRequest,
        CicsUnitOfWorkOutcome, DatasetName, EffectRequest, EffectResult, HostProvider, Mutation,
        RegistrySnapshot, ScopedHostService, canonical_audit_resource_digest,
        canonical_request_digest, canonical_result_digest,
    };
    use mainframe_env_store::MemoryStore;
    use mainframe_env_store_api::{
        ArtifactRecord, ArtifactStore, AuditSink, ExecutableArtifactMetadata, ProviderStateRecord,
        ProviderStateStore,
    };
    use sha2::{Digest, Sha256};
    use std::collections::{BTreeMap, BTreeSet, VecDeque};
    use std::sync::{Arc, Mutex};

    // A scripted boundary fixture, not a second policy evaluator: each response is
    // independently seeded, and an unexpected additional dispatch fails the test.
    struct Replies {
        descriptor: CapabilityDescriptor,
        outcomes: Mutex<VecDeque<Result<HostResult, HostProblem>>>,
        seen: Mutex<Vec<(Invocation, EffectRequest)>>,
        cancel_after_first: Mutex<Option<CancellationProbe>>,
        wrong_sequence: Mutex<bool>,
    }

    impl HostProvider for Replies {
        fn descriptor(&self) -> &CapabilityDescriptor {
            &self.descriptor
        }

        fn invoke(&self, actor: &Invocation, effect: EffectRequest) -> EffectResult {
            let mut seen = self.seen.lock().unwrap();
            seen.push((actor.clone(), effect.clone()));
            if seen.len() == 1
                && let Some(probe) = self.cancel_after_first.lock().unwrap().as_ref()
            {
                probe.request();
            }
            EffectResult {
                sequence: effect.sequence + u64::from(*self.wrong_sequence.lock().unwrap()),
                outcome: self
                    .outcomes
                    .lock()
                    .unwrap()
                    .pop_front()
                    .expect("unplanned dispatch"),
            }
        }
    }

    fn allow() -> Result<HostResult, HostProblem> {
        Ok(HostResult::Security(SecurityDecision::Allow))
    }

    fn plan() -> NamedInquirySecurityPlan {
        NamedInquirySecurityPlan {
            program: "ONPGM".into(),
            command: Some(ConfiguredCheck {
                active: true,
                class: "HOSTCMD".into(),
                profile: "CONFIGURED.COMMAND.PROGRAM".into(),
                intent: AccessIntent::Read,
            }),
            resource: Some(ConfiguredCheck {
                active: true,
                class: "HOSTPGM".into(),
                profile: "CONFIGURED.RESOURCE.ONPGM".into(),
                // Deliberately different, explicit pilot configuration, not an IBM default.
                intent: AccessIntent::Control,
            }),
        }
    }

    fn definition(
        store: &MemoryStore,
        generation: u64,
        enabled: bool,
    ) -> super::super::CicsProgramDefinition {
        let bytes = format!("security-fixture:{generation}").into_bytes();
        let digest = Sha256::digest(&bytes).into();
        let artifact = ArtifactRef::new(
            format!("sha256:{:x}", Sha256::digest(&bytes)),
            InvocationLimits::default(),
        )
        .unwrap();
        let semantic_identity = format!("semantic-sha256:{:x}", Sha256::digest(&bytes));
        let executable = ExecutableArtifactMetadata {
            artifact_contract: "mainframe-env.artifact@3".into(),
            compatibility_profile: "mainframe-env.cobol.reference@1".into(),
            compiler_generation: "security-test".into(),
            target: "reference".into(),
            options: BTreeMap::new(),
            host_interfaces: BTreeSet::from(["mainframe-env.cics@1".into()]),
            ir_contract: "mainframe-env.ir@1".into(),
            dialect_contracts: Some(BTreeSet::from(["mainframe-env.cobol@1".into()])),
            semantic_identity: semantic_identity.clone(),
            manifest_payload_digest: [0; 32],
        }
        .bind_to_payload(&digest);
        store
            .put_artifact(ArtifactRecord {
                artifact: artifact.clone(),
                media_type: "application/vnd.mainframe-env.core-mir".into(),
                payload_digest: digest,
                payload: bytes,
                executable: Some(executable),
            })
            .unwrap();
        super::super::CicsProgramDefinition {
            name: "ONPGM".into(),
            generation,
            artifact,
            semantic_identity,
            entry_offset: 1,
            enabled,
            remote: false,
            reload: false,
            java_status: CicsJavaStatus::NotJava,
        }
    }

    struct Fixture {
        cics: Arc<CicsService>,
        store: Arc<MemoryStore>,
        replies: Arc<Replies>,
        run: Run,
    }

    impl Fixture {
        fn new() -> Self {
            let store = Arc::new(MemoryStore::new(Default::default()));
            let replies = Arc::new(Replies {
                descriptor: CapabilityDescriptor {
                    capability: CapabilityId::new(
                        "host.security.authorize",
                        InvocationLimits::default(),
                    )
                    .unwrap(),
                    provider_id: "independent-script".into(),
                    generation: "1".into(),
                    request_schema: "request@1".into(),
                    result_schema: "result@1".into(),
                    max_request_bytes: 8192,
                    max_result_bytes: 8192,
                    ready: true,
                },
                outcomes: Mutex::new(VecDeque::from([allow(), allow()])),
                seen: Mutex::new(Vec::new()),
                cancel_after_first: Mutex::new(None),
                wrong_sequence: Mutex::new(false),
            });
            let host = Arc::new(ScopedHostService::new(
                Arc::new(
                    RegistrySnapshot::new(1, vec![replies.clone()], InvocationLimits::default())
                        .unwrap(),
                ),
                HostLimits::default(),
            ));
            let cics = CicsService::open(host, store.clone(), CicsLimits::default()).unwrap();
            cics.bind_artifact_store(store.clone()).unwrap();
            cics.register_programs(&BTreeSet::from(["LEGACY".into()]))
                .unwrap();
            let old = definition(&store, 2, true);
            cics.register_program_definitions(&[old.clone()]).unwrap();
            let actor = crate::service::tests::invocation();
            // Explicit fixture prerequisite: register_run itself does not perform
            // transaction admission. This is not a selected SPI caller binding.
            cics.authorize_terminal(&actor, "DEFAULT").unwrap();
            cics.ensure_run(&actor).unwrap();
            let mut run = cics.lock().unwrap().runs[&actor.run_unit_id].clone();
            let request = CicsRequest {
                operation: CicsOperation::Load,
                arguments: BTreeMap::from([
                    (
                        "PROGRAM".into(),
                        BoundedPayload::new(
                            "mainframe-env.cics.literal@1",
                            b"ONPGM".to_vec(),
                            InvocationLimits::default(),
                        )
                        .unwrap(),
                    ),
                    (
                        "OPTION.HOLD".into(),
                        BoundedPayload::new(
                            "mainframe-env.cics.option@1",
                            Vec::new(),
                            InvocationLimits::default(),
                        )
                        .unwrap(),
                    ),
                ]),
                condition_policy: CicsConditionPolicy::Default,
                mutation: Some(Mutation {
                    sequence: 1,
                    idempotency_key: IdempotencyKey::new(
                        "security-fixture-load",
                        InvocationLimits::default(),
                    )
                    .unwrap(),
                    transaction: None,
                }),
            };
            crate::service::handlers::program_control::load::invoke(&cics, &mut run, &request)
                .unwrap();
            cics.register_application_entries(&[CicsApplicationEntryDefinition {
                application: "FIXTURE".into(),
                platform: "FIXTURE".into(),
                major_version: 1,
                minor_version: 0,
                micro_version: 0,
                operation: "ENTRY".into(),
                program: "ONPGM".into(),
                program_generation: 2,
                program_artifact: old.artifact,
                application_identity: format!("sha256:{:064x}", 1),
                available: true,
            }])
            .unwrap();
            cics.register_program_definitions(&[definition(&store, 3, false)])
                .unwrap();
            cics.append_undo(
                &mut run,
                DatasetUndo::Restore {
                    dataset: DatasetName::new("TEST.DATA", 246).unwrap(),
                    key: b"1".to_vec(),
                    record: b"before".to_vec(),
                },
            )
            .unwrap();
            store
                .put_provider_state(
                    ProviderStateRecord {
                        namespace: "cics-uow".into(),
                        key: "security-fixture-uow".into(),
                        version: 1,
                        payload: encode_uow(&UowRecord {
                            finalized: false,
                            outcome: CicsUnitOfWorkOutcome::Committed,
                            transaction: "DEFAULT".into(),
                            metadata: None,
                        })
                        .unwrap(),
                    },
                    None,
                )
                .unwrap();
            assert!(replies.outcomes.lock().unwrap().is_empty());
            replies.seen.lock().unwrap().clear();
            Self {
                cics,
                store,
                replies,
                run,
            }
        }

        fn script(&self, outcomes: Vec<Result<HostResult, HostProblem>>) {
            *self.replies.outcomes.lock().unwrap() = outcomes.into();
        }

        fn requests(&self) -> Vec<(Invocation, EffectRequest)> {
            self.replies.seen.lock().unwrap().clone()
        }

        fn snapshot(&self) -> Snapshot {
            let state = self.cics.lock().unwrap();
            let frame = &self.run.current_program;
            let mut frame_actor = frame.effect_invocation.clone();
            // The live probe can change during a real host check; its control flag is
            // checked separately. All immutable invocation fields remain in this snapshot.
            frame_actor.cancellation_probe = None;
            Snapshot {
                installed: format!(
                    "{:?}",
                    (
                        &state.programs,
                        &state.program_definitions,
                        &state.application_entries,
                        &state.program_loads
                    )
                ),
                rows: self.store.list_provider_state_prefix("cics", 1024).unwrap(),
                // Frame metadata and the whole retained invocation are independent of the
                // legitimate task-global host sequence advancement.
                frame: format!(
                    "{:?}",
                    (
                        &frame_actor,
                        frame.program_occurrence,
                        frame.logical_level,
                        &frame.invoking_program,
                        &frame.return_program,
                        &frame.current,
                        &frame.channel,
                        &frame.parent_execution_id,
                        frame.initial_entry
                    )
                ),
                task: format!(
                    "{:?}{:?}{:?}",
                    (
                        &self.run.invocation,
                        &self.run.session,
                        &self.run.transaction,
                        &self.run.applid,
                        &self.run.sysid,
                        &self.run.originating_task,
                        &self.run.outer_effect_key
                    ),
                    (
                        &self.run.handlers,
                        &self.run.aid_handlers,
                        &self.run.ignored_conditions,
                        &self.run.abend_handler,
                        &self.run.cancelled_abend_handler,
                        &self.run.handle_stack,
                        &self.run.latest_abend,
                        self.run.program_abend.as_ref().map(|pending| (
                            &pending.response,
                            &pending.record,
                            pending.cancel_exits
                        ))
                    ),
                    (
                        &self.run.retrieve,
                        &self.run.current_records,
                        &self.run.undo,
                        self.run.undo_version,
                        &self.run.browses,
                        &self.run.trace
                    )
                ),
                file_updates: format!(
                    "{:?}{:?}",
                    &self.run.file_updates.current_record_values,
                    self.run
                        .file_updates
                        .file_tokens
                        .iter()
                        .map(|(id, token)| (
                            id,
                            &token.dataset,
                            &token.identity,
                            &token.record,
                            &token.browse_cursor
                        ))
                        .collect::<Vec<_>>()
                ),
            }
        }

        fn audits(&self) -> Vec<mainframe_env_execution_api::AuditRecord> {
            self.store
                .audit_records(
                    &self.run.current_program.effect_invocation.execution_id,
                    2,
                    16,
                )
                .unwrap()
        }
    }

    #[derive(Debug, Eq, PartialEq)]
    struct Snapshot {
        installed: String,
        rows: Vec<ProviderStateRecord>,
        frame: String,
        task: String,
        file_updates: String,
    }

    fn expected_request(boundary: CheckBoundary) -> HostRequest {
        let (class, profile, intent) = match boundary {
            CheckBoundary::Command => ("HOSTCMD", "CONFIGURED.COMMAND.PROGRAM", AccessIntent::Read),
            CheckBoundary::Resource => (
                "HOSTPGM",
                "CONFIGURED.RESOURCE.ONPGM",
                AccessIntent::Control,
            ),
        };
        HostRequest::Security(SecurityRequest::Authorize {
            principal: mainframe_env_execution_api::PrincipalId::new(
                "IBMUSER",
                InvocationLimits::default(),
            )
            .unwrap(),
            class: class.into(),
            resource: ResourceName::new(profile, 128).unwrap(),
            intent,
        })
    }

    fn assert_trace(f: &Fixture, decisions: &[AuditDecision], dispatches: usize) {
        let seen = f.requests();
        assert_eq!(seen.len(), dispatches);
        let actor = &f.run.current_program.effect_invocation;
        for (index, (actual_actor, effect)) in seen.iter().enumerate() {
            let expected = expected_request(if index == 0 {
                CheckBoundary::Command
            } else {
                CheckBoundary::Resource
            });
            assert_eq!(effect.request, expected);
            assert_eq!(
                canonical_request_digest(&effect.request).unwrap(),
                canonical_request_digest(&expected).unwrap()
            );
            assert_eq!(actual_actor.principal, actor.principal);
            assert_eq!(actual_actor.execution_id, actor.execution_id);
            assert_eq!(effect.run_unit, actor.run_unit_id);
            assert_eq!(effect.sequence, index as u64 + 2);
            assert_eq!(effect.deadline_tick, 100);
            assert_eq!(effect.idempotency_key, None);
            assert!(!effect.request.is_mutating());
        }
        let audits = f.audits();
        assert_eq!(audits.len(), decisions.len());
        for (index, (audit, decision)) in audits.iter().zip(decisions).enumerate() {
            assert_eq!(audit.decision, *decision);
            assert_eq!(audit.effect_sequence, index as u64 + 2);
            assert_eq!(audit.observed_tick, 99);
            assert_eq!(audit.principal.as_str(), "IBMUSER");
            assert_eq!(audit.execution_id, actor.execution_id);
            assert_eq!(audit.run_unit_id, actor.run_unit_id);
            assert_eq!(audit.attempt, actor.attempt);
            assert_eq!(audit.invocation_key, actor.idempotency_key);
            assert_eq!(audit.capability.as_str(), "host.security.authorize");
            assert_eq!(
                audit.resource,
                canonical_audit_resource_digest(&expected_request(if index == 0 {
                    CheckBoundary::Command
                } else {
                    CheckBoundary::Resource
                }))
            );
        }
        assert_eq!(f.run.host_sequence, 1 + decisions.len() as u64);
    }

    #[test]
    fn missing_or_inactive_configuration_never_dispatches() {
        for choice in 0..5 {
            let mut f = Fixture::new();
            let mut p = plan();
            let expected = match choice {
                0 => InquirySecurityFailure::MissingPlan,
                1 => {
                    p.command = None;
                    InquirySecurityFailure::MissingCheck(CheckBoundary::Command)
                }
                2 => {
                    p.resource = None;
                    InquirySecurityFailure::MissingCheck(CheckBoundary::Resource)
                }
                3 => {
                    p.command.as_mut().unwrap().active = false;
                    InquirySecurityFailure::InactiveCheck(CheckBoundary::Command)
                }
                _ => {
                    p.resource.as_mut().unwrap().active = false;
                    InquirySecurityFailure::InactiveCheck(CheckBoundary::Resource)
                }
            };
            let before = f.snapshot();
            assert_eq!(
                check_named_inquiry(&f.cics, &mut f.run, "ONPGM", (choice != 0).then_some(&p)),
                Err(expected)
            );
            assert_eq!(f.snapshot(), before);
            assert_trace(&f, &[], 0);
        }
    }

    #[test]
    fn invalid_names_and_both_configurations_are_preflighted() {
        for name in ["", " ", "ABCDEFGHI", "A_B", "A B", "A\0B", "é"] {
            let mut f = Fixture::new();
            let before = f.snapshot();
            assert_eq!(
                check_named_inquiry(&f.cics, &mut f.run, name, Some(&plan())),
                Err(InquirySecurityFailure::Product(HostProblem::Malformed))
            );
            assert_eq!(f.snapshot(), before);
            assert_trace(&f, &[], 0);
        }
        for boundary in [CheckBoundary::Command, CheckBoundary::Resource] {
            for class in [true, false] {
                for value in [
                    String::new(),
                    "X".repeat(129),
                    "BAD SPACE".into(),
                    "A\0B".into(),
                    "é".into(),
                ] {
                    let mut f = Fixture::new();
                    let mut p = plan();
                    let check = if boundary == CheckBoundary::Command {
                        p.command.as_mut()
                    } else {
                        p.resource.as_mut()
                    }
                    .unwrap();
                    if class {
                        check.class = value;
                    } else {
                        check.profile = value;
                    }
                    let before = f.snapshot();
                    assert_eq!(
                        check_named_inquiry(&f.cics, &mut f.run, "ONPGM", Some(&p)),
                        Err(InquirySecurityFailure::InvalidConfiguration(boundary))
                    );
                    assert_eq!(f.snapshot(), before);
                    assert_trace(&f, &[], 0);
                }
            }
        }
        for (configured, expected) in [
            ("OTHER", InquirySecurityFailure::ProgramBindingMismatch),
            (
                "ABCDEFGHI",
                InquirySecurityFailure::Product(HostProblem::Malformed),
            ),
        ] {
            let mut f = Fixture::new();
            let mut p = plan();
            p.program = configured.into();
            let before = f.snapshot();
            assert_eq!(
                check_named_inquiry(&f.cics, &mut f.run, "ONPGM", Some(&p)),
                Err(expected)
            );
            assert_eq!(f.snapshot(), before);
            assert_trace(&f, &[], 0);
        }
    }

    #[test]
    fn allows_require_two_exact_configured_requests_and_preserve_complete_state() {
        let mut f = Fixture::new();
        f.script(vec![allow(), allow()]);
        // Arbitrary caller bindings cannot supply a class, profile, access or principal.
        f.run.current_program.effect_invocation.bindings.insert(
            "security.profile".into(),
            BoundedPayload::new("caller@1", b"FORGED".to_vec(), InvocationLimits::default())
                .unwrap(),
        );
        let before = f.snapshot();
        assert_eq!(
            check_named_inquiry(&f.cics, &mut f.run, " onpgm ", Some(&plan())),
            Ok(())
        );
        assert_trace(&f, &[AuditDecision::Success, AuditDecision::Success], 2);
        assert_eq!(f.snapshot(), before);
        assert!(f.replies.outcomes.lock().unwrap().is_empty());
    }

    #[test]
    fn source_denials_use_existing_condition_owner_without_later_checks() {
        for (boundary, response2) in [
            (CheckBoundary::Command, 100),
            (CheckBoundary::Resource, 101),
        ] {
            let mut f = Fixture::new();
            let script = if boundary == CheckBoundary::Command {
                vec![Ok(HostResult::Security(SecurityDecision::Deny)), allow()]
            } else {
                vec![allow(), Ok(HostResult::Security(SecurityDecision::Deny))]
            };
            f.script(script);
            let before = f.snapshot();
            let condition = HostProblem::Condition {
                name: "NOTAUTH".into(),
                response: 70,
                response2,
            };
            assert_eq!(
                check_named_inquiry(&f.cics, &mut f.run, "ONPGM", Some(&plan())),
                Err(InquirySecurityFailure::Denied {
                    boundary,
                    condition: condition.clone()
                })
            );
            let response = crate::service::handlers::condition::respond(
                &f.cics,
                &f.run,
                &CicsConditionPolicy::NoHandle,
                condition,
            )
            .unwrap();
            assert_eq!(
                (
                    response.condition.as_str(),
                    response.response,
                    response.response2
                ),
                ("NOTAUTH", 70, response2)
            );
            if boundary == CheckBoundary::Command {
                assert_trace(&f, &[AuditDecision::Deny], 1);
                assert_eq!(f.replies.outcomes.lock().unwrap().len(), 1);
            } else {
                assert_trace(&f, &[AuditDecision::Success, AuditDecision::Deny], 2);
            }
            assert_eq!(f.snapshot(), before);
        }
    }

    #[test]
    fn every_non_deny_security_decision_is_retained_as_a_product_failure() {
        for boundary in [CheckBoundary::Command, CheckBoundary::Resource] {
            for decision in [
                SecurityDecision::NotFound,
                SecurityDecision::InvalidCredentials,
                SecurityDecision::Expired,
                SecurityDecision::Revoked,
                SecurityDecision::Locked,
            ] {
                let mut f = Fixture::new();
                let mut script = Vec::new();
                if boundary == CheckBoundary::Resource {
                    script.push(allow());
                }
                script.push(Ok(HostResult::Security(decision.clone())));
                script.push(allow());
                f.script(script);
                let before = f.snapshot();
                assert_eq!(
                    check_named_inquiry(&f.cics, &mut f.run, "ONPGM", Some(&plan())),
                    Err(InquirySecurityFailure::UnexpectedDecision { boundary, decision })
                );
                let count = if boundary == CheckBoundary::Command {
                    1
                } else {
                    2
                };
                // The host's existing transport audit is Success for non-Deny decisions;
                // it does not turn the private inquiry check into an authorization success.
                assert_trace(&f, &vec![AuditDecision::Success; count], count);
                assert_eq!(f.replies.outcomes.lock().unwrap().len(), 1);
                assert_eq!(f.snapshot(), before);
            }
        }
    }

    #[test]
    fn host_failures_and_wrong_results_never_become_source_denials() {
        let cases = [
            (
                Err(HostProblem::Unauthorized),
                HostProblem::Unauthorized,
                AuditDecision::Deny,
            ),
            (
                Err(HostProblem::ProviderFailure),
                HostProblem::ProviderFailure,
                AuditDecision::ProviderFailure,
            ),
            (
                Err(HostProblem::InfrastructureFailure),
                HostProblem::InfrastructureFailure,
                AuditDecision::InfrastructureFailure,
            ),
            (
                Err(HostProblem::TimedOut),
                HostProblem::TimedOut,
                AuditDecision::TimedOut,
            ),
            (
                Err(HostProblem::Cancelled),
                HostProblem::Cancelled,
                AuditDecision::Cancelled,
            ),
            (
                Err(HostProblem::UnknownOutcome),
                HostProblem::UnknownOutcome,
                AuditDecision::UnknownOutcome,
            ),
            (
                Ok(HostResult::Clock("unexpected".into())),
                HostProblem::ProviderFailure,
                AuditDecision::Success,
            ),
            (
                Err(HostProblem::Condition {
                    name: "NOTAUTH".into(),
                    response: 70,
                    response2: 100,
                }),
                HostProblem::Condition {
                    name: "NOTAUTH".into(),
                    response: 70,
                    response2: 100,
                },
                AuditDecision::Rejected,
            ),
        ];
        for boundary in [CheckBoundary::Command, CheckBoundary::Resource] {
            for (outcome, expected, audit) in &cases {
                let mut f = Fixture::new();
                let mut script = Vec::new();
                let mut audits = Vec::new();
                if boundary == CheckBoundary::Resource {
                    script.push(allow());
                    audits.push(AuditDecision::Success);
                }
                script.push(outcome.clone());
                script.push(allow());
                audits.push(*audit);
                f.script(script);
                let before = f.snapshot();
                assert_eq!(
                    check_named_inquiry(&f.cics, &mut f.run, "ONPGM", Some(&plan())),
                    Err(InquirySecurityFailure::Product(expected.clone()))
                );
                assert_trace(&f, &audits, audits.len());
                assert_eq!(f.replies.outcomes.lock().unwrap().len(), 1);
                assert_eq!(f.snapshot(), before);
            }
        }
    }

    #[test]
    fn missing_grant_is_a_host_failure_with_no_provider_dispatch() {
        let mut f = Fixture::new();
        let actor = &mut f.run.current_program.effect_invocation;
        actor.principal = Principal::new(
            actor.principal.id().clone(),
            BTreeSet::new(),
            InvocationLimits::default(),
        )
        .unwrap();
        let before = f.snapshot();
        assert_eq!(
            check_named_inquiry(&f.cics, &mut f.run, "ONPGM", Some(&plan())),
            Err(InquirySecurityFailure::Product(HostProblem::Unauthorized))
        );
        assert_trace(&f, &[AuditDecision::Deny], 0);
        assert_eq!(f.snapshot(), before);
    }

    #[test]
    fn live_cancellation_stops_first_or_second_host_dispatch() {
        for after_first in [false, true] {
            let mut f = Fixture::new();
            let probe = CancellationProbe::new();
            f.run.current_program.effect_invocation = f
                .run
                .current_program
                .effect_invocation
                .clone()
                .with_cancellation_probe(probe.clone());
            if after_first {
                f.script(vec![allow(), allow()]);
                *f.replies.cancel_after_first.lock().unwrap() = Some(probe.clone());
            } else {
                probe.request();
            }
            // Compare complete state after control enrichment. A live probe's observed flag
            // legitimately changes during dispatch and is not a resource mutation.
            let before = f.snapshot();
            assert_eq!(
                check_named_inquiry(&f.cics, &mut f.run, "ONPGM", Some(&plan())),
                Err(InquirySecurityFailure::Product(HostProblem::Cancelled))
            );
            if after_first {
                assert_trace(&f, &[AuditDecision::Success, AuditDecision::Cancelled], 1);
            } else {
                assert_trace(&f, &[AuditDecision::Cancelled], 0);
            }
            assert_eq!(f.snapshot(), before);
            assert!(probe.is_requested());
        }
    }

    #[test]
    fn malformed_reply_envelope_stays_a_product_failure() {
        let mut f = Fixture::new();
        f.script(vec![allow(), allow()]);
        *f.replies.wrong_sequence.lock().unwrap() = true;
        let before = f.snapshot();
        assert_eq!(
            check_named_inquiry(&f.cics, &mut f.run, "ONPGM", Some(&plan())),
            Err(InquirySecurityFailure::Product(HostProblem::Malformed))
        );
        assert_trace(&f, &[AuditDecision::Rejected], 1);
        assert_eq!(f.snapshot(), before);
    }

    #[test]
    fn actual_host_deadline_rejection_is_audited_without_a_security_decision() {
        let f = Fixture::new();
        let actor = &f.run.current_program.effect_invocation;
        let before = f.snapshot();
        let result = f
            .cics
            .host
            .invoke(
                actor,
                100,
                false,
                EffectRequest {
                    run_unit: actor.run_unit_id.clone(),
                    sequence: 2,
                    deadline_tick: 100,
                    idempotency_key: None,
                    request: expected_request(CheckBoundary::Command),
                },
            )
            .persist_with(|audit| {
                f.store
                    .record_audit(audit)
                    .map_err(|_| HostProblem::InfrastructureFailure)
            });
        assert_eq!(result.outcome, Err(HostProblem::TimedOut));
        assert_eq!(
            canonical_result_digest(&result.outcome).unwrap(),
            canonical_result_digest(&Err(HostProblem::TimedOut)).unwrap()
        );
        assert!(f.requests().is_empty());
        let audit = f.audits();
        assert_eq!(audit.len(), 1);
        assert_eq!(audit[0].decision, AuditDecision::TimedOut);
        assert_eq!(audit[0].observed_tick, 100);
        assert_eq!(
            audit[0].resource,
            canonical_audit_resource_digest(&expected_request(CheckBoundary::Command))
        );
        assert_eq!(f.snapshot(), before);
        // This supplements nested-path tests. nested currently chooses deadline-1 as
        // now_tick; expiry from a trusted live clock is a manager-owned prerequisite.
    }

    #[test]
    fn existing_host_name_bounds_accept_exact_edges_without_policy_defaults() {
        for length in [1, 128] {
            let mut f = Fixture::new();
            let mut p = plan();
            p.program = "A1$@#XYZ".into();
            for check in [&mut p.command, &mut p.resource] {
                let check = check.as_mut().unwrap();
                check.class = "C".repeat(length);
                check.profile = "P".repeat(length);
            }
            f.script(vec![allow(), allow()]);
            let before = f.snapshot();
            assert_eq!(
                check_named_inquiry(&f.cics, &mut f.run, " a1$@#xyz ", Some(&p)),
                Ok(())
            );
            let requests = f.requests();
            assert_eq!(requests.len(), 2);
            for (_, effect) in requests {
                let HostRequest::Security(SecurityRequest::Authorize {
                    class, resource, ..
                }) = effect.request
                else {
                    panic!("wrong request");
                };
                assert_eq!(class, "C".repeat(length));
                assert_eq!(resource.as_str(), "P".repeat(length));
            }
            assert_eq!(f.audits().len(), 2);
            assert_eq!(f.snapshot(), before);
        }
    }
}
