use super::*;
use crate::cobol::{hardening::parent, storage_scope::BINDING};
use mainframe_env_host_api::ProgramLinkSelection;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

struct Fixture {
    root_actor: Invocation,
    source: Invocation,
    source_entry: Entry,
    target: Invocation,
    target_entry: Entry,
    root: ScopedRun,
    rows: Vec<ProviderStateRecord>,
}

fn child(source: &Invocation, call: &str, program: &str) -> Invocation {
    let mut actor = source.clone();
    actor.bindings.remove(BINDING);
    crate::cobol::replay::bind_protocol_owner(source, &mut actor.bindings).unwrap();
    actor.execution_id = ExecutionId::new(
        format!("online-call-execution-{call}"),
        InvocationLimits::default(),
    )
    .unwrap();
    actor.parent_execution_id = Some(source.execution_id.clone());
    actor.selector =
        Selector::new(format!("program:{program}"), InvocationLimits::default()).unwrap();
    actor.artifact = ArtifactRef::new(
        format!("sha256:{}", "2".repeat(64)),
        InvocationLimits::default(),
    )
    .unwrap();
    actor
}

fn selection(actor: &Invocation) -> ProgramLinkSelection {
    ProgramLinkSelection {
        artifact: actor.artifact.clone(),
        generation: 7,
        content_identity: format!("sha256:{}", "3".repeat(64)),
    }
}

impl Fixture {
    fn new() -> Self {
        let mut root_actor = parent();
        root_actor.selector = Selector::new("program:MAIN", InvocationLimits::default()).unwrap();
        root_actor.artifact = ArtifactRef::new(
            format!("sha256:{}", "1".repeat(64)),
            InvocationLimits::default(),
        )
        .unwrap();
        root_actor.limits.max_storage_bytes = 256;
        root_actor.limits.max_output_bytes = 1024;
        root_actor.limits.max_effects = 32;
        let root = ScopedRun::fresh(&root_actor).unwrap();
        root.root.bind(&mut root_actor).unwrap();
        let source = root_actor.clone();
        let source_entry = root.root.clone();
        let mut target = child(&source, &"a".repeat(64), "COUNT");
        let target_entry = source_entry
            .native_call(&source, &target, &"a".repeat(64))
            .unwrap();
        target_entry.bind(&mut target).unwrap();
        Self {
            root_actor,
            source,
            source_entry,
            target,
            target_entry,
            root,
            rows: Vec::new(),
        }
    }

    fn root_row(&self, version: u64) -> ProviderStateRecord {
        ProviderStateRecord {
            namespace: RUN_STATE_NAMESPACE.into(),
            key: run_key(&self.root_actor),
            version,
            payload: canonical(&self.root).unwrap(),
        }
    }

    fn select(&mut self, call: &str, program: &str, link: bool) {
        self.target = child(&self.source, call, program);
        self.target_entry = if link {
            self.source_entry
                .cics_link(
                    &self.source,
                    &self.target,
                    call,
                    &selection(&self.target),
                    self.source_entry.logical_level() + 1,
                )
                .unwrap()
        } else {
            self.source_entry
                .native_call(&self.source, &self.target, call)
                .unwrap()
        };
        self.target_entry.bind(&mut self.target).unwrap();
    }

    // Construct pre-existing reader-valid rows independently of admission preparation.
    fn insert(
        &mut self,
        scope: Entry,
        program: &str,
        owner: Option<Entry>,
        initial: bool,
        state: Option<Vec<u8>>,
        version: u64,
    ) -> String {
        let key = scope.member_key(program).unwrap();
        if let Some(call) = owner.as_ref().and_then(Entry::call_key) {
            self.root.calls.insert(call.into());
            self.root.receipt_charge = self.root.calls.len() as u64 * 132_096;
        }
        let mut value = ScopedInstance {
            schema_version: 3,
            run_key: run_key(&self.root_actor),
            scope_entry: scope.clone(),
            max_state_bytes: 256,
            program: program.into(),
            artifact: format!("sha256:{}", "2".repeat(64)),
            owner,
            initial,
            state,
            metadata_digest: String::new(),
        };
        value.metadata_digest = value.expected_digest(&key).unwrap();
        let payload = canonical(&value).unwrap();
        let busy = value.owner.is_some();
        let charged_bytes = if busy {
            256
        } else {
            value.state.as_ref().map_or(0, |state| state.len() as u64)
        };
        self.root.members.insert(
            key.clone(),
            Member {
                scope: scope.scope_id().into(),
                program: program.into(),
                artifact: value.artifact.clone(),
                row_version: version,
                payload_digest: payload_digest(&payload),
                charged_bytes,
                busy,
            },
        );
        self.root.active += usize::from(busy);
        self.root.charged_bytes += charged_bytes;
        self.rows.push(ProviderStateRecord {
            namespace: namespace(&run_key(&self.root_actor)),
            key: key.clone(),
            version,
            payload,
        });
        self.refresh();
        key
    }

    fn refresh(&mut self) {
        self.root.refresh(&run_key(&self.root_actor)).unwrap();
    }

    fn validate(&self) {
        self.root.validate_live_limits(&self.root_actor).unwrap();
        self.root
            .validate_members(&run_key(&self.root_actor), &self.rows)
            .unwrap();
    }

    fn prepare(&self, initial: bool) -> Result<Prepared, HostProblem> {
        prepare(
            &self.root_actor,
            &self.source,
            &self.target,
            &self.source_entry,
            &self.target_entry,
            RootRow::Existing(&self.root_row(3)),
            &self.rows,
            initial,
        )
    }

    fn rejects(&self, initial: bool) {
        let root = self.root_row(3);
        let rows = self.rows.clone();
        let actors = (
            self.root_actor.clone(),
            self.source.clone(),
            self.target.clone(),
        );
        assert!(self.prepare(initial).is_err());
        assert_eq!(self.root_row(3), root);
        assert_eq!(self.rows, rows);
        assert_eq!(
            (&self.root_actor, &self.source, &self.target),
            (&actors.0, &actors.1, &actors.2)
        );
    }

    fn postimage(&self, prepared: &Prepared) -> ScopedRun {
        let root = ScopedRun::decode(&prepared.root_write.record).unwrap();
        let mut rows = self.rows.clone();
        rows.retain(|row| row.key != prepared.target_write.record.key);
        rows.push(prepared.target_write.record.clone());
        root.validate_live_limits(&self.root_actor).unwrap();
        root.validate_members(&run_key(&self.root_actor), &rows)
            .unwrap();
        root
    }
}

#[test]
fn fresh_native_prepares_independent_key_charge_and_owned_cas_postimage() {
    let f = Fixture::new();
    let prepared = prepare(
        &f.root_actor,
        &f.source,
        &f.target,
        &f.source_entry,
        &f.target_entry,
        RootRow::Absent,
        &[],
        false,
    )
    .unwrap();
    assert_eq!(prepared.root_write.expected_version, None);
    assert_eq!(prepared.root_write.record.version, 1);
    assert_eq!(prepared.target_write.expected_version, None);
    assert_eq!(prepared.target_write.record.version, 1);
    assert_eq!(
        prepared.target_write.record.key,
        "872e7fe509ca603211833d8985fbc09f60c7b8f584feeab81a9b88cd081e7827"
    );
    assert_eq!(prepared.state, None);
    assert!(!prepared.initial);
    let root = f.postimage(&prepared);
    assert_eq!(root.active, 1);
    assert_eq!(root.root_charge, 256);
    assert_eq!(root.charged_bytes, 512);
    assert_eq!(root.receipt_charge, 132_096);
    assert_eq!(
        root.calls,
        std::collections::BTreeSet::from(["a".repeat(64)])
    );
    assert_eq!(root.scopes.len(), 1);
    let instance = ScopedInstance::decode(&prepared.target_write.record).unwrap();
    assert_eq!(instance.scope_entry, f.root.root);
    assert_eq!(instance.owner, Some(f.target_entry));
}

#[test]
fn existing_idle_native_retains_state_and_increments_only_target_and_root_versions() {
    let mut f = Fixture::new();
    f.insert(
        f.root.root.clone(),
        "COUNT",
        None,
        false,
        Some(vec![3, 7, 11]),
        9,
    );
    f.insert(
        f.root.root.clone(),
        "SIBLING",
        None,
        false,
        Some(vec![29, 31]),
        41,
    );
    f.validate();
    let sibling = f.rows[1].clone();
    let prepared = f.prepare(false).unwrap();
    assert_eq!(prepared.state, Some(vec![3, 7, 11]));
    assert_eq!(prepared.root_write.expected_version, Some(3));
    assert_eq!(prepared.root_write.record.version, 4);
    assert_eq!(prepared.target_write.expected_version, Some(9));
    assert_eq!(prepared.target_write.record.version, 10);
    let root = f.postimage(&prepared);
    assert_eq!(root.active, 1);
    assert_eq!(root.charged_bytes, 514);
    assert_eq!(
        canonical(&root.members[&sibling.key]).unwrap(),
        canonical(&f.root.members[&sibling.key]).unwrap()
    );
    assert_eq!(f.rows[1], sibling);
    assert_eq!(
        ScopedInstance::decode(&prepared.target_write.record)
            .unwrap()
            .state,
        prepared.state
    );
}

#[test]
fn initial_is_selected_metadata_and_busy_cannot_be_reset() {
    let mut f = Fixture::new();
    f.insert(f.root.root.clone(), "COUNT", None, true, None, 5);
    let prepared = f.prepare(true).unwrap();
    assert!(prepared.initial);
    assert_eq!(prepared.state, None);
    f.postimage(&prepared);
    f.rejects(false);
    let mut f = Fixture::new();
    f.insert(f.root.root.clone(), "COUNT", None, false, Some(vec![7]), 5);
    f.rejects(true);
    for initial in [false, true] {
        let mut f = Fixture::new();
        let busy_actor = child(&f.source, &"b".repeat(64), "COUNT");
        let busy = f
            .source_entry
            .native_call(&f.source, &busy_actor, &"b".repeat(64))
            .unwrap();
        f.insert(f.root.root.clone(), "COUNT", Some(busy), initial, None, 5);
        f.validate();
        f.rejects(initial);
    }
}

#[test]
fn fresh_link_creates_new_level_and_busy_creator_with_independent_key() {
    let mut f = Fixture::new();
    f.insert(
        f.root.root.clone(),
        "COUNT",
        None,
        false,
        Some(vec![23]),
        11,
    );
    f.select(&"a".repeat(64), "COUNT", true);
    let prepared = f.prepare(false).unwrap();
    assert_eq!(prepared.state, None);
    assert_eq!(prepared.target_write.expected_version, None);
    assert_eq!(
        prepared.target_write.record.key,
        "03f1e9e63d564d0343ba6eb3e26edf9dfc366d01cb4ec7d5bd786117c3cb205f"
    );
    let root = f.postimage(&prepared);
    assert_eq!(root.scopes.len(), 2);
    assert_eq!(root.active, 1);
    assert_eq!(root.charged_bytes, 513);
    let instance = ScopedInstance::decode(&prepared.target_write.record).unwrap();
    assert_eq!(instance.scope_entry.logical_level(), 2);
    assert_eq!(
        instance.scope_entry.creation_identity().3,
        Some("b9e9ccd39054ab3d261eb1f8e23ffdfd3303eb1053aa6d732f3dd33bbe78464d")
    );
    assert_eq!(
        instance.scope_entry.scope_id(),
        "7bdd49e2b44c3179d838a0b49aae75fcf916d1b029a7415b8d9eb29d5be529d0"
    );
    assert_eq!(instance.owner, Some(f.target_entry.clone()));
    assert_eq!(
        canonical(&root.members[&f.rows[0].key]).unwrap(),
        canonical(&f.root.members[&f.rows[0].key]).unwrap()
    );
}

#[test]
fn native_siblings_of_link_preserve_creator_source_bytes_and_higher_version() {
    let mut f = Fixture::new();
    f.select(&"a".repeat(64), "MID", true);
    f.root
        .scopes
        .insert(f.target_entry.scope_id().into(), f.target_entry.clone());
    f.insert(
        f.target_entry.clone(),
        "MID",
        Some(f.target_entry.clone()),
        false,
        Some(vec![17, 19]),
        87,
    );
    f.source = f.target.clone();
    f.source_entry = f.target_entry.clone();
    f.insert(
        f.source_entry.clone(),
        "COUNT",
        None,
        false,
        Some(vec![43]),
        6,
    );
    f.select(&"b".repeat(64), "COUNT", false);
    f.validate();
    let source = f.rows[0].clone();
    let prepared = f.prepare(false).unwrap();
    assert_eq!(prepared.state, Some(vec![43]));
    let root = f.postimage(&prepared);
    assert_eq!(root.scopes.len(), 2);
    assert_eq!(root.active, 2);
    assert_eq!(root.charged_bytes, 768);
    assert_eq!(root.receipt_charge, 264_192);
    assert_eq!(
        canonical(&root.members[&source.key]).unwrap(),
        canonical(&f.root.members[&source.key]).unwrap()
    );
    assert_eq!(f.rows[0], source);
    assert_eq!(root.members[&source.key].row_version, 87);
}

#[test]
fn exact_complete_rows_reject_missing_extra_duplicate_stale_and_payload_changes() {
    for case in 0..7 {
        let mut f = Fixture::new();
        f.insert(f.root.root.clone(), "COUNT", None, false, Some(vec![3]), 5);
        match case {
            0 => f.rows.clear(),
            1 => f.rows.push(f.rows[0].clone()),
            2 => f.rows[0].version += 1,
            3 => f.rows[0].namespace = "wrong".into(),
            4 => f.rows[0].key = "f".repeat(64),
            5 => f.rows[0].payload.push(b' '),
            _ => {
                let mut row = f.rows[0].clone();
                row.key = "f".repeat(64);
                f.rows.push(row);
            }
        }
        f.rejects(false);
    }
}

#[test]
fn artifact_and_rehashed_member_index_mismatches_fail_without_mutation() {
    for case in 0..9 {
        let mut f = Fixture::new();
        let key = f.insert(f.root.root.clone(), "COUNT", None, false, Some(vec![3]), 5);
        match case {
            0 => {
                let mut value = ScopedInstance::decode(&f.rows[0]).unwrap();
                value.artifact = format!("sha256:{}", "4".repeat(64));
                value.metadata_digest = value.expected_digest(&key).unwrap();
                f.rows[0].payload = canonical(&value).unwrap();
                let member = f.root.members.get_mut(&key).unwrap();
                member.artifact = value.artifact;
                member.payload_digest = payload_digest(&f.rows[0].payload);
            }
            1 => f.root.members.get_mut(&key).unwrap().scope = "f".repeat(64),
            2 => f.root.members.get_mut(&key).unwrap().program = "OTHER".into(),
            3 => {
                f.root.members.get_mut(&key).unwrap().artifact =
                    format!("sha256:{}", "4".repeat(64))
            }
            4 => f.root.members.get_mut(&key).unwrap().row_version += 1,
            5 => f.root.members.get_mut(&key).unwrap().payload_digest = "e".repeat(64),
            6 => {
                f.root.members.get_mut(&key).unwrap().charged_bytes += 1;
                f.root.charged_bytes += 1;
            }
            7 => {
                f.root.members.get_mut(&key).unwrap().busy = true;
                f.root.active = 1;
                f.root.members.get_mut(&key).unwrap().charged_bytes = 256;
                f.root.charged_bytes = 512;
            }
            _ => f.root.active = 1,
        }
        f.refresh();
        if case == 0 {
            f.validate();
        }
        f.rejects(false);
    }
}

// Recompute BOTH identity hashes after mutations. Preserve the Entry serializer's
// declared field order so rejection cannot rely on a stale checksum.
fn rehashed(entry: &Entry, mutate: impl FnOnce(&mut Value)) -> Entry {
    let mut value = serde_json::to_value(entry).unwrap();
    mutate(&mut value);
    value["scope"]["id"] = json!("");
    value["metadata_digest"] = json!("");
    let parsed: Entry = serde_json::from_value(value).unwrap();
    let bytes = canonical(&parsed).unwrap();
    let text = std::str::from_utf8(&bytes).unwrap();
    let start = text.find("\"scope\":").unwrap() + 8;
    let end = text.find(",\"kind\":").unwrap();
    let id = framed(
        b"mainframe-env.cobol-storage-scope@1",
        &[&bytes[start..end]],
    );
    let mut value = serde_json::to_value(&parsed).unwrap();
    value["scope"]["id"] = json!(id);
    let parsed: Entry = serde_json::from_value(value).unwrap();
    let digest = framed(
        b"mainframe-env.cobol-storage-entry@1",
        &[&canonical(&parsed).unwrap()],
    );
    let mut value = serde_json::to_value(&parsed).unwrap();
    value["metadata_digest"] = json!(digest);
    serde_json::from_value(value).unwrap()
}

fn framed(domain: &[u8], fields: &[&[u8]]) -> String {
    let mut hash = Sha256::new();
    hash.update(domain);
    hash.update([0]);
    for field in fields {
        hash.update((field.len() as u64).to_be_bytes());
        hash.update(field);
    }
    format!("{:x}", hash.finalize())
}

fn replace_binding(actor: &mut Invocation, entry: &Entry) {
    actor.bindings.insert(
        BINDING.into(),
        BoundedPayload::new(
            "mainframe-env.cobol.storage-entry@1",
            canonical(entry).unwrap(),
            InvocationLimits::default(),
        )
        .unwrap(),
    );
}

#[test]
fn rehashed_context_and_target_identity_cannot_change_actual_parent_scope_or_actor() {
    for case in 0..8 {
        let mut f = Fixture::new();
        f.select(&"a".repeat(64), "COUNT", true);
        f.target_entry = rehashed(&f.target_entry, |value| match case {
            0 => value["scope"]["root_execution"] = json!("foreign-root"),
            1 => value["scope"]["task_run"] = json!("foreign-run"),
            2 => value["scope"]["principal"] = json!("FOREIGN"),
            3 => value["scope"]["parent_scope"] = json!("e".repeat(64)),
            4 => value["scope"]["logical_level"] = json!(3),
            5 => value["source_execution"] = json!("foreign-source"),
            6 => value["program"] = json!("OTHER"),
            _ => value["call_key"] = json!("b".repeat(64)),
        });
        replace_binding(&mut f.target, &f.target_entry);
        f.rejects(false);
    }
}

#[test]
fn bindings_must_be_present_canonical_and_equal_to_supplied_typed_entries() {
    for case in 0..8 {
        let mut f = Fixture::new();
        match case {
            0 => {
                f.source.bindings.remove(BINDING);
            }
            1 => {
                f.target.bindings.remove(BINDING);
            }
            2 => {
                f.target_entry = rehashed(&f.target_entry, |v| {
                    v["artifact"] = json!(format!("sha256:{}", "4".repeat(64)))
                });
            }
            3 => {
                f.source_entry = rehashed(&f.source_entry, |v| {
                    v["artifact"] = json!(format!("sha256:{}", "4".repeat(64)));
                    v["scope"]["owner_artifact"] = v["artifact"].clone();
                });
            }
            4 => {
                let mut bytes = canonical(&f.target_entry).unwrap();
                bytes.push(b' ');
                f.target.bindings.insert(
                    BINDING.into(),
                    BoundedPayload::new(
                        "mainframe-env.cobol.storage-entry@1",
                        bytes,
                        InvocationLimits::default(),
                    )
                    .unwrap(),
                );
            }
            5 => {
                f.target.bindings.insert(
                    BINDING.into(),
                    BoundedPayload::new(
                        "wrong",
                        canonical(&f.target_entry).unwrap(),
                        InvocationLimits::default(),
                    )
                    .unwrap(),
                );
            }
            6 => {
                f.target.parent_execution_id =
                    Some(ExecutionId::new("foreign-source", InvocationLimits::default()).unwrap());
            }
            _ => {
                f.root_actor.artifact = ArtifactRef::new(
                    format!("sha256:{}", "4".repeat(64)),
                    InvocationLimits::default(),
                )
                .unwrap();
            }
        }
        f.rejects(false);
    }
}

#[test]
fn rehashed_root_limits_cannot_widen_trusted_actual_root() {
    for case in 0..4 {
        let mut f = Fixture::new();
        match case {
            0 => {
                f.root.max_member_bytes += 1;
                f.root.root_charge += 1;
                f.root.charged_bytes += 1;
            }
            1 => f.root.max_scopes -= 1,
            2 => f.root.max_calls += 1,
            _ => f.root.max_receipt_bytes += 1,
        }
        f.refresh();
        ScopedRun::decode(&f.root_row(3)).unwrap();
        f.rejects(false);
    }
}

#[test]
fn duplicate_calls_stay_monotonic_and_terminal_roots_cannot_admit() {
    let mut f = Fixture::new();
    f.root.calls.insert("a".repeat(64));
    f.root.receipt_charge = 132_096;
    f.refresh();
    f.validate();
    assert!(matches!(
        f.prepare(false),
        Err(HostProblem::IdempotencyConflict)
    ));
    f.rejects(false);
    f.root_actor.limits.max_effects = 1;
    f.root.max_calls = 1;
    f.refresh();
    f.select(&"b".repeat(64), "COUNT", false);
    f.validate();
    f.rejects(false);
    let mut f = Fixture::new();
    f.root.scopes.clear();
    f.root.root_charge = 0;
    f.root.charged_bytes = 0;
    f.root.ended_tick = Some(9);
    f.refresh();
    f.validate();
    f.rejects(false);
}

#[test]
fn exact_signed_versions_reject_overflow_and_accept_final_successor() {
    let mut f = Fixture::new();
    f.insert(
        f.root.root.clone(),
        "COUNT",
        None,
        false,
        None,
        i64::MAX as u64 - 1,
    );
    let prepared = f.prepare(false).unwrap();
    assert_eq!(prepared.target_write.record.version, i64::MAX as u64);
    f.postimage(&prepared);
    f.rows[0].version += 1;
    f.root.members.get_mut(&f.rows[0].key).unwrap().row_version += 1;
    f.refresh();
    f.validate();
    f.rejects(false);
    for version in [0, i64::MAX as u64, i64::MAX as u64 + 1, u64::MAX] {
        let f = Fixture::new();
        assert!(
            prepare(
                &f.root_actor,
                &f.source,
                &f.target,
                &f.source_entry,
                &f.target_entry,
                RootRow::Existing(&f.root_row(version)),
                &[],
                false
            )
            .is_err()
        );
    }
    let f = Fixture::new();
    let prepared = prepare(
        &f.root_actor,
        &f.source,
        &f.target,
        &f.source_entry,
        &f.target_entry,
        RootRow::Existing(&f.root_row(i64::MAX as u64 - 1)),
        &[],
        false,
    )
    .unwrap();
    assert_eq!(prepared.root_write.record.version, i64::MAX as u64);
}

#[test]
fn managed_source_requires_exact_busy_owner_even_when_other_entries_are_valid() {
    for case in 0..4 {
        let mut f = Fixture::new();
        f.select(&"a".repeat(64), "MID", false);
        let owner = f.target_entry.clone();
        if case != 0 {
            f.insert(
                f.root.root.clone(),
                "MID",
                (case != 1).then_some(owner.clone()),
                false,
                Some(vec![13, 17]),
                73,
            );
        }
        f.source = f.target.clone();
        f.source_entry = owner;
        if case == 2 {
            // Another independently valid actor of the same program/scope is
            // not the indexed busy owner, even with a valid binding and hash.
            f.source = child(&f.root_actor, &"c".repeat(64), "MID");
            f.source_entry = f
                .root
                .root
                .native_call(&f.root_actor, &f.source, &"c".repeat(64))
                .unwrap();
            f.source_entry.bind(&mut f.source).unwrap();
        } else if case == 3 {
            // A read-valid rehashed source actor cannot borrow another member.
            f.source_entry = rehashed(&f.source_entry, |v| {
                v["execution"] = json!(format!("online-call-execution-{}", "c".repeat(64)));
                v["call_key"] = json!("c".repeat(64));
            });
            f.source.execution_id = ExecutionId::new(
                format!("online-call-execution-{}", "c".repeat(64)),
                InvocationLimits::default(),
            )
            .unwrap();
            replace_binding(&mut f.source, &f.source_entry);
        }
        f.source_entry.validate_for(&f.source).unwrap();
        f.select(&"b".repeat(64), "COUNT", false);
        f.validate();
        f.rejects(false);
    }
}

#[test]
fn native_target_cannot_inherit_another_valid_scope_or_wrong_source_attempt() {
    let mut f = Fixture::new();
    f.select(&"a".repeat(64), "MID", true);
    let linked = f.target_entry.clone();
    let linked_actor = f.target.clone();
    f.root
        .scopes
        .insert(linked.scope_id().into(), linked.clone());
    f.insert(linked.clone(), "MID", Some(linked.clone()), false, None, 7);
    f.select(&"b".repeat(64), "COUNT", false);
    // The binding validates for the actual target but its immutable scope was
    // inherited from a different, already existing logical level.
    f.target_entry = rehashed(&f.target_entry, |v| {
        v["scope"] = serde_json::to_value(&linked).unwrap()["scope"].clone();
    });
    replace_binding(&mut f.target, &f.target_entry);
    f.target_entry.validate_for(&f.target).unwrap();
    f.validate();
    f.rejects(false);

    // Trusting each actor separately cannot skip the source/target attempt
    // relation. The forged entry itself is internally consistent.
    f.source = linked_actor;
    f.source_entry = linked;
    f.select(&"b".repeat(64), "COUNT", true);
    f.target.attempt = 2;
    f.target_entry = rehashed(&f.target_entry, |v| {
        v["attempt"] = json!(2);
        v["scope"]["owner_attempt"] = json!(2);
    });
    replace_binding(&mut f.target, &f.target_entry);
    f.target_entry.validate_for(&f.target).unwrap();
    f.rejects(false);
}

#[test]
fn explicit_absence_never_discards_existing_rows_or_managed_source() {
    let mut f = Fixture::new();
    f.insert(f.root.root.clone(), "OTHER", None, false, Some(vec![5]), 3);
    assert!(
        prepare(
            &f.root_actor,
            &f.source,
            &f.target,
            &f.source_entry,
            &f.target_entry,
            RootRow::Absent,
            &f.rows,
            false
        )
        .is_err()
    );
    f.select(&"a".repeat(64), "MID", false);
    f.source = f.target.clone();
    f.source_entry = f.target_entry.clone();
    f.select(&"b".repeat(64), "COUNT", false);
    assert!(
        prepare(
            &f.root_actor,
            &f.source,
            &f.target,
            &f.source_entry,
            &f.target_entry,
            RootRow::Absent,
            &[],
            false
        )
        .is_err()
    );
}

#[test]
fn root_wide_member_256_boundary_allows_reuse_but_rejects_new_native_or_link() {
    let mut f = Fixture::new();
    for n in 0..255 {
        f.insert(
            f.root.root.clone(),
            &format!("P{n}"),
            None,
            false,
            Some(vec![n as u8]),
            n + 1,
        );
    }
    f.validate();
    let prepared = f.prepare(false).unwrap();
    let root = f.postimage(&prepared);
    assert_eq!(root.members.len(), 256);
    assert_eq!(root.charged_bytes, 767);
    f.insert(
        f.root.root.clone(),
        "COUNT",
        None,
        false,
        Some(vec![41]),
        301,
    );
    f.validate();
    let prepared = f.prepare(false).unwrap();
    assert_eq!(prepared.state, Some(vec![41]));
    assert_eq!(f.postimage(&prepared).members.len(), 256);
    f.select(&"a".repeat(64), "NEW", false);
    f.rejects(false);
    f.select(&"a".repeat(64), "NEW", true);
    f.rejects(false);
}

#[test]
fn scope_limit_16_and_root_frame_limit_are_independent_root_wide_caps() {
    let mut f = Fixture::new();
    f.root_actor.limits.max_frames = 17;
    for n in 0..14 {
        let call = format!("{n:064x}");
        f.select(&call, "MID", true);
        f.root
            .scopes
            .insert(f.target_entry.scope_id().into(), f.target_entry.clone());
        f.insert(
            f.target_entry.clone(),
            "MID",
            Some(f.target_entry.clone()),
            false,
            None,
            n + 1,
        );
    }
    f.select(&"a".repeat(64), "COUNT", true);
    f.validate();
    let prepared = f.prepare(false).unwrap();
    let root = f.postimage(&prepared);
    assert_eq!(root.scopes.len(), 16);
    assert_eq!(root.active, 15);
    assert_eq!(root.members.len(), 15);
    assert_eq!(root.charged_bytes, 4096);
    assert_eq!(root.receipt_charge, 1_981_440);
    // Adopt the postimage only as the next fixture, without executing a store.
    f.root = root;
    f.rows.push(prepared.target_write.record);
    f.select(&"b".repeat(64), "NEXT", true);
    f.validate();
    f.rejects(false);
    f.select(&"b".repeat(64), "NEXT", false);
    let prepared = f.prepare(false).unwrap();
    assert_eq!(f.postimage(&prepared).active, 16);
    f.root = f.postimage(&prepared);
    f.rows.push(prepared.target_write.record);
    f.select(&"c".repeat(64), "EXCESS", false);
    f.validate();
    f.rejects(false);
}

#[test]
fn frame_edges_count_root_and_idle_members_do_not_consume_frames() {
    let mut f = Fixture::new();
    f.root_actor.limits.max_frames = 1;
    f.root = ScopedRun::fresh(&f.root_actor).unwrap();
    f.rejects(false);
    f.root_actor.limits.max_frames = 2;
    f.root = ScopedRun::fresh(&f.root_actor).unwrap();
    for n in 0..4 {
        f.insert(
            f.root.root.clone(),
            &format!("P{n}"),
            None,
            false,
            Some(vec![1]),
            3,
        );
    }
    let prepared = f.prepare(false).unwrap();
    assert_eq!(f.postimage(&prepared).active, 1);
    f.root = f.postimage(&prepared);
    f.rows.push(prepared.target_write.record);
    f.select(&"b".repeat(64), "OTHER", false);
    f.validate();
    f.rejects(false);
}

#[test]
fn stored_valid_storage_and_receipt_budgets_accept_near_signed_edge_only() {
    let mut f = Fixture::new();
    f.root_actor.limits.max_effects = 1;
    f.root_actor.limits.max_storage_bytes = (i64::MAX as u64 - 132_096) / 257;
    f.root = ScopedRun::fresh(&f.root_actor).unwrap();
    let prepared = f.prepare(false).unwrap();
    assert_eq!(
        f.postimage(&prepared).charged_bytes,
        2 * f.root_actor.limits.max_storage_bytes
    );
    f.root_actor.limits.max_storage_bytes += 1;
    f.rejects(false);
    assert!(ScopedRun::fresh(&f.root_actor).is_err());

    let mut f = Fixture::new();
    f.root_actor.limits.max_output_bytes = u64::MAX;
    assert!(
        prepare(
            &f.root_actor,
            &f.source,
            &f.target,
            &f.source_entry,
            &f.target_entry,
            RootRow::Absent,
            &[],
            false
        )
        .is_err()
    );
}

#[test]
fn rehashed_wrong_root_row_and_reused_link_scope_never_gain_admission() {
    let f = Fixture::new();
    let mut row = f.root_row(3);
    row.key = "e".repeat(64);
    let mut root = f.root.clone();
    root.refresh(&row.key).unwrap();
    row.payload = canonical(&root).unwrap();
    assert!(
        prepare(
            &f.root_actor,
            &f.source,
            &f.target,
            &f.source_entry,
            &f.target_entry,
            RootRow::Existing(&row),
            &[],
            false
        )
        .is_err()
    );

    let mut f = Fixture::new();
    f.select(&"a".repeat(64), "COUNT", true);
    let prepared = f.prepare(false).unwrap();
    f.root = f.postimage(&prepared);
    f.rows.push(prepared.target_write.record);
    f.validate();
    f.rejects(false);
    f.target_entry = f.root.root.clone();
    f.target = f.root_actor.clone();
    f.rejects(false);
}
