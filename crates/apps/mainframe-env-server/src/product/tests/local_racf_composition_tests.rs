//! Seven real profile/snapshot/atomic-store controls; no fabricated journal or Ready receipt.
use super::*;
use mainframe_env_application::SecurityResource;
use mainframe_env_host_api::{AccessIntent, ResourceName};
use mainframe_env_racf::{AccessLevel, RacfLimits, RacfProfileComposition, RacfProfileProjection};

const PROFILE: &str = "LOCAL.ACCESS";

fn declaration(access: AccessLevel) -> RacfProfileComposition {
    RacfProfileComposition {
        class: "FACILITY".into(),
        name: PROFILE.into(),
        owner: "LOCALOWN".into(),
        uacc: AccessLevel::None,
        permissions: BTreeMap::from([("LOCALOWN".into(), access)]),
    }
}

fn signed_profiles(
    package: &mut ApplicationPackageV2,
    definition: &mut LocalApplicationDefinition,
    trust: &HmacSha256PackageTrust,
    profiles: Vec<RacfProfileComposition>,
) {
    package
        .base
        .manifest
        .entries
        .retain(|entry| entry.kind != EntryKind::Profile);
    package.sections.security_resources = profiles
        .iter()
        .map(|profile| SecurityResource {
            class: profile.class.clone(),
            profile: profile.name.clone(),
            owner: profile.owner.clone(),
        })
        .collect();
    for profile in &profiles {
        let bytes = profile.canonical_payload().unwrap();
        let sha = format!("sha256:{:x}", Sha256::digest(&bytes));
        package.base.blobs.insert(sha.clone(), bytes.clone());
        package.base.manifest.entries.push(PackageEntry {
            path: profile.entry_path().unwrap(),
            kind: EntryKind::Profile,
            sha256: sha,
            bytes: bytes.len(),
            depends_on: Vec::new(),
        });
    }
    let used = package
        .base
        .manifest
        .entries
        .iter()
        .map(|entry| entry.sha256.clone())
        .collect::<BTreeSet<_>>();
    package.base.blobs.retain(|sha, _| used.contains(sha));
    definition.security_profiles = profiles;
    resign_package(package, trust);
}

fn secured_definitions(
    server: &ProductServer,
    trust: &HmacSha256PackageTrust,
    generation: u64,
) -> (ApplicationPackageV2, LocalApplicationDefinition) {
    let (mut package, mut definition) = definitions(server, trust, generation);
    signed_profiles(
        &mut package,
        &mut definition,
        trust,
        vec![declaration(if generation == 1 {
            AccessLevel::Read
        } else {
            AccessLevel::Update
        })],
    );
    (package, definition)
}

fn authority(store: &dyn ProviderStateStore) -> ProviderStateRecord {
    store
        .get_provider_state("racf-database-v2", "authority")
        .unwrap()
        .unwrap()
}
fn snapshot(row: &ProviderStateRecord) -> serde_json::Value {
    assert!(row.payload.starts_with(b"MERACF2\0"));
    let value: serde_json::Value = serde_json::from_slice(&row.payload[8..]).unwrap();
    assert_eq!(value["generation"].as_u64(), Some(row.version));
    value
}
fn untouched_runtime(row: &ProviderStateRecord) -> serde_json::Value {
    let mut value = snapshot(row);
    value.as_object_mut().unwrap().remove("generation");
    value["profiles"]
        .as_object_mut()
        .unwrap()
        .remove("FACILITY:LOCAL.ACCESS");
    value
}
fn rows(store: &dyn ProviderStateStore) -> Vec<ProviderStateRecord> {
    let mut result = selected_rows(store);
    result.push(authority(store));
    result.sort_by(|left, right| (&left.namespace, &left.key).cmp(&(&right.namespace, &right.key)));
    result
}
fn secured_install(
    server: &ProductServer,
    trust: &HmacSha256PackageTrust,
    generation: u64,
) -> LocalApplicationReceipt {
    let (package, definition) = secured_definitions(server, trust, generation);
    let expected = server.install_application_package_v2(&package).unwrap();
    server
        .publish_local_application(&expected, definition)
        .unwrap()
}
fn add_owner(server: &ProductServer) {
    server.racf.add_group("LOCALOWN").unwrap();
}

#[test]
fn racf_signed_full_profile_snapshot_and_ready_reopen_share_one_memory_store() {
    let trust = Arc::new(test_package_trust());
    let store = Arc::new(MemoryStore::new(Default::default()));
    let server = fixture(store.clone(), trust.clone());
    add_owner(&server);
    let before = authority(store.as_ref());
    let installed = secured_install(&server, &trust, 1);
    let after = authority(store.as_ref());
    assert_eq!(after.version, before.version + 1);
    assert_eq!(untouched_runtime(&after), untouched_runtime(&before));
    let profile = &snapshot(&after)["profiles"]["FACILITY:LOCAL.ACCESS"];
    assert_eq!(profile["owner"], "LOCALOWN");
    assert_eq!(profile["uacc"], "NONE");
    assert_eq!(profile["access_list"][0]["principal"], "LOCALOWN");
    assert_eq!(profile["access_list"][0]["access"], "READ");
    assert_eq!(profile["version"], 1);
    assert_eq!(
        seed_records(&server),
        RAW_ONE
            .as_chunks::<4>()
            .0
            .iter()
            .map(|chunk| chunk.to_vec())
            .collect::<Vec<_>>()
    );
    let full = rows(store.as_ref());
    drop(server);
    let reopened = fixture(store.clone(), trust);
    assert_eq!(rows(store.as_ref()), full);
    assert_eq!(
        reopened
            .selected_local_application("LOCAL-COMPOSE")
            .unwrap(),
        Some(installed.generation)
    );
}

#[test]
fn racf_real_authorize_audit_updates_preserve_projection_retry_and_reopen() {
    let trust = Arc::new(test_package_trust());
    let store = Arc::new(MemoryStore::new(Default::default()));
    let server = fixture(store.clone(), trust.clone());
    add_owner(&server);
    let installed = secured_install(&server, &trust, 1);
    let original = authority(store.as_ref());
    let _decision = server
        .racf
        .authorize(
            &PrincipalId::new("MISSING", InvocationLimits::default()).unwrap(),
            "FACILITY",
            &ResourceName::new(PROFILE, 246).unwrap(),
            AccessIntent::Read,
        )
        .unwrap();
    let audited = authority(store.as_ref());
    assert!(audited.version > original.version);
    assert_eq!(
        snapshot(&audited)["audits"].as_array().unwrap().len(),
        snapshot(&original)["audits"].as_array().unwrap().len() + 1
    );
    assert_eq!(
        snapshot(&audited)["profiles"],
        snapshot(&original)["profiles"]
    );
    let (_, definition) = secured_definitions(&server, &trust, 1);
    let retry = server
        .publish_local_application(&installed.generation, definition)
        .unwrap();
    assert!(retry.replayed);
    assert_eq!(authority(store.as_ref()), audited);
    assert_eq!(retry.binding, installed.binding);
    drop(server);
    let reopened = fixture(store.clone(), trust);
    assert_eq!(authority(store.as_ref()), audited);
    assert_eq!(
        reopened
            .selected_local_application("LOCAL-COMPOSE")
            .unwrap(),
        Some(installed.generation)
    );
}

#[test]
fn racf_unsigned_or_wrong_full_profile_and_section_closure_never_mutate() {
    for variant in 0..8 {
        let trust = Arc::new(test_package_trust());
        let store = Arc::new(MemoryStore::new(Default::default()));
        let server = fixture(store.clone(), trust.clone());
        add_owner(&server);
        let (mut package, mut definition) = secured_definitions(&server, &trust, 1);
        match variant {
            0 => definition.security_profiles[0].uacc = AccessLevel::Alter,
            1 => {
                definition.security_profiles[0].permissions.clear();
            }
            2 => {
                definition.security_profiles[0]
                    .permissions
                    .insert("MISSING".into(), AccessLevel::Read);
            }
            3 => definition.security_profiles[0].owner = "MISSING".into(),
            4 => definition.security_profiles[0].class = "UNKNOWN".into(),
            5 => definition.security_profiles[0].name = "OTHER.ACCESS".into(),
            6 => {
                package.sections.security_resources[0].owner = "MISSING".into();
                resign_package(&mut package, &trust);
            }
            _ => {
                let extra = declaration(AccessLevel::Read).canonical_payload().unwrap();
                let sha = format!("sha256:{:x}", Sha256::digest(&extra));
                package.base.manifest.entries.push(PackageEntry {
                    path: "profile/unowned".into(),
                    kind: EntryKind::Profile,
                    sha256: sha,
                    bytes: extra.len(),
                    depends_on: Vec::new(),
                });
                resign_package(&mut package, &trust);
            }
        }
        let expected = server.install_application_package_v2(&package).unwrap();
        let before = rows(store.as_ref());
        assert!(
            server
                .publish_local_application(&expected, definition)
                .is_err()
        );
        assert_eq!(rows(store.as_ref()), before);
        assert_eq!(
            server.selected_local_application("LOCAL-COMPOSE").unwrap(),
            None
        );
    }
    // A correctly signed declaration still cannot synthesize a missing existing principal or class.
    for missing_class in [false, true] {
        let trust = Arc::new(test_package_trust());
        let store = Arc::new(MemoryStore::new(Default::default()));
        let server = fixture(store.clone(), trust.clone());
        add_owner(&server);
        let (mut package, mut definition) = definitions(&server, &trust, 1);
        let mut profile = declaration(AccessLevel::Read);
        if missing_class {
            profile.class = "UNKNOWN".into();
        } else {
            profile.owner = "MISSING".into();
        }
        signed_profiles(&mut package, &mut definition, &trust, vec![profile]);
        let expected = server.install_application_package_v2(&package).unwrap();
        let before = rows(store.as_ref());
        assert!(
            server
                .publish_local_application(&expected, definition)
                .is_err()
        );
        assert_eq!(rows(store.as_ref()), before);
    }
}

fn rewrite_resource(package: &mut ApplicationPackageV2, path: &str, bytes: Vec<u8>) {
    let entry = package
        .base
        .manifest
        .entries
        .iter_mut()
        .find(|entry| entry.path == path)
        .unwrap();
    entry.sha256 = format!("sha256:{:x}", Sha256::digest(&bytes));
    entry.bytes = bytes.len();
    package.base.blobs.insert(entry.sha256.clone(), bytes);
    let used = package
        .base
        .manifest
        .entries
        .iter()
        .map(|entry| entry.sha256.clone())
        .collect::<BTreeSet<_>>();
    package.base.blobs.retain(|sha, _| used.contains(sha));
}
fn foreign_profile_definitions(
    server: &ProductServer,
    trust: &HmacSha256PackageTrust,
) -> (ApplicationPackageV2, LocalApplicationDefinition) {
    let (mut package, mut definition) = foreign_definitions(server, trust);
    let dataset = DatasetName::new("OTHER.SEED", 128).unwrap();
    definition.seeds[0].dataset = dataset.clone();
    definition.files.get_mut("OTHERFIL").unwrap().dataset = dataset;
    rewrite_resource(
        &mut package,
        "resource/file",
        server
            .cics
            .composition_file_payload(&definition.files["OTHERFIL"])
            .unwrap(),
    );
    let digest = server
        .dataset
        .prepare_seed_selection("FOREIGN-COMPOSE", "1", definition.seeds.clone(), true)
        .unwrap()
        .snapshot_digest()
        .unwrap();
    rewrite_resource(&mut package, "seed/snapshot", digest.into_bytes());
    signed_profiles(
        &mut package,
        &mut definition,
        trust,
        vec![declaration(AccessLevel::Alter)],
    );
    (package, definition)
}

fn foreign_selected_refusal<S: PlatformStore + 'static>(store: Arc<S>) {
    let trust = Arc::new(test_package_trust());
    let server = fixture(store.clone(), trust.clone());
    add_owner(&server);
    let foreign = foreign_profile_definitions(&server, &trust);
    let installed = secured_install(&server, &trust, 1);
    server
        .racf
        .permit("FACILITY", PROFILE, "LOCALOWN", AccessIntent::Read)
        .unwrap();
    // Trusted administration empties the allowed ACL through the genuine whole authority codec/CAS.
    let current = authority(store.as_ref());
    let mut body = snapshot(&current);
    body["generation"] = (current.version + 1).into();
    let profile = &mut body["profiles"]["FACILITY:LOCAL.ACCESS"];
    profile["access_list"] = serde_json::json!([]);
    let profile_version = profile["version"].as_u64().unwrap();
    profile["version"] = (profile_version + 1).into();
    let mut payload = b"MERACF2\0".to_vec();
    payload.extend(serde_json::to_vec(&body).unwrap());
    store
        .put_provider_state(
            ProviderStateRecord {
                version: current.version + 1,
                payload,
                ..current.clone()
            },
            Some(current.version),
        )
        .unwrap();
    let expected = server.install_application_package_v2(&foreign.0).unwrap();
    let before = rows(store.as_ref());
    let selected = store
        .get_provider_state("application-local-selection", "LOCAL-COMPOSE")
        .unwrap();
    assert_eq!(
        server.publish_local_application(&expected, foreign.1),
        Err(HostProblem::IdempotencyConflict)
    );
    assert_eq!(rows(store.as_ref()), before);
    assert_eq!(
        store
            .get_provider_state("application-local-selection", "LOCAL-COMPOSE")
            .unwrap(),
        selected
    );
    assert_eq!(
        server
            .selected_local_application("FOREIGN-COMPOSE")
            .unwrap(),
        None
    );
    assert_eq!(installed.generation.state, InstallState::Ready);
    // The edited current profile no longer matches its original selected projection.
    assert_eq!(
        server.selected_local_application("LOCAL-COMPOSE"),
        Err(HostProblem::IdempotencyConflict)
    );
}

#[test]
fn racf_foreign_current_selected_profile_remains_owned_after_permission_emptying() {
    foreign_selected_refusal(Arc::new(MemoryStore::new(Default::default())));
    static SERIAL: AtomicU64 = AtomicU64::new(1);
    let root = std::env::temp_dir().join(format!(
        "foreign-racf-owner-{}-{}",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir(&root).unwrap();
    let url = format!("sqlite://{}?mode=rwc", root.join("state.sqlite").display());
    foreign_selected_refusal(Arc::new(
        SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap(),
    ));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn racf_upgrade_and_retained_rollback_preserve_complete_unrelated_runtime() {
    let trust = Arc::new(test_package_trust());
    let store = Arc::new(MemoryStore::new(Default::default()));
    let server = fixture(store.clone(), trust.clone());
    add_owner(&server);
    let first = secured_install(&server, &trust, 1);
    server.racf.add_group("RUNTIME").unwrap();
    server
        .racf
        .authorize(
            &PrincipalId::new("MISSING", InvocationLimits::default()).unwrap(),
            "FACILITY",
            &ResourceName::new(PROFILE, 246).unwrap(),
            AccessIntent::Read,
        )
        .unwrap();
    let runtime = authority(store.as_ref());
    let second = secured_install(&server, &trust, 2);
    let upgraded = authority(store.as_ref());
    assert_eq!(untouched_runtime(&upgraded), untouched_runtime(&runtime));
    assert_eq!(
        snapshot(&upgraded)["profiles"]["FACILITY:LOCAL.ACCESS"]["access_list"][0]["access"],
        "UPDATE"
    );
    let (_, definition) = secured_definitions(&server, &trust, 1);
    let rollback = server
        .rollback_local_application(&first.generation, definition)
        .unwrap();
    let rolled = authority(store.as_ref());
    assert_eq!(untouched_runtime(&rolled), untouched_runtime(&runtime));
    assert!(rolled.version > upgraded.version);
    assert!(
        snapshot(&rolled)["profiles"]["FACILITY:LOCAL.ACCESS"]["version"]
            .as_u64()
            .unwrap()
            > snapshot(&upgraded)["profiles"]["FACILITY:LOCAL.ACCESS"]["version"]
                .as_u64()
                .unwrap()
    );
    assert_eq!(
        snapshot(&rolled)["profiles"]["FACILITY:LOCAL.ACCESS"]["access_list"][0]["access"],
        "READ"
    );
    assert_eq!(rollback.generation.generation, 1);
    assert_eq!(second.generation.generation, 2);
    drop(server);
    let reopened = fixture(store.clone(), trust);
    assert_eq!(authority(store.as_ref()), rolled);
    assert_eq!(
        reopened
            .selected_local_application("LOCAL-COMPOSE")
            .unwrap()
            .unwrap()
            .generation,
        1
    );
}

#[test]
fn racf_capacity_and_unknown_atomic_outcomes_keep_real_authority_fenced() {
    for outcome in [Outcome::Before, Outcome::After] {
        let trust = Arc::new(test_package_trust());
        let store = Arc::new(FaultStore::new(Arc::new(MemoryStore::new(
            Default::default(),
        ))));
        let server = fixture(store.clone(), trust.clone());
        add_owner(&server);
        let (package, definition) = secured_definitions(&server, &trust, 1);
        let expected = server.install_application_package_v2(&package).unwrap();
        let before = rows(store.as_ref());
        store.arm(outcome);
        assert_eq!(
            server.publish_local_application(&expected, definition),
            Err(HostProblem::UnknownOutcome)
        );
        assert_eq!(
            server.racf.database().summary(),
            Err(HostProblem::UnknownOutcome)
        );
        assert_eq!(
            server.racf.add_group("REFUSED"),
            Err(HostProblem::UnknownOutcome)
        );
        drop(server);
        let reopened = fixture(store.clone(), trust);
        if matches!(outcome, Outcome::Before) {
            assert_eq!(rows(store.as_ref()), before);
            assert_eq!(
                reopened
                    .selected_local_application("LOCAL-COMPOSE")
                    .unwrap(),
                None
            );
        } else {
            assert!(
                reopened
                    .selected_local_application("LOCAL-COMPOSE")
                    .unwrap()
                    .is_some()
            );
        }
    }
    let actual = Arc::new(MemoryStore::new(Default::default()));
    let other = Arc::new(MemoryStore::new(Default::default()));
    let trust = Arc::new(test_package_trust());
    let left = fixture(actual.clone(), trust.clone());
    let right = fixture(other.clone(), trust);
    add_owner(&left);
    add_owner(&right);
    let before = authority(actual.as_ref());
    let profiles = vec![declaration(AccessLevel::Read)];
    let prepared = left
        .racf
        .database()
        .prepare_profile_composition(&profiles, &[], &BTreeSet::new())
        .unwrap();
    let commit = ProviderCompositionCommit::publish(other, prepared.writes().to_vec()).unwrap();
    assert_eq!(prepared.finish(&commit), Err(HostProblem::UnknownOutcome));
    assert_eq!(authority(actual.as_ref()), before);
    assert_eq!(
        left.racf.database().summary(),
        Err(HostProblem::UnknownOutcome)
    );
    let limited_store = Arc::new(MemoryStore::new(Default::default()));
    let limited = RacfService::open(
        limited_store.clone(),
        Arc::new(MemorySecretResolver::default()),
        RacfLimits {
            max_profiles: 0,
            ..Default::default()
        },
    )
    .unwrap();
    limited.add_group("LOCALOWN").unwrap();
    let before = authority(limited_store.as_ref());
    assert!(matches!(
        limited
            .database()
            .prepare_profile_composition(&profiles, &[], &BTreeSet::new()),
        Err(HostProblem::ResourceExhausted)
    ));
    assert_eq!(authority(limited_store.as_ref()), before);
    // A maximum profile version in an otherwise genuine stored snapshot refuses a changed declaration.
    // The provider authority row itself still uses the original exact version+1 CAS rule.
    let overflow_store = Arc::new(MemoryStore::new(Default::default()));
    let trust = Arc::new(test_package_trust());
    let overflow_server = fixture(overflow_store.clone(), trust);
    add_owner(&overflow_server);
    let prepared = overflow_server
        .racf
        .database()
        .prepare_profile_composition(&profiles, &[], &BTreeSet::new())
        .unwrap();
    let commit =
        ProviderCompositionCommit::publish(overflow_store.clone(), prepared.writes().to_vec())
            .unwrap();
    prepared.finish(&commit).unwrap();
    let current = authority(overflow_store.as_ref());
    let mut body = snapshot(&current);
    body["generation"] = (current.version + 1).into();
    body["profiles"]["FACILITY:LOCAL.ACCESS"]["version"] = u64::MAX.into();
    let previous = vec![RacfProfileProjection {
        profile: serde_json::from_value(body["profiles"]["FACILITY:LOCAL.ACCESS"].clone()).unwrap(),
        class: serde_json::from_value(body["classes"]["FACILITY"].clone()).unwrap(),
        template: serde_json::from_value(body["templates"]["RESOURCE"].clone()).unwrap(),
    }];
    let mut payload = b"MERACF2\0".to_vec();
    payload.extend(serde_json::to_vec(&body).unwrap());
    let overflow = ProviderStateRecord {
        version: current.version + 1,
        payload,
        ..current.clone()
    };
    overflow_store
        .put_provider_state(overflow.clone(), Some(current.version))
        .unwrap();
    assert!(matches!(
        overflow_server.racf.database().prepare_profile_composition(
            &[declaration(AccessLevel::Update)],
            &previous,
            &BTreeSet::new()
        ),
        Err(HostProblem::ResourceExhausted)
    ));
    assert_eq!(authority(overflow_store.as_ref()), overflow);
}

#[test]
fn racf_sqlite_atomic_failure_and_upgrade_rollback_reopen_keep_full_rows() {
    static SERIAL: AtomicU64 = AtomicU64::new(1);
    let root = std::env::temp_dir().join(format!(
        "local-racf-composition-{}-{}",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir(&root).unwrap();
    let url = format!("sqlite://{}?mode=rwc", root.join("state.sqlite").display());
    let trust = Arc::new(test_package_trust());
    let inner = Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
    let store = Arc::new(FaultStore::new(inner));
    let server = fixture(store.clone(), trust.clone());
    add_owner(&server);
    let first = secured_install(&server, &trust, 1);
    let (package, definition) = secured_definitions(&server, &trust, 2);
    let expected = server.install_application_package_v2(&package).unwrap();
    let companion = ProviderStateRecord {
        namespace: "zz-racf-companion".into(),
        key: "owned".into(),
        version: 1,
        payload: b"unchanged".to_vec(),
    };
    store.put_provider_state(companion.clone(), None).unwrap();
    let before = rows(store.as_ref());
    // A real CAS conflict on one companion row must roll back the whole SQLite transaction.
    let seed = server
        .dataset
        .prepare_seed_selection("LOCAL-COMPOSE", "2", definition.seeds.clone(), true)
        .unwrap();
    let profiles = vec![declaration(AccessLevel::Update)];
    let selection = store
        .get_provider_state("application-local-selection", "LOCAL-COMPOSE")
        .unwrap()
        .unwrap();
    let saved: serde_json::Value = serde_json::from_slice(&selection.payload).unwrap();
    let previous: Vec<RacfProfileProjection> =
        serde_json::from_value(saved["racf"].clone()).unwrap();
    let racf = server
        .racf
        .database()
        .prepare_profile_composition(&profiles, &previous, &BTreeSet::new())
        .unwrap();
    let mut writes = seed.writes().to_vec();
    writes.extend_from_slice(racf.writes());
    writes.push(ProviderStateWrite {
        record: companion.clone(),
        expected_version: None,
    });
    assert!(ProviderCompositionCommit::publish(store.clone(), writes).is_err());
    drop((seed, racf));
    assert_eq!(rows(store.as_ref()), before);
    assert_eq!(
        store
            .get_provider_state(&companion.namespace, &companion.key)
            .unwrap(),
        Some(companion)
    );
    let upgraded = server
        .publish_local_application(&expected, definition)
        .unwrap();
    server.racf.add_group("RUNTIME").unwrap();
    let runtime = authority(store.as_ref());
    let (_, definition) = secured_definitions(&server, &trust, 1);
    server
        .rollback_local_application(&first.generation, definition)
        .unwrap();
    let rolled = authority(store.as_ref());
    assert_eq!(untouched_runtime(&rolled), untouched_runtime(&runtime));
    assert!(rolled.version > runtime.version);
    assert_eq!(upgraded.generation.generation, 2);
    let full = rows(store.as_ref());
    drop((server, store));
    let final_store = Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
    let reopened = fixture(final_store.clone(), trust);
    assert_eq!(rows(final_store.as_ref()), full);
    assert_eq!(
        reopened
            .selected_local_application("LOCAL-COMPOSE")
            .unwrap()
            .unwrap()
            .generation,
        1
    );
    drop((reopened, final_store));
    std::fs::remove_dir_all(root).unwrap();
}
