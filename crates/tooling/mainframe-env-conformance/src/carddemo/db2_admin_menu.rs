//! Selected actual admin-menu integration; no SQL, cursor, rollback or IBM credit.
use super::*;
use mainframe_env_cics::{
    CicsLimits, CicsReplayCodecVersion, CicsReplayRetentionState, CicsTraceEntry,
    describe_cics_replay_row,
};
use mainframe_env_execution_api::{
    AuditDecision, AuditResourceDigestFormat, LifecycleEvent, LifecycleEventKind,
};
use mainframe_env_store::LocalArtifactStore;
use mainframe_env_store_api::{
    ArtifactRecord, ArtifactStore, ExecutionRecord, ExecutionState, PlatformStore,
    ProviderStateRecord,
};

mod pins;
#[cfg(test)]
mod tests;

const SELECTED: [&str; 3] = ["COADM01C", "COTRTLIC", "COTRTUPC"];
const LABEL5: &[u8] = b"05. Transaction Type List/Update (Db2)  ";
const LABEL6: &[u8] = b"06. Transaction Type Maintenance (Db2)  ";
const MAX_ROWS: usize = 512;
const MAX_BYTES: usize = 2 * 1024 * 1024;

fn refused() -> CorpusProblem {
    CorpusProblem::new(
        "carddemo.db2.admin_menu_binding",
        "selected source, menu, installed artifact or actual actor evidence differs",
    )
}
fn require(value: bool) -> Result<(), CorpusProblem> {
    if value { Ok(()) } else { Err(refused()) }
}

pub(super) fn check_inputs(corpus: &Path) -> Result<(), CorpusProblem> {
    for (path, digest) in pins::SOURCE_PINS {
        let bytes = read_corpus_file(corpus, &corpus.join(path))?;
        require(format!("{:x}", Sha256::digest(bytes)) == *digest)?;
    }
    Ok(())
}

type SourceFiles = BTreeMap<String, (String, SourceFormat, SourceEncoding)>;
fn check_source_closure(
    name: &str,
    primary: &str,
    bundle: &SourceBundle,
) -> Result<(), CorpusProblem> {
    let is_db2 = name != "COADM01C";
    let expected_primary = if is_db2 {
        format!("app/app-transaction-type-db2/cbl/{name}.cbl")
    } else {
        "app/cbl/COADM01C.cbl".into()
    };
    let expected_options = if is_db2 {
        BTreeMap::from([("cobol.sql-precompile".into(), "true".into())])
    } else {
        BTreeMap::new()
    };
    require(
        SELECTED.contains(&name)
            && primary == expected_primary
            && bundle
                .file(bundle.primary())
                .map(|file| file.path().as_str())
                == Some(primary)
            && bundle.options() == &expected_options
            && bundle.provenance().is_empty(),
    )?;
    let limits = SourceLimits::default();
    let mut expected = SourceFiles::new();
    for (path, digest) in pins::SOURCE_PINS {
        if *path == primary
            || pins::COPY_ROOTS
                .iter()
                .any(|root| path.starts_with(&format!("{root}/")))
            || is_db2 && path.starts_with("app/app-transaction-type-db2/dcl/")
        {
            require(
                expected
                    .insert(
                        (*path).into(),
                        ((*digest).into(), SourceFormat::Fixed, SourceEncoding::Utf8),
                    )
                    .is_none(),
            )?;
        }
    }
    require(expected.len() == if is_db2 { 65 } else { 63 })?;
    // Repository-authored ABI inputs are separately qualified local authority.
    // They are not first-party source text or an IBM execution oracle.
    let abi = subsystem_abi_libraries(limits)?;
    for file in &abi.files {
        require(
            expected
                .insert(
                    file.path().as_str().into(),
                    (
                        format!("{:x}", Sha256::digest(file.bytes())),
                        file.format(),
                        file.encoding(),
                    ),
                )
                .is_none(),
        )?;
    }
    let mut actual = SourceFiles::new();
    for file in bundle.files() {
        require(
            actual
                .insert(
                    file.path().as_str().into(),
                    (
                        format!("{:x}", Sha256::digest(file.bytes())),
                        file.format(),
                        file.encoding(),
                    ),
                )
                .is_none(),
        )?;
    }
    require(expected == actual)?;
    let mut libraries = Vec::new();
    for (index, root) in pins::COPY_ROOTS.iter().enumerate() {
        let members = pins::SOURCE_PINS
            .iter()
            .filter(|(path, _)| path.starts_with(&format!("{root}/")))
            .map(|(path, _)| LogicalPath::new(*path, limits.max_path_bytes))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| refused())?;
        libraries.push(
            SourceLibrary::new(format!("application-{index:02}"), members, limits)
                .map_err(|_| refused())?,
        );
    }
    libraries.extend(abi.libraries);
    if is_db2 {
        let members = pins::SOURCE_PINS
            .iter()
            .filter(|(path, _)| path.starts_with("app/app-transaction-type-db2/dcl/"))
            .map(|(path, _)| LogicalPath::new(*path, limits.max_path_bytes))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| refused())?;
        let dcl = SourceLibrary::new("db2-dcl", members, limits).map_err(|_| refused())?;
        libraries.insert(libraries.len().saturating_sub(1), dcl);
    }
    require(libraries == bundle.libraries())
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Installed {
    programs: BTreeMap<String, ProviderStateRecord>,
    transactions: BTreeMap<String, ProviderStateRecord>,
    selected_artifacts: BTreeMap<String, ArtifactRecord>,
}
pub(super) struct Expectation {
    publishers: BTreeMap<String, ArtifactRecord>,
    expected: Option<Installed>,
}
pub(super) struct Proof {
    expected: Installed,
    selected: BTreeSet<u8>,
    reopened: bool,
}

fn state(namespace: &str, key: &str, payload: Vec<u8>) -> ProviderStateRecord {
    ProviderStateRecord {
        namespace: namespace.into(),
        key: key.into(),
        version: 1,
        payload,
    }
}
impl Expectation {
    pub(super) fn new(corpus: &Path) -> Result<Self, CorpusProblem> {
        check_inputs(corpus)?;
        Ok(Self {
            publishers: BTreeMap::new(),
            expected: None,
        })
    }
    /// Called beside the original compile request and its actual returned publisher.
    pub(super) fn observe_publisher(
        &mut self,
        name: &str,
        primary: &str,
        bundle: &SourceBundle,
        program: &OnlineProgramDefinition,
    ) -> Result<(), CorpusProblem> {
        if !SELECTED.contains(&name) {
            return Ok(());
        }
        require(program.name == name)?;
        check_source_closure(name, primary, bundle)?;
        let record = source_artifact_binding::publisher_record(program)?;
        require(self.publishers.insert(name.into(), record).is_none())
    }
    pub(super) fn finish(
        &mut self,
        offered: &OnlineApplicationDefinition,
    ) -> Result<(), CorpusProblem> {
        let expected_names = pins::PROGRAMS
            .iter()
            .map(|name| (*name).to_string())
            .collect::<BTreeSet<_>>();
        let transactions = pins::TRANSACTIONS
            .iter()
            .map(|(tx, pgm)| ((*tx).into(), (*pgm).into()))
            .collect::<BTreeMap<String, String>>();
        require(
            offered.programs.len() == 20
                && offered
                    .programs
                    .iter()
                    .map(|pgm| pgm.name.clone())
                    .collect::<BTreeSet<_>>()
                    == expected_names
                && offered.transactions == transactions
                && self
                    .publishers
                    .keys()
                    .map(String::as_str)
                    .collect::<BTreeSet<_>>()
                    == BTreeSet::from(SELECTED),
        )?;
        let mut programs = BTreeMap::new();
        for program in &offered.programs {
            if SELECTED.contains(&program.name.as_str()) {
                require(
                    self.publishers.get(&program.name)
                        == Some(&source_artifact_binding::publisher_record(program)?),
                )?;
            }
            require(
                programs
                    .insert(
                        program.name.clone(),
                        state(
                            "online-program",
                            &program.name,
                            program.artifact.as_str().as_bytes().to_vec(),
                        ),
                    )
                    .is_none(),
            )?;
        }
        let transactions = transactions
            .into_iter()
            .map(|(tx, pgm)| {
                (
                    tx.clone(),
                    state("online-transaction", &tx, pgm.into_bytes()),
                )
            })
            .collect();
        require(
            self.expected
                .replace(Installed {
                    programs,
                    transactions,
                    selected_artifacts: self.publishers.clone(),
                })
                .is_none(),
        )
    }
    pub(super) fn observe_installed(
        self,
        corpus: &Path,
        store: &dyn PlatformStore,
        root: &Path,
    ) -> Result<Proof, CorpusProblem> {
        let expected = self.expected.ok_or_else(refused)?;
        require_installed(corpus, store, root, &expected)?;
        Ok(Proof {
            expected,
            selected: BTreeSet::new(),
            reopened: false,
        })
    }
}
fn read_states(
    store: &dyn PlatformStore,
    namespace: &str,
    max: usize,
) -> Result<BTreeMap<String, ProviderStateRecord>, CorpusProblem> {
    let rows = store
        .list_provider_state_bounded(namespace, max + 1, 32 * 1024)
        .map_err(|_| refused())?;
    require(rows.len() <= max)?;
    let mut result = BTreeMap::new();
    for row in rows {
        require(row.namespace == namespace && row.version == 1)?;
        require(result.insert(row.key.clone(), row).is_none())?;
    }
    Ok(result)
}
fn require_installed(
    corpus: &Path,
    store: &dyn PlatformStore,
    root: &Path,
    expected: &Installed,
) -> Result<(), CorpusProblem> {
    check_inputs(corpus)?;
    let mut actual = Installed {
        programs: read_states(store, "online-program", 20)?,
        transactions: read_states(store, "online-transaction", 19)?,
        selected_artifacts: BTreeMap::new(),
    };
    require(actual.programs == expected.programs && actual.transactions == expected.transactions)?;
    // This is the exclusively owned fixture root used by ProductServer. Opening
    // LocalArtifactStore may sync its directory: no general read-only API claim.
    let artifacts = LocalArtifactStore::open(root, 64 * 1024 * 1024).map_err(|_| refused())?;
    for name in SELECTED {
        let row = actual.programs.get(name).ok_or_else(refused)?;
        let reference = ArtifactRef::new(
            String::from_utf8(row.payload.clone()).map_err(|_| refused())?,
            InvocationLimits::default(),
        )
        .map_err(|_| refused())?;
        let record = artifacts
            .get_artifact(&reference)
            .map_err(|_| refused())?
            .ok_or_else(refused)?;
        require(
            actual
                .selected_artifacts
                .insert(name.into(), record)
                .is_none(),
        )?;
    }
    compare_installed(expected, &actual)
}
fn compare_installed(expected: &Installed, actual: &Installed) -> Result<(), CorpusProblem> {
    require(expected == actual)
}

pub(super) struct Before {
    rows: BTreeMap<String, ProviderStateRecord>,
    trace: Vec<CicsTraceEntry>,
}
#[derive(Clone, Copy)]
pub(super) struct Context<'a> {
    corpus: &'a Path,
    store: &'a dyn PlatformStore,
    server: &'a ProductServer,
    root: &'a Path,
    session: &'a str,
}
impl<'a> Context<'a> {
    pub(super) fn new(
        corpus: &'a Path,
        store: &'a dyn PlatformStore,
        server: &'a ProductServer,
        root: &'a Path,
        session: &'a str,
    ) -> Self {
        Self {
            corpus,
            store,
            server,
            root,
            session,
        }
    }
}
pub(super) fn before(
    store: &dyn PlatformStore,
    server: &ProductServer,
    session: &str,
) -> Result<Before, CorpusProblem> {
    let rows = store
        .list_provider_state_bounded("cics-effect-replay-v1", MAX_ROWS + 1, MAX_BYTES)
        .map_err(|_| refused())?;
    let trace = server.online_trace(session).map_err(|_| refused())?;
    require(rows.len() <= MAX_ROWS && trace.len() <= MAX_ROWS)?;
    let mut states = BTreeMap::new();
    for row in rows {
        require(row.namespace == "cics-effect-replay-v1")?;
        require(states.insert(row.key.clone(), row).is_none())?;
    }
    Ok(Before {
        rows: states,
        trace,
    })
}
fn menu_matches(menu: &serde_json::Value, session: &str) -> Result<bool, CorpusProblem> {
    let fields = online_screen_fields(menu)?;
    Ok(surface_matches(menu, session, "COADM01", "COADM1A")
        && fields.get("PGMNAME").is_some_and(|v| v == b"COADM01C")
        && fields.get("TRNNAME").is_some_and(|v| v == b"CA00")
        && fields.get("OPTN005").is_some_and(|v| v == LABEL5)
        && fields.get("OPTN006").is_some_and(|v| v == LABEL6))
}
fn target_matches(
    terminal: &serde_json::Value,
    session: &str,
    option: u8,
) -> Result<bool, CorpusProblem> {
    let (program, transaction, mapset, map) = target(option).ok_or_else(refused)?;
    let fields = online_screen_fields(terminal)?;
    Ok(surface_matches(terminal, session, mapset, map)
        && fields
            .get("PGMNAME")
            .is_some_and(|v| v == program.as_bytes())
        && fields
            .get("TRNNAME")
            .is_some_and(|v| v == transaction.as_bytes()))
}
fn surface_matches(terminal: &serde_json::Value, session: &str, mapset: &str, map: &str) -> bool {
    terminal["session"] == session
        && terminal["principal"] == "WEBADM"
        && terminal["connected"] == true
        && terminal["suspended"] == false
        && terminal["mapset"] == mapset
        && terminal["map"] == map
}
fn target(option: u8) -> Option<(&'static str, &'static str, &'static str, &'static str)> {
    match option {
        5 => Some(("COTRTLIC", "CTLI", "COTRTLI", "CTRTLIA")),
        6 => Some(("COTRTUPC", "CTTU", "COTRTUP", "CTRTUPA")),
        _ => None,
    }
}
fn actor_matches(
    owner: &ExecutionRecord,
    name: &str,
    artifact: &ArtifactRef,
    run: &RunUnitId,
) -> bool {
    owner.selector.as_str() == format!("program:{name}")
        && owner.artifact == *artifact
        && owner.run_unit_id == *run
        && owner.principal.as_str() == "WEBADM"
        && owner.attempt > 0
        && owner.state == ExecutionState::Completed
        && owner.terminal_tick.is_some_and(|tick| tick > 0)
}
fn event_matches(owner: &ExecutionRecord, event: &LifecycleEvent, menu: bool) -> bool {
    let kind = if menu {
        LifecycleEventKind::HandoffCompleted
    } else {
        LifecycleEventKind::Completed { return_code: 0 }
    };
    event.validate()
        && event.execution_id == owner.execution_id
        && event.run_unit_id == owner.run_unit_id
        && event.sequence == owner.version
        && event.attempt == owner.attempt
        && Some(event.tick) == owner.terminal_tick
        && event.tick > 0
        && event.kind == kind
}
impl Proof {
    pub(super) fn observe_selection(
        &mut self,
        context: &Context<'_>,
        menu: &serde_json::Value,
        terminal: &serde_json::Value,
        before: Before,
        option: u8,
    ) -> Result<(), CorpusProblem> {
        let Context {
            corpus,
            store,
            server,
            root,
            session,
        } = *context;
        let (program, _, _, _) = target(option).ok_or_else(refused)?;
        require(
            !self.selected.contains(&option)
                && !self.reopened
                && menu_matches(menu, session)?
                && target_matches(terminal, session, option)?,
        )?;
        require_installed(corpus, store, root, &self.expected)?;
        let run = RunUnitId::new(
            terminal["run_unit"].as_str().ok_or_else(refused)?,
            InvocationLimits::default(),
        )
        .map_err(|_| refused())?;
        let after = self::before(store, server, session)?;
        require(
            after.trace.starts_with(&before.trace)
                && before
                    .rows
                    .iter()
                    .all(|(key, row)| after.rows.get(key) == Some(row)),
        )?;
        let fresh_trace = &after.trace[before.trace.len()..];
        let transfers = fresh_trace
            .iter()
            .filter(|entry| entry.operation == CicsOperation::Xctl)
            .collect::<Vec<_>>();
        require(
            matches!(transfers.as_slice(), [entry] if entry.outcome == "NORMAL"
            && entry.response == 0 && entry.response2 == 0),
        )?;
        let mut owners = BTreeMap::<String, ExecutionRecord>::new();
        for (key, row) in &after.rows {
            if before.rows.contains_key(key) {
                continue;
            }
            let key =
                IdempotencyKey::new(key, InvocationLimits::default()).map_err(|_| refused())?;
            let effect = store
                .effect(&key)
                .map_err(|_| refused())?
                .ok_or_else(refused)?;
            let descriptor = describe_cics_replay_row(row, Some(&effect), CicsLimits::default())
                .map_err(|_| refused())?;
            require(
                descriptor.codec == CicsReplayCodecVersion::RetentionV3
                    && descriptor.retention == CicsReplayRetentionState::Terminal
                    && descriptor.owner_run_unit.as_deref() == Some(run.as_str())
                    && descriptor.owner_execution.as_deref() == Some(effect.execution_id.as_str())
                    && effect.run_unit_id == run,
            )?;
            let owner = store
                .get_execution(&effect.execution_id)
                .map_err(|_| refused())?
                .ok_or_else(refused)?;
            let name = owner
                .selector
                .as_str()
                .strip_prefix("program:")
                .ok_or_else(refused)?;
            require(name == "COADM01C" || name == program)?;
            let record = self
                .expected
                .selected_artifacts
                .get(name)
                .ok_or_else(refused)?;
            require(
                actor_matches(&owner, name, &record.artifact, &run)
                    && owner.execution_id == effect.execution_id
                    && effect.intent.owner == owner.execution_id
                    && effect.intent.attempt == owner.attempt
                    && effect.intent.capability.as_ref().map(CapabilityId::as_str)
                        == Some("host.cics.execute"),
            )?;
            let events = store
                .events(&owner.execution_id, owner.version, 2)
                .map_err(|_| refused())?;
            require(
                matches!(events.as_slice(), [event] if event_matches(&owner, event, name == "COADM01C")),
            )?;
            let resource = effect.intent.audit_resource.as_ref().ok_or_else(refused)?;
            let invocation = effect
                .intent
                .audit_invocation_key
                .as_ref()
                .ok_or_else(refused)?;
            require(resource.format == AuditResourceDigestFormat::CanonicalHostResourceV1)?;
            let audits = store
                .audit_records(&owner.execution_id, 0, MAX_ROWS + 1)
                .map_err(|_| refused())?;
            require(audits.len() <= MAX_ROWS)?;
            let matches = audits
                .iter()
                .filter(|audit| {
                    audit.effect_sequence == effect.sequence
                        && audit.capability.as_str() == "host.cics.execute"
                })
                .collect::<Vec<_>>();
            require(
                matches!(matches.as_slice(), [audit] if audit.execution_id == owner.execution_id
                && audit.run_unit_id == run && audit.principal == owner.principal
                && audit.attempt == owner.attempt && audit.invocation_key == *invocation
                && audit.resource == *resource && audit.decision == AuditDecision::Success),
            )?;
            require(owners.get(name).is_none_or(|previous| previous == &owner))?;
            owners.insert(name.into(), owner);
        }
        require(
            owners.len() == 2 && owners.contains_key("COADM01C") && owners.contains_key(program),
        )?;
        require(self.selected.insert(option))
    }
    pub(super) fn observe_reopened(
        &mut self,
        corpus: &Path,
        store: &dyn PlatformStore,
        root: &Path,
    ) -> Result<(), CorpusProblem> {
        require(!self.reopened && self.selected == BTreeSet::from([5, 6]))?;
        require_installed(corpus, store, root, &self.expected)?;
        self.reopened = true;
        Ok(())
    }
    /// Owning caller invokes this only after both checked drains and exact-root cleanup.
    pub(super) fn bind(self, observations: &mut RouteObservations) -> Result<(), CorpusProblem> {
        require(self.reopened && self.selected == BTreeSet::from([5, 6]))?;
        observations.compare(
            journey_closure::AuthorityKind::Journey,
            "CD.J13",
            "admin-menu integration",
            false,
            || Ok(refused()),
        )
    }
}
