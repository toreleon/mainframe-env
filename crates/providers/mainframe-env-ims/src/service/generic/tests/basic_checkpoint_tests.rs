use super::*;
use std::sync::atomic::AtomicBool;

fn seed_pending(service: &ImsService, run: &str) {
    service.install_metadata(catalog()).unwrap();
    assert_eq!(
        execute(
            service,
            run,
            &request(run, ImsOperation::Schedule, 1, &[], b"")
        )
        .status,
        "  "
    );
    assert_eq!(
        execute(
            service,
            run,
            &request(run, ImsOperation::Insert, 2, &["ROOT"], b"R1A")
        )
        .status,
        "  "
    );
    assert_eq!(
        execute(
            service,
            run,
            &request(run, ImsOperation::GetHoldUnique, 3, &["ROOT"], b"")
        )
        .segments[0]
            .data,
        b"R1A"
    );
    let durable = service.lock().unwrap();
    assert!(durable.state.generic_pending_undo.contains_key(run));
    assert!(durable.state.sessions[run].position.is_held());
}

fn check_boundary(
    store: Arc<dyn ProviderStateStore>,
    reopen: impl FnOnce(Arc<dyn ProviderStateStore>) -> Arc<dyn ProviderStateStore>,
) {
    let run = "basic-checkpoint";
    let service = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
    seed_pending(&service, run);
    let checkpoint = request(run, ImsOperation::Checkpoint, 4, &[], b"");
    let result = execute(&service, run, &checkpoint);
    assert_eq!(result.status, "  ");
    assert_eq!(result.checkpoint_id, checkpoint.checkpoint_id);
    {
        let durable = service.lock().unwrap();
        assert!(!durable.state.generic_pending_undo.contains_key(run));
        assert_eq!(durable.state.sessions[run].position, PcbPosition::default());
        assert_eq!(
            durable.state.checkpoints[checkpoint.checkpoint_id.as_deref().unwrap()].position,
            PcbPosition::default()
        );
    }
    drop(service);
    let reopened = ImsService::open(reopen(store), ImsLimits::default()).unwrap();
    assert_eq!(
        reopened.lock().unwrap().state.sessions[run].position,
        PcbPosition::default()
    );
    assert_eq!(
        execute(
            &reopened,
            run,
            &request(run, ImsOperation::Replace, 5, &[], b"R1Z")
        )
        .status,
        "DJ",
        "checkpoint invalidates the prior hold"
    );
    assert_eq!(
        execute(
            &reopened,
            run,
            &request(run, ImsOperation::Insert, 6, &["ROOT"], b"R2B")
        )
        .status,
        "  "
    );
    assert_eq!(execute(&reopened, run, &checkpoint), result);
    assert!(
        reopened
            .lock()
            .unwrap()
            .state
            .generic_pending_undo
            .contains_key(run)
    );
    execute(
        &reopened,
        run,
        &request(run, ImsOperation::Rollback, 7, &[], b""),
    );
    let mut get = request(run, ImsOperation::GetUnique, 8, &["ROOT"], b"");
    get.qualifiers.push(qualifier(b"R1"));
    assert_eq!(execute(&reopened, run, &get).segments[0].data, b"R1A");
    get.mutation.as_mut().unwrap().sequence = 9;
    get.mutation.as_mut().unwrap().idempotency_key =
        IdempotencyKey::new(format!("{run}-9"), InvocationLimits::default()).unwrap();
    get.qualifiers[0].value = b"R2".to_vec();
    assert_eq!(execute(&reopened, run, &get).status, "GE");
}

#[test]
fn basic_checkpoint_commits_loses_position_and_replays_across_memory_reopen() {
    check_boundary(
        Arc::new(MemoryStore::new(Default::default())),
        std::convert::identity,
    );
}

#[test]
fn basic_checkpoint_commits_loses_position_and_replays_across_sqlite_reopen() {
    let file = std::env::temp_dir().join(format!(
        "ims-basic-checkpoint-{}-{}.sqlite",
        std::process::id(),
        NEXT_FILE.fetch_add(1, Ordering::Relaxed)
    ));
    let url = format!("sqlite://{}?mode=rwc", file.display());
    check_boundary(
        Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap()),
        |store| {
            drop(store);
            Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap())
        },
    );
    std::fs::remove_file(file).unwrap();
}

#[test]
fn basic_checkpoint_missing_id_or_capacity_preserves_undo_position_and_replay() {
    let store = Arc::new(MemoryStore::new(Default::default()));
    let service = ImsService::open(
        store,
        ImsLimits {
            max_checkpoints: 1,
            ..Default::default()
        },
    )
    .unwrap();
    let run = "checkpoint-capacity";
    seed_pending(&service, run);
    let mut missing = request(run, ImsOperation::Checkpoint, 4, &[], b"");
    missing.checkpoint_id = None;
    let before = service.lock().unwrap().state.clone();
    assert_eq!(
        service.execute(&invocation(run), &missing),
        Err(HostProblem::Malformed)
    );
    assert_eq!(service.lock().unwrap().state, before);
    execute(
        &service,
        run,
        &request(run, ImsOperation::Checkpoint, 5, &[], b""),
    );
    execute(
        &service,
        run,
        &request(run, ImsOperation::Insert, 6, &["ROOT"], b"R2B"),
    );
    execute(
        &service,
        run,
        &request(run, ImsOperation::GetHoldUnique, 7, &["ROOT"], b""),
    );
    let before = service.lock().unwrap().state.clone();
    let mut full = request(run, ImsOperation::Checkpoint, 8, &[], b"");
    full.checkpoint_id = Some("another-checkpoint".into());
    assert_eq!(
        service.execute(&invocation(run), &full),
        Err(HostProblem::ResourceExhausted)
    );
    assert_eq!(service.lock().unwrap().state, before);
}

#[derive(Default)]
struct CheckpointPolicy {
    deny_alternate: AtomicBool,
}

impl EnterpriseAuthorizer for CheckpointPolicy {
    fn authorize(&self, _: &PrincipalId, resource: &EnterpriseResource) -> Result<(), HostProblem> {
        if self.deny_alternate.load(Ordering::SeqCst)
            && resource.class == EnterpriseResourceClass::ImsDatabase
            && resource.name.as_str() == "ALTDB"
        {
            Err(HostProblem::Unauthorized)
        } else {
            Ok(())
        }
    }
}

#[test]
fn basic_checkpoint_authorizes_all_pending_databases_before_commit() {
    let policy = Arc::new(CheckpointPolicy::default());
    let store = Arc::new(MemoryStore::new(Default::default()));
    let service = ImsService::open_authorized(store, ImsLimits::default(), policy.clone()).unwrap();
    let mut metadata = catalog();
    let mut alternate = metadata.databases[0].clone();
    alternate.name = "ALTDB".into();
    metadata.databases.push(alternate);
    service.install_metadata(metadata).unwrap();
    let run = "checkpoint-authority";
    execute(
        &service,
        run,
        &request(run, ImsOperation::Schedule, 1, &[], b""),
    );
    let image = ImsGenericLoadImage {
        database: "ALTDB".into(),
        records: vec![ImsGenericLoadRecord {
            segment: "ROOT".into(),
            parent: None,
            data: b"R1A".to_vec(),
        }],
    };
    execute(
        &service,
        run,
        &request(
            run,
            ImsOperation::Load,
            2,
            &[],
            &serde_json::to_vec(&image).unwrap(),
        ),
    );
    assert!(service.lock().unwrap().state.generic_pending_undo[run].contains_key("ALTDB"));
    policy.deny_alternate.store(true, Ordering::SeqCst);
    let before = service.lock().unwrap().state.clone();
    let checkpoint = request(run, ImsOperation::Checkpoint, 3, &[], b"");
    assert_eq!(
        service.execute(&invocation(run), &checkpoint),
        Err(HostProblem::Unauthorized)
    );
    assert_eq!(service.lock().unwrap().state, before);
    policy.deny_alternate.store(false, Ordering::SeqCst);
    assert_eq!(execute(&service, run, &checkpoint).status, "  ");
    assert!(
        !service
            .lock()
            .unwrap()
            .state
            .generic_pending_undo
            .contains_key(run)
    );
}
