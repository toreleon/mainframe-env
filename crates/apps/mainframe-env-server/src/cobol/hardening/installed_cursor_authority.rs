//! Cursor ownership at the installed executor's staged completion boundary.
use super::*;
use mainframe_env_store_api::ProviderStateStore;

const SOURCE: &str = "IDENTIFICATION DIVISION. PROGRAM-ID. CURSOR-LEAF.
ENVIRONMENT DIVISION. INPUT-OUTPUT SECTION. FILE-CONTROL.
SELECT INPUT-FILE ASSIGN TO INPUTDD ORGANIZATION IS SEQUENTIAL.
DATA DIVISION. FILE SECTION. FD INPUT-FILE. 01 INPUT-RECORD PIC X.
LINKAGE SECTION. 01 MODE-X PIC X. 01 OUT-X PIC X.
PROCEDURE DIVISION USING MODE-X OUT-X.
IF MODE-X = 'C' CLOSE INPUT-FILE GOBACK END-IF.
IF MODE-X = 'O' OPEN INPUT INPUT-FILE END-IF.
READ INPUT-FILE INTO OUT-X. GOBACK.";
const CURSOR_KEY: &str = "parent-run:CURSOR-LEAF";

// Stateless reads make the restored cursor observable, independent of any
// provider's own browse-position persistence. Each OPEN issues a new identity.
struct CursorProvider {
    descriptor: CapabilityDescriptor,
    opened: AtomicU64,
    read: AtomicU64,
    closed: AtomicU64,
}
impl CursorProvider {
    fn new() -> Self {
        Self {
            descriptor: CapabilityDescriptor {
                capability: CapabilityId::new("host.dataset.read", InvocationLimits::default())
                    .unwrap(),
                provider_id: "cursor-authority-test".into(),
                generation: "1".into(),
                request_schema: DATASET_REQUEST_CONTRACT.into(),
                result_schema: DATASET_RESULT_CONTRACT.into(),
                max_request_bytes: 1024 * 1024,
                max_result_bytes: 1024 * 1024,
                ready: true,
            },
            opened: AtomicU64::new(0),
            read: AtomicU64::new(0),
            closed: AtomicU64::new(0),
        }
    }
}
impl HostProvider for CursorProvider {
    fn descriptor(&self) -> &CapabilityDescriptor {
        &self.descriptor
    }
    fn invoke(&self, _: &Invocation, effect: EffectRequest) -> EffectResult {
        let (cursor, record) = match effect.request {
            HostRequest::Dataset(DatasetRequest::StartBrowse { .. }) => (
                (self.opened.fetch_add(1, Ordering::SeqCst) + 1).to_string(),
                None,
            ),
            HostRequest::Dataset(DatasetRequest::ReadNext { cursor, .. }) => {
                self.read.fetch_add(1, Ordering::SeqCst);
                let index: u8 = cursor.parse().unwrap();
                (cursor, Some(vec![b'A' + index - 1]))
            }
            HostRequest::Dataset(DatasetRequest::Close { cursor, .. }) => {
                self.closed.fetch_add(1, Ordering::SeqCst);
                (cursor.unwrap(), None)
            }
            request => panic!("unexpected cursor request: {request:?}"),
        };
        EffectResult {
            sequence: effect.sequence,
            outcome: Ok(HostResult::Dataset(DatasetResult::Browse {
                cursor,
                identity: record.clone(),
                key: record.clone(),
                record,
            })),
        }
    }
}
fn actor() -> Invocation {
    let mut invocation = parent();
    let limits = InvocationLimits::default();
    let mut grants = invocation.principal.grants().clone();
    grants.insert(CapabilityId::new("host.dataset.read", limits).unwrap());
    invocation.principal =
        Principal::new(invocation.principal.id().clone(), grants, limits).unwrap();
    invocation.bindings.insert(
        "cobol.dd.INPUTDD".into(),
        BoundedPayload::new(
            "mainframe-env.dataset-name@1",
            b"IBMUSER.INPUT".to_vec(),
            limits,
        )
        .unwrap(),
    );
    invocation
}
fn new_fixture(
    root: &TestRoot,
    store: Arc<dyn PlatformStore>,
    provider: Arc<CursorProvider>,
) -> Fixture {
    let fixture =
        Fixture::with_providers(root, store, HostProblem::NotFound, false, vec![provider]);
    fixture.install("CURSOR-LEAF", SOURCE);
    fixture
}
fn reply(expected: u8) -> Result<HostResult, HostProblem> {
    Ok(HostResult::Program(
        encode_cobol_call_result(&[vec![b'O'], vec![expected]]).unwrap(),
    ))
}
fn cursor_row(store: &dyn PlatformStore) -> Option<ProviderStateRecord> {
    store
        .get_provider_state("batch-file-cursor", CURSOR_KEY)
        .unwrap()
}

#[test]
fn installed_call_interrupted_completion_never_publishes_cursor_ahead_of_retained_state() {
    let root = TestRoot::new();
    let url = format!("sqlite://{}?mode=rwc", root.0.join("state.db").display());
    let store: Arc<dyn PlatformStore> =
        Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
    let provider = Arc::new(CursorProvider::new());
    let fixture = new_fixture(&root, store, provider.clone());
    fixture
        .router
        .bind_execution_control(Arc::new(move |invocation: &Invocation| {
            // The completion clock is read after the child returns, immediately
            // before publication of its retained state and replay receipt.
            if invocation.execution_id.as_str() == "parent-execution"
                && provider.opened.load(Ordering::SeqCst) == 2
            {
                return Err(ExecutionControlError::Unavailable);
            }
            Ok(ExecutionControl {
                now_tick: 1,
                cancellation_requested: false,
            })
        }))
        .unwrap();
    let payload = call_payload(&[vec![b'O'], vec![b' ']]);
    assert_eq!(
        fixture
            .call(&actor(), "CURSOR-LEAF", 1, payload.clone())
            .outcome,
        reply(b'A')
    );
    let committed_cursor = cursor_row(fixture.store.as_ref()).unwrap();
    let namespace = format!(
        "{}{}",
        crate::cobol::retention::INSTANCE_NAMESPACE_PREFIX,
        crate::cobol::instance::run_key(&actor())
    );
    let instance = fixture
        .store
        .get_provider_state(&namespace, "CURSOR-LEAF")
        .unwrap()
        .unwrap();
    let committed_instance: serde_json::Value = serde_json::from_slice(&instance.payload).unwrap();
    // Interrupt after execution but before the ready-state/reply CAS. The
    // pending receipt and busy instance must survive without a cursor commit.
    assert_eq!(
        fixture
            .call(&actor(), "CURSOR-LEAF", 2, payload.clone())
            .outcome,
        Err(HostProblem::InfrastructureFailure)
    );
    drop(fixture);
    let reopened = SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap();
    assert_eq!(
        cursor_row(&reopened),
        Some(committed_cursor),
        "interrupted completion published an advanced cursor while the retained snapshot is stale",
    );
    let instance = reopened
        .get_provider_state(&namespace, "CURSOR-LEAF")
        .unwrap()
        .unwrap();
    let interrupted_instance: serde_json::Value =
        serde_json::from_slice(&instance.payload).unwrap();
    assert_eq!(interrupted_instance["busy"], true);
    assert_eq!(interrupted_instance["state"], committed_instance["state"]);
    let receipts = reopened
        .list_provider_state("cobol-call-replay@1", 128)
        .unwrap();
    assert_eq!(receipts.len(), 2);
    assert_eq!(receipts.iter().filter(|row| row.version == 1).count(), 1);
    // The busy lease also prevents either the old snapshot or a fresh instance
    // from being used to redispatch the interrupted execution after restart.
    let fixture = Fixture::with_providers(
        &root,
        Arc::new(reopened),
        HostProblem::NotFound,
        false,
        vec![Arc::new(CursorProvider::new())],
    );
    assert_eq!(
        fixture
            .call(&actor(), "CURSOR-LEAF", 2, payload.clone())
            .outcome,
        Err(HostProblem::UnknownOutcome)
    );
    assert_eq!(
        fixture.call(&actor(), "CURSOR-LEAF", 3, payload).outcome,
        Err(HostProblem::UnknownOutcome)
    );
}

#[test]
fn installed_call_cursors_advance_once_and_committed_replay_never_advances_again() {
    for sqlite in [false, true] {
        let root = TestRoot::new();
        let url = format!("sqlite://{}?mode=rwc", root.0.join("state.db").display());
        let store: Arc<dyn PlatformStore> = if sqlite {
            Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap())
        } else {
            Arc::new(MemoryStore::new(Default::default()))
        };
        let provider = Arc::new(CursorProvider::new());
        let fixture = new_fixture(&root, store.clone(), provider.clone());
        let payload = call_payload(&[vec![b'O'], vec![b' ']]);
        for sequence in 1..=3 {
            let expected = reply(b'A' + sequence as u8 - 1);
            assert_eq!(
                fixture
                    .call(&actor(), "CURSOR-LEAF", sequence, payload.clone())
                    .outcome,
                expected
            );
            let cursor = cursor_row(store.as_ref()).unwrap();
            let map: BTreeMap<String, String> = serde_json::from_slice(&cursor.payload).unwrap();
            assert_eq!(map["IBMUSER.INPUT"], sequence.to_string());
            assert_eq!(
                fixture
                    .call(&actor(), "CURSOR-LEAF", sequence, payload.clone())
                    .outcome,
                expected
            );
            assert_eq!(cursor_row(store.as_ref()), Some(cursor));
            assert_eq!(provider.opened.load(Ordering::SeqCst), sequence);
            assert_eq!(provider.read.load(Ordering::SeqCst), sequence);
        }
        let close = call_payload(&[vec![b'C'], vec![b' ']]);
        let closed_reply = Ok(HostResult::Program(
            encode_cobol_call_result(&[vec![b'C'], vec![b' ']]).unwrap(),
        ));
        assert_eq!(
            fixture
                .call(&actor(), "CURSOR-LEAF", 4, close.clone())
                .outcome,
            closed_reply
        );
        assert!(
            cursor_row(store.as_ref()).is_none(),
            "CLOSE atomically removes the cursor row"
        );
        assert_eq!(
            fixture.call(&actor(), "CURSOR-LEAF", 4, close).outcome,
            closed_reply
        );
        assert_eq!(provider.closed.load(Ordering::SeqCst), 1);
        if sqlite {
            drop(fixture);
            drop(store);
            let reopened =
                Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
            let fixture = Fixture::with_providers(
                &root,
                reopened,
                HostProblem::NotFound,
                false,
                vec![provider.clone()],
            );
            assert_eq!(
                fixture.call(&actor(), "CURSOR-LEAF", 3, payload).outcome,
                reply(b'C')
            );
            assert_eq!(provider.read.load(Ordering::SeqCst), 3);
        }
    }
}

#[test]
fn installed_call_divergent_legacy_cursor_cannot_be_overwritten_by_a_stale_snapshot() {
    for missing in [false, true] {
        let root = TestRoot::new();
        let provider = Arc::new(CursorProvider::new());
        let fixture = new_fixture(
            &root,
            Arc::new(MemoryStore::new(Default::default())),
            provider.clone(),
        );
        assert_eq!(
            fixture
                .call(
                    &actor(),
                    "CURSOR-LEAF",
                    1,
                    call_payload(&[vec![b'O'], vec![b' ']])
                )
                .outcome,
            reply(b'A')
        );
        let mut cursor = cursor_row(fixture.store.as_ref()).unwrap();
        let previous = cursor.version;
        if missing {
            fixture
                .store
                .delete_provider_state(&cursor.namespace, &cursor.key, previous)
                .unwrap();
        } else {
            cursor.version += 1;
            cursor.payload = br#"{"IBMUSER.INPUT":"2"}"#.to_vec();
            fixture
                .store
                .put_provider_state(cursor, Some(previous))
                .unwrap();
        }
        // Legacy recovery material has two disagreeing records. The old executor
        // silently restores cursor 1 and returns A instead of failing closed.
        assert_eq!(
            fixture
                .call(
                    &actor(),
                    "CURSOR-LEAF",
                    2,
                    call_payload(&[vec![b'R'], vec![b' ']])
                )
                .outcome,
            Err(HostProblem::Condition {
                name: "INSTALLED-CALL-CURSOR-MISMATCH".into(),
                response: -10,
                response2: 0,
            }),
            "a stale retained cursor must not win over divergent legacy recovery material",
        );
        assert_eq!(provider.read.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn installed_call_orphaned_legacy_cursor_is_rejected_before_first_dispatch() {
    let root = TestRoot::new();
    let provider = Arc::new(CursorProvider::new());
    let fixture = new_fixture(
        &root,
        Arc::new(MemoryStore::new(Default::default())),
        provider.clone(),
    );
    fixture
        .store
        .put_provider_state(
            ProviderStateRecord {
                namespace: "batch-file-cursor".into(),
                key: CURSOR_KEY.into(),
                version: 1,
                payload: br#"{"IBMUSER.INPUT":"2"}"#.to_vec(),
            },
            None,
        )
        .unwrap();
    assert_eq!(
        fixture
            .call(
                &actor(),
                "CURSOR-LEAF",
                1,
                call_payload(&[vec![b'R'], vec![b' ']])
            )
            .outcome,
        Err(HostProblem::Condition {
            name: "INSTALLED-CALL-CURSOR-MISMATCH".into(),
            response: -10,
            response2: 0,
        })
    );
    assert_eq!(provider.read.load(Ordering::SeqCst), 0);
}
