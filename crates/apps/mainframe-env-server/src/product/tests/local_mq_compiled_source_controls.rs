//! Explicit local embedding controls: genuine installed compiled producer and original core journal.
//! No Complete publication, native MQI frame, IBM structure, or trigger launch is fabricated.
use super::*;
use mainframe_env_execution_api::{AuditDecision, AuditRecord, LifecycleEvent};
use mainframe_env_host_api::{
    MqResult, canonical_audit_resource_digest, canonical_mq_request_digest, canonical_result_digest,
};
use mainframe_env_mq::{
    MqCommittedPutOffer, MqReplayDependency, MqReplayRetentionState, describe_mq_replay_row,
};
use mainframe_env_store_api::{EffectIntentMetadata, EffectRecord, ExecutionRecord};

const LOCALPUT: &[u8] = include_bytes!("local_mq_compiled_source_controls/localput.cbl");
const DATA: &[u8] = b"LOCAL-COMMITTED-PUT1";
const MESSAGE_ID: &[u8] = b"MSG000000000000000000001";
const CORRELATION_ID: &[u8] = b"COR000000000000000000001";
const OFFERED_TICK: u64 = 1;

struct InstalledProducer {
    server: Arc<ProductServer>,
    source: Invocation,
    request: MqRequest,
    before_manifest: ProviderStateRecord,
    before_queue: ProviderStateRecord,
    stable: Vec<ProviderStateRecord>,
}

fn compiled_definitions(
    server: &ProductServer,
    trust: &HmacSha256PackageTrust,
) -> (
    ApplicationPackageV2,
    LocalApplicationDefinition,
    ArtifactRef,
) {
    let (mut package, mut definition) = launch_graph_definitions(server, trust);
    let published = published_source_fixture("LOCALPUT", std::str::from_utf8(LOCALPUT).unwrap());
    let program = OnlineProgramDefinition::current("LOCALPUT", &published);
    let artifact = program.artifact.clone();
    for (kind, path, raw) in [
        (EntryKind::Source, "source/LOCALPUT", LOCALPUT.to_vec()),
        (
            EntryKind::Program,
            "program/LOCALPUT",
            program.payload.clone(),
        ),
    ] {
        let sha = format!("sha256:{:x}", Sha256::digest(&raw));
        package.base.blobs.insert(sha.clone(), raw.clone());
        package.base.manifest.entries.push(PackageEntry {
            path: path.into(),
            kind,
            sha256: sha,
            bytes: raw.len(),
            depends_on: if kind == EntryKind::Source {
                Vec::new()
            } else {
                vec!["source/LOCALPUT".into()]
            },
        });
    }
    definition.online.programs.push(program);
    definition
        .compilations
        .push(source_compilation("LOCALPUT", LOCALPUT));
    source_commitments(&mut package, &definition, trust);
    (package, definition, artifact)
}

fn stable_rows(store: &dyn ProviderStateStore, key: &str) -> Vec<ProviderStateRecord> {
    launch_rows(store)
        .into_iter()
        .filter(|row| {
            !matches!(
                (row.namespace.as_str(), row.key.as_str()),
                ("mq-state", "queues") | ("mq-v1-queue", "LOCAL.QUEUE")
            ) && !(row.namespace == "mq-v1-replay" && row.key == key)
        })
        .collect()
}

impl InstalledProducer {
    fn new(store: Arc<dyn PlatformStore>) -> Self {
        let trust = Arc::new(test_package_trust());
        let mut settings = config();
        settings.store_profile = crate::StoreProfile::Postgres;
        settings.artifact_profile = ArtifactProfile::Shared;
        settings.postgres_url_reference =
            Some("env-base64:MAINFRAME_ENV_SECRET_POSTGRES_URL".into());
        settings.validate().unwrap();
        let artifacts: Arc<dyn ArtifactStore> = store.clone();
        // Existing constructor owns this explicit test clock only. No global clock is changed.
        let server = ProductServer::open_configured(
            settings,
            store.clone(),
            Arc::new(MemorySecretResolver::default()),
            default_program_router(),
            trust.clone(),
            Arc::new(ProductArtifactStore::Shared(artifacts)),
            Some(Arc::new(ManualJesClock::new(OFFERED_TICK))),
        )
        .unwrap();
        let (package, definition, artifact) = compiled_definitions(&server, &trust);
        let expected = server.install_application_package_v2(&package).unwrap();
        server
            .publish_local_application(&expected, definition)
            .unwrap();
        assert_eq!(
            server.online_programs.lock().unwrap().get("LOCALPUT"),
            Some(&artifact)
        );
        let installed = server.artifacts.get_artifact(&artifact).unwrap().unwrap();
        let admitted = crate::cobol::artifact::admit_executable_artifact(&installed).unwrap();
        assert_eq!(admitted.content_id().to_reference(), artifact.as_str());
        assert_eq!(admitted.payload(), installed.payload.as_slice());
        let mut source = invocation("local-compiled-source");
        source.selector = Selector::new("program:LOCALPUT", InvocationLimits::default()).unwrap();
        source.artifact = artifact;
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
        assert_eq!(
            store
                .get_provider_state(
                    mainframe_env_application::APPLICATION_PUBLICATION_NAMESPACE,
                    "LOCAL-COMPOSE"
                )
                .unwrap(),
            None
        );
        assert_eq!(server.mq.queue_depth("LOCAL.QUEUE"), Ok(0));
        Self {
            server,
            source,
            request,
            before_manifest,
            before_queue,
            stable,
        }
    }

    fn drive(&self) -> ExecutionOutcome {
        // Read only the genuinely installed and codec-admitted artifact. No test-authored MIR/effect rows.
        let row = self
            .server
            .artifacts
            .get_artifact(&self.source.artifact)
            .unwrap()
            .unwrap();
        let executable = crate::cobol::artifact::admit_executable_artifact(&row).unwrap();
        let mut machine = ReferenceMachine::from_binary(
            executable.payload(),
            self.source.clone(),
            CodecLimits::default(),
        )
        .unwrap();
        ExecutionCoordinator::durable(
            self.server.host.clone(),
            self.server.store.clone(),
            Default::default(),
        )
        .execute_with_control(&mut machine, &self.source, || {
            Ok(ExecutionControl {
                now_tick: OFFERED_TICK,
                cancellation_requested: false,
            })
        })
    }

    fn known(&self) {
        match self.drive() {
            ExecutionOutcome::Completed(completed) => {
                assert_eq!(completed.return_code, 0);
                assert!(completed.output.bytes().is_empty());
            }
            outcome => panic!("installed explicit local producer did not complete: {outcome:?}"),
        }
        let expected_effect = expected_effect(&self.source, &self.request);
        assert_eq!(
            self.server.store.effect(source_key(&self.request)).unwrap(),
            Some(expected_effect.clone())
        );
        assert_eq!(
            self.server
                .store
                .get_execution(&self.source.execution_id)
                .unwrap(),
            Some(ExecutionRecord {
                execution_id: self.source.execution_id.clone(),
                run_unit_id: self.source.run_unit_id.clone(),
                selector: self.source.selector.clone(),
                artifact: self.source.artifact.clone(),
                principal: self.source.principal.id().clone(),
                state: ExecutionState::Completed,
                attempt: 1,
                version: 7,
                owner_lease: None,
                lease_expiry_tick: None,
                terminal_tick: Some(OFFERED_TICK)
            })
        );
        let kinds = [
            LifecycleEventKind::Admitted,
            LifecycleEventKind::Queued,
            LifecycleEventKind::Started,
            LifecycleEventKind::EffectIntent { sequence: 1 },
            LifecycleEventKind::EffectResult { sequence: 1 },
            LifecycleEventKind::Completing,
            LifecycleEventKind::Completed { return_code: 0 },
        ];
        let expected_events = kinds
            .into_iter()
            .enumerate()
            .map(|(index, kind)| LifecycleEvent {
                execution_id: self.source.execution_id.clone(),
                run_unit_id: self.source.run_unit_id.clone(),
                sequence: index as u64 + 1,
                attempt: 1,
                tick: OFFERED_TICK,
                kind,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            self.server
                .store
                .events(&self.source.execution_id, 1, 16)
                .unwrap(),
            expected_events
        );
        let expected_audit = expected_audit(&self.source, &self.request);
        assert_eq!(
            self.server
                .store
                .audit_records(&self.source.execution_id, 1, 16)
                .unwrap(),
            vec![expected_audit]
        );
        let manifest = self
            .server
            .store
            .get_provider_state("mq-state", "queues")
            .unwrap()
            .unwrap();
        let queue = self
            .server
            .store
            .get_provider_state("mq-v1-queue", "LOCAL.QUEUE")
            .unwrap()
            .unwrap();
        let replay = self
            .server
            .store
            .get_provider_state("mq-v1-replay", source_key(&self.request).as_str())
            .unwrap()
            .unwrap();
        // Two actual persist calls: first queue/result, then replay resolution. Both fence the manifest.
        assert_eq!(manifest.version, self.before_manifest.version + 2);
        assert_eq!(manifest.payload, self.before_manifest.payload);
        assert_eq!(
            json(&manifest),
            serde_json::json!({"schema_version":"mainframe-env.mq-row-store@1",
            "definitions":null,"next_handle":1})
        );
        assert_eq!(queue.version, self.before_queue.version + 1);
        assert_eq!(queue.namespace, "mq-v1-queue");
        assert_eq!(queue.key, "LOCAL.QUEUE");
        assert_eq!(json(&queue), expected_queue());
        assert_eq!(replay.namespace, "mq-v1-replay");
        assert_eq!(replay.key, source_key(&self.request).as_str());
        assert_eq!(replay.version, 2);
        assert_eq!(
            json(&replay),
            expected_replay(&self.source, &self.request, self.source.deadline_tick)
        );
        let descriptor =
            describe_mq_replay_row(&replay, Some(&expected_effect), MqLimits::default()).unwrap();
        assert_eq!(descriptor.retention, MqReplayRetentionState::Terminal);
        assert_eq!(descriptor.dependency, Some(MqReplayDependency::CoreEffect));
        assert_eq!(
            descriptor.owner_execution.as_deref(),
            Some(self.source.execution_id.as_str())
        );
        assert_eq!(
            descriptor.owner_run_unit.as_deref(),
            Some(self.source.run_unit_id.as_str())
        );
        assert_eq!(descriptor.sequence, Some(1));
        assert_eq!(
            descriptor.recorded_deadline_tick,
            Some(self.source.deadline_tick)
        );
        assert_eq!(descriptor.resolution_tick, Some(self.source.deadline_tick));
        assert_eq!(
            stable_rows(
                self.server.store.as_ref(),
                source_key(&self.request).as_str()
            ),
            self.stable
        );
        assert_eq!(
            self.server.mq.queue_messages("LOCAL.QUEUE"),
            Ok(vec![DATA.to_vec()])
        );
        assert!(matches_known_rows(self, &manifest, &queue, &replay));
        // Refusals exercise the same complete oracle that admits the actual compiled source.
        let mut bad_byte = queue.clone();
        let mut value = expected_queue();
        value["value"]["messages"][0]["data"][0] = serde_json::json!(0);
        bad_byte.payload = serde_json::to_vec(&value).unwrap();
        assert!(!matches_known_rows(self, &manifest, &bad_byte, &replay));
        let mut bad_key = queue.clone();
        bad_key.key = "FOREIGN.QUEUE".into();
        assert!(!matches_known_rows(self, &manifest, &bad_key, &replay));
        let mut bad_version = replay.clone();
        bad_version.version += 1;
        assert!(!matches_known_rows(self, &manifest, &queue, &bad_version));
        let mut extra = queue.clone();
        let mut value = expected_queue();
        value["value"]["messages"]
            .as_array_mut()
            .unwrap()
            .push(expected_queue()["value"]["messages"][0].clone());
        extra.payload = serde_json::to_vec(&value).unwrap();
        assert!(!matches_known_rows(self, &manifest, &extra, &replay));
        let mut bad_id = queue.clone();
        let mut value = expected_queue();
        value["value"]["messages"][0]["message_id"][23] = serde_json::json!(0);
        bad_id.payload = serde_json::to_vec(&value).unwrap();
        assert!(!matches_known_rows(self, &manifest, &bad_id, &replay));
        let mut bad_replay = replay.clone();
        let mut value = expected_replay(&self.source, &self.request, self.source.deadline_tick);
        value["value"]["recorded_sequence"] = serde_json::json!(2);
        bad_replay.payload = serde_json::to_vec(&value).unwrap();
        assert!(!matches_known_rows(self, &manifest, &queue, &bad_replay));
    }
}

fn matches_known_rows(
    fixture: &InstalledProducer,
    manifest: &ProviderStateRecord,
    queue: &ProviderStateRecord,
    replay: &ProviderStateRecord,
) -> bool {
    manifest.namespace == "mq-state"
        && manifest.key == "queues"
        && manifest.version == fixture.before_manifest.version + 2
        && manifest.payload == fixture.before_manifest.payload
        && queue.namespace == "mq-v1-queue"
        && queue.key == "LOCAL.QUEUE"
        && queue.version == fixture.before_queue.version + 1
        && serde_json::from_slice::<serde_json::Value>(&queue.payload).ok()
            == Some(expected_queue())
        && replay.namespace == "mq-v1-replay"
        && replay.key == source_key(&fixture.request).as_str()
        && replay.version == 2
        && serde_json::from_slice::<serde_json::Value>(&replay.payload).ok()
            == Some(expected_replay(
                &fixture.source,
                &fixture.request,
                fixture.source.deadline_tick,
            ))
}

fn source_key(request: &MqRequest) -> &IdempotencyKey {
    &request.mutation.as_ref().unwrap().idempotency_key
}
fn json(row: &ProviderStateRecord) -> serde_json::Value {
    serde_json::from_slice(&row.payload).unwrap()
}
fn expected_result() -> MqResult {
    MqResult {
        completion_code: 0,
        reason_code: 0,
        handle: None,
        message: Vec::new(),
        message_id: Some(MESSAGE_ID.to_vec()),
        correlation_id: Some(CORRELATION_ID.to_vec()),
        trigger_program: Some("LOCAL.PROCESS".into()),
    }
}
fn expected_audit(source: &Invocation, request: &MqRequest) -> AuditRecord {
    AuditRecord {
        execution_id: source.execution_id.clone(),
        run_unit_id: source.run_unit_id.clone(),
        attempt: 1,
        effect_sequence: 1,
        observed_tick: OFFERED_TICK,
        principal: source.principal.id().clone(),
        invocation_key: source.idempotency_key.clone(),
        capability: CapabilityId::new("host.mq.write", InvocationLimits::default()).unwrap(),
        resource: canonical_audit_resource_digest(&HostRequest::Mq(request.clone())),
        decision: AuditDecision::Success,
    }
}
fn expected_effect(source: &Invocation, request: &MqRequest) -> EffectRecord {
    let audit = expected_audit(source, request);
    EffectRecord {
        execution_id: source.execution_id.clone(),
        run_unit_id: source.run_unit_id.clone(),
        sequence: 1,
        key: source_key(request).clone(),
        digest_format: EffectDigestFormat::CanonicalHostV1,
        request_digest: canonical_mq_request_digest(request).unwrap(),
        intent: EffectIntentMetadata {
            owner: source.execution_id.clone(),
            attempt: 1,
            capability: Some(audit.capability),
            audit_resource: Some(audit.resource),
            audit_invocation_key: Some(source.idempotency_key.clone()),
            created_tick: OFFERED_TICK,
            recovery_after_tick: source.deadline_tick,
            epoch: 4,
            recovery_lease: None,
        },
        state: EffectState::Completed,
        result_digest: Some(
            canonical_result_digest(&Ok(HostResult::Mq(expected_result()))).unwrap(),
        ),
        resolved_tick: Some(OFFERED_TICK),
    }
}
fn expected_queue() -> serde_json::Value {
    serde_json::json!({"schema_version":"mainframe-env.mq-object-row@1",
    "object_key":"LOCAL.QUEUE","value":{"trigger_program":"LOCAL.PROCESS","messages":[{"data":DATA,
    "message_id":MESSAGE_ID,"correlation_id":CORRELATION_ID}]}})
}
fn binding(source: &Invocation, request: &MqRequest, resolution: u64) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"mainframe-env.mq-replay-retention-binding@1\0");
    for raw in [
        source_key(request).as_str().as_bytes(),
        source.execution_id.as_str().as_bytes(),
        source.run_unit_id.as_str().as_bytes(),
        b"".as_slice(),
    ] {
        hash.update((raw.len() as u64).to_be_bytes());
        hash.update(raw);
    }
    hash.update([1]);
    hash.update(1u64.to_be_bytes());
    hash.update(source.deadline_tick.to_be_bytes());
    hash.update(resolution.to_be_bytes());
    hash.update(canonical_mq_request_digest(request).unwrap());
    hash.update(canonical_result_digest(&Ok(HostResult::Mq(expected_result()))).unwrap());
    hash.finalize().into()
}
fn expected_replay(source: &Invocation, request: &MqRequest, resolution: u64) -> serde_json::Value {
    serde_json::json!({"schema_version":"mainframe-env.mq-object-row@1", "object_key":source_key(request).as_str(),
        "value":{"request_digest_format":"mainframe-env.provider-replay-canonical@1",
        "request_sha256":canonical_mq_request_digest(request).unwrap(), "recorded_deadline_tick":source.deadline_tick,
        "owner_execution":source.execution_id.as_str(),"owner_run_unit":source.run_unit_id.as_str(),
        "recorded_sequence":1,"resolution_tick":resolution,"owner_kind":"core-effect","outer_effect_key":null,
        "result_sha256":canonical_result_digest(&Ok(HostResult::Mq(expected_result()))).unwrap(),
        "retention_binding_sha256":binding(source,request,resolution),"completion_code":0,"reason_code":0,"handle":null,
        "message":[],"message_id":MESSAGE_ID,"correlation_id":CORRELATION_ID,"trigger_program":"LOCAL.PROCESS"}})
}

fn both_stores(exercise: impl Fn(Arc<dyn PlatformStore>)) {
    exercise(Arc::new(MemoryStore::new(Default::default())));
    static SERIAL: AtomicU64 = AtomicU64::new(1);
    let root = std::env::temp_dir().join(format!(
        "local-mq-compiled-control-{}-{}",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir(&root).unwrap();
    exercise(Arc::new(
        SqliteStateStore::open(
            &format!("sqlite://{}?mode=rwc", root.join("state.sqlite").display()),
            64 * 1024 * 1024,
            262_144,
        )
        .unwrap(),
    ));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn mq_installed_compiled_local_put_has_exact_core_audit_replay_and_same_store_fencing_on_both_stores()
 {
    both_stores(|store| {
        let fixture = InstalledProducer::new(store.clone());
        fixture.known();
        let catalog = projection(store.as_ref());
        let before = launch_rows(store.as_ref());
        let effect = store.effect(source_key(&fixture.request)).unwrap();
        let events = store.events(&fixture.source.execution_id, 1, 16).unwrap();
        let audits = store
            .audit_records(&fixture.source.execution_id, 1, 16)
            .unwrap();
        let prepared = fixture
            .server
            .mq
            .prepare_committed_local_put(
                &store,
                MqCommittedPutOffer {
                    source: &fixture.source,
                    request: &fixture.request,
                    catalog: &catalog,
                },
            )
            .unwrap();
        assert_eq!(prepared.queue(), "LOCAL.QUEUE");
        assert_eq!(prepared.process(), "LOCAL.PROCESS");
        assert_eq!(
            prepared.source_effect(),
            &expected_effect(&fixture.source, &fixture.request)
        );
        assert_eq!(
            prepared.source_row(),
            &store
                .get_provider_state("mq-v1-replay", source_key(&fixture.request).as_str())
                .unwrap()
                .unwrap()
        );
        let writes = prepared.writes().to_vec();
        assert_eq!(writes.len(), 3);
        for (write, (namespace, key)) in writes.iter().zip([
            ("mq-state", "queues"),
            ("mq-v1-queue", "LOCAL.QUEUE"),
            ("mq-v1-replay", source_key(&fixture.request).as_str()),
        ]) {
            let current = store.get_provider_state(namespace, key).unwrap().unwrap();
            assert_eq!(write.expected_version, Some(current.version));
            assert_eq!(
                write.record,
                ProviderStateRecord {
                    version: current.version + 1,
                    ..current
                }
            );
        }
        assert_eq!(launch_rows(store.as_ref()), before);
        let provider: Arc<dyn ProviderStateStore> = store.clone();
        let acknowledgement = ProviderCompositionCommit::publish(provider, writes.clone()).unwrap();
        assert!(prepared.accepts(&acknowledgement));
        prepared.finish(&acknowledgement).unwrap();
        let expected = before
            .into_iter()
            .map(|row| {
                writes
                    .iter()
                    .find(|write| {
                        write.record.namespace == row.namespace && write.record.key == row.key
                    })
                    .map_or(row.clone(), |write| write.record.clone())
            })
            .collect::<Vec<_>>();
        assert_eq!(launch_rows(store.as_ref()), expected);
        assert_eq!(store.effect(source_key(&fixture.request)).unwrap(), effect);
        assert_eq!(
            store.events(&fixture.source.execution_id, 1, 16).unwrap(),
            events
        );
        assert_eq!(
            store
                .audit_records(&fixture.source.execution_id, 1, 16)
                .unwrap(),
            audits
        );
        assert_eq!(
            fixture.server.mq.queue_messages("LOCAL.QUEUE"),
            Ok(vec![DATA.to_vec()])
        );
        // Source admission/fencing cannot manufacture Complete, a controller Job, Work, or an occurrence.
        assert_eq!(
            store
                .get_provider_state(
                    mainframe_env_application::APPLICATION_PUBLICATION_NAMESPACE,
                    "LOCAL-COMPOSE"
                )
                .unwrap(),
            None
        );
        assert!(store.list_provider_state("jes-job", 16).unwrap().is_empty());
        assert!(
            store
                .list_provider_state("application-local-mq-occurrence", 16)
                .unwrap()
                .is_empty()
        );
    });
}

#[test]
fn mq_compiled_source_wrong_owner_request_catalog_or_store_refuses_without_writes_on_both_stores() {
    both_stores(|store| {
        let fixture = InstalledProducer::new(store.clone());
        fixture.known();
        let catalog = projection(store.as_ref());
        let before = launch_rows(store.as_ref());
        let core = store.effect(source_key(&fixture.request)).unwrap();
        for mode in 0..6 {
            let mut source = fixture.source.clone();
            let expected = match mode {
                0 => {
                    source.execution_id =
                        ExecutionId::new("foreign-execution", InvocationLimits::default()).unwrap();
                    HostProblem::NotFound
                }
                1 => {
                    source.run_unit_id =
                        RunUnitId::new("foreign-run", InvocationLimits::default()).unwrap();
                    HostProblem::Unauthorized
                }
                2 => {
                    source.selector =
                        Selector::new("program:LOCALPGM", InvocationLimits::default()).unwrap();
                    HostProblem::Unauthorized
                }
                3 => {
                    source.artifact =
                        ArtifactRef::new("foreign-artifact", InvocationLimits::default()).unwrap();
                    HostProblem::Unauthorized
                }
                4 => {
                    source.principal = Principal::new(
                        PrincipalId::new("FOREIGN", InvocationLimits::default()).unwrap(),
                        source.principal.grants().clone(),
                        InvocationLimits::default(),
                    )
                    .unwrap();
                    HostProblem::Unauthorized
                }
                _ => {
                    source.attempt += 1;
                    HostProblem::Unauthorized
                }
            };
            assert!(
                matches!(fixture.server.mq.prepare_committed_local_put(&store,
                MqCommittedPutOffer { source:&source,request:&fixture.request,catalog:&catalog }), Err(problem) if problem == expected)
            );
            assert_eq!(launch_rows(store.as_ref()), before);
        }
        for mode in 0..14 {
            let mut request = fixture.request.clone();
            let expected = match mode {
                0 => {
                    request.options = 2;
                    HostProblem::Unsupported
                }
                1 => {
                    request.handle = Some(1);
                    HostProblem::Unsupported
                }
                2 => {
                    request.wait_ticks = 1;
                    HostProblem::Unsupported
                }
                3 => {
                    request.queue = Some("LOCAL.QUEUE ".into());
                    HostProblem::Unsupported
                }
                4 => {
                    request.operation = MqOperation::Put;
                    HostProblem::Unsupported
                }
                5 => {
                    request.mutation = None;
                    HostProblem::MissingIdempotency
                }
                6 => {
                    request.mutation.as_mut().unwrap().transaction = Some("FOREIGN".into());
                    HostProblem::Unsupported
                }
                7 => {
                    request.mutation.as_mut().unwrap().sequence = 0;
                    HostProblem::Unsupported
                }
                8 => {
                    request.mutation.as_mut().unwrap().sequence = 2;
                    HostProblem::IdempotencyConflict
                }
                9 => {
                    request.mutation.as_mut().unwrap().idempotency_key =
                        IdempotencyKey::new("foreign-key", InvocationLimits::default()).unwrap();
                    HostProblem::NotFound
                }
                10 => {
                    request.message[0] ^= 1;
                    HostProblem::IdempotencyConflict
                }
                11 => {
                    request.message_id.as_mut().unwrap()[23] ^= 1;
                    HostProblem::IdempotencyConflict
                }
                12 => {
                    request.correlation_id.as_mut().unwrap()[23] ^= 1;
                    HostProblem::IdempotencyConflict
                }
                _ => {
                    request.max_message_bytes += 1;
                    HostProblem::IdempotencyConflict
                }
            };
            assert!(
                matches!(fixture.server.mq.prepare_committed_local_put(&store,
                MqCommittedPutOffer { source:&fixture.source,request:&request,catalog:&catalog }), Err(problem) if problem == expected)
            );
            assert_eq!(launch_rows(store.as_ref()), before);
        }
        let mut foreign_catalog = catalog.clone();
        foreign_catalog.package_identity = format!("sha256:{}", "0".repeat(64));
        assert!(matches!(
            fixture.server.mq.prepare_committed_local_put(
                &store,
                MqCommittedPutOffer {
                    source: &fixture.source,
                    request: &fixture.request,
                    catalog: &foreign_catalog
                }
            ),
            Err(HostProblem::IdempotencyConflict)
        ));
        let foreign: Arc<dyn PlatformStore> = Arc::new(MemoryStore::new(Default::default()));
        assert!(matches!(
            fixture.server.mq.prepare_committed_local_put(
                &foreign,
                MqCommittedPutOffer {
                    source: &fixture.source,
                    request: &fixture.request,
                    catalog: &catalog
                }
            ),
            Err(HostProblem::Unsupported)
        ));
        assert_eq!(launch_rows(store.as_ref()), before);
        assert_eq!(store.effect(source_key(&fixture.request)).unwrap(), core);
    });
}

#[test]
fn mq_compiled_source_equal_rows_foreign_ack_cannot_release_owner_on_both_stores() {
    both_stores(|store| {
        let fixture = InstalledProducer::new(store.clone());
        fixture.known();
        let catalog = projection(store.as_ref());
        let before = launch_rows(store.as_ref());
        let core = store.effect(source_key(&fixture.request)).unwrap();
        let prepared = fixture
            .server
            .mq
            .prepare_committed_local_put(
                &store,
                MqCommittedPutOffer {
                    source: &fixture.source,
                    request: &fixture.request,
                    catalog: &catalog,
                },
            )
            .unwrap();
        let writes = prepared.writes().to_vec();
        let foreign: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        // Copy only generic rows to construct the independent negative. This grants no source/owner authority.
        for write in &writes {
            for version in 1..=write.expected_version.unwrap() {
                foreign
                    .put_provider_state(
                        ProviderStateRecord {
                            version,
                            ..write.record.clone()
                        },
                        (version > 1).then_some(version - 1),
                    )
                    .unwrap();
            }
        }
        let foreign_ack = ProviderCompositionCommit::publish(foreign, writes).unwrap();
        assert!(!prepared.accepts(&foreign_ack));
        assert_eq!(
            prepared.finish(&foreign_ack),
            Err(HostProblem::UnknownOutcome)
        );
        assert_eq!(launch_rows(store.as_ref()), before);
        assert_eq!(store.effect(source_key(&fixture.request)).unwrap(), core);
        assert_eq!(
            fixture.server.mq.queue_depth("LOCAL.QUEUE"),
            Err(HostProblem::UnknownOutcome)
        );
    });
}

#[test]
fn mq_compiled_source_real_post_persist_unknown_retains_message_and_never_admits_completed_source_on_both_stores()
 {
    both_stores(|store| {
        let fixture = InstalledProducer::new(store.clone());
        fixture.server.mq.inject_unknown_outcome_once();
        assert!(!matches!(fixture.drive(), ExecutionOutcome::Completed(_)));
        let effect = store.effect(source_key(&fixture.request)).unwrap().unwrap();
        assert_eq!(effect.state, EffectState::UnknownOutcome);
        assert_eq!(effect.resolved_tick, None);
        assert_eq!(
            effect.request_digest,
            canonical_mq_request_digest(&fixture.request).unwrap()
        );
        assert_eq!(
            effect.result_digest,
            Some(canonical_result_digest(&Err(HostProblem::UnknownOutcome)).unwrap())
        );
        assert_ne!(
            store
                .get_execution(&fixture.source.execution_id)
                .unwrap()
                .unwrap()
                .state,
            ExecutionState::Completed
        );
        let queue = store
            .get_provider_state("mq-v1-queue", "LOCAL.QUEUE")
            .unwrap()
            .unwrap();
        assert_eq!(json(&queue), expected_queue());
        assert_eq!(queue.version, fixture.before_queue.version + 1);
        let replay = store
            .get_provider_state("mq-v1-replay", source_key(&fixture.request).as_str())
            .unwrap()
            .unwrap();
        assert_eq!(replay.version, 1);
        assert_eq!(
            json(&replay),
            expected_replay(&fixture.source, &fixture.request, 0)
        );
        let before = launch_rows(store.as_ref());
        let catalog = projection(store.as_ref());
        assert!(matches!(
            fixture.server.mq.prepare_committed_local_put(
                &store,
                MqCommittedPutOffer {
                    source: &fixture.source,
                    request: &fixture.request,
                    catalog: &catalog
                }
            ),
            Err(HostProblem::Unauthorized)
        ));
        assert_eq!(launch_rows(store.as_ref()), before);
        assert!(
            store
                .list_provider_state("application-local-mq-occurrence", 16)
                .unwrap()
                .is_empty()
        );
        // No repeat dispatch, settlement, queue purge, or guessed recovery follows this actual Unknown.
    });
}

// Separate real Complete/Applied and ordinary JES owner controls; original four remain exact.
#[path = "local_mq_launch_owner_controls.rs"]
mod launch_owner;
