//! Explicit local owner integration: real initial Complete CAS, compiled source, ordinary JES.
//! Every native/IBM/CardDemo automatic-trigger claim remains outside these controls.
use super::*;
use mainframe_env_application::{
    APPLICATION_PUBLICATION_CONTRACT, APPLICATION_PUBLICATION_NAMESPACE,
    ApplicationGenerationRecord, ApplicationPublicationState, PublicationAction,
    PublicationSectionState,
};
use mainframe_env_batch::{
    JclBundle, JobSnapshot, JobState, StepExecution, StepState, StepTermination,
};
use mainframe_env_store_api::{CheckpointRecord, WorkRecord};

#[path = "local_mq_occurrence_fault.rs"]
mod occurrence_fault;
use occurrence_fault::{OccurrenceFaultStore, Outcome as OccurrenceOutcome};

const OCCURRENCES: &str = "application-local-mq-occurrence";
const BINDINGS: &str = "batch-local-controller-submission";
const OCCURRENCE_SCHEMA: &str = "mainframe-env.local-mq-occurrence@1";
const WORKER: &str = "compiled-mq-local-owner";

struct CompleteProducer {
    producer: InstalledProducer,
    generation: ApplicationGenerationRecord,
    publication: ProviderStateRecord,
    controller_identity: String,
    stable: Vec<ProviderStateRecord>,
}

fn bind_launch_fixture_jes(
    server: &ProductServer,
    package: &mut ApplicationPackageV2,
    definition: &mut LocalApplicationDefinition,
) -> Vec<String> {
    // An isolated first submission owns JOB00001; no job or counter is manufactured.
    assert!(
        server
            .store
            .list_provider_state("jes-job", 1)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        server
            .store
            .get_provider_state("jes-meta", "next-id")
            .unwrap(),
        None
    );
    let principal = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();
    assert!(
        server
            .racf
            .active_principal_epoch(&principal)
            .unwrap()
            .is_some()
    );
    server.bootstrap_identity("JESOTHER", b"TESTPASS").unwrap();
    let mut declarations = vec![("JOB.MQLOCAL".to_string(), AccessLevel::Execute)];
    let mut spool_resources = Vec::new();
    // Mandatory submit records plus the ordinary successful RUN step's bookkeeping.
    for file in ["JESJCL", "JESMSGLG", "JOBLOG", "RUN:JOBLOG"] {
        let mut digest = Sha256::new();
        digest.update(b"mainframe-env.jes-spool-id@1");
        digest.update(b"JOB00001");
        digest.update(file.as_bytes());
        let encoded = format!("{:x}", digest.finalize());
        // The persisted profile and SAF resource normalization require canonical uppercase.
        let resource = format!("JOB.MQLOCAL.SP-{}", &encoded[..60]).to_ascii_uppercase();
        declarations.push((resource.clone(), AccessLevel::Update));
        spool_resources.push(resource);
    }
    for (name, access) in declarations {
        let profile = RacfProfileComposition {
            class: "JESJOBS".into(),
            name,
            owner: "IBMUSER".into(),
            uacc: AccessLevel::None,
            permissions: BTreeMap::from([("IBMUSER".into(), access)]),
        };
        // Hyphens encode actual discrete spool IDs; whitespace and lowercase stay malformed.
        let mut invalid = profile.clone();
        invalid.name.push(' ');
        assert_eq!(invalid.canonical_payload(), Err(HostProblem::Malformed));
        invalid.name = profile.name.to_ascii_lowercase();
        assert_eq!(invalid.canonical_payload(), Err(HostProblem::Malformed));
        package.sections.security_resources.push(SecurityResource {
            class: profile.class.clone(),
            profile: profile.name.clone(),
            owner: profile.owner.clone(),
        });
        let raw = profile.canonical_payload().unwrap();
        let sha = format!("sha256:{:x}", Sha256::digest(&raw));
        package.base.blobs.insert(sha.clone(), raw.clone());
        package.base.manifest.entries.push(PackageEntry {
            path: profile.entry_path().unwrap(),
            kind: EntryKind::Profile,
            sha256: sha,
            bytes: raw.len(),
            depends_on: Vec::new(),
        });
        definition.security_profiles.push(profile);
    }
    spool_resources
}

fn assert_launch_fixture_jes(server: &ProductServer, spool_resources: &[String]) {
    for (user, resource, intent, expected) in [
        (
            "IBMUSER",
            "JOB.MQLOCAL",
            AccessIntent::Execute,
            SecurityDecision::Allow,
        ),
        (
            "JESOTHER",
            "JOB.MQLOCAL",
            AccessIntent::Execute,
            SecurityDecision::Deny,
        ),
        (
            "IBMUSER",
            "JOB.OTHER",
            AccessIntent::Execute,
            SecurityDecision::Deny,
        ),
        (
            "IBMUSER",
            "JOB.MQLOCAL",
            AccessIntent::Update,
            SecurityDecision::Deny,
        ),
    ] {
        assert_eq!(
            server
                .racf
                .authorize(
                    &PrincipalId::new(user, InvocationLimits::default()).unwrap(),
                    "JESJOBS",
                    &ResourceName::new(resource, 246).unwrap(),
                    intent,
                )
                .unwrap(),
            expected,
        );
    }
    assert_eq!(spool_resources.len(), 4);
    for resource in spool_resources {
        assert!(!resource.bytes().any(|byte| matches!(byte, b'*' | b'%')));
        for (user, intent, expected) in [
            ("IBMUSER", AccessIntent::Update, SecurityDecision::Allow),
            ("JESOTHER", AccessIntent::Update, SecurityDecision::Deny),
            ("IBMUSER", AccessIntent::Control, SecurityDecision::Deny),
        ] {
            assert_eq!(
                server
                    .racf
                    .authorize(
                        &PrincipalId::new(user, InvocationLimits::default()).unwrap(),
                        "JESJOBS",
                        &ResourceName::new(resource, 246).unwrap(),
                        intent,
                    )
                    .unwrap(),
                expected,
            );
        }
        let other_job = resource.replacen("JOB.MQLOCAL.", "JOB.OTHER.", 1);
        assert_eq!(
            server
                .racf
                .authorize(
                    &PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap(),
                    "JESJOBS",
                    &ResourceName::new(other_job, 246).unwrap(),
                    AccessIntent::Update,
                )
                .unwrap(),
            SecurityDecision::Deny,
        );
    }
}

fn launch_fixture_program_and_clock() -> (Arc<DefaultProgramRouter>, Arc<ManualJesClock>) {
    let clock = Arc::new(ManualJesClock::new(OFFERED_TICK));
    let program = default_program_router();
    let execution_clock = clock.clone();
    program
        .bind_execution_control(Arc::new(move |invocation: &Invocation| {
            Ok(ExecutionControl {
                now_tick: execution_clock
                    .now_tick()
                    .map_err(|_| ExecutionControlError::Unavailable)?,
                cancellation_requested: invocation.cancellation_requested(),
            })
        }))
        .unwrap();
    (program, clock)
}

fn complete_fixture(store: Arc<dyn PlatformStore>) -> CompleteProducer {
    let trust = Arc::new(test_package_trust());
    let mut settings = config();
    settings.store_profile = crate::StoreProfile::Postgres;
    settings.artifact_profile = ArtifactProfile::Shared;
    settings.postgres_url_reference = Some("env-base64:MAINFRAME_ENV_SECRET_POSTGRES_URL".into());
    settings.validate().unwrap();
    let artifacts: Arc<dyn ArtifactStore> = store.clone();
    let (program, clock) = launch_fixture_program_and_clock();
    let server = ProductServer::open_configured(
        settings,
        store.clone(),
        Arc::new(MemorySecretResolver::default()),
        program,
        trust.clone(),
        Arc::new(ProductArtifactStore::Shared(artifacts)),
        Some(clock),
    )
    .unwrap();
    let (mut package, mut definition, artifact) = compiled_definitions(&server, &trust);
    let spool_resources = bind_launch_fixture_jes(&server, &mut package, &mut definition);
    let marker = b"mainframe-env.application-local-complete-initial@1";
    let digest = format!("sha256:{:x}", Sha256::digest(marker));
    package.base.blobs.insert(digest.clone(), marker.to_vec());
    package.base.manifest.entries.push(PackageEntry {
        path: "resource/local-complete-publication".into(),
        kind: EntryKind::Resource,
        sha256: digest,
        bytes: marker.len(),
        depends_on: vec!["program/LOCALPGM".into()],
    });
    resign_package(&mut package, &trust);
    let controller_identity = package
        .base
        .manifest
        .entries
        .iter()
        .find(|entry| entry.path == "program/LOCALPGM" && entry.kind == EntryKind::Program)
        .unwrap()
        .sha256
        .clone();
    let staged = server.install_application_package_v2(&package).unwrap();
    // The actual six-owner initial publication port mints and acknowledges this row.
    // No control writes a Complete/Applied provider row by hand.
    let installed = server
        .publish_complete_local_application(&staged, definition)
        .unwrap();
    let generation = installed.generation;
    let publication = store
        .get_provider_state(APPLICATION_PUBLICATION_NAMESPACE, "LOCAL-COMPOSE")
        .unwrap()
        .unwrap();
    let expected = ApplicationPublicationState {
        schema_version: APPLICATION_PUBLICATION_CONTRACT.into(),
        package: "LOCAL-COMPOSE".into(),
        generation: 1,
        identity: generation.identity.clone(),
        action: PublicationAction::Install,
        controllers: PublicationSectionState::Applied,
        db2: PublicationSectionState::NotApplicable,
        ims: PublicationSectionState::NotApplicable,
        complete: true,
    };
    assert_eq!(
        publication,
        ProviderStateRecord {
            namespace: APPLICATION_PUBLICATION_NAMESPACE.into(),
            key: "LOCAL-COMPOSE".into(),
            version: 1,
            payload: serde_json::to_vec(&expected).unwrap()
        }
    );
    assert_eq!(
        server
            .selected_complete_local_application("LOCAL-COMPOSE")
            .unwrap(),
        Some(generation.clone())
    );
    assert_eq!(
        server.selected_local_application("LOCAL-COMPOSE"),
        Err(HostProblem::IdempotencyConflict)
    );
    assert_launch_fixture_jes(&server, &spool_resources);
    let mut source = invocation("local-compiled-source");
    source.selector = Selector::new("program:LOCALPUT", InvocationLimits::default()).unwrap();
    source.artifact = artifact;
    let installed = server
        .artifacts
        .get_artifact(&source.artifact)
        .unwrap()
        .unwrap();
    let executable = crate::cobol::artifact::admit_executable_artifact(&installed).unwrap();
    assert_eq!(
        executable.content_id().to_reference(),
        source.artifact.as_str()
    );
    assert_eq!(
        server.online_programs.lock().unwrap().get("LOCALPUT"),
        Some(&source.artifact)
    );
    let request = MqRequest {
        operation: MqOperation::PutOne,
        queue: Some("LOCAL.QUEUE".into()),
        handle: None,
        options: 0,
        message: DATA.to_vec(),
        message_id: Some(MESSAGE_ID.to_vec()),
        correlation_id: Some(CORRELATION_ID.to_vec()),
        wait_ticks: 0,
        max_message_bytes: 20,
        mutation: Some(Mutation {
            sequence: 1,
            transaction: None,
            idempotency_key: IdempotencyKey::new(
                "invocation-local-compiled-source:1",
                InvocationLimits::default(),
            )
            .unwrap(),
        }),
    };
    let before_manifest = store
        .get_provider_state("mq-state", "queues")
        .unwrap()
        .unwrap();
    let before_queue = store
        .get_provider_state("mq-v1-queue", "LOCAL.QUEUE")
        .unwrap()
        .unwrap();
    let stable = stable_rows(store.as_ref(), source_key(&request).as_str());
    let producer = InstalledProducer {
        server,
        source,
        request,
        before_manifest,
        before_queue,
        stable,
    };
    // Full independent message/ID, core, lifecycle, audit and replay oracle is the preserved source control.
    producer.known();
    let stable = unchanged_catalog_rows(store.as_ref());
    CompleteProducer {
        producer,
        generation,
        publication,
        controller_identity,
        stable,
    }
}

fn unchanged_catalog_rows(store: &dyn ProviderStateStore) -> Vec<ProviderStateRecord> {
    launch_rows(store)
        .into_iter()
        .filter(|row| {
            !matches!(
                row.namespace.as_str(),
                "jes-job" | "jes-meta" | OCCURRENCES | BINDINGS
            ) && !matches!(
                (row.namespace.as_str(), row.key.as_str()),
                ("mq-state", "queues")
                    | ("mq-v1-queue", "LOCAL.QUEUE")
                    | ("mq-v1-replay", "invocation-local-compiled-source:1")
            )
        })
        .collect()
}

fn key(fixture: &CompleteProducer) -> IdempotencyKey {
    let mut hash = Sha256::new();
    for raw in [
        OCCURRENCE_SCHEMA.as_bytes(),
        b"LOCAL-COMPOSE".as_slice(),
        source_key(&fixture.producer.request).as_str().as_bytes(),
    ] {
        hash.update((raw.len() as u64).to_be_bytes());
        hash.update(raw);
    }
    IdempotencyKey::new(
        format!("local-mq:{:x}", hash.finalize()),
        InvocationLimits::default(),
    )
    .unwrap()
}

fn plan() -> mainframe_env_batch::JobPlan {
    let bundle = JclBundle {
        primary: std::str::from_utf8(LAUNCH_JCL).unwrap().into(),
        ..JclBundle::default()
    };
    let plan = mainframe_env_batch::parse_jcl(&bundle, Default::default()).unwrap();
    assert_eq!(plan.name, "MQLOCAL");
    assert_eq!(plan.class, 'A');
    assert_eq!(plan.steps.len(), 1);
    assert_eq!(plan.steps[0].name, "RUN");
    assert_eq!(plan.steps[0].program, "IKJEFT01");
    assert_eq!(
        plan.steps[0].condition,
        mainframe_env_batch::StepCondition::Always
    );
    plan
}

fn plan_hash() -> [u8; 32] {
    Sha256::digest(serde_json::to_vec(&plan()).unwrap()).into()
}

fn expected_occurrence(
    fixture: &CompleteProducer,
    phase: &str,
    job: Option<&str>,
    completed: Option<&ProviderStateRecord>,
) -> serde_json::Value {
    let producer = &fixture.producer;
    let mut value = serde_json::json!({"schema_version":OCCURRENCE_SCHEMA,
        "application":"LOCAL-COMPOSE","generation":1,"identity":fixture.generation.identity,
        "queue":"LOCAL.QUEUE","process":"LOCAL.PROCESS","controller":"LOCAL-CONTROLLER",
        "source_key":source_key(&producer.request).as_str(),"source_execution":producer.source.execution_id.as_str(),
        "source_run":producer.source.run_unit_id.as_str(),"source_principal":"IBMUSER",
        "source_selector":"program:LOCALPUT","source_artifact":producer.source.artifact.as_str(),
        "source_attempt":1,"source_sequence":1,"request_digest":canonical_mq_request_digest(&producer.request).unwrap(),
        "result_digest":canonical_result_digest(&Ok(HostResult::Mq(expected_result()))).unwrap(),
        "source_resolved_tick":OFFERED_TICK,"plan_sha256":plan_hash(),"phase":{"phase":phase}});
    if let Some(job) = job {
        value["phase"]["job"] = serde_json::json!(job);
    }
    if let Some(row) = completed {
        value["phase"]["row_version"] = serde_json::json!(row.version);
        let sha: [u8; 32] = Sha256::digest(&row.payload).into();
        value["phase"]["row_sha256"] = serde_json::json!(sha);
    }
    value
}

fn occurrence(
    fixture: &CompleteProducer,
    version: u64,
    expected: serde_json::Value,
) -> ProviderStateRecord {
    let row = fixture
        .producer
        .server
        .store
        .get_provider_state(OCCURRENCES, key(fixture).as_str())
        .unwrap()
        .unwrap();
    assert_eq!(row.namespace, OCCURRENCES);
    assert_eq!(row.key, key(fixture).as_str());
    assert_eq!(row.version, version);
    assert_eq!(json(&row), expected);
    row
}

fn expected_binding(fixture: &CompleteProducer) -> serde_json::Value {
    serde_json::json!({"schema_version":"mainframe-env.local-controller-submission@1",
        "application":"LOCAL-COMPOSE","generation":1,"identity":fixture.generation.identity,
        "controller":"LOCAL-CONTROLLER","program_identity":fixture.controller_identity,
        "program":"LOCALPGM","owner":"IBMUSER","plan_sha256":plan_hash()})
}

fn assert_source_and_catalog(fixture: &CompleteProducer) {
    let p = &fixture.producer;
    assert_eq!(
        p.server.store.effect(source_key(&p.request)).unwrap(),
        Some(expected_effect(&p.source, &p.request))
    );
    assert_eq!(
        p.server.mq.queue_messages("LOCAL.QUEUE"),
        Ok(vec![DATA.to_vec()])
    );
    assert_eq!(
        json(
            &p.server
                .store
                .get_provider_state("mq-v1-queue", "LOCAL.QUEUE")
                .unwrap()
                .unwrap()
        ),
        expected_queue()
    );
    assert_eq!(
        json(
            &p.server
                .store
                .get_provider_state("mq-v1-replay", source_key(&p.request).as_str())
                .unwrap()
                .unwrap()
        ),
        expected_replay(&p.source, &p.request, p.source.deadline_tick)
    );
    assert_eq!(
        unchanged_catalog_rows(p.server.store.as_ref()),
        fixture.stable
    );
    assert_eq!(
        p.server
            .store
            .get_provider_state(APPLICATION_PUBLICATION_NAMESPACE, "LOCAL-COMPOSE")
            .unwrap(),
        Some(fixture.publication.clone())
    );
    assert_eq!(
        p.server
            .selected_complete_local_application("LOCAL-COMPOSE")
            .unwrap(),
        Some(fixture.generation.clone())
    );
}

fn start(fixture: &CompleteProducer) -> (String, WorkRecord) {
    let p = &fixture.producer;
    let source_rows = [
        ("mq-state", "queues"),
        ("mq-v1-queue", "LOCAL.QUEUE"),
        ("mq-v1-replay", source_key(&p.request).as_str()),
    ]
    .map(|(ns, key)| p.server.store.get_provider_state(ns, key).unwrap().unwrap());
    let job = p
        .server
        .start_committed_local_mq_controller("LOCAL-COMPOSE", &p.source, &p.request)
        .unwrap();
    occurrence(
        fixture,
        2,
        expected_occurrence(fixture, "Submitted", Some(&job), None),
    );
    let binding = p
        .server
        .store
        .get_provider_state(BINDINGS, key(fixture).as_str())
        .unwrap()
        .unwrap();
    assert_eq!(binding.version, 1);
    assert_eq!(json(&binding), expected_binding(fixture));
    for old in source_rows {
        assert_eq!(
            p.server
                .store
                .get_provider_state(&old.namespace, &old.key)
                .unwrap(),
            Some(ProviderStateRecord {
                version: old.version + 1,
                ..old
            })
        );
    }
    let queued = p.server.batch.get(&job).unwrap();
    assert_eq!(queued.state, JobState::Queued);
    assert_eq!(queued.attempt, 0);
    assert_eq!(
        queued.steps,
        vec![StepExecution {
            name: "RUN".into(),
            state: StepState::Pending,
            attempt: 0,
            termination: None
        }]
    );
    let work = p
        .server
        .store
        .get_work(&format!("jes:{job}"))
        .unwrap()
        .unwrap();
    let capabilities = BTreeSet::from(
        [
            "host.db2.read",
            "host.db2.write",
            "host.program.invoke",
            "host.security.authorize",
            "host.spool.read",
            "host.spool.write",
        ]
        .map(str::to_string),
    );
    let expected = WorkRecord {
        work_id: format!("jes:{job}"),
        execution_id: work.execution_id.clone(),
        required_selector: Selector::new("zosmf:job-submit", InvocationLimits::default()).unwrap(),
        required_generation: "mainframe-env-batch@1".into(),
        artifact: ArtifactRef::new("artifact:none", InvocationLimits::default()).unwrap(),
        state: WorkState::Queued,
        priority: plan().priority,
        attempt: 0,
        max_attempts: 3,
        available_tick: OFFERED_TICK,
        deadline_tick: OFFERED_TICK + 86_400_000,
        cancellation_requested: false,
        worker_id: None,
        lease_id: None,
        lease_epoch: 0,
        lease_expiry_tick: None,
        heartbeat_tick: None,
        terminal_tick: None,
        checkpoint_id: None,
        effect_sequence: 0,
        payload: crate::jes_worker::JesWorkPayload::new(&job, "IBMUSER", capabilities)
            .unwrap()
            .encode()
            .unwrap(),
    };
    assert_eq!(work, expected);
    assert_source_and_catalog(fixture);
    (job, work)
}

fn assert_real_retirement(
    fixture: &CompleteProducer,
    job: &str,
    queued: &WorkRecord,
) -> (
    JobSnapshot,
    ProviderStateRecord,
    CheckpointRecord,
    WorkRecord,
) {
    let p = &fixture.producer;
    let actual = p.server.batch.get(job).unwrap();
    assert_eq!(actual.id, job);
    assert_eq!(actual.name, "MQLOCAL");
    assert_eq!(actual.owner, "IBMUSER");
    assert_eq!(actual.class, 'A');
    assert_eq!(actual.priority, plan().priority);
    assert_eq!(
        actual.state,
        JobState::Completed,
        "JES job did not complete: {actual:#?}"
    );
    assert_eq!(actual.return_code, Some(0));
    assert_eq!(actual.abend_code, None);
    assert_eq!(actual.active_step, None);
    assert_eq!(actual.initiator, None);
    assert_eq!(actual.attempt, 1);
    assert!(actual.version > 1);
    assert_eq!(actual.cancellation, None);
    assert_eq!(
        actual.steps,
        vec![StepExecution {
            name: "RUN".into(),
            state: StepState::Completed,
            attempt: 1,
            termination: Some(StepTermination::ReturnCode { code: 0 })
        }]
    );
    let row = p
        .server
        .store
        .get_provider_state("jes-job", job)
        .unwrap()
        .unwrap();
    assert_eq!(row.version, actual.version);
    assert_eq!(json(&row)["submit_key"], key(fixture).as_str());
    assert_eq!(json(&row)["plan"], serde_json::to_value(plan()).unwrap());
    let checkpoint = p.server.batch.checkpoint(job).unwrap().unwrap();
    assert_eq!(
        checkpoint.schema_version,
        mainframe_env_batch::JES_CHECKPOINT_CONTRACT
    );
    assert_eq!(checkpoint.job_id, job);
    assert_eq!(checkpoint.job_version, actual.version);
    assert_eq!(checkpoint.attempt, 1);
    assert_eq!(checkpoint.active_step, None);
    assert_eq!(checkpoint.committed_steps, vec!["RUN".to_string()]);
    assert_eq!(checkpoint.dataset_resolutions, BTreeMap::new());
    assert_eq!(checkpoint.cancellation, None);
    assert_eq!(
        checkpoint.state_digest,
        format!("sha256:{:x}", Sha256::digest(&row.payload))
    );
    assert_eq!(json(&row)["effect_sequence"], checkpoint.effect_sequence);
    let retained = p
        .server
        .store
        .get_checkpoint(
            &ExecutionId::new(format!("jes-checkpoint-{job}"), InvocationLimits::default())
                .unwrap(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(retained.payload, serde_json::to_vec(&checkpoint).unwrap());
    assert_eq!(retained.payload_size, retained.payload.len() as u64);
    assert_eq!(
        retained.payload_digest,
        <[u8; 32]>::from(Sha256::digest(&retained.payload))
    );
    assert_eq!(retained.run_unit_id.as_str(), format!("jes-run-{job}"));
    assert_eq!(retained.session_id, None);
    assert_eq!(retained.schema_version, 1);
    assert_eq!(retained.machine_schema_version, 2);
    assert_eq!(
        retained.artifact.as_str(),
        format!("mainframe-env-batch@{}", env!("CARGO_PKG_VERSION"))
    );
    assert_eq!(retained.provider_generation, "1");
    assert!(retained.required_host_interfaces.is_empty());
    assert_eq!(retained.effect_sequence, checkpoint.effect_sequence);
    assert_eq!(retained.transaction.as_deref(), Some(job));
    assert_eq!(retained.principal.as_str(), "IBMUSER");
    assert_eq!(retained.security_classification, "batch");
    assert_eq!(retained.encryption_key_reference, None);
    let work = p.server.store.get_work(&queued.work_id).unwrap().unwrap();
    assert_eq!(
        work,
        WorkRecord {
            state: WorkState::Completed,
            attempt: 1,
            lease_epoch: 1,
            terminal_tick: Some(OFFERED_TICK),
            ..queued.clone()
        }
    );
    let audits = p
        .server
        .store
        .audit_records(&queued.execution_id, 1, 128)
        .unwrap();
    let invokes = audits
        .iter()
        .filter(|audit| audit.capability.as_str() == "host.program.invoke")
        .collect::<Vec<_>>();
    assert_eq!(invokes.len(), 1);
    let audit = invokes[0];
    assert_eq!(audit.execution_id, queued.execution_id);
    assert_eq!(audit.run_unit_id.as_str(), format!("jes-run-{job}-1"));
    assert_eq!(audit.attempt, 1);
    assert!(audit.effect_sequence > 0);
    assert_eq!(audit.observed_tick, queued.deadline_tick - 1);
    assert_eq!(audit.principal.as_str(), "IBMUSER");
    assert_eq!(audit.invocation_key.as_str(), format!("jes-work-{job}"));
    assert_eq!(audit.decision, AuditDecision::Success);
    // Construct the original ProgramCall payload from the signed first-party JCL input, not the audit output.
    let step = plan().steps.remove(0);
    let records = step
        .dds
        .iter()
        .map(|dd| {
            let mut records = dd
                .inline_data
                .split(|byte| *byte == b'\n')
                .map(<[u8]>::to_vec)
                .collect::<Vec<_>>();
            if dd.inline_data.ends_with(b"\n") {
                records.pop();
            }
            (dd.name.to_ascii_uppercase(), records)
        })
        .collect();
    let input = mainframe_env_batch::ProgramInput {
        parameter: step.parameter,
        dds: step.dds,
        dd_records: records,
        execution: Some(mainframe_env_batch::ProgramExecutionContext {
            job_name: "MQLOCAL".into(),
            step_name: "RUN".into(),
        }),
    };
    let expected_request = HostRequest::Program(mainframe_env_host_api::ProgramRequest::Call {
        program: mainframe_env_host_api::ProgramName::new("LOCALPGM", 128).unwrap(),
        payload: mainframe_env_execution_api::BoundedPayload::new(
            "mainframe-env.program.input@1",
            serde_json::to_vec(&input).unwrap(),
            InvocationLimits::default(),
        )
        .unwrap(),
        service: None,
    });
    assert_eq!(
        audit.resource,
        canonical_audit_resource_digest(&expected_request)
    );
    // The ordinary outer Batch dispatcher has a real audit, not a manufactured core EffectRecord.
    let outer = IdempotencyKey::new(
        format!("jes:{job}:RUN:{}", audit.effect_sequence),
        InvocationLimits::default(),
    )
    .unwrap();
    assert_eq!(p.server.store.effect(&outer).unwrap(), None);
    assert_source_and_catalog(fixture);
    (actual, row, retained, work)
}

#[test]
fn mq_real_complete_compiled_source_launches_one_controller_and_retires_actual_jes_on_both_stores()
{
    both_stores(|store| {
        let fixture = complete_fixture(store.clone());
        let (job, queued) = start(&fixture);
        assert_eq!(
            fixture
                .producer
                .server
                .retire_committed_local_mq_controller(
                    "LOCAL-COMPOSE",
                    source_key(&fixture.producer.request)
                ),
            Err(HostProblem::UnknownOutcome)
        );
        assert_eq!(
            fixture.producer.server.run_jes_worker_once(WORKER).unwrap(),
            Some(queued.work_id.clone())
        );
        let (actual, row, checkpoint, work) = assert_real_retirement(&fixture, &job, &queued);
        let before = launch_rows(store.as_ref());
        assert_eq!(
            fixture
                .producer
                .server
                .retire_committed_local_mq_controller(
                    "LOCAL-COMPOSE",
                    source_key(&fixture.producer.request)
                )
                .unwrap(),
            job
        );
        let completed = occurrence(
            &fixture,
            3,
            expected_occurrence(&fixture, "Completed", Some(&job), Some(&row)),
        );
        let expected = before
            .into_iter()
            .map(|old| {
                if old.namespace == OCCURRENCES && old.key == completed.key {
                    completed.clone()
                } else {
                    old
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(launch_rows(store.as_ref()), expected);
        assert_eq!(fixture.producer.server.batch.get(&job).unwrap(), actual);
        assert_eq!(
            store.get_checkpoint(&checkpoint.execution_id).unwrap(),
            Some(checkpoint)
        );
        assert_eq!(store.get_work(&queued.work_id).unwrap(), Some(work));
        let before = launch_rows(store.as_ref());
        assert_eq!(
            fixture
                .producer
                .server
                .retire_committed_local_mq_controller(
                    "LOCAL-COMPOSE",
                    source_key(&fixture.producer.request)
                ),
            Err(HostProblem::UnknownOutcome)
        );
        assert_eq!(launch_rows(store.as_ref()), before);
        assert_eq!(
            fixture.producer.server.run_jes_worker_once(WORKER).unwrap(),
            None
        );
        assert_source_and_catalog(&fixture);
    });
}

#[test]
fn mq_real_complete_foreign_source_and_occupied_occurrence_refuse_without_redispatch_on_both_stores()
 {
    both_stores(|store| {
        let fixture = complete_fixture(store.clone());
        let p = &fixture.producer;
        let before = launch_rows(store.as_ref());
        let mut wrong = p.source.clone();
        wrong.artifact = ArtifactRef::new("foreign-artifact", InvocationLimits::default()).unwrap();
        assert_eq!(
            p.server
                .start_committed_local_mq_controller("LOCAL-COMPOSE", &wrong, &p.request),
            Err(HostProblem::Unauthorized)
        );
        assert_eq!(launch_rows(store.as_ref()), before);
        let mut wrong = p.request.clone();
        wrong.message[0] ^= 1;
        assert_eq!(
            p.server
                .start_committed_local_mq_controller("LOCAL-COMPOSE", &p.source, &wrong),
            Err(HostProblem::IdempotencyConflict)
        );
        assert_eq!(launch_rows(store.as_ref()), before);
        let (job, queued) = start(&fixture);
        let before = launch_rows(store.as_ref());
        assert_eq!(
            p.server
                .start_committed_local_mq_controller("LOCAL-COMPOSE", &p.source, &p.request),
            Err(HostProblem::IdempotencyConflict)
        );
        assert_eq!(launch_rows(store.as_ref()), before);
        assert_eq!(
            store.get_work(&queued.work_id).unwrap(),
            Some(queued.clone())
        );
        assert_eq!(store.list_provider_state("jes-job", 16).unwrap().len(), 1);
        assert_eq!(
            p.server.run_jes_worker_once(WORKER).unwrap(),
            Some(queued.work_id.clone())
        );
        let (_, row, _, _) = assert_real_retirement(&fixture, &job, &queued);
        p.server
            .retire_committed_local_mq_controller("LOCAL-COMPOSE", source_key(&p.request))
            .unwrap();
        occurrence(
            &fixture,
            3,
            expected_occurrence(&fixture, "Completed", Some(&job), Some(&row)),
        );
        let before = launch_rows(store.as_ref());
        assert_eq!(
            p.server
                .start_committed_local_mq_controller("LOCAL-COMPOSE", &p.source, &p.request),
            Err(HostProblem::IdempotencyConflict)
        );
        assert_eq!(launch_rows(store.as_ref()), before);
        assert_eq!(p.server.run_jes_worker_once(WORKER).unwrap(), None);
    });
}

#[test]
fn mq_real_complete_before_and_after_occurrence_ack_loss_retains_unknown_without_launch_on_both_stores()
 {
    // Each outcome owns an isolated physical store; generation1 is never reinstalled into itself.
    both_stores(|inner| {
        exercise_ack_loss(
            Arc::new(OccurrenceFaultStore::new(inner)),
            OccurrenceOutcome::Before,
        )
    });
    both_stores(|inner| {
        exercise_ack_loss(
            Arc::new(OccurrenceFaultStore::new(inner)),
            OccurrenceOutcome::After,
        )
    });
}

fn exercise_ack_loss(
    fault: Arc<OccurrenceFaultStore<dyn PlatformStore>>,
    outcome: OccurrenceOutcome,
) {
    let store: Arc<dyn PlatformStore> = fault.clone();
    let fixture = complete_fixture(store.clone());
    let before = launch_rows(store.as_ref());
    fault.arm(outcome);
    assert_eq!(
        fixture.producer.server.start_committed_local_mq_controller(
            "LOCAL-COMPOSE",
            &fixture.producer.source,
            &fixture.producer.request
        ),
        Err(HostProblem::UnknownOutcome)
    );
    match outcome {
        OccurrenceOutcome::Before => assert_eq!(launch_rows(store.as_ref()), before),
        OccurrenceOutcome::After => {
            let unknown = occurrence(
                &fixture,
                1,
                expected_occurrence(&fixture, "LaunchUnknown", None, None),
            );
            let binding = store
                .get_provider_state(BINDINGS, key(&fixture).as_str())
                .unwrap()
                .unwrap();
            assert_eq!(binding.version, 1);
            assert_eq!(json(&binding), expected_binding(&fixture));
            let mut expected = before
                .into_iter()
                .map(|row| {
                    if matches!(
                        (row.namespace.as_str(), row.key.as_str()),
                        ("mq-state", "queues")
                            | ("mq-v1-queue", "LOCAL.QUEUE")
                            | ("mq-v1-replay", "invocation-local-compiled-source:1")
                    ) {
                        ProviderStateRecord {
                            version: row.version + 1,
                            ..row
                        }
                    } else {
                        row
                    }
                })
                .collect::<Vec<_>>();
            expected.extend([unknown, binding]);
            expected.sort_by(|a, b| (&a.namespace, &a.key).cmp(&(&b.namespace, &b.key)));
            assert_eq!(launch_rows(store.as_ref()), expected);
        }
    }
    assert!(store.list_provider_state("jes-job", 16).unwrap().is_empty());
    assert_eq!(
        fixture.producer.server.run_jes_worker_once(WORKER).unwrap(),
        None
    );
    assert_eq!(
        store.effect(source_key(&fixture.producer.request)).unwrap(),
        Some(expected_effect(
            &fixture.producer.source,
            &fixture.producer.request
        ))
    );
    assert_eq!(
        store
            .get_provider_state(APPLICATION_PUBLICATION_NAMESPACE, "LOCAL-COMPOSE")
            .unwrap(),
        Some(fixture.publication.clone())
    );
    assert_eq!(
        fixture.producer.server.mq.queue_depth("LOCAL.QUEUE"),
        Err(HostProblem::UnknownOutcome)
    );
    let before = launch_rows(store.as_ref());
    assert_eq!(
        fixture.producer.server.start_committed_local_mq_controller(
            "LOCAL-COMPOSE",
            &fixture.producer.source,
            &fixture.producer.request
        ),
        Err(HostProblem::UnknownOutcome)
    );
    assert_eq!(launch_rows(store.as_ref()), before);
    assert_eq!(
        fixture
            .producer
            .server
            .retire_committed_local_mq_controller(
                "LOCAL-COMPOSE",
                source_key(&fixture.producer.request)
            ),
        Err(match outcome {
            OccurrenceOutcome::Before => HostProblem::NotFound,
            OccurrenceOutcome::After => HostProblem::UnknownOutcome,
        })
    );
    assert_eq!(launch_rows(store.as_ref()), before);
    // Lost ACK remains retained; no replacement publication, redispatch or guessed settlement follows.
}

#[test]
fn mq_real_complete_retirement_requires_finished_work_exact_job_and_checkpoint_on_both_stores() {
    both_stores(|store| {
        let fixture = complete_fixture(store.clone());
        let p = &fixture.producer;
        let (job, queued) = start(&fixture);
        let claimed = p.server.claim_jes_work(WORKER).unwrap().unwrap();
        let outcome = p.server.process_claimed_jes_work(&claimed);
        let actual_job = p.server.batch.get(&job).unwrap();
        assert_eq!(
            actual_job.state,
            JobState::Completed,
            "JES job did not complete: {actual_job:#?}"
        );
        assert_eq!(
            store.get_work(&queued.work_id).unwrap().unwrap().state,
            WorkState::Claimed
        );
        let before = launch_rows(store.as_ref());
        assert_eq!(
            p.server
                .retire_committed_local_mq_controller("LOCAL-COMPOSE", source_key(&p.request)),
            Err(HostProblem::UnknownOutcome)
        );
        assert_eq!(launch_rows(store.as_ref()), before);
        p.server.finish_claimed_jes_work(&claimed, outcome).unwrap();
        let (_, row, checkpoint, work) = assert_real_retirement(&fixture, &job, &queued);
        let mut wrong = checkpoint.clone();
        wrong.effect_sequence += 1;
        store.put_checkpoint(wrong.clone()).unwrap();
        let before = launch_rows(store.as_ref());
        assert_eq!(
            p.server
                .retire_committed_local_mq_controller("LOCAL-COMPOSE", source_key(&p.request)),
            Err(HostProblem::InfrastructureFailure)
        );
        assert_eq!(launch_rows(store.as_ref()), before);
        assert_eq!(store.get_work(&queued.work_id).unwrap(), Some(work.clone()));
        assert_eq!(
            store.get_checkpoint(&wrong.execution_id).unwrap(),
            Some(wrong)
        );
        // Restore the actual retained checkpoint bytes only, then prove the separately altered Job version refuses.
        store.put_checkpoint(checkpoint.clone()).unwrap();
        store
            .put_provider_state(
                ProviderStateRecord {
                    version: row.version + 1,
                    ..row.clone()
                },
                Some(row.version),
            )
            .unwrap();
        let before = launch_rows(store.as_ref());
        assert_eq!(
            p.server
                .retire_committed_local_mq_controller("LOCAL-COMPOSE", source_key(&p.request)),
            Err(HostProblem::UnknownOutcome)
        );
        assert_eq!(launch_rows(store.as_ref()), before);
        assert_eq!(store.get_work(&queued.work_id).unwrap(), Some(work));
        assert_eq!(
            store.get_checkpoint(&checkpoint.execution_id).unwrap(),
            Some(checkpoint)
        );
        occurrence(
            &fixture,
            2,
            expected_occurrence(&fixture, "Submitted", Some(&job), None),
        );
    });
}

#[path = "local_postgres_composition_parity.rs"]
mod postgres;
