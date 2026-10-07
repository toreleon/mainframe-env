//! Bounded row-0218 dependency proof; no official or licensed coverage is assigned.
//!
//! Expectations come from the frozen participant fixtures and application contract,
//! not the SYNCPOINT handler. IBM sources: sources-c dfhp4_syncpoint.html lines
//! 22-41 and dfhp4_syncpointrollback.html lines 22-24, 46-51 (pins in the handoff).

use super::*;
use mainframe_env_cics::{
    CicsUowCodecVersion, CicsUowDependencyState, CicsUowState, describe_cics_uow_row,
};
use mainframe_env_execution_api::{
    AuditDecision, ExplicitSyncpoint, LifecycleEventKind, ParticipantMode, SyncpointOwner,
};
use mainframe_env_host_api::{
    AccessIntent, CicsDisposition, CicsResponse, CicsUnitOfWorkOutcome, ResourceName,
    canonical_audit_resource_digest, canonical_request_digest, canonical_result_digest,
    cics_application_command_identity,
};
use mainframe_env_ir::{
    Attribute, CicsApplicationHandlerReadiness, CicsCondition, CicsOutputName, CicsPlanLimits,
    CicsPlanOperation, CicsPlanOption, CodecLimits, cics_application_registry_for_tokens,
    cics_executable_descriptor, decode_binary, decode_cics_effect_plan,
};
use mainframe_env_store_api::{
    AuditSink, EffectDigestFormat, EffectState, EventStore, ExecutionState, ExecutionStore,
    IdempotencyStore,
};

const ROW: &str = "ibm-cics-ts-6x-2026-08-31:api-commands:0218";
const COMMAND_CONTRACTS: &str = include_str!(
    "../../../../../../conformance/subsystems/cics/application/generated/cics-application-command-contracts.json"
);
const PARTICIPANT_FIXTURES: &str = include_str!(
    "../../../../../../conformance/subsystems/integration/fixtures/transaction-participant-compatibility.json"
);

fn row_contract() -> Value {
    let contracts: Value = serde_json::from_str(COMMAND_CONTRACTS).unwrap();
    contracts["batches"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|batch| batch["commands"].as_array().unwrap())
        .find(|row| row["official_row"] == ROW)
        .unwrap()["contract"]
        .clone()
}

fn source(options: &str) -> String {
    format!(
        "IDENTIFICATION DIVISION.\n\
         PROGRAM-ID. SYNCCONS.\n\
         DATA DIVISION.\n\
         WORKING-STORAGE SECTION.\n\
         01 RESP-X PIC 9(3) VALUE 999.\n\
         01 RESP2-X PIC 9(3) VALUE 999.\n\
         PROCEDURE DIVISION.\n\
         EXEC CICS SYNCPOINT {options} END-EXEC.\n\
         DISPLAY 'CONSUMED:' RESP-X ':' RESP2-X.\n\
         DISPLAY 'EIBFN:' EIBFN.\n\
         STOP RUN.\n"
    )
}

fn bind_artifact(
    artifact: &mainframe_env_compiler_api::PublishedArtifact,
    rollback: bool,
    expected_outputs: &[CicsOutputName],
) {
    let row = row_contract();
    let registration = cics_application_registry_for_tokens(&["SYNCPOINT"]).unwrap();
    let catalog = cics_application_command_identity(ROW).unwrap();
    assert_eq!(registration.official_row, ROW);
    assert_eq!(catalog.label, "SYNCPOINT");
    assert_eq!(catalog.eibfn, [0x16, 0x02]);
    assert_eq!(registration.eibfn, catalog.eibfn);
    assert_eq!(registration.handler_id, row["registry"]["handler_id"]);
    assert_eq!(
        registration.handler_sha256,
        row["registry"]["handler_sha256"]
    );
    assert_eq!(registration.runtime_operation, Some("Syncpoint"));
    assert_eq!(
        registration.readiness,
        CicsApplicationHandlerReadiness::TypedRuntime
    );
    assert!(registration.advertised);
    assert_eq!(
        row["effect"]["canonical_identity"],
        "mainframe-env.effect-canonical@1"
    );
    assert_eq!(row["resource"]["scope"], "cics-transaction");
    assert_eq!(row["resource"]["access_intent"], "update");
    assert_eq!(row["capability"]["route"], "host.cics.execute");
    assert_eq!(row["audit"]["required"], true);
    assert_eq!(row["recovery"]["automatic_redispatch"], false);
    assert_eq!(row["applicability"]["dpl_restriction"]["resp2"], 200);

    let module = decode_binary(artifact.payload(), CodecLimits::default()).unwrap();
    let operations = module
        .regions()
        .iter()
        .flat_map(|region| &region.blocks)
        .flat_map(|block| &block.operations)
        .collect::<Vec<_>>();
    assert!(
        !operations
            .iter()
            .any(|op| op.identity.name() == "exec_cics")
    );
    let typed = operations
        .iter()
        .filter(|op| op.identity.namespace() == "cics.recovery")
        .collect::<Vec<_>>();
    assert_eq!(typed.len(), 1);
    let descriptor = cics_executable_descriptor(CicsPlanOperation::Syncpoint);
    assert!(descriptor.is_registered());
    assert_eq!(typed[0].identity, descriptor.identity());
    assert_eq!(typed[0].effects, descriptor.effects);
    let Attribute::Bytes(bytes) = &typed[0].attributes["cics_plan"] else {
        panic!("row 0218 must carry a typed plan");
    };
    let plan = decode_cics_effect_plan(bytes, CicsPlanLimits::default()).unwrap();
    assert_eq!(plan.operation, CicsPlanOperation::Syncpoint);
    assert!(plan.operands.is_empty());
    assert_eq!(
        plan.outputs.iter().map(|o| o.name).collect::<Vec<_>>(),
        expected_outputs
    );
    assert_eq!(plan.options.contains(&CicsPlanOption::Rollback), rollback);
    assert!(
        artifact
            .manifest()
            .dialect_contracts
            .iter()
            .any(|d| d == "cics.recovery@1")
    );
}

#[test]
fn syncpoint_contract_consumption_binds_registration_options_and_conditions() {
    let row = row_contract();
    let registration = cics_application_registry_for_tokens(&["SYNCPOINT"]).unwrap();
    let entries = row["options"]["entries"].as_array().unwrap();
    assert_eq!(registration.options.len(), 4);
    for (descriptor, frozen) in registration.options.iter().zip(entries) {
        assert_eq!(descriptor.name, frozen["name"]);
        assert_eq!(
            descriptor.source_max_value_bytes,
            frozen["source_max_value_bytes"]
                .as_u64()
                .map(|n| usize::try_from(n).unwrap())
        );
        assert_eq!(
            format!("{:?}", descriptor.direction).to_lowercase(),
            frozen["directions"][0]
        );
        assert_eq!(
            format!("{:?}", descriptor.value_shape).to_lowercase(),
            frozen["value_shape"]
        );
    }
    assert_eq!(
        registration.top_level_options,
        ["NOHANDLE", "RESP", "RESP2", "ROLLBACK"]
    );
    assert_eq!(registration.dependencies.len(), 1);
    assert_eq!(registration.dependencies[0].option, "RESP2");
    assert_eq!(registration.dependencies[0].requires, ["RESP"]);
    let conditions = row["eib_response"]["conditions"].as_array().unwrap();
    assert_eq!(
        conditions
            .iter()
            .map(|c| (
                c["condition"].as_str().unwrap(),
                c["resp"].as_i64().unwrap()
            ))
            .collect::<Vec<_>>(),
        [("INVREQ", 16), ("ROLLEDBACK", 82)]
    );
    assert!(
        conditions[0]["outcomes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["resp2"] == 200)
    );
    for (options, expected_outputs) in [
        ("", &[][..]),
        ("NOHANDLE", &[][..]),
        ("RESP(RESP-X)", &[CicsOutputName::Resp][..]),
        (
            "RESP(RESP-X) RESP2(RESP2-X)",
            &[CicsOutputName::Resp, CicsOutputName::Resp2][..],
        ),
        (
            "ROLLBACK RESP(RESP-X) RESP2(RESP2-X)",
            &[CicsOutputName::Resp, CicsOutputName::Resp2][..],
        ),
    ] {
        let artifact = crate::compile(&source(options)).unwrap();
        bind_artifact(&artifact, options.starts_with("ROLLBACK"), expected_outputs);
        let module = decode_binary(artifact.payload(), CodecLimits::default()).unwrap();
        let op = module
            .regions()
            .iter()
            .flat_map(|r| &r.blocks)
            .flat_map(|b| &b.operations)
            .find(|op| op.identity.namespace() == "cics.recovery")
            .unwrap();
        let Attribute::Bytes(bytes) = &op.attributes["cics_plan"] else {
            unreachable!()
        };
        let plan = decode_cics_effect_plan(bytes, CicsPlanLimits::default()).unwrap();
        match (&plan.condition, options) {
            (CicsCondition::Default, "") | (CicsCondition::NoHandle, "NOHANDLE") => {}
            (
                CicsCondition::Respond {
                    response,
                    response2,
                },
                "RESP(RESP-X)"
                | "RESP(RESP-X) RESP2(RESP2-X)"
                | "ROLLBACK RESP(RESP-X) RESP2(RESP2-X)",
            ) => {
                assert_eq!(response.qualified_layout_name, "RESP-X");
                assert_eq!(response2.is_some(), options.contains("RESP2"));
                if let Some(slot) = response2 {
                    assert_eq!(slot.qualified_layout_name, "RESP2-X");
                }
            }
            other => panic!("unexpected frozen condition policy {other:?}"),
        }
    }
    // Frozen command contract, not handler behavior, defines these admission failures.
    for options in ["RESP2(RESP2-X)", "FILE('ACCTDAT')", "BOGUS"] {
        assert!(
            crate::compile(&source(options)).is_err(),
            "accepted {options}"
        );
    }
}

fn invocation(
    case: &Value,
    artifact: &mainframe_env_compiler_api::PublishedArtifact,
    omitted_local: bool,
) -> Invocation {
    let identity = format!(
        "consume-{}-{}",
        case["id"].as_str().unwrap(),
        if omitted_local { "omitted" } else { "explicit" }
    );
    let mut invocation = pilot_invocation(artifact, "IBMUSER", &identity, "SYNCCONS").unwrap();
    if !omitted_local {
        invocation.bindings.insert(
            "cics.execution-context".into(),
            BoundedPayload::new(
                "mainframe-env.cics.execution-context@1",
                case["execution_context"]
                    .as_str()
                    .unwrap()
                    .as_bytes()
                    .to_vec(),
                InvocationLimits::default(),
            )
            .unwrap(),
        );
    } else {
        assert_eq!(case["execution_context"], "local");
    }
    if let Some(remote) = case["remote_outcome"].as_str() {
        invocation.bindings.insert(
            "cics.syncpoint.remote-outcome".into(),
            BoundedPayload::new(
                "mainframe-env.cics.syncpoint.remote-outcome@1",
                remote.as_bytes().to_vec(),
                InvocationLimits::default(),
            )
            .unwrap(),
        );
    }
    invocation
}

// This is the independently specified wire request for the single compiled statement.
// It uses the existing canonical encoder; it never obtains a request from the handler.
fn expected_request(case: &Value, invocation: &Invocation) -> HostRequest {
    let mut arguments = BTreeMap::from([
        (
            "RESP".into(),
            BoundedPayload::new(
                "mainframe-env.cics.argument@1",
                b"RESP-X".to_vec(),
                InvocationLimits::default(),
            )
            .unwrap(),
        ),
        (
            "RESP2".into(),
            BoundedPayload::new(
                "mainframe-env.cics.argument@1",
                b"RESP2-X".to_vec(),
                InvocationLimits::default(),
            )
            .unwrap(),
        ),
    ]);
    if case["requested"] == "rollback" {
        arguments.insert(
            "OPTION.ROLLBACK".into(),
            BoundedPayload::new(
                "mainframe-env.cics.option@1",
                Vec::new(),
                InvocationLimits::default(),
            )
            .unwrap(),
        );
    }
    HostRequest::Cics(CicsRequest {
        operation: CicsOperation::Syncpoint,
        arguments,
        condition_policy: CicsConditionPolicy::Respond {
            response_field: "RESP-X".into(),
            response2_field: Some("RESP2-X".into()),
        },
        mutation: Some(Mutation {
            sequence: 1,
            idempotency_key: effect_key(invocation),
            transaction: Some("DEFAULT".into()),
        }),
    })
}

fn expected_result(case: &Value) -> Result<HostResult, HostProblem> {
    Ok(HostResult::Cics(CicsResponse {
        disposition: CicsDisposition::Complete,
        condition: case["expected"]["condition"].as_str().unwrap().into(),
        response: i32::try_from(case["expected"]["response"].as_i64().unwrap()).unwrap(),
        response2: i32::try_from(case["expected"]["response2"].as_i64().unwrap()).unwrap(),
        applid: "ME01".into(),
        sysid: "S001".into(),
        transaction: "DEFAULT".into(),
        aid: 0,
        target: None,
        next_transaction: None,
        payload: BoundedPayload::new(
            "mainframe-env.cics.payload@1",
            Vec::new(),
            InvocationLimits::default(),
        )
        .unwrap(),
        outputs: BTreeMap::new(),
        // ROLLEDBACK is a condition response: its UOW state is checked independently below.
        unit_of_work: if case["expected"]["condition"] == "NORMAL" {
            Some(if case["expected"]["outcome"] == "committed" {
                CicsUnitOfWorkOutcome::Committed
            } else {
                CicsUnitOfWorkOutcome::RolledBack
            })
        } else {
            None
        },
    }))
}

fn effect_key(invocation: &Invocation) -> IdempotencyKey {
    IdempotencyKey::new(
        format!("{}:1", invocation.idempotency_key.as_str()),
        InvocationLimits::default(),
    )
    .unwrap()
}

fn assert_durable<S: PlatformStore>(store: &S, case: &Value, invocation: &Invocation) {
    let contract = mainframe_env_execution_api::read_transaction_participant_contract(1).unwrap();
    let capabilities = contract
        .participant("cics")
        .unwrap()
        .capabilities
        .as_ref()
        .unwrap();
    let context = capabilities
        .contexts
        .iter()
        .find(|c| c.context_id == case["execution_context"].as_str().unwrap())
        .unwrap();
    assert_eq!(
        context.mode,
        match case["mode"].as_str().unwrap() {
            "local" => ParticipantMode::Local,
            "distributed-owned" => ParticipantMode::DistributedOwned,
            "distributed-subordinate" => ParticipantMode::DistributedSubordinate,
            other => panic!("unknown frozen mode {other}"),
        }
    );
    assert_eq!(
        context.syncpoint_owner,
        if case["syncpoint_owner"] == "participant" {
            SyncpointOwner::Participant
        } else {
            SyncpointOwner::UpstreamHost
        }
    );
    assert_eq!(
        context.explicit_syncpoint == ExplicitSyncpoint::Supported,
        case["expected"]["uow_state"] != "absent"
    );
    assert_eq!(capabilities.schemas.uow_write, "MECU2");
    assert_eq!(capabilities.schemas.uow_frame_write, Some("MECU3"));
    assert_eq!(capabilities.schemas.uow_read, ["MECU1", "MECU2", "MECU3"]);
    let key = effect_key(invocation);
    let effect = store.effect(&key).unwrap().unwrap();
    assert_eq!(effect.state, EffectState::Completed);
    assert_eq!(effect.digest_format, EffectDigestFormat::CanonicalHostV1);
    assert_eq!(effect.key, key);
    assert_eq!(effect.execution_id, invocation.execution_id);
    assert_eq!(effect.run_unit_id, invocation.run_unit_id);
    assert_eq!(effect.sequence, 1);
    assert_eq!(
        effect.request_digest,
        canonical_request_digest(&expected_request(case, invocation)).unwrap()
    );
    assert_eq!(
        effect.result_digest,
        Some(canonical_result_digest(&expected_result(case)).unwrap())
    );
    assert!(effect.resolved_tick.is_some_and(|tick| tick > 0));
    assert_eq!(
        effect.intent.capability.as_ref().unwrap().as_str(),
        "host.cics.execute"
    );
    let uow = store
        .get_provider_state(capabilities.schemas.uow_namespace, key.as_str())
        .unwrap();
    if case["expected"]["uow_state"] == "absent" {
        assert!(uow.is_none());
        assert!(
            store
                .list_provider_state(capabilities.schemas.uow_namespace, 16)
                .unwrap()
                .is_empty()
        );
    } else {
        let row = uow.unwrap();
        assert_eq!(&row.payload[..5], capabilities.schemas.uow_write.as_bytes());
        let descriptor = describe_cics_uow_row(&row, None).unwrap();
        assert_eq!(descriptor.codec, CicsUowCodecVersion::RetentionV2);
        assert_eq!(descriptor.row_version, 2);
        assert_eq!(
            descriptor.state,
            if case["expected"]["uow_state"] == "committed" {
                CicsUowState::Committed
            } else {
                CicsUowState::RolledBack
            }
        );
        assert_eq!(descriptor.effect_key.as_deref(), Some(key.as_str()));
        assert_eq!(
            descriptor.owner_execution.as_deref(),
            Some(invocation.execution_id.as_str())
        );
        assert_eq!(descriptor.task_owner_execution, None);
        assert_eq!(
            descriptor.owner_run_unit.as_deref(),
            Some(invocation.run_unit_id.as_str())
        );
        assert_eq!(descriptor.transaction, "DEFAULT");
        assert!(descriptor.deadline_tick.is_some_and(|tick| tick > 0));
        // The clockless pilot has no terminal-age authority and earns no retention/restart seal.
        assert_eq!(descriptor.terminal_tick, None);
        assert_eq!(
            descriptor.dependency,
            CicsUowDependencyState::TerminalObservationRequired
        );
    }
    assert!(
        store
            .list_provider_state(capabilities.schemas.undo_namespace, 16)
            .unwrap()
            .is_empty()
    );
    let replay = store
        .get_provider_state(capabilities.schemas.replay_namespace, key.as_str())
        .unwrap()
        .unwrap();
    assert!(
        replay
            .payload
            .starts_with(capabilities.schemas.replay_write.as_bytes())
    );
    let execution = store
        .get_execution(&invocation.execution_id)
        .unwrap()
        .unwrap();
    assert_eq!(execution.execution_id, invocation.execution_id);
    assert_eq!(execution.run_unit_id, invocation.run_unit_id);
    assert_eq!(execution.artifact, invocation.artifact);
    assert_eq!(execution.principal, *invocation.principal.id());
    assert_eq!(execution.state, ExecutionState::Completed);
    let events = store.events(&invocation.execution_id, 1, 32).unwrap();
    assert_eq!(
        events.iter().map(|e| e.kind.clone()).collect::<Vec<_>>(),
        [
            LifecycleEventKind::Admitted,
            LifecycleEventKind::Queued,
            LifecycleEventKind::Started,
            LifecycleEventKind::EffectIntent { sequence: 1 },
            LifecycleEventKind::EffectResult { sequence: 1 },
            LifecycleEventKind::Completing,
            LifecycleEventKind::Completed { return_code: 0 },
        ]
    );
    let audit = store
        .audit_records(&invocation.execution_id, 1, 32)
        .unwrap()
        .into_iter()
        .filter(|a| a.capability.as_str() == "host.cics.execute")
        .collect::<Vec<_>>();
    assert_eq!(audit.len(), 1);
    let audit = &audit[0];
    assert_eq!(audit.principal, invocation.principal.id().clone());
    assert_eq!(audit.execution_id, invocation.execution_id);
    assert_eq!(audit.run_unit_id, invocation.run_unit_id);
    assert_eq!(audit.invocation_key, invocation.idempotency_key);
    assert_eq!(audit.effect_sequence, 1);
    assert_eq!(audit.attempt, 1);
    assert_eq!(audit.decision, AuditDecision::Success);
    assert_eq!(
        audit.resource,
        canonical_audit_resource_digest(&expected_request(case, invocation))
    );
    let security = store
        .audit_records(&invocation.execution_id, 1, 32)
        .unwrap()
        .into_iter()
        .filter(|a| a.capability.as_str() == "host.security.authorize")
        .collect::<Vec<_>>();
    assert_eq!(security.len(), 1);
    assert_eq!(security[0].decision, AuditDecision::Success);
    assert_eq!(security[0].principal, *invocation.principal.id());
    assert_eq!(
        security[0].resource,
        canonical_audit_resource_digest(&HostRequest::Security(SecurityRequest::Authorize {
            principal: invocation.principal.id().clone(),
            class: "TCICSTRN".into(),
            resource: ResourceName::new("CICS.DEFAULT", 246).unwrap(),
            intent: AccessIntent::Execute,
        }))
    );
}

fn run_case<S: PlatformStore + 'static>(store: Arc<S>, case: &Value, omitted: bool) -> Invocation {
    let rollback = case["requested"] == "rollback";
    let options = format!(
        "{}RESP(RESP-X) RESP2(RESP2-X)",
        if rollback { "ROLLBACK " } else { "" }
    );
    let artifact = crate::compile(&source(&options)).unwrap();
    bind_artifact(
        &artifact,
        rollback,
        &[CicsOutputName::Resp, CicsOutputName::Resp2],
    );
    let invocation = invocation(case, &artifact, omitted);
    let dataset = DatasetService::open(store.clone(), DatasetLimits::default()).unwrap();
    seed_dataset(&dataset, invocation.idempotency_key.as_str()).unwrap();
    let cics = CicsService::open(
        pilot_inner_host(dataset.clone()).unwrap(),
        store.clone(),
        CicsLimits::default(),
    )
    .unwrap();
    let session = SessionId::new(format!("session-{}", invocation.run_unit_id), 1024).unwrap();
    cics.create_session(&session, 24, 80).unwrap();
    cics.register_run(invocation.clone(), &session, "DEFAULT", "ME01", "S001")
        .unwrap();
    // Snapshot every CICS business row after provider open. Only the documented outer
    // replay receipt may be added for a rejected command, not a UOW/resource mutation.
    let before = store.list_provider_state_prefix("cics-", 1024).unwrap();
    let execution = PilotExecution::new(pilot_outer_host(cics).unwrap(), store.clone());
    let output = drive_artifact(&artifact, invocation.clone(), &execution).unwrap();
    let expected = &case["expected"];
    assert_eq!(
        output,
        format!(
            "CONSUMED:{:03}:{:03}\nEIBFN:\u{16}\u{2}\n",
            expected["response"].as_u64().unwrap(),
            expected["response2"].as_u64().unwrap()
        )
    );
    assert_durable(store.as_ref(), case, &invocation);
    assert_eq!(
        read_record_hex(&dataset).unwrap(),
        hex(&encode("AA11").unwrap())
    );
    if expected["uow_state"] == "absent" {
        let after = store
            .list_provider_state_prefix("cics-", 1024)
            .unwrap()
            .into_iter()
            .filter(|r| r.namespace != "cics-effect-replay-v1")
            .collect::<Vec<_>>();
        assert_eq!(
            after, before,
            "subordinate rejection changed provider business state"
        );
    }
    invocation
}

#[test]
fn syncpoint_contract_consumption_memory_and_physical_sqlite_reopen() {
    let fixtures: Value = serde_json::from_str(PARTICIPANT_FIXTURES).unwrap();
    let cases = fixtures["cics_cases"].as_array().unwrap();
    assert_eq!(cases.len(), 6);
    let directory = std::env::temp_dir().join(format!(
        "mainframe-env-sync-consumption-{}-{}",
        std::process::id(),
        PROFILE_NONCE.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&directory).unwrap();
    for case in cases {
        for omitted in if case["execution_context"] == "local" {
            vec![false, true]
        } else {
            vec![false]
        } {
            run_case(
                Arc::new(MemoryStore::new(StoreLimits::default())),
                case,
                omitted,
            );
            let url = format!(
                "sqlite://{}?mode=rwc",
                directory
                    .join(format!("{}-{omitted}.db", case["id"].as_str().unwrap()))
                    .display()
            );
            let store = Arc::new(SqliteStateStore::open(&url, 4 * 1024 * 1024, 65_536).unwrap());
            let invocation = run_case(store.clone(), case, omitted);
            let key = effect_key(&invocation);
            let effect = store.effect(&key).unwrap();
            let rows = store.list_provider_state_prefix("cics-", 1024).unwrap();
            let events = store.events(&invocation.execution_id, 1, 32).unwrap();
            let audit = store
                .audit_records(&invocation.execution_id, 1, 32)
                .unwrap();
            let execution = store.get_execution(&invocation.execution_id).unwrap();
            // No provider/coordinator Arc survives: close the physical SQLite authority.
            assert_eq!(Arc::strong_count(&store), 1);
            drop(store);
            let reopened = Arc::new(SqliteStateStore::open(&url, 4 * 1024 * 1024, 65_536).unwrap());
            assert_durable(reopened.as_ref(), case, &invocation);
            assert_eq!(reopened.effect(&key).unwrap(), effect);
            assert_eq!(
                reopened.list_provider_state_prefix("cics-", 1024).unwrap(),
                rows
            );
            assert_eq!(
                reopened.events(&invocation.execution_id, 1, 32).unwrap(),
                events
            );
            assert_eq!(
                reopened
                    .audit_records(&invocation.execution_id, 1, 32)
                    .unwrap(),
                audit
            );
            assert_eq!(
                reopened.get_execution(&invocation.execution_id).unwrap(),
                execution
            );
            let dataset = DatasetService::open(reopened.clone(), DatasetLimits::default()).unwrap();
            assert_eq!(
                read_record_hex(&dataset).unwrap(),
                hex(&encode("AA11").unwrap())
            );
            let cics = CicsService::open(
                pilot_inner_host(dataset).unwrap(),
                reopened,
                CicsLimits::default(),
            )
            .unwrap();
            assert_eq!(
                cics.reconciled_effect_result_digest(
                    &key,
                    canonical_request_digest(&expected_request(case, &invocation)).unwrap()
                )
                .unwrap(),
                effect.unwrap().result_digest.unwrap()
            );
        }
    }
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn syncpoint_contract_consumption_reopens_committed_and_backed_out_record() {
    let fixtures: Value = serde_json::from_str(FIXTURES).unwrap();
    let directory = std::env::temp_dir().join(format!(
        "mainframe-env-sync-consumption-record-{}-{}",
        std::process::id(),
        PROFILE_NONCE.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&directory).unwrap();
    let url = format!("sqlite://{}?mode=rwc", directory.join("state.db").display());
    for (index, (source, program, label, fixture, state)) in [
        (
            COMMIT_SOURCE,
            "CICSPILOT",
            "COMMIT",
            "syncpoint_commit",
            CicsUowState::Committed,
        ),
        (
            ROLLBACK_SOURCE,
            "CICSROLL",
            "ROLLBACK",
            "syncpoint_rollback",
            CicsUowState::RolledBack,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let store = Arc::new(SqliteStateStore::open(&url, 4 * 1024 * 1024, 65_536).unwrap());
        let dataset = DatasetService::open(store.clone(), DatasetLimits::default()).unwrap();
        if index == 0 {
            seed_dataset(&dataset, "consume-record").unwrap();
        }
        let cics = CicsService::open(
            pilot_inner_host(dataset.clone()).unwrap(),
            store.clone(),
            CicsLimits::default(),
        )
        .unwrap();
        cics.register_file_definitions(&BTreeMap::from([(
            "ACCTDAT".into(),
            CicsFileDefinition {
                dataset: dataset_name().unwrap(),
                ccsid: Some(37),
            },
        )]))
        .unwrap();
        let artifact = crate::compile(source).unwrap();
        bind_artifact(
            &artifact,
            index == 1,
            &[CicsOutputName::Resp, CicsOutputName::Resp2],
        );
        let invocation = pilot_invocation(
            &artifact,
            "IBMUSER",
            &format!("consume-record-{index}"),
            program,
        )
        .unwrap();
        let execution = PilotExecution::new(pilot_outer_host(cics).unwrap(), store.clone());
        let output = drive_artifact(&artifact, invocation.clone(), &execution).unwrap();
        let expected = &fixtures["expected"][fixture];
        assert_eq!(
            parse_command(&output, label, false).unwrap(),
            expected_command(&fixtures["expected"], fixture).unwrap()
        );
        if index == 1 {
            assert_eq!(
                parse_command(&output, "PREBACKOUT", true).unwrap(),
                expected_command(&fixtures["expected"], "read_before_rollback").unwrap()
            );
            assert_eq!(
                parse_command(&output, "FINAL", true).unwrap(),
                expected_command(&fixtures["expected"], "final_read").unwrap()
            );
        }
        let record = hex(&encode(expected["record_after"].as_str().unwrap()).unwrap());
        assert_eq!(read_record_hex(&dataset).unwrap(), record);
        // In both existing golden programs SYNCPOINT is the fourth typed host effect.
        let key = IdempotencyKey::new(
            format!("{}:4", invocation.idempotency_key),
            InvocationLimits::default(),
        )
        .unwrap();
        let effect = store.effect(&key).unwrap().unwrap();
        assert_eq!(effect.digest_format, EffectDigestFormat::CanonicalHostV1);
        assert_eq!(effect.state, EffectState::Completed);
        assert_eq!(effect.execution_id, invocation.execution_id);
        assert_eq!(effect.run_unit_id, invocation.run_unit_id);
        let uow = store
            .get_provider_state("cics-uow", key.as_str())
            .unwrap()
            .unwrap();
        let descriptor = describe_cics_uow_row(&uow, None).unwrap();
        assert_eq!(descriptor.state, state);
        assert_eq!(descriptor.codec, CicsUowCodecVersion::RetentionV2);
        assert_eq!(
            descriptor.owner_execution.as_deref(),
            Some(invocation.execution_id.as_str())
        );
        assert_eq!(
            descriptor.owner_run_unit.as_deref(),
            Some(invocation.run_unit_id.as_str())
        );
        assert!(
            store
                .list_provider_state("cics-uow-undo", 16)
                .unwrap()
                .is_empty()
        );
        let events = store.events(&invocation.execution_id, 1, 64).unwrap();
        let audit = store
            .audit_records(&invocation.execution_id, 1, 64)
            .unwrap();
        drop(execution);
        drop(dataset);
        assert_eq!(Arc::strong_count(&store), 1);
        drop(store);
        let reopened = Arc::new(SqliteStateStore::open(&url, 4 * 1024 * 1024, 65_536).unwrap());
        assert_eq!(reopened.effect(&key).unwrap().unwrap(), effect);
        assert_eq!(
            reopened
                .get_provider_state("cics-uow", key.as_str())
                .unwrap()
                .unwrap(),
            uow
        );
        assert_eq!(
            reopened.events(&invocation.execution_id, 1, 64).unwrap(),
            events
        );
        assert_eq!(
            reopened
                .audit_records(&invocation.execution_id, 1, 64)
                .unwrap(),
            audit
        );
        let dataset = DatasetService::open(reopened, DatasetLimits::default()).unwrap();
        assert_eq!(read_record_hex(&dataset).unwrap(), record);
    }
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn syncpoint_contract_consumption_rejects_duplicate_rollback() {
    // The frozen admission contract rejects a repeated rollback request.
    assert_eq!(row_contract()["options"]["duplicate_option"], "reject");
    assert!(
        crate::compile(&source("ROLLBACK ROLLBACK")).is_err(),
        "row 0218 frozen options.duplicate_option=reject: accepted duplicate ROLLBACK"
    );
}

mod ledger;
