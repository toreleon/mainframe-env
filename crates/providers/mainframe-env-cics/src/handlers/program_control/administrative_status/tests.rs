use super::*;
use crate::service::tests::{invocation, service};
use mainframe_env_execution_api::{ArtifactRef, AuditRecord, ExecutionId, InvocationLimits};
use mainframe_env_host_api::{HostLimits, RegistrySnapshot, ScopedHostService};
use mainframe_env_store::MemoryStore;
use mainframe_env_store_api::{
    ArtifactRecord, ArtifactStore, AuditSink, ExecutableArtifactMetadata, ProviderStateMutation,
    ProviderStateRecord, ProviderStateStore, ProviderStateWrite, StoreError,
};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};

fn definition(
    store: &dyn ArtifactStore,
    name: &str,
    generation: u64,
    enabled: bool,
) -> CicsProgramDefinition {
    let bytes = format!("{name}:{generation}").into_bytes();
    let digest = Sha256::digest(&bytes).into();
    let artifact = ArtifactRef::new(
        format!("sha256:{:x}", Sha256::digest(&bytes)),
        InvocationLimits::default(),
    )
    .unwrap();
    let semantic_identity = format!(
        "semantic-sha256:{:x}",
        Sha256::digest([b"semantic".as_slice(), &bytes].concat())
    );
    let executable = ExecutableArtifactMetadata {
        artifact_contract: "mainframe-env.artifact@3".into(),
        compatibility_profile: "mainframe-env.cobol.reference@1".into(),
        compiler_generation: "mainframe-env-cobol-test".into(),
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
    CicsProgramDefinition {
        name: name.into(),
        generation,
        artifact,
        semantic_identity,
        entry_offset: 1,
        enabled,
        remote: true,
        reload: true,
        java_status: super::super::CicsJavaStatus::Available,
    }
}

fn fixture() -> (Arc<CicsService>, Arc<MemoryStore>) {
    let store = Arc::new(MemoryStore::new(Default::default()));
    let cics = service(store.clone());
    cics.bind_artifact_store(store.clone()).unwrap();
    (cics, store)
}

#[test]
fn whole_enabled_and_disabled_definitions_are_owned_snapshots() {
    let (cics, store) = fixture();
    for (name, generation, enabled) in [("ONPGM", 3, true), ("OFFPGM", 9, false)] {
        let expected = definition(store.as_ref(), name, generation, enabled);
        cics.register_program_definitions(&[expected.clone()])
            .unwrap();
        assert_eq!(
            observe_named_status(&cics, name),
            Ok(ProgramStatusObservation::Definition(expected.clone()))
        );
        let ProgramStatusObservation::Definition(mut copy) =
            observe_named_status(&cics, name).unwrap()
        else {
            panic!("typed definition was lost");
        };
        copy.name = "OTHER".into();
        copy.enabled = !enabled;
        copy.generation = 100;
        copy.semantic_identity.clear();
        assert_eq!(
            observe_named_status(&cics, name),
            Ok(ProgramStatusObservation::Definition(expected))
        );
    }
}

#[test]
fn legacy_name_membership_is_distinct_from_catalog_absence_and_typed_wins() {
    let (cics, store) = fixture();
    cics.register_programs(&BTreeSet::from(["LEGACY".into()]))
        .unwrap();
    assert_eq!(
        observe_named_status(&cics, "LEGACY"),
        Ok(ProgramStatusObservation::NameOnly)
    );
    assert_eq!(
        observe_named_status(&cics, "MISSING"),
        Ok(ProgramStatusObservation::NotCatalogued)
    );
    let expected = definition(store.as_ref(), "LEGACY", 4, false);
    cics.register_program_definitions(&[expected.clone()])
        .unwrap();
    assert!(cics.lock().unwrap().programs.contains("LEGACY"));
    assert_eq!(
        observe_named_status(&cics, "LEGACY"),
        Ok(ProgramStatusObservation::Definition(expected))
    );
}

#[test]
fn existing_project_name_policy_handles_eight_characters_and_rejects_invalid_inputs() {
    let (cics, store) = fixture();
    let expected = definition(store.as_ref(), "A1$@#XYZ", 1, true);
    cics.register_program_definitions(&[expected.clone()])
        .unwrap();
    assert_eq!(
        observe_named_status(&cics, " a1$@#xyz "),
        Ok(ProgramStatusObservation::Definition(expected))
    );
    assert_eq!(
        observe_named_status(&cics, "Z"),
        Ok(ProgramStatusObservation::NotCatalogued)
    );
    for invalid in ["", "   ", "ABCDEFGHI", "A B", "A_B", "A-B", "A\0B", "é"] {
        assert_eq!(
            observe_named_status(&cics, invalid),
            Err(HostProblem::Malformed),
            "{invalid:?}"
        );
    }
}

#[test]
fn latest_definition_is_selected_without_replacing_old_loaded_or_application_references() {
    use mainframe_env_execution_api::IdempotencyKey;
    use mainframe_env_host_api::{CicsConditionPolicy, CicsOperation, CicsRequest, Mutation};
    let (cics, store) = fixture();
    let mut old = definition(store.as_ref(), "SNAPSHOT", 2, true);
    old.remote = false;
    old.java_status = super::super::CicsJavaStatus::NotJava;
    cics.register_program_definitions(&[old.clone()]).unwrap();
    let actor = invocation();
    cics.ensure_run(&actor).unwrap();
    let area = |schema, bytes: &[u8]| {
        mainframe_env_execution_api::BoundedPayload::new(
            schema,
            bytes.to_vec(),
            InvocationLimits::default(),
        )
        .unwrap()
    };
    let request = CicsRequest {
        operation: CicsOperation::Load,
        arguments: BTreeMap::from([
            (
                "PROGRAM".into(),
                area("mainframe-env.cics.literal@1", b"SNAPSHOT"),
            ),
            (
                "OPTION.HOLD".into(),
                area("mainframe-env.cics.option@1", b""),
            ),
        ]),
        condition_policy: CicsConditionPolicy::Default,
        mutation: Some(Mutation {
            sequence: 1,
            idempotency_key: IdempotencyKey::new("status-old-load", InvocationLimits::default())
                .unwrap(),
            transaction: None,
        }),
    };
    let mut run = cics.lock().unwrap().runs[&actor.run_unit_id].clone();
    super::super::load::invoke(&cics, &mut run, &request).unwrap();
    let entry = super::super::CicsApplicationEntryDefinition {
        application: "APP".into(),
        platform: "PLATFORM".into(),
        major_version: 1,
        minor_version: 0,
        micro_version: 0,
        operation: "ENTRY".into(),
        program: "SNAPSHOT".into(),
        program_generation: 2,
        program_artifact: old.artifact.clone(),
        application_identity: format!("sha256:{:064x}", 1),
        available: true,
    };
    cics.register_application_entries(&[entry.clone()]).unwrap();
    let saved = observe_named_status(&cics, "SNAPSHOT").unwrap();
    let newest = definition(store.as_ref(), "SNAPSHOT", 11, false);
    cics.register_program_definitions(&[newest.clone()])
        .unwrap();
    let middle = definition(store.as_ref(), "SNAPSHOT", 7, true);
    cics.register_program_definitions(&[middle.clone()])
        .unwrap();
    assert_eq!(
        observe_named_status(&cics, "SNAPSHOT"),
        Ok(ProgramStatusObservation::Definition(newest.clone()))
    );
    assert_eq!(saved, ProgramStatusObservation::Definition(old.clone()));
    let state = cics.lock().unwrap();
    assert_eq!(
        state.program_definitions["SNAPSHOT"],
        BTreeMap::from([(2, old.clone()), (7, middle), (11, newest)])
    );
    assert_eq!(state.program_loads["SNAPSHOT"].events[0].generation, 2);
    assert_eq!(
        state.program_loads["SNAPSHOT"].events[0].artifact,
        old.artifact
    );
    assert!(state.program_loads["SNAPSHOT"].events[0].hold);
    assert_eq!(state.application_entries, vec![entry]);
}

#[test]
fn corrupt_selected_catalog_metadata_fails_instead_of_name_only_or_status() {
    for corruption in 0..5 {
        let (cics, store) = fixture();
        let valid = definition(store.as_ref(), "CORRUPT", 2, true);
        cics.register_program_definitions(&[valid]).unwrap();
        {
            let mut state = cics.lock().unwrap();
            let generations = state.program_definitions.get_mut("CORRUPT").unwrap();
            match corruption {
                0 => generations.clear(),
                1 => generations.get_mut(&2).unwrap().name = "OTHER".into(),
                2 => generations.get_mut(&2).unwrap().generation = 3,
                3 => {
                    let mut bad = generations.remove(&2).unwrap();
                    bad.generation = 0;
                    generations.insert(0, bad);
                }
                4 => generations.get_mut(&2).unwrap().name.clear(),
                _ => unreachable!(),
            }
        }
        assert_eq!(
            observe_named_status(&cics, "CORRUPT"),
            Err(HostProblem::InfrastructureFailure),
            "corruption {corruption}"
        );
    }
}

#[test]
fn corrupt_old_generation_is_not_hidden_by_a_valid_latest_definition() {
    let (cics, store) = fixture();
    let old = definition(store.as_ref(), "VERSIONS", 1, true);
    let latest = definition(store.as_ref(), "VERSIONS", 5, false);
    cics.register_program_definitions(&[old, latest]).unwrap();
    cics.lock()
        .unwrap()
        .program_definitions
        .get_mut("VERSIONS")
        .unwrap()
        .get_mut(&1)
        .unwrap()
        .generation = 2;
    assert_eq!(
        observe_named_status(&cics, "VERSIONS"),
        Err(HostProblem::InfrastructureFailure)
    );
}

#[test]
fn new_service_reads_existing_immutable_catalog_from_same_retained_memory_store() {
    let (cics, store) = fixture();
    let old = definition(store.as_ref(), "RETAINED", 1, true);
    let latest = definition(store.as_ref(), "RETAINED", 8, false);
    cics.register_program_definitions(&[old.clone(), latest.clone()])
        .unwrap();
    cics.register_programs(&BTreeSet::from(["LEGACY".into()]))
        .unwrap();
    let before = store.list_provider_state_prefix("cics-", 1024).unwrap();
    assert!(
        store
            .list_provider_state("cics-program-definition-v1", 32)
            .unwrap()
            .iter()
            .all(|r| r.payload.starts_with(b"MECPGD1"))
    );
    drop(cics);
    // New Rust owner over the same retained MemoryStore, not a physical restart proof.
    let reopened = service(store.clone());
    reopened.bind_artifact_store(store.clone()).unwrap();
    assert_eq!(
        observe_named_status(&reopened, "RETAINED"),
        Ok(ProgramStatusObservation::Definition(latest))
    );
    assert_eq!(
        observe_named_status(&reopened, "LEGACY"),
        Ok(ProgramStatusObservation::NameOnly)
    );
    assert_eq!(
        observe_named_status(&reopened, "MISSING"),
        Ok(ProgramStatusObservation::NotCatalogued)
    );
    assert_eq!(
        reopened.lock().unwrap().program_definitions["RETAINED"][&1],
        old
    );
    assert_eq!(
        store.list_provider_state_prefix("cics-", 1024).unwrap(),
        before
    );
}

#[test]
fn registration_and_observation_share_one_live_owner_and_never_mix_generations() {
    let (cics, store) = fixture();
    let expected = (1..=32)
        .map(|g| definition(store.as_ref(), "LIVE", g, g % 2 == 0))
        .collect::<Vec<_>>();
    cics.register_program_definitions(&expected[..1]).unwrap();
    let barrier = Arc::new(Barrier::new(2));
    std::thread::scope(|scope| {
        let writer = cics.clone();
        let start = barrier.clone();
        let definitions = &expected;
        let registration = scope.spawn(move || {
            start.wait();
            for value in &definitions[1..] {
                writer
                    .register_program_definitions(&[value.clone()])
                    .unwrap();
                std::thread::yield_now();
            }
        });
        barrier.wait();
        let mut previous = 1;
        for _ in 0..256 {
            let ProgramStatusObservation::Definition(value) =
                observe_named_status(&cics, "LIVE").unwrap()
            else {
                panic!("registered definition disappeared");
            };
            assert!(value.generation >= previous);
            assert_eq!(value, expected[value.generation as usize - 1]);
            previous = value.generation;
            std::thread::yield_now();
        }
        registration.join().unwrap();
    });
    assert_eq!(
        observe_named_status(&cics, "LIVE"),
        Ok(ProgramStatusObservation::Definition(expected[31].clone()))
    );
    assert_eq!(cics.lock().unwrap().program_definitions["LIVE"].len(), 32);
}

// Count all non-default provider/artifact/audit calls while forwarding actual MemoryStore
// behavior. Observation must not consult these authorities, even for typed definitions.
struct CountedStore {
    inner: MemoryStore,
    calls: AtomicUsize,
}
impl CountedStore {
    fn touched(&self) {
        self.calls.fetch_add(1, Ordering::SeqCst);
    }
}
impl AuditSink for CountedStore {
    fn record_audit(&self, r: AuditRecord) -> Result<(), StoreError> {
        self.touched();
        self.inner.record_audit(r)
    }
    fn audit_records(
        &self,
        e: &ExecutionId,
        s: u64,
        m: usize,
    ) -> Result<Vec<AuditRecord>, StoreError> {
        self.touched();
        self.inner.audit_records(e, s, m)
    }
}
impl ArtifactStore for CountedStore {
    fn put_artifact(&self, r: ArtifactRecord) -> Result<(), StoreError> {
        self.touched();
        self.inner.put_artifact(r)
    }
    fn get_artifact(&self, a: &ArtifactRef) -> Result<Option<ArtifactRecord>, StoreError> {
        self.touched();
        self.inner.get_artifact(a)
    }
    fn delete_artifact(&self, a: &ArtifactRef) -> Result<(), StoreError> {
        self.touched();
        self.inner.delete_artifact(a)
    }
}
impl ProviderStateStore for CountedStore {
    fn advance_logical_clock(&self, f: u64) -> Result<u64, StoreError> {
        self.touched();
        self.inner.advance_logical_clock(f)
    }
    fn get_provider_state(
        &self,
        n: &str,
        k: &str,
    ) -> Result<Option<ProviderStateRecord>, StoreError> {
        self.touched();
        self.inner.get_provider_state(n, k)
    }
    fn list_provider_state(
        &self,
        n: &str,
        m: usize,
    ) -> Result<Vec<ProviderStateRecord>, StoreError> {
        self.touched();
        self.inner.list_provider_state(n, m)
    }
    fn list_provider_state_prefix(
        &self,
        n: &str,
        m: usize,
    ) -> Result<Vec<ProviderStateRecord>, StoreError> {
        self.touched();
        self.inner.list_provider_state_prefix(n, m)
    }
    fn put_provider_state(&self, r: ProviderStateRecord, v: Option<u64>) -> Result<(), StoreError> {
        self.touched();
        self.inner.put_provider_state(r, v)
    }
    fn delete_provider_state(&self, n: &str, k: &str, v: u64) -> Result<(), StoreError> {
        self.touched();
        self.inner.delete_provider_state(n, k, v)
    }
    fn move_provider_state(
        &self,
        r: ProviderStateRecord,
        k: &str,
        v: u64,
    ) -> Result<(), StoreError> {
        self.touched();
        self.inner.move_provider_state(r, k, v)
    }
    fn put_provider_states_atomic(&self, w: Vec<ProviderStateWrite>) -> Result<(), StoreError> {
        self.touched();
        self.inner.put_provider_states_atomic(w)
    }
    fn mutate_provider_states_atomic(
        &self,
        m: Vec<ProviderStateMutation>,
    ) -> Result<(), StoreError> {
        self.touched();
        self.inner.mutate_provider_states_atomic(m)
    }
}

#[test]
fn observation_needs_no_host_authority_store_access_load_or_uow_change() {
    let store = Arc::new(CountedStore {
        inner: MemoryStore::new(Default::default()),
        calls: AtomicUsize::new(0),
    });
    let host = Arc::new(ScopedHostService::new(
        Arc::new(RegistrySnapshot::new(1, vec![], InvocationLimits::default()).unwrap()),
        HostLimits::default(),
    ));
    let cics =
        CicsService::open(host, store.clone(), crate::service::CicsLimits::default()).unwrap();
    cics.bind_artifact_store(store.clone()).unwrap();
    let expected = definition(store.as_ref(), "NOLOAD", 6, false);
    cics.register_program_definitions(&[expected.clone()])
        .unwrap();
    cics.register_programs(&BTreeSet::from(["LEGACY".into()]))
        .unwrap();
    let actor = invocation();
    cics.ensure_run(&actor).unwrap();
    let before = store
        .inner
        .list_provider_state_prefix("cics-", 1024)
        .unwrap();
    let (names, definitions, loads, run) = {
        let s = cics.lock().unwrap();
        (
            s.programs.clone(),
            s.program_definitions.clone(),
            s.program_loads.clone(),
            s.runs[&actor.run_unit_id].clone(),
        )
    };
    store.calls.store(0, Ordering::SeqCst);
    for _ in 0..4 {
        assert_eq!(
            observe_named_status(&cics, "NOLOAD"),
            Ok(ProgramStatusObservation::Definition(expected.clone()))
        );
        assert_eq!(
            observe_named_status(&cics, "LEGACY"),
            Ok(ProgramStatusObservation::NameOnly)
        );
        assert_eq!(
            observe_named_status(&cics, "MISSING"),
            Ok(ProgramStatusObservation::NotCatalogued)
        );
        assert_eq!(
            observe_named_status(&cics, "BAD NAME"),
            Err(HostProblem::Malformed)
        );
    }
    assert_eq!(store.calls.load(Ordering::SeqCst), 0);
    let state = cics.lock().unwrap();
    assert_eq!(state.programs, names);
    assert_eq!(state.program_definitions, definitions);
    assert_eq!(state.program_loads, loads);
    let after_run = &state.runs[&actor.run_unit_id];
    assert_eq!(after_run.host_sequence, run.host_sequence);
    assert_eq!(after_run.outer_effect_key, run.outer_effect_key);
    assert_eq!(after_run.undo_version, run.undo_version);
    assert_eq!(format!("{:?}", after_run.undo), format!("{:?}", run.undo));
    assert_eq!(after_run.current_records, run.current_records);
    assert_eq!(after_run.browses, run.browses);
    assert_eq!(after_run.trace.len(), run.trace.len());
    assert_eq!(
        store
            .inner
            .list_provider_state_prefix("cics-", 1024)
            .unwrap(),
        before
    );
}

mod sqlite_reopen;
