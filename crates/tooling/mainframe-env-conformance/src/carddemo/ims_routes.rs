//! Exact selected-package CardDemo IMS integration exercise.
use super::*;
use mainframe_env_ims::database::{DatabaseEngine, DatabaseEngineImage, EngineLimits};
use mainframe_env_store_api::PlatformStore;

fn drift(detail: impl Into<String>) -> CorpusProblem {
    CorpusProblem::new("carddemo.ims.package_route_drift", detail)
}

fn open_store(backend: StoreProfile, url: &str) -> Result<Arc<dyn PlatformStore>, CorpusProblem> {
    match backend {
        StoreProfile::Memory => Ok(Arc::new(MemoryStore::new(Default::default()))),
        StoreProfile::Sqlite => Ok(Arc::new(
            SqliteStateStore::open(url, 64 * 1024 * 1024, 262_144).map_err(package_problem)?,
        )),
        _ => Err(drift("unsupported IMS profile backend")),
    }
}

fn reopen_store(
    store: Arc<dyn PlatformStore>,
    backend: StoreProfile,
    url: &str,
) -> Result<Arc<dyn PlatformStore>, CorpusProblem> {
    if backend == StoreProfile::Memory {
        return Ok(store);
    }
    drop(store);
    open_store(backend, url)
}

fn check_selected(
    server: &ProductServer,
    installed: &mainframe_env_application::ApplicationGenerationRecord,
    expected: &mainframe_env_host_api::ImsMetadataCatalog,
) -> Result<(), CorpusProblem> {
    let metadata = server
        .ims_service()
        .selected_metadata_generation(ims_packages::APPLICATION)
        .map_err(terminal_problem)?
        .ok_or_else(|| drift("selected IMS metadata missing"))?;
    if metadata.generation != installed.generation
        || metadata.package_identity != installed.identity
        || &metadata.catalog != expected
    {
        return Err(drift("package and metadata selections differ"));
    }
    Ok(())
}

struct SelectedIms<'a> {
    server: &'a ProductServer,
    store: Arc<dyn PlatformStore>,
}

impl SelectedIms<'_> {
    fn execute(
        &self,
        invocation: &Invocation,
        request: &ImsRequest,
    ) -> Result<mainframe_env_host_api::ImsResult, HostProblem> {
        self.server
            .ims_execute_selected(ims_packages::APPLICATION, invocation, request)
    }

    fn hierarchy(&self, database: &str) -> Result<Vec<ImsLoadRoot>, HostProblem> {
        let invocation =
            ims_invocation("ims-observe", true, None).map_err(|_| HostProblem::Malformed)?;
        let request = ims_request(
            ImsOperation::Unload,
            4001,
            Some(database),
            &[],
            Vec::new(),
            Vec::new(),
            None,
        )
        .map_err(|_| HostProblem::Malformed)?;
        let result = self.execute(&invocation, &request)?;
        if result.status != "  " {
            return Err(HostProblem::ProviderFailure);
        }
        let mut roots: Vec<ImsLoadRoot> = Vec::new();
        for segment in result.segments {
            match segment.name.as_str() {
                "PAUTSUM0" if segment.parent_key.is_none() => roots.push(ImsLoadRoot {
                    data: segment.data,
                    children: Vec::new(),
                }),
                "PAUTDTL1" => {
                    let parent = roots.last_mut().ok_or(HostProblem::ProviderFailure)?;
                    if segment.parent_key.as_deref() != Some(&parent.data[..6]) {
                        return Err(HostProblem::ProviderFailure);
                    }
                    parent.children.push(segment.data);
                }
                _ => return Err(HostProblem::ProviderFailure),
            }
        }
        Ok(roots)
    }

    fn secondary_index_entries(&self, database: &str) -> Result<usize, HostProblem> {
        // Inspect the actual generic object row with the provider's strict image
        // reader. Rebuilding validates retained records and index authority.
        if self
            .store
            .get_provider_state("ims-v1-database", database)
            .map_err(|_| HostProblem::InfrastructureFailure)?
            .is_some()
        {
            return Err(HostProblem::ProviderFailure);
        }
        let row = self
            .store
            .get_provider_state("ims-v1-generic-database", database)
            .map_err(|_| HostProblem::InfrastructureFailure)?
            .ok_or(HostProblem::NotFound)?;
        let envelope: serde_json::Value =
            serde_json::from_slice(&row.payload).map_err(|_| HostProblem::InfrastructureFailure)?;
        let image: DatabaseEngineImage = serde_json::from_value(envelope["value"].clone())
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let engine = DatabaseEngine::restore(image, EngineLimits::default())
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let indexes = &engine.definition().secondary_indexes;
        if indexes.len() != 1
            || indexes[0].name != "DBPAUTX0"
            || indexes[0].source_segment != "PAUTSUM0"
            || indexes[0].field != "ACCNTID"
        {
            return Err(HostProblem::ProviderFailure);
        }
        let mut count = 0;
        for key in [b"000001", b"000002"] {
            let found = engine
                .lookup_index("DBPAUTX0", key)
                .map_err(|_| HostProblem::ProviderFailure)?;
            if found.len() != 1 || found[0].segment != "PAUTSUM0" || &found[0].data[..6] != key {
                return Err(HostProblem::ProviderFailure);
            }
            count += found.len();
        }
        Ok(count)
    }
}

fn replay_insert() -> Result<ImsRequest, CorpusProblem> {
    ims_request(
        ImsOperation::Insert,
        3002,
        None,
        &["PAUTSUM0"],
        ims_record(100, b"999999", b"REPLAY-ROLLBACK")?,
        Vec::new(),
        None,
    )
}

fn exercise_replay_rollback(
    ims: &SelectedIms<'_>,
    _control: &Invocation,
    expected: &[ImsLoadRoot],
) -> Result<(), CorpusProblem> {
    let mut invocation = ims_invocation("ims-replay", true, None)?;
    invocation.service_class = ServiceClass::Interactive;
    let request = ims_request(
        ImsOperation::Schedule,
        3001,
        Some("PSBPAUTB"),
        &[],
        Vec::new(),
        Vec::new(),
        None,
    )?;
    if ims
        .execute(&invocation, &request)
        .map_err(terminal_problem)?
        .status
        != "  "
    {
        return Err(drift("replay schedule status differs"));
    }
    let insert = replay_insert()?;
    let first = ims
        .execute(&invocation, &insert)
        .map_err(terminal_problem)?;
    if first.status != "  "
        || first.affected_segments != 1
        || ims
            .execute(&invocation, &insert)
            .map_err(terminal_problem)?
            != first
    {
        return Err(drift("insert replay differs"));
    }
    let mut changed = insert;
    changed.data[6] = b'X';
    if ims.execute(&invocation, &changed) != Err(HostProblem::IdempotencyConflict)
        || ims.hierarchy("DBPAUTP0").map_err(terminal_problem)?.len() != expected.len() + 1
    {
        return Err(drift("conflicting replay changed the image"));
    }
    let rollback = ims_request(
        ImsOperation::Rollback,
        3003,
        None,
        &[],
        Vec::new(),
        Vec::new(),
        None,
    )?;
    let result = ims
        .execute(&invocation, &rollback)
        .map_err(terminal_problem)?;
    if result.status != "  "
        || ims
            .execute(&invocation, &rollback)
            .map_err(terminal_problem)?
            != result
        || ims.hierarchy("DBPAUTP0").map_err(terminal_problem)? != expected
    {
        return Err(drift("rollback or its replay differs"));
    }
    Ok(())
}

fn verify_reopened_replay(
    ims: &SelectedIms<'_>,
    _control: &Invocation,
) -> Result<(), CorpusProblem> {
    let before = ims.hierarchy("DBPAUTP0").map_err(terminal_problem)?;
    let mut invocation = ims_invocation("ims-replay", true, None)?;
    invocation.service_class = ServiceClass::Interactive;
    let replay = ims
        .execute(&invocation, &replay_insert()?)
        .map_err(terminal_problem)?;
    if replay.status != "  "
        || replay.affected_segments != 1
        || ims.hierarchy("DBPAUTP0").map_err(terminal_problem)? != before
    {
        return Err(drift("reopen redispatched the rolled-back insert"));
    }
    let mut load_invocation = ims_invocation("ims-load-replay", true, None)?;
    load_invocation.service_class = ServiceClass::Interactive;
    let result = ims
        .execute(&load_invocation, &replay_load(&before)?)
        .map_err(terminal_problem)?;
    if result.status != "  "
        || result.affected_segments != 4
        || ims.hierarchy("DBPAUTP0").map_err(terminal_problem)? != before
    {
        return Err(drift("reopen redispatched the rolled-back two-level load"));
    }
    Ok(())
}

fn replay_load(expected: &[ImsLoadRoot]) -> Result<ImsRequest, CorpusProblem> {
    let mut roots = expected.to_vec();
    roots[0].data[6] = b'Z';
    ims_request(
        ImsOperation::Load,
        5001,
        Some("DBPAUTP0"),
        &[],
        serde_json::to_vec(&ImsLoadImage {
            database: "DBPAUTP0".into(),
            roots,
        })
        .map_err(package_problem)?,
        Vec::new(),
        None,
    )
}

fn exercise_load_failure_replay(
    ims: &SelectedIms<'_>,
    expected: &[ImsLoadRoot],
) -> Result<(), CorpusProblem> {
    let mut invocation = ims_invocation("ims-load-replay", true, None)?;
    invocation.service_class = ServiceClass::Interactive;
    let mut malformed = replay_load(expected)?;
    malformed
        .mutation
        .as_mut()
        .expect("load mutation")
        .idempotency_key =
        IdempotencyKey::new("carddemo-malformed-bulk-image", InvocationLimits::default())
            .map_err(package_problem)?;
    let mut image = ImsLoadImage {
        database: "DBPAUTP0".into(),
        roots: expected.to_vec(),
    };
    image.roots[0].data.pop();
    malformed.data = serde_json::to_vec(&image).map_err(package_problem)?;
    if ims.execute(&invocation, &malformed) != Err(HostProblem::Malformed)
        || ims.hierarchy("DBPAUTP0").map_err(terminal_problem)? != expected
    {
        return Err(drift("malformed two-level load changed committed bytes"));
    }
    let load = replay_load(expected)?;
    let result = ims.execute(&invocation, &load).map_err(terminal_problem)?;
    let mut loaded = expected.to_vec();
    loaded[0].data[6] = b'Z';
    if result.status != "  "
        || result.affected_segments != 4
        || ims.execute(&invocation, &load).map_err(terminal_problem)? != result
        || ims.hierarchy("DBPAUTP0").map_err(terminal_problem)? != loaded
    {
        return Err(drift("two-level load or exact replay differs"));
    }
    let mut conflict = load;
    conflict.data = serde_json::to_vec(&ImsLoadImage {
        database: "DBPAUTP0".into(),
        roots: expected.to_vec(),
    })
    .map_err(package_problem)?;
    if ims.execute(&invocation, &conflict) != Err(HostProblem::IdempotencyConflict)
        || ims.hierarchy("DBPAUTP0").map_err(terminal_problem)? != loaded
    {
        return Err(drift("conflicting bulk load replay mutated bytes"));
    }
    let rollback = ims_request(
        ImsOperation::Rollback,
        5002,
        None,
        &[],
        Vec::new(),
        Vec::new(),
        None,
    )?;
    let rolled = ims
        .execute(&invocation, &rollback)
        .map_err(terminal_problem)?;
    if rolled.status != "  "
        || ims
            .execute(&invocation, &rollback)
            .map_err(terminal_problem)?
            != rolled
        || ims.hierarchy("DBPAUTP0").map_err(terminal_problem)? != expected
    {
        return Err(drift("two-level load rollback or replay differs"));
    }
    Ok(())
}

#[derive(Debug, Eq, PartialEq)]
pub(super) struct ImsExercise {
    pub(super) databases_installed: usize,
    pub(super) psbs_installed: usize,
    pub(super) pcbs_installed: usize,
    pub(super) roots: usize,
    pub(super) children: usize,
    pub(super) secondary_index_entries: usize,
    pub(super) hierarchy_sha256: String,
    pub(super) selected_job_routes: usize,
    pub(super) spool_sha256: BTreeMap<String, String>,
}

pub(super) async fn exercise_ims_routes(
    corpus_dir: &Path,
    backend: StoreProfile,
    programs: &[BatchProgramDefinition],
) -> Result<ImsExercise, CorpusProblem> {
    exercise_ims_routes_at(corpus_dir, backend, programs, None).await
}

async fn exercise_ims_routes_at(
    corpus_dir: &Path,
    backend: StoreProfile,
    programs: &[BatchProgramDefinition],
    retained_root: Option<&Path>,
) -> Result<ImsExercise, CorpusProblem> {
    let artifact_root = retained_root.map(Path::to_path_buf).unwrap_or_else(|| {
        env::temp_dir().join(format!(
            "mainframe-env-carddemo-ims-{}-{backend:?}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("host clock after epoch")
                .as_nanos()
        ))
    });
    fs::create_dir_all(&artifact_root).map_err(package_problem)?;
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
    let store = open_store(backend, &sqlite_url)?;
    let server = ProductServer::open_with_package_trust(
        config.clone(),
        store.clone(),
        Arc::new(MemorySecretResolver::default()),
        default_program_router(),
        carddemo_package_trust()?,
    )
    .map_err(terminal_problem)?;
    let package = ims_packages::package(corpus_dir, 1, programs)?;
    let catalog = package
        .sections
        .ims_metadata
        .clone()
        .expect("fixture metadata");
    let mut invalid = package.clone();
    invalid.signature.value = "invalid".into();
    if server.install_application_package_v2(&invalid) != Err(HostProblem::Malformed) {
        return Err(drift("invalid signature did not fail closed"));
    }
    let control = ims_invocation("ims-control", true, None)?;
    let schedule = ims_request(
        ImsOperation::Schedule,
        101,
        Some("PSBPAUTB"),
        &[],
        Vec::new(),
        Vec::new(),
        None,
    )?;
    if server.ims_execute_selected(ims_packages::APPLICATION, &control, &schedule)
        != Err(HostProblem::NotFound)
    {
        return Err(drift("unselected package executed"));
    }
    let installed = server
        .install_application_package_v2(&package)
        .map_err(terminal_problem)?;
    if server
        .install_application_package_v2(&package)
        .map_err(terminal_problem)?
        != installed
    {
        return Err(drift("signed package installation replay differs"));
    }
    server
        .publish_application_generation(&installed)
        .map_err(terminal_problem)?;
    if !server
        .publish_application_generation(&installed)
        .map_err(terminal_problem)?
        .replayed
    {
        return Err(drift("signed package publication did not replay"));
    }
    let ims = SelectedIms {
        server: &server,
        store: store.clone(),
    };
    let root_one = ims_record(100, b"000001", b"ROOT-ONE")?;
    let root_two = ims_record(100, b"000002", b"ROOT-TWO")?;
    let child_one = ims_record(200, b"99900001", b"CHILD-ONE")?;
    let child_two = ims_record(200, b"99900002", b"CHILD-TWO")?;
    let image = ImsLoadImage {
        database: "DBPAUTP0".into(),
        roots: vec![
            ImsLoadRoot {
                data: root_one.clone(),
                children: vec![child_one.clone()],
            },
            ImsLoadRoot {
                data: root_two.clone(),
                children: vec![child_two.clone()],
            },
        ],
    };
    server
        .bootstrap_user("IBMUSER", b"TESTPASS")
        .map_err(terminal_problem)?;
    let racf = server.racf_service();
    racf.define_profile("DATASET", "AWS.M2.CARDDEMO.**", "IBMUSER", None)
        .map_err(terminal_problem)?;
    racf.permit(
        "DATASET",
        "AWS.M2.CARDDEMO.**",
        "IBMUSER",
        AccessIntent::Alter,
    )
    .map_err(terminal_problem)?;
    let mut sequence = 70_000u64;
    utility_seed_dataset(
        &server,
        "AWS.M2.CARDDEMO.PAUTDB.ROOT.FILEO",
        DatasetOrganization::Sequential,
        RecordFormat::Fixed,
        100,
        None,
        vec![root_one.clone(), root_two.clone()],
        &mut sequence,
    )?;
    let child_records: Vec<Vec<u8>> = [
        (b"000001".as_slice(), &child_one),
        (b"000002".as_slice(), &child_two),
    ]
    .into_iter()
    .map(|(parent, child)| {
        let mut record = parent.to_vec();
        record.extend_from_slice(child);
        record
    })
    .collect();
    utility_seed_dataset(
        &server,
        "AWS.M2.CARDDEMO.PAUTDB.CHILD.FILEO",
        DatasetOrganization::Sequential,
        RecordFormat::Fixed,
        206,
        None,
        child_records.clone(),
        &mut sequence,
    )?;
    for (class, name) in [("IMSPSB", "PSBPAUTB"), ("IMSDB", "DBPAUTP0")] {
        racf.define_profile(class, name, "IBMUSER", Some(AccessIntent::Control))
            .map_err(terminal_problem)?;
    }
    // Activate only the verified selected catalog before JES dispatches through
    // the existing host provider. No legacy definitions are installed.
    let scheduled = ims.execute(&control, &schedule).map_err(terminal_problem)?;
    if scheduled.status != "  " {
        return Err(drift("selected schedule status differs"));
    }
    let app = server.router();
    let load_jcl = String::from_utf8(read_corpus_file(
        corpus_dir,
        &corpus_dir.join("app/app-authorization-ims-db2-mq/jcl/LOADPADB.JCL"),
    )?)
    .map_err(|_| CorpusProblem::new("carddemo.ims.jcl_invalid", "LOADPADB is not UTF-8"))?;
    let load_job = submit_job_with_retcode(&server, &app, &load_jcl, "CC 0000").await?;
    ims.execute(
        &control,
        &ims_request(
            ImsOperation::Schedule,
            101,
            Some("PSBPAUTB"),
            &[],
            Vec::new(),
            Vec::new(),
            None,
        )?,
    )
    .map_err(terminal_problem)?;
    let root = ims
        .execute(
            &control,
            &ims_request(
                ImsOperation::GetUnique,
                2,
                None,
                &["PAUTSUM0"],
                Vec::new(),
                vec![ims_qualifier("PAUTSUM0", "ACCNTID", b"000001")],
                None,
            )?,
        )
        .map_err(terminal_problem)?;
    if root.status != "  "
        || root
            .segments
            .first()
            .is_none_or(|segment| segment.data != root_one)
    {
        return Err(CorpusProblem::new(
            "carddemo.ims.gu_drift",
            "GU did not return the qualified root segment",
        ));
    }
    let child = ims
        .execute(
            &control,
            &ims_request(
                ImsOperation::GetNextParent,
                4,
                None,
                &["PAUTDTL1"],
                Vec::new(),
                Vec::new(),
                None,
            )?,
        )
        .map_err(terminal_problem)?;
    if child
        .segments
        .first()
        .is_none_or(|segment| segment.data != child_one)
    {
        return Err(CorpusProblem::new(
            "carddemo.ims.gnp_drift",
            "GNP did not preserve root/child hierarchy",
        ));
    }
    let held = ims
        .execute(
            &control,
            &ims_request(
                ImsOperation::GetHoldUnique,
                104,
                None,
                &["PAUTSUM0", "PAUTDTL1"],
                Vec::new(),
                vec![
                    ims_qualifier("PAUTSUM0", "ACCNTID", b"000001"),
                    ims_qualifier("PAUTDTL1", "PAUT9CTS", b"99900001"),
                ],
                None,
            )?,
        )
        .map_err(terminal_problem)?;
    if held.status != "  " || held.segments.len() != 1 || held.segments[0].data != child_one {
        return Err(drift("hold did not return the exact child"));
    }
    let mut replaced_child = child_one.clone();
    replaced_child[8..12].copy_from_slice(b"EDIT");
    ims.execute(
        &control,
        &ims_request(
            ImsOperation::Replace,
            5,
            None,
            &["PAUTDTL1"],
            replaced_child.clone(),
            Vec::new(),
            None,
        )?,
    )
    .map_err(terminal_problem)?;
    let transient_child = ims_record(200, b"99900003", b"TRANSIENT")?;
    ims.execute(
        &control,
        &ims_request(
            ImsOperation::Insert,
            6,
            None,
            &["PAUTSUM0", "PAUTDTL1"],
            transient_child,
            vec![ims_qualifier("PAUTSUM0", "ACCNTID", b"000001")],
            None,
        )?,
    )
    .map_err(terminal_problem)?;
    ims.execute(
        &control,
        &ims_request(
            ImsOperation::GetHoldUnique,
            7,
            None,
            &["PAUTSUM0", "PAUTDTL1"],
            Vec::new(),
            vec![
                ims_qualifier("PAUTSUM0", "ACCNTID", b"000001"),
                ims_qualifier("PAUTDTL1", "PAUT9CTS", b"99900003"),
            ],
            None,
        )?,
    )
    .map_err(terminal_problem)?;
    ims.execute(
        &control,
        &ims_request(
            ImsOperation::Delete,
            8,
            None,
            &["PAUTDTL1"],
            Vec::new(),
            Vec::new(),
            None,
        )?,
    )
    .map_err(terminal_problem)?;
    let next = ims
        .execute(
            &control,
            &ims_request(
                ImsOperation::GetNext,
                9,
                None,
                &["PAUTSUM0"],
                Vec::new(),
                Vec::new(),
                None,
            )?,
        )
        .map_err(terminal_problem)?;
    if next.status != "  " || next.segments.len() != 1 || next.segments[0].data != root_two {
        return Err(CorpusProblem::new(
            "carddemo.ims.gn_drift",
            "GN did not return a root",
        ));
    }
    ims.execute(
        &control,
        &ims_request(
            ImsOperation::Checkpoint,
            10,
            None,
            &[],
            Vec::new(),
            Vec::new(),
            Some("CD025001"),
        )?,
    )
    .map_err(terminal_problem)?;
    ims.execute(
        &control,
        &ims_request(
            ImsOperation::Terminate,
            11,
            None,
            &[],
            Vec::new(),
            Vec::new(),
            None,
        )?,
    )
    .map_err(terminal_problem)?;
    let unload_jcl = String::from_utf8(read_corpus_file(
        corpus_dir,
        &corpus_dir.join("app/app-authorization-ims-db2-mq/jcl/UNLDPADB.JCL"),
    )?)
    .map_err(|_| CorpusProblem::new("carddemo.ims.jcl_invalid", "UNLDPADB is not UTF-8"))?;
    let unload_job = submit_job_with_retcode(&server, &app, &unload_jcl, "CC 0000").await?;
    let unloaded_roots = utility_records(&server, "AWS.M2.CARDDEMO.PAUTDB.ROOT.FILEO", None)?;
    let unloaded_children = utility_records(&server, "AWS.M2.CARDDEMO.PAUTDB.CHILD.FILEO", None)?;
    let mut expected_children = child_records.clone();
    expected_children[0][6..].copy_from_slice(&replaced_child);
    if unloaded_roots != vec![root_one.clone(), root_two.clone()]
        || unloaded_children != expected_children
    {
        return Err(CorpusProblem::new(
            "carddemo.ims.unload_drift",
            "IMS unload job did not write two roots and two qualified children",
        ));
    }

    let route_source = "IDENTIFICATION DIVISION. PROGRAM-ID. IMSROUTE. DATA DIVISION. WORKING-STORAGE SECTION. 01 PSB-NAME PIC X(8) VALUE 'PSBPAUTB'. 01 PCB-N PIC S9(4) COMP VALUE 1. 01 ROOT-X PIC X(100). 01 ACCT-X PIC X(6) VALUE '000001'. PROCEDURE DIVISION. EXEC DLI SCHD PSB((PSB-NAME)) END-EXEC. EXEC DLI GU USING PCB(PCB-N) SEGMENT(PAUTSUM0) INTO(ROOT-X) WHERE(ACCNTID = ACCT-X) END-EXEC. DISPLAY ROOT-X. EXEC DLI TERM END-EXEC. STOP RUN.";
    let artifact = crate::compile(route_source).map_err(|error| {
        CorpusProblem::new("carddemo.ims.route_compile", format!("IMS route: {error}"))
    })?;
    let invocation = ims_artifact_invocation("ims-route", &artifact, true, None)?;
    let host = Arc::new(ScopedHostService::new(
        Arc::new(
            RegistrySnapshot::new(
                1,
                ims_providers(server.ims_service(), InvocationLimits::default()),
                InvocationLimits::default(),
            )
            .map_err(|_| CorpusProblem::new("carddemo.ims.registry", "registry invalid"))?,
        ),
        mainframe_env_host_api::HostLimits::default(),
    ));
    let mut machine = mainframe_env_interpreter::ReferenceMachine::from_binary(
        artifact.payload(),
        invocation.clone(),
        mainframe_env_ir::CodecLimits::default(),
    )
    .map_err(|problem| CorpusProblem::new("carddemo.ims.route", format!("{problem:?}")))?;
    let coordinator = mainframe_env_interpreter::ExecutionCoordinator::with_host(
        host.clone(),
        Arc::new(MemoryStore::new(Default::default())),
        mainframe_env_interpreter::CoordinatorLimits::default(),
    );
    let outcome = coordinator.execute(
        &mut machine,
        &invocation,
        mainframe_env_interpreter::ExecutionControl::default(),
    );
    if !matches!(
        outcome,
        mainframe_env_execution_api::ExecutionOutcome::Completed(ref completion)
            if completion.output.bytes() == [root_one.clone(), b"\n".to_vec()].concat()
    ) {
        return Err(CorpusProblem::new(
            "carddemo.ims.route_failed",
            format!("typed DLI application route did not complete: {outcome:?}"),
        ));
    }

    let denied_invocation = ims_invocation("ims-denied", false, None)?;
    let denied_request = ims_request(
        ImsOperation::Schedule,
        1,
        Some("PSBPAUTB"),
        &[],
        Vec::new(),
        Vec::new(),
        None,
    )?;
    let denied_effect = EffectRequest {
        run_unit: denied_invocation.run_unit_id.clone(),
        sequence: 1,
        deadline_tick: denied_invocation.deadline_tick,
        idempotency_key: denied_request
            .mutation
            .as_ref()
            .map(|mutation| mutation.idempotency_key.clone()),
        request: mainframe_env_host_api::HostRequest::Ims(denied_request),
    };
    if host
        .invoke(&denied_invocation, 1, false, denied_effect)
        .persist_with(|audit| {
            store
                .record_audit(audit)
                .map_err(|_| HostProblem::InfrastructureFailure)
        })
        .outcome
        != Err(HostProblem::Unauthorized)
    {
        return Err(CorpusProblem::new(
            "carddemo.ims.authorization_drift",
            "missing IMS grant did not fail closed",
        ));
    }
    let mismatch = ims_invocation(
        "ims-generation-mismatch",
        true,
        Some(("host.ims.write", "wrong")),
    )?;
    let mismatch_request = ims_request(
        ImsOperation::Schedule,
        1,
        Some("PSBPAUTB"),
        &[],
        Vec::new(),
        Vec::new(),
        None,
    )?;
    let mismatch_effect = EffectRequest {
        run_unit: mismatch.run_unit_id.clone(),
        sequence: 1,
        deadline_tick: mismatch.deadline_tick,
        idempotency_key: mismatch_request
            .mutation
            .as_ref()
            .map(|mutation| mutation.idempotency_key.clone()),
        request: mainframe_env_host_api::HostRequest::Ims(mismatch_request),
    };
    if host
        .invoke(&mismatch, 1, false, mismatch_effect)
        .persist_with(|audit| {
            store
                .record_audit(audit)
                .map_err(|_| HostProblem::InfrastructureFailure)
        })
        .outcome
        != Err(HostProblem::ProviderFailure)
    {
        return Err(CorpusProblem::new(
            "carddemo.ims.provider_failure_drift",
            "provider generation mismatch did not fail closed",
        ));
    }

    let malformed = ims_invocation("ims-malformed", true, None)?;
    ims.execute(
        &malformed,
        &ims_request(
            ImsOperation::Schedule,
            1,
            Some("PSBPAUTB"),
            &[],
            Vec::new(),
            Vec::new(),
            None,
        )?,
    )
    .map_err(terminal_problem)?;
    let before_malformed = ims.hierarchy("DBPAUTP0").map_err(terminal_problem)?;
    let malformed_result = ims.execute(
        &malformed,
        &ims_request(
            ImsOperation::Insert,
            102,
            None,
            &["PAUTSUM0"],
            vec![0],
            Vec::new(),
            None,
        )?,
    );
    if !matches!(malformed_result, Ok(ref result) if result.status == "AT"
        && result.affected_segments == 0 && result.segments.is_empty())
        || ims.hierarchy("DBPAUTP0").map_err(terminal_problem)? != before_malformed
    {
        return Err(CorpusProblem::new(
            "carddemo.ims.malformed_drift",
            format!("malformed segment length returned {malformed_result:?}"),
        ));
    }

    let limited_store = Arc::new(MemoryStore::new(Default::default()));
    let limited = ImsService::open(
        limited_store,
        ImsLimits {
            max_roots: 1,
            ..ImsLimits::default()
        },
    )
    .map_err(terminal_problem)?;
    limited
        .install_metadata(catalog.clone())
        .map_err(terminal_problem)?;
    let limited_result = limited.execute(
        &ims_invocation("ims-limit", true, None)?,
        &ims_request(
            ImsOperation::Load,
            1,
            None,
            &[],
            serde_json::to_vec(&image)
                .map_err(|error| CorpusProblem::new("carddemo.ims.load", error.to_string()))?,
            Vec::new(),
            None,
        )?,
    );
    if limited_result != Err(HostProblem::ResourceExhausted) {
        return Err(CorpusProblem::new(
            "carddemo.ims.resource_drift",
            "root limit did not fail closed",
        ));
    }

    let hierarchy = ims.hierarchy("DBPAUTP0").map_err(terminal_problem)?;
    if hierarchy
        != vec![
            ImsLoadRoot {
                data: root_one.clone(),
                children: vec![replaced_child.clone()],
            },
            ImsLoadRoot {
                data: root_two.clone(),
                children: vec![child_two.clone()],
            },
        ]
    {
        return Err(drift("post-control hierarchy differs"));
    }
    exercise_replay_rollback(&ims, &control, &hierarchy)?;
    exercise_load_failure_replay(&ims, &hierarchy)?;
    let second = server
        .install_application_package_v2(&ims_packages::package(corpus_dir, 2, programs)?)
        .map_err(terminal_problem)?;
    server
        .publish_application_generation(&second)
        .map_err(terminal_problem)?;
    server
        .rollback_application_generation(&installed)
        .map_err(terminal_problem)?;
    check_selected(&server, &installed, &catalog)?;
    let hierarchy_sha256 = ims_hierarchy_digest(&hierarchy);
    let roots = hierarchy.len();
    let children = hierarchy.iter().map(|root| root.children.len()).sum();
    let secondary_index_entries = ims
        .secondary_index_entries("DBPAUTP0")
        .map_err(terminal_problem)?;
    let spool_sha256 = base_batch_spool_digests(
        &server,
        &BTreeMap::from([
            ("LOADPADB".into(), load_job),
            ("UNLDPADB".into(), unload_job),
        ]),
    )?;
    if !server.graceful_shutdown().await {
        return Err(CorpusProblem::new(
            "carddemo.ims.shutdown_failed",
            "IMS server did not shut down",
        ));
    }
    drop(coordinator);
    drop(host);
    drop(ims);
    drop(app);
    drop(racf);
    drop(server);
    let store = reopen_store(store, backend, &sqlite_url)?;
    let restarted = ProductServer::open_with_package_trust(
        config,
        store.clone(),
        Arc::new(MemorySecretResolver::default()),
        default_program_router(),
        carddemo_package_trust()?,
    )
    .map_err(terminal_problem)?;
    check_selected(&restarted, &installed, &catalog)?;
    let reopened = SelectedIms {
        server: &restarted,
        store,
    };
    if ims_hierarchy_digest(&reopened.hierarchy("DBPAUTP0").map_err(terminal_problem)?)
        != hierarchy_sha256
        || restarted
            .ims_service()
            .checkpoint_count()
            .map_err(terminal_problem)?
            != 1
    {
        return Err(CorpusProblem::new(
            "carddemo.ims.restart_drift",
            "IMS hierarchy or checkpoint changed across restart",
        ));
    }
    verify_reopened_replay(&reopened, &control)?;
    drop(reopened);
    let _ = restarted.graceful_shutdown().await;
    drop(restarted);
    if retained_root.is_none() {
        fs::remove_dir_all(&artifact_root).map_err(package_problem)?;
    }
    Ok(ImsExercise {
        databases_installed: catalog.databases.len(),
        psbs_installed: catalog.psbs.len(),
        pcbs_installed: catalog.psbs.iter().map(|psb| psb.pcbs.len()).sum(),
        roots,
        children,
        secondary_index_entries,
        hierarchy_sha256,
        selected_job_routes: 2,
        spool_sha256,
    })
}

#[cfg(test)]
#[path = "ims_process_tests.rs"]
mod tests;
