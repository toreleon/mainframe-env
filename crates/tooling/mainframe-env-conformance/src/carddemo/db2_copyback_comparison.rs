//! Actual ordinary CTLI SQL storage; zero application, licensed ABI or journey credit.
//! Full expected/raw bytes stay private; outward receipts contain technical metadata only.
use super::*;
use mainframe_env_execution_api::AuditResourceDigestFormat;
use mainframe_env_interpreter::{Db2CopybackPlan, Db2CopybackSlot, Db2CopybackValue};
use mainframe_env_ir::{Attribute, CodecLimits, Module, Operation, StorageId, decode_binary};
use mainframe_env_server::{
    ArmedDb2CopybackObservation, CompletedDb2Copyback, Db2ObserverManager,
    SourceBoundDb2CopybackProfile,
};
use mainframe_env_store_api::{ArtifactRecord, ArtifactStore, ExecutionState, PlatformStore};
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
#[cfg(test)]
#[path = "db2_copyback_comparison/tests.rs"]
mod tests;
const SOURCE: &str = "916a5fe2279ad626147990d8f86fac1e4b296f827889485b020cc310b5323601";
const TARGETS: [(&str, usize); 4] = [
    ("SQLCA.SQLCODE", 4),
    ("SQLCA.SQLSTATE", 5),
    ("WS-MISC-STORAGE.WS-DB2-COMMON-VARS.WS-DUMMY-DB2-INT", 3),
    ("DCLTRANSACTION-TYPE", 54),
];
const ROWS: [(&str, usize, &str); 7] = [
    (
        "dcl-row-01.bin",
        54,
        "ae6e42d1e7f457e858bf64a8e7ef544ca08c62b2175cd87bd46d2033a9e01735",
    ),
    (
        "dcl-row-02.bin",
        54,
        "28f56c809173018d66f955fe0b5a4be6017106e5342bcbdaa672e7939ff1b1d3",
    ),
    (
        "dcl-row-03.bin",
        54,
        "c829f1c15e08a27285ee35eb4c3dae18c53772d75f30763d093bf5bffc152024",
    ),
    (
        "dcl-row-04.bin",
        54,
        "2bde12dfa3747ebd2a9d85d80d6df1b7a7ea16837238121bc3a93675c547dbb0",
    ),
    (
        "dcl-row-05.bin",
        54,
        "4db3ab488ec84cc8ad657c763441787f0341ad57767272ac7d34e2e42778b562",
    ),
    (
        "dcl-row-06.bin",
        54,
        "7fa510d5c0d55afccc4bbf4216ecabfcfa9a0d3b66035e2a9af622ff10ca3bba",
    ),
    (
        "dcl-row-07.bin",
        54,
        "7a245231f005e0b8c36811d54dc131cf97ff5683c0ee163324124d66654aa8ce",
    ),
];
const FIXED: [(&str, usize, &str); 5] = [
    (
        "sqlcode0-local-comp5.bin",
        4,
        "df3f619804a92fdb4057192dc43dd748ea778adc52bc498ce80524c014b81119",
    ),
    (
        "sqlcode100-local-comp5.bin",
        4,
        "3655ca59b7d566ae06297c200f98d04da2e8e89812d627bc29297c25db60362d",
    ),
    (
        "sqlstate00000.bin",
        5,
        "e7042ac7d09c7bc41c8cfa5749e41858f6980643bc0db1a83cc793d3e24d3f77",
    ),
    (
        "sqlstate02000.bin",
        5,
        "3e4ccbd3e078e820cfd04bc68d4fe661004ada92785b0a1999055f35351cc749",
    ),
    (
        "priming1-local-packed.bin",
        3,
        "7919f0bae8dce9f13cfa8fd7acca5d88265af7a3aa92bc86b742997217813559",
    ),
];
fn problem() -> CorpusProblem {
    CorpusProblem::new(
        "carddemo.db2.ordinary_sql_copyback",
        "actual SQL postimage, source oracle or completed owner differs",
    )
}
fn require(value: bool) -> Result<(), CorpusProblem> {
    if value { Ok(()) } else { Err(problem()) }
}
fn digest(value: &str) -> Result<[u8; 32], CorpusProblem> {
    require(value.len() == 64)?;
    let mut bytes = [0; 32];
    for (index, pair) in value.as_bytes().as_chunks::<2>().0.iter().enumerate() {
        bytes[index] = u8::from_str_radix(std::str::from_utf8(pair).map_err(|_| problem())?, 16)
            .map_err(|_| problem())?;
    }
    Ok(bytes)
}
#[derive(Clone)]
struct Slot {
    descriptor: Db2CopybackSlot,
    base: usize,
    offset: usize,
    length: usize,
}
fn selected_slot(module: &Module, name: &str, width: usize) -> Result<Slot, CorpusProblem> {
    // MIR uses lowercase names; the live machine's descriptor remains uppercase.
    let ir_name = name.to_ascii_lowercase();
    let matches = module
        .storage()
        .iter()
        .filter(|slot| slot.name == ir_name)
        .collect::<Vec<_>>();
    let [selected] = matches.as_slice() else {
        return Err(problem());
    };
    require(selected.size == width as u64)?;
    let mut current = *selected;
    let mut offset = 0u64;
    let mut visited = BTreeSet::<StorageId>::new();
    while let Some(alias) = &current.alias_of {
        require(visited.insert(current.id) && visited.len() <= module.storage().len())?;
        require(alias.length == current.size)?;
        offset = offset.checked_add(alias.offset).ok_or_else(problem)?;
        current = module
            .storage()
            .iter()
            .find(|item| item.id == alias.storage)
            .ok_or_else(problem)?;
    }
    require(
        offset
            .checked_add(width as u64)
            .is_some_and(|end| end <= current.size),
    )?;
    let base = module
        .storage()
        .iter()
        .filter(|slot| slot.alias_of.is_none())
        .position(|slot| slot.id == current.id)
        .ok_or_else(problem)?;
    Ok(Slot {
        descriptor: Db2CopybackSlot::new(selected.id, name.into(), width).map_err(|_| problem())?,
        base,
        offset: usize::try_from(offset).map_err(|_| problem())?,
        length: width,
    })
}

fn load_expected(
    root: &Path,
    name: &str,
    width: usize,
    hash: &str,
) -> Result<Vec<u8>, CorpusProblem> {
    let path = root.join(name);
    let before = fs::symlink_metadata(&path).map_err(|_| problem())?;
    require(
        before.file_type().is_file()
            && before.len() == width as u64
            && before.nlink() == 1
            && before.permissions().mode() & 0o777 == 0o600,
    )?;
    let mut file = fs::File::open(&path).map_err(|_| problem())?;
    let held = file.metadata().map_err(|_| problem())?;
    require((before.dev(), before.ino()) == (held.dev(), held.ino()))?;
    let mut bytes = Vec::with_capacity(width + 1);
    (&mut file)
        .take(width as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| problem())?;
    let after = fs::symlink_metadata(&path).map_err(|_| problem())?;
    let final_held = file.metadata().map_err(|_| problem())?;
    require(
        (
            before.dev(),
            before.ino(),
            before.len(),
            before.mode(),
            before.nlink(),
        ) == (
            after.dev(),
            after.ino(),
            after.len(),
            after.mode(),
            after.nlink(),
        ) && (
            before.dev(),
            before.ino(),
            before.len(),
            before.mode(),
            before.nlink(),
        ) == (
            final_held.dev(),
            final_held.ino(),
            final_held.len(),
            final_held.mode(),
            final_held.nlink(),
        ) && bytes.len() == width
            && format!("{:x}", Sha256::digest(&bytes)) == hash,
    )?;
    Ok(bytes)
}
fn operations() -> Vec<Db2Operation> {
    let mut result = vec![Db2Operation::Select, Db2Operation::OpenCursor];
    result.extend([Db2Operation::FetchCursor; 8]);
    result.push(Db2Operation::CloseCursor);
    result
}
fn arguments(operation: &Operation) -> Result<Vec<String>, CorpusProblem> {
    if let Some(Attribute::Bytes(bytes)) = operation.attributes.get("arguments") {
        let mut cursor = 0usize;
        let mut result = Vec::new();
        while cursor < bytes.len() {
            let end = cursor.checked_add(8).ok_or_else(problem)?;
            let length = u64::from_be_bytes(
                bytes
                    .get(cursor..end)
                    .ok_or_else(problem)?
                    .try_into()
                    .map_err(|_| problem())?,
            );
            cursor = end;
            let end = cursor
                .checked_add(usize::try_from(length).map_err(|_| problem())?)
                .ok_or_else(problem)?;
            result.push(
                String::from_utf8(bytes.get(cursor..end).ok_or_else(problem)?.to_vec())
                    .map_err(|_| problem())?,
            );
            require(result.len() <= 65_536)?;
            cursor = end;
        }
        return Ok(result);
    }
    Ok(operation
        .attributes
        .iter()
        .filter_map(|(key, value)| match value {
            Attribute::Text(value) if key.starts_with("arg_") => Some(value.clone()),
            _ => None,
        })
        .collect())
}
fn source_sites(module: &Module) -> Result<Vec<usize>, CorpusProblem> {
    let mut select = Vec::new();
    let mut open = Vec::new();
    let mut fetch = Vec::new();
    let mut close = Vec::new();
    for (pc, operation) in module
        .regions()
        .iter()
        .flat_map(|region| &region.blocks)
        .flat_map(|block| &block.operations)
        .enumerate()
    {
        if operation.identity.name() != "exec_sql" {
            continue;
        }
        let args = arguments(operation)?;
        let tokens = args
            .iter()
            .map(String::as_str)
            .filter(|token| !matches!(*token, "SQL" | "END-EXEC"))
            .collect::<Vec<_>>();
        match tokens.as_slice() {
            ["SELECT", ..] if tokens.contains(&":WS-DUMMY-DB2-INT") => select.push(pc),
            ["OPEN", "C-TR-TYPE-FORWARD", ..] => open.push(pc),
            ["FETCH", "C-TR-TYPE-FORWARD", ..] => fetch.push(pc),
            ["CLOSE", "C-TR-TYPE-FORWARD", ..] => close.push(pc),
            _ => {}
        }
    }
    require(select.len() == 1 && open.len() == 1 && fetch.len() == 2 && close.len() == 1)?;
    let mut sites = vec![select[0], open[0]];
    sites.extend([fetch[0]; 7]);
    sites.push(fetch[1]);
    sites.push(close[0]);
    Ok(sites)
}
pub(super) struct Prepared {
    record: ArtifactRecord,
    plan: Db2CopybackPlan,
    slots: Vec<Slot>,
    sites: Vec<usize>,
    rows: Vec<Vec<u8>>,
    fixed: Vec<Vec<u8>>,
}
impl Prepared {
    pub(super) fn new(
        corpus: &Path,
        programs: &[OnlineProgramDefinition],
    ) -> Result<Self, CorpusProblem> {
        db2_online_rows::check_sources(corpus)?;
        let candidates = programs
            .iter()
            .filter(|program| program.name == "COTRTLIC")
            .collect::<Vec<_>>();
        let [definition] = candidates.as_slice() else {
            return Err(problem());
        };
        require(
            format!(
                "{:x}",
                Sha256::digest(read_corpus_file(
                    corpus,
                    &corpus.join("app/app-transaction-type-db2/cbl/COTRTLIC.cbl")
                )?)
            ) == SOURCE,
        )?;
        let record = source_artifact_binding::publisher_record(definition)?;
        let module =
            decode_binary(&record.payload, CodecLimits::default()).map_err(|_| problem())?;
        let slots = TARGETS
            .iter()
            .map(|(name, width)| selected_slot(&module, name, *width))
            .collect::<Result<Vec<_>, _>>()?;
        let plan = Db2CopybackPlan::new(
            operations(),
            slots.iter().map(|slot| slot.descriptor.clone()).collect(),
        )
        .map_err(|_| problem())?;
        let offered = env::var_os("CARDDEMO_DB2_COPYBACK_ORACLE_DIR").ok_or_else(problem)?;
        let root = Path::new(&offered);
        let rows = ROWS
            .iter()
            .map(|(name, width, hash)| load_expected(root, name, *width, hash))
            .collect::<Result<Vec<_>, _>>()?;
        let fixed = FIXED
            .iter()
            .map(|(name, width, hash)| load_expected(root, name, *width, hash))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            record,
            plan,
            slots,
            sites: source_sites(&module)?,
            rows,
            fixed,
        })
    }
    pub(super) fn arm(
        &self,
        manager: &Db2ObserverManager,
        session: &str,
    ) -> Result<ArmedDb2CopybackObservation, CorpusProblem> {
        let limits = InvocationLimits::default();
        let profile = SourceBoundDb2CopybackProfile::new(
            SessionId::new(session, limits.max_binding_bytes).map_err(|_| problem())?,
            Selector::new("program:COTRTLIC", limits).map_err(|_| problem())?,
            PrincipalId::new("WEBADM", limits).map_err(|_| problem())?,
            self.record.artifact.clone(),
            self.record
                .executable
                .as_ref()
                .ok_or_else(problem)?
                .semantic_identity
                .clone(),
            digest(SOURCE)?,
            self.plan.clone(),
        )
        .map_err(|_| problem())?;
        manager.arm(profile).map_err(|_| problem())
    }
    pub(super) fn installed(&self, root: &Path) -> Result<(), CorpusProblem> {
        let store = mainframe_env_store::LocalArtifactStore::open(root, 64 * 1024 * 1024)
            .map_err(|_| problem())?;
        require(
            store
                .get_artifact(&self.record.artifact)
                .map_err(|_| problem())?
                == Some(self.record.clone()),
        )
    }
    pub(super) fn compare(
        &self,
        complete: &CompletedDb2Copyback,
        terminal: &serde_json::Value,
        session: &str,
        store: &dyn PlatformStore,
        root: &Path,
    ) -> Result<(), CorpusProblem> {
        self.installed(root)?;
        let origin = complete.archive().origin();
        let actor = store
            .get_execution(origin.execution())
            .map_err(|_| problem())?
            .ok_or_else(problem)?;
        require(
            terminal_matches(terminal, session, origin.run().as_str())
                && complete.session().as_str() == session
                && complete.asserted_source_digest() == &digest(SOURCE)?
                && complete.payload_digest() == &self.record.payload_digest
                && complete.semantic_identity()
                    == self
                        .record
                        .executable
                        .as_ref()
                        .ok_or_else(problem)?
                        .semantic_identity
                && origin.artifact() == &self.record.artifact
                && origin.selector().as_str() == "program:COTRTLIC"
                && origin.principal().as_str() == "WEBADM"
                && actor.execution_id == *origin.execution()
                && actor.run_unit_id == *origin.run()
                && actor.principal == *origin.principal()
                && actor.artifact == *origin.artifact()
                && actor.selector == *origin.selector()
                && actor.attempt == origin.attempt()
                && actor.state == ExecutionState::Completed
                && actor.version == complete.terminal_version()
                && actor.terminal_tick == Some(complete.terminal_tick())
                && actor.owner_lease.is_none()
                && actor.lease_expiry_tick.is_none(),
        )?;
        let data = complete
            .archive()
            .records()
            .iter()
            .map(|record| Data {
                operation: record.operation(),
                sequence: record.sequence(),
                pc: record.program_counter(),
                keyed: record.key().is_some(),
                inputs: record.input_count(),
                indicators: record.present_indicators(),
                sqlcode: record.provider_sqlcode(),
                sqlstate: *record.provider_sqlstate(),
                affected: record.provider_affected_rows(),
                rows: record.provider_rows(),
                canonical_resource: record.resource().format
                    == AuditResourceDigestFormat::CanonicalHostResourceV1,
                cells: record
                    .cells()
                    .iter()
                    .map(|cell| Cell {
                        id: cell.selected().storage().get(),
                        name: cell.selected().name().into(),
                        base: cell.base(),
                        offset: cell.offset(),
                        bytes: cell.bytes().to_vec(),
                    })
                    .collect(),
                implicit: record.implicit().to_vec(),
            })
            .collect::<Vec<_>>();
        compare_data(&data, &self.slots, &self.sites, &self.rows, &self.fixed)
    }
}
#[derive(Clone)]
struct Cell {
    id: u32,
    name: String,
    base: usize,
    offset: usize,
    bytes: Vec<u8>,
}
#[derive(Clone)]
struct Data {
    operation: Db2Operation,
    sequence: u64,
    pc: usize,
    keyed: bool,
    inputs: usize,
    indicators: usize,
    sqlcode: i32,
    sqlstate: [u8; 5],
    affected: u64,
    rows: usize,
    canonical_resource: bool,
    cells: Vec<Cell>,
    implicit: Vec<(&'static str, Db2CopybackValue)>,
}
fn compare_data(
    data: &[Data],
    slots: &[Slot],
    sites: &[usize],
    rows: &[Vec<u8>],
    fixed: &[Vec<u8>],
) -> Result<(), CorpusProblem> {
    require(
        data.len() == 11
            && slots.len() == 4
            && sites.len() == 11
            && rows.len() == 7
            && fixed.len() == 5
            && data[0].sequence > 0
            && data
                .windows(2)
                .all(|pair| pair[0].sequence < pair[1].sequence),
    )?;
    for (index, (record, operation)) in data.iter().zip(operations()).enumerate() {
        let eof = index == 9;
        let code = if eof { 100 } else { 0 };
        let state = if eof { *b"02000" } else { *b"00000" };
        require(
            record.operation == operation
                && record.pc == sites[index]
                && record.keyed == operation.is_mutating()
                && record.inputs == usize::from(index == 1) * 5
                && record.indicators == 0
                && record.canonical_resource
                && record.sqlcode == code
                && record.sqlstate == state
                && record.affected == 0
                && record.rows == usize::from(index == 0 || (2..=8).contains(&index))
                && record.cells.len() == 4
                && record.implicit
                    == vec![
                        (
                            "SQLCODE",
                            Db2CopybackValue::Decimal {
                                coefficient: i128::from(code),
                                scale: 0,
                            },
                        ),
                        ("SQLSTATE", Db2CopybackValue::Bytes(state.to_vec())),
                    ],
        )?;
        for (role, (cell, selected)) in record.cells.iter().zip(slots).enumerate() {
            require(
                cell.id == selected.descriptor.storage().get()
                    && cell.name == selected.descriptor.name()
                    && cell.base == selected.base
                    && cell.offset == selected.offset
                    && cell.bytes.len() == selected.length,
            )?;
            let expected = match role {
                0 => Some(&fixed[usize::from(eof)]),
                1 => Some(&fixed[2 + usize::from(eof)]),
                2 => Some(&fixed[4]),
                3 if index >= 2 => Some(&rows[(index - 2).min(6)]),
                3 => None,
                _ => return Err(problem()),
            };
            if let Some(expected) = expected {
                require(&cell.bytes == expected)?;
            }
        }
    }
    Ok(())
}
fn terminal_matches(value: &serde_json::Value, session: &str, run: &str) -> bool {
    value["schema_version"] == "mainframe-env.cics-terminal@1"
        && value["session"] == session
        && value["principal"] == "WEBADM"
        && value["run_unit"] == run
        && value["connected"] == true
        && value["suspended"] == false
        && value["mapset"] == "COTRTLI"
        && value["map"] == "CTRTLIA"
        && value["aid"] == 125
        && value["transaction"] == "CA00"
        && value["rows"] == 24
        && value["columns"] == 80
        && value["version"].as_u64().is_some_and(|version| version > 0)
}
