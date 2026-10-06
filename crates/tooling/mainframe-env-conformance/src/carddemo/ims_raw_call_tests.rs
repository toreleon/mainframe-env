//! Gap witnesses, not raw DL/I execution acceptance or official row verdicts.
use super::*;
use mainframe_env_execution_api::{AuditDecision, ExecutionOutcome};
use mainframe_env_host_api::{HostProvider, HostRequest, ProgramRequest};
use mainframe_env_interpreter::{
    CoordinatorLimits, ExecutionControl, ExecutionCoordinator, ReferenceMachine,
};

// These are deliberately unbound application-owned canaries, NOT IMS-issued PCBs.
// Equal bytes or a matching DBD name must never grant a selected PCB identity.
fn source(function: &str, mode: &str, pcb: &str, area: &str, ssa: &str) -> String {
    format!(
        "IDENTIFICATION DIVISION. PROGRAM-ID. RAWDLI.\n\
         DATA DIVISION. WORKING-STORAGE SECTION.\n\
         01 FUNC PIC X(4) VALUE '{function}'.\n\
         01 COUNT-N PIC S9(9) COMP-5 VALUE 4.\n\
         01 PCB-A PIC X(46) VALUE ALL 'A'.\n\
         01 PCB-B PIC X(46) VALUE ALL 'B'.\n\
         01 AREA PIC X(100) VALUE ALL 'Z'.\n\
         01 SHORT-AREA PIC X VALUE 'Y'.\n\
         01 SSA PIC X(25) VALUE 'PAUTSUM0(ACCNTID =000001)'.\n\
         01 BAD-SSA PIC X(9) VALUE 'PAUTSUM0('.\n\
         PROCEDURE DIVISION.\n\
         CALL 'CBLTDLI' USING {mode} FUNC {pcb} {area} {ssa}\n\
           ON EXCEPTION DISPLAY 'RAW-REJECTED'\n\
           NOT ON EXCEPTION DISPLAY 'RAW-RETURNED'\n\
         END-CALL.\n\
         DISPLAY PCB-A. DISPLAY PCB-B. DISPLAY AREA. STOP RUN."
    )
}

fn invocation(run: &str, definition: &BatchProgramDefinition, program_grant: bool) -> Invocation {
    let mut invocation = ims_invocation(run, true, None).unwrap();
    invocation.artifact = definition.artifact.clone();
    let ids = InvocationLimits::default();
    let mut grants = invocation.principal.grants().clone();
    if program_grant {
        grants.insert(CapabilityId::new("host.program.invoke", ids).unwrap());
    }
    invocation.principal = Principal::new(invocation.principal.id().clone(), grants, ids).unwrap();
    invocation
}

fn probe(definition: &BatchProgramDefinition) -> EffectRequest {
    let invocation = invocation("raw-producer", definition, true);
    let mut machine = ReferenceMachine::from_binary(
        &definition.payload,
        invocation,
        mainframe_env_ir::CodecLimits::default(),
    )
    .unwrap();
    let MachineDrive::HostCall(effect) = machine.drive(
        MachineResume::Start,
        Quantum {
            max_steps: 100,
            max_allocated_bytes: 1_048_576,
        },
    ) else {
        panic!("compiled CALL must produce an actual host effect")
    };
    effect
}

fn literal_payload() -> Vec<u8> {
    let mut bytes = 4u32.to_be_bytes().to_vec();
    for (name, value) in [
        ("FUNC", b"GU  ".to_vec()),
        ("PCB-A", vec![b'A'; 46]),
        ("AREA", vec![b'Z'; 100]),
        ("SSA", b"PAUTSUM0(ACCNTID =000001)".to_vec()),
    ] {
        bytes.extend_from_slice(&(name.len() as u64).to_be_bytes());
        bytes.extend_from_slice(name.as_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&(value.len() as u64).to_be_bytes());
        bytes.extend_from_slice(&value);
    }
    bytes
}

#[test]
fn raw_dli_compiled_call_exposes_value_only_abi_gap() {
    let reference = compile_free_batch_definition(
        "RAWDLI",
        &source("GU", "BY REFERENCE", "PCB-A", "AREA", "SSA"),
    )
    .unwrap();
    let content = compile_free_batch_definition(
        "RAWDLI",
        &source("GU", "BY CONTENT", "PCB-A", "AREA", "SSA"),
    )
    .unwrap();
    let extract = |effect: EffectRequest| {
        let HostRequest::Program(ProgramRequest::Call {
            program,
            payload,
            service,
        }) = effect.request
        else {
            panic!("raw CALL was silently substituted by a typed IMS request")
        };
        assert_eq!(program.as_str(), "CBLTDLI");
        assert!(service.is_none(), "no raw IMS ABI registration exists");
        assert_eq!(payload.schema(), "mainframe-env.cobol.call@1");
        payload.bytes().to_vec()
    };
    let reference = extract(probe(&reference));
    assert_eq!(reference, literal_payload());
    // Source says same-storage reference and independent content are different.
    // The current transport erases that distinction. This assertion records a gap.
    assert_eq!(extract(probe(&content)), reference);
}

fn provider_bytes(store: &dyn PlatformStore) -> Vec<(String, String, u64, Vec<u8>)> {
    let mut rows = Vec::new();
    for namespace in [
        "ims-state",
        "ims-v1-database",
        "ims-v1-generic-database",
        "ims-v1-session-index",
        "ims-v1-replay",
        "ims-v1-generic-unit-of-work",
        "ims-v1-unit-of-work",
        "ims-v1-checkpoint",
        "ims-v1-system",
        "ims-recovery-v1-session",
        "ims-v1-metadata-selection",
        "ims-v1-metadata-generation:CARDDEMO-IMS-CORPUS",
    ] {
        for row in store.list_provider_state(namespace, 4096).unwrap() {
            rows.push((namespace.into(), row.key, row.version, row.payload));
        }
    }
    rows
}

async fn exercise(backend: StoreProfile) {
    let corpus = PathBuf::from(env::var_os(CORPUS_ENV).expect("pinned CardDemo corpus required"));
    verify_carddemo_corpus(
        &corpus,
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../conformance/profiles/carddemo/inventory/carddemo-corpus.json"),
    )
    .unwrap();
    let artifact_root = env::temp_dir().join(format!("raw-dli-{}-{backend:?}", std::process::id()));
    fs::create_dir_all(&artifact_root).unwrap();
    let sqlite_url = format!(
        "sqlite://{}?mode=rwc",
        artifact_root.join("state.db").display()
    );
    let config = ServerConfig {
        store_profile: backend,
        sqlite_url: sqlite_url.clone(),
        artifact_root: artifact_root.clone(),
        tls: TlsConfig {
            enabled: false,
            certificate_path: None,
            private_key_reference: None,
        },
        ..ServerConfig::default()
    };
    let store = open_store(backend, &sqlite_url).unwrap();
    let program = default_program_router();
    let server = ProductServer::open_with_package_trust(
        config.clone(),
        store.clone(),
        Arc::new(MemorySecretResolver::default()),
        program.clone(),
        carddemo_package_trust().unwrap(),
    )
    .unwrap();
    let cases = [
        ("GU", "BY REFERENCE", "PCB-A", "AREA", "SSA"),
        ("GHU", "BY REFERENCE", "PCB-B", "AREA", "SSA"),
        ("GN", "BY REFERENCE", "PCB-A", "AREA", "SSA"),
        ("REPL", "BY REFERENCE", "PCB-B", "AREA", "SSA"),
        ("GU", "BY REFERENCE", "PCB-A", "SHORT-AREA", "SSA"),
        ("GU", "BY REFERENCE", "PCB-A", "AREA", "BAD-SSA"),
        ("GU", "BY REFERENCE", "AREA", "AREA", "SSA"),
        ("GUX", "BY CONTENT", "PCB-A", "AREA", "SSA"),
    ];
    let mut definitions = cases
        .iter()
        .enumerate()
        .map(|(n, (function, mode, pcb, area, ssa))| {
            compile_free_batch_definition(
                &format!("RAW{n}"),
                &source(function, mode, pcb, area, ssa),
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    definitions.push(
        compile_free_batch_definition(
            "RAWCOUNT",
            &source("GU", "BY REFERENCE", "PCB-A", "AREA", "SSA")
                .replace("USING BY REFERENCE FUNC", "USING BY REFERENCE COUNT-N FUNC"),
        )
        .unwrap(),
    );
    let package = ims_packages::package(&corpus, 1, &definitions).unwrap();
    let installed = server.install_application_package_v2(&package).unwrap();
    server.publish_application_generation(&installed).unwrap();
    check_selected(
        &server,
        &installed,
        package.sections.ims_metadata.as_ref().unwrap(),
    )
    .unwrap();
    server
        .bootstrap_administrator("IBMUSER", b"TESTPASS")
        .unwrap();
    for (class, name) in [("IMSPSB", "PSBPAUTB"), ("IMSDB", "DBPAUTP0")] {
        server
            .racf_service()
            .define_profile(class, name, "IBMUSER", Some(AccessIntent::Control))
            .unwrap();
    }
    let control = ims_invocation("raw-selected-control", true, None).unwrap();
    for request in [
        ims_request(
            ImsOperation::Schedule,
            101,
            Some("PSBPAUTB"),
            &[],
            vec![],
            vec![],
            None,
        )
        .unwrap(),
        ims_request(
            ImsOperation::Load,
            102,
            Some("DBPAUTP0"),
            &[],
            serde_json::to_vec(&ImsLoadImage {
                database: "DBPAUTP0".into(),
                roots: vec![ImsLoadRoot {
                    data: ims_record(100, b"000001", b"ROOT-ONE").unwrap(),
                    children: vec![],
                }],
            })
            .unwrap(),
            vec![],
            None,
        )
        .unwrap(),
        ims_request(
            ImsOperation::Checkpoint,
            103,
            None,
            &[],
            vec![],
            vec![],
            Some("RAWSEED1"),
        )
        .unwrap(),
        ims_request(
            ImsOperation::GetHoldUnique,
            104,
            None,
            &["PAUTSUM0"],
            vec![],
            vec![ims_qualifier("PAUTSUM0", "ACCNTID", b"000001")],
            None,
        )
        .unwrap(),
    ] {
        assert_eq!(
            server
                .ims_execute_selected(ims_packages::APPLICATION, &control, &request)
                .unwrap()
                .status,
            "  "
        );
    }
    // Both the signed metadata authority and a real held occurrence are present.
    // They do not grant this guest-owned mask a raw PCB identity.
    let before = provider_bytes(store.as_ref());
    assert!(
        before
            .iter()
            .any(|(namespace, _, _, _)| namespace == "ims-v1-generic-database")
    );
    assert!(
        before
            .iter()
            .any(|(namespace, _, _, _)| namespace == "ims-v1-session-index")
    );
    let providers: Vec<Arc<dyn HostProvider>> = vec![program.clone()];
    let host = Arc::new(ScopedHostService::new(
        Arc::new(RegistrySnapshot::new(1, providers, InvocationLimits::default()).unwrap()),
        mainframe_env_host_api::HostLimits::default(),
    ));
    for (n, definition) in definitions
        .iter()
        .chain(std::iter::once(&definitions[0]))
        .enumerate()
    {
        let granted = n < definitions.len();
        let entry = package
            .base
            .manifest
            .entries
            .iter()
            .find(|entry| {
                entry.kind == EntryKind::Program
                    && package.base.blobs[&entry.sha256] == definition.payload
            })
            .unwrap();
        assert_eq!(
            entry.sha256,
            format!("sha256:{:x}", Sha256::digest(&definition.payload))
        );
        let invocation = invocation(&format!("raw-signed-route-{n}"), definition, granted);
        let mut machine = ReferenceMachine::from_binary(
            &package.base.blobs[&entry.sha256],
            invocation.clone(),
            mainframe_env_ir::CodecLimits::default(),
        )
        .unwrap();
        let mut initialized = ReferenceMachine::from_binary(
            &package.base.blobs[&entry.sha256],
            invocation.clone(),
            mainframe_env_ir::CodecLimits::default(),
        )
        .unwrap();
        assert!(matches!(
            initialized.drive(
                MachineResume::Start,
                Quantum {
                    max_steps: 100,
                    max_allocated_bytes: 1_048_576
                }
            ),
            MachineDrive::HostCall(_)
        ));
        let guest_before = initialized.snapshot().base_storage;
        let outcome = ExecutionCoordinator::durable(
            host.clone(),
            store.clone(),
            CoordinatorLimits::default(),
        )
        .execute(
            &mut machine,
            &invocation,
            ExecutionControl {
                now_tick: 1,
                cancellation_requested: false,
            },
        );
        let ExecutionOutcome::Completed(completion) = outcome else {
            panic!("{outcome:?}")
        };
        let expected = [
            b"RAW-REJECTED\n".to_vec(),
            vec![b'A'; 46],
            b"\n".to_vec(),
            vec![b'B'; 46],
            b"\n".to_vec(),
            vec![b'Z'; 100],
            b"\n".to_vec(),
        ]
        .concat();
        assert_eq!(completion.output.bytes(), expected, "case {n}");
        assert_eq!(machine.snapshot().base_storage, guest_before, "case {n}");
        assert_eq!(provider_bytes(store.as_ref()), before, "case {n}");
        let audits = store
            .audit_records(&invocation.execution_id, 0, 16)
            .unwrap();
        assert_eq!(audits.len(), 1);
        assert_ne!(audits[0].decision, AuditDecision::Success);
        let key = IdempotencyKey::new(
            format!("{}:1", invocation.idempotency_key.as_str()),
            InvocationLimits::default(),
        )
        .unwrap();
        let effect = store.effect(&key).unwrap().unwrap();
        let problem = if granted {
            HostProblem::Condition {
                name: "PROGRAM-NOTFOUND:CBLTDLI".into(),
                response: -7,
                response2: 0,
            }
        } else {
            HostProblem::Unauthorized
        };
        assert_eq!(
            effect.result_digest,
            Some(mainframe_env_host_api::canonical_result_digest(&Err(problem)).unwrap())
        );
    }
    assert!(server.graceful_shutdown().await);
    drop(host);
    drop(server);
    drop(program);
    let store = reopen_store(store, backend, &sqlite_url).unwrap();
    let server = ProductServer::open_with_package_trust(
        config,
        store.clone(),
        Arc::new(MemorySecretResolver::default()),
        default_program_router(),
        carddemo_package_trust().unwrap(),
    )
    .unwrap();
    check_selected(
        &server,
        &installed,
        package.sections.ims_metadata.as_ref().unwrap(),
    )
    .unwrap();
    assert_eq!(provider_bytes(store.as_ref()), before);
    assert!(server.graceful_shutdown().await);
    drop(server);
    drop(store);
    fs::remove_dir_all(artifact_root).unwrap();
}

#[tokio::test]
async fn raw_dli_signed_memory_gap_witness() {
    exercise(StoreProfile::Memory).await;
}

#[tokio::test]
async fn raw_dli_signed_sqlite_gap_witness() {
    exercise(StoreProfile::Sqlite).await;
}
