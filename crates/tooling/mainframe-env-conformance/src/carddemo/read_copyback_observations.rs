//! Actual existing compiled account-detail READ storage control, with no new tokens.
//! All customer postimages and expectations remain private and absent from diagnostics.
use super::*;
use mainframe_env_execution_api::{AuditResourceDigest, AuditResourceDigestFormat};
use mainframe_env_interpreter::{
    CicsReadCopybackPlan, CicsReadCopybackSelection, CicsReadCopybackSlotSelection,
    CicsReadCopybackValue,
};
use mainframe_env_ir::{CicsStorageSlot, CodecLimits, Module, StorageId, decode_binary};
use mainframe_env_server::{
    ArmedReadCopybackObservation, CicsReadObserverManager, CompletedReadCopyback,
    SourceBoundReadCopybackProfile,
};
use mainframe_env_store_api::{ArtifactRecord, ExecutionState, PlatformStore};
use std::io::Read;

#[cfg(test)]
mod tests;

const SOURCE: &str = "4f1e55176f69edfb55b7af7a161aceb8c3bc3bd1b2924a704a76256e0e751b54";
const RESPONSE: &str = "WS-MISC-STORAGE.WS-CICS-PROCESSNG-VARS.WS-RESP-CD";
const RESPONSE2: &str = "WS-MISC-STORAGE.WS-CICS-PROCESSNG-VARS.WS-REAS-CD";
const ORACLES: [(&str, &str, usize, &str, &str); 3] = [
    (
        "CARD-XREF-RECORD",
        "xref.expected-ascii.bin",
        50,
        "4ff3eb62da5b8a9e231510aec342e22e650ad0c117a3adeae428e0fd43eae48d",
        "9ef8e833cb962f60bf6123307caaeec1df124591d53adc46264e4278a28d2a86",
    ),
    (
        "ACCOUNT-RECORD",
        "account.expected-ascii.bin",
        300,
        "51951dd1947dbc4f7a77f966306bd5549558e12a5d613abe23c36234d33282c6",
        "890a196627f46934d0e5812eb838263b11ea6957daef109a6105e448a8f5fe08",
    ),
    (
        "CUSTOMER-RECORD",
        "customer.expected-ascii.bin",
        500,
        "bc701cfb2743d8810f1b6c1b2fc6813c6078763231e4e1241ef8793ffe1aeff1",
        "a3f258ba713185148b3ff590b199c0760551699f6ed4662f9b8249b1943c8bc6",
    ),
];

fn problem() -> CorpusProblem {
    CorpusProblem::new(
        "carddemo.online.read_copyback_storage",
        "selected actual READ storage, independent oracle or owner binding differs",
    )
}
fn require(value: bool) -> Result<(), CorpusProblem> {
    if value { Ok(()) } else { Err(problem()) }
}
// Fixed metadata only: never interpolate actual names, values, keys or bytes.
#[derive(Clone, Copy)]
enum RefusalStage {
    OwnerBinding,
    ReadChain,
    OracleWidth,
    CellMembership,
    Resource,
    Eib,
    CellGeometry,
    CellBytes,
}

fn refusal(
    stage: RefusalStage,
    read: Option<usize>,
    cell_role: Option<usize>,
    eib_field: Option<usize>,
) -> CorpusProblem {
    let stage = match stage {
        RefusalStage::OwnerBinding => "owner-binding",
        RefusalStage::ReadChain => "read-chain",
        RefusalStage::OracleWidth => "oracle-width",
        RefusalStage::CellMembership => "cell-membership",
        RefusalStage::Resource => "resource",
        RefusalStage::Eib => "eib",
        RefusalStage::CellGeometry => "cell-geometry",
        RefusalStage::CellBytes => "cell-bytes",
    };
    CorpusProblem::new(
        "carddemo.online.read_copyback_storage",
        format!(
            "selected READ comparison refused: stage={stage}; read={read:?}; \
             cell_role={cell_role:?}; eib_field={eib_field:?}"
        ),
    )
}

fn require_at(
    value: bool,
    stage: RefusalStage,
    read: Option<usize>,
    cell_role: Option<usize>,
) -> Result<(), CorpusProblem> {
    if value {
        Ok(())
    } else {
        Err(refusal(stage, read, cell_role, None))
    }
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

// No Debug/Serialize: even these fixture-selected data are not outward receipts.
#[derive(Clone)]
struct Slot {
    descriptor: CicsStorageSlot,
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
        descriptor: CicsStorageSlot {
            storage: selected.id,
            qualified_layout_name: name.into(),
        },
        base,
        offset: usize::try_from(offset).map_err(|_| problem())?,
        length: width,
    })
}

fn load_expected(
    root: &Path,
    name: &str,
    width: usize,
    expected: &str,
) -> Result<Vec<u8>, CorpusProblem> {
    let path = root.join(name);
    let metadata = fs::symlink_metadata(&path).map_err(|_| problem())?;
    require(metadata.file_type().is_file() && metadata.len() == width as u64)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        require(metadata.permissions().mode() & 0o077 == 0)?;
    }
    let mut bytes = Vec::with_capacity(width + 1);
    fs::File::open(&path)
        .map_err(|_| problem())?
        .take(width as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| problem())?;
    require(bytes.len() == width && format!("{:x}", Sha256::digest(&bytes)) == expected)?;
    Ok(bytes)
}

pub(super) struct Prepared {
    record: ArtifactRecord,
    plan: CicsReadCopybackPlan,
    slots: Vec<[Slot; 3]>,
    expected: Vec<Vec<u8>>,
}

pub(super) fn install(
    server: &Arc<ProductServer>,
) -> Result<CicsReadObserverManager, CorpusProblem> {
    server.install_cics_read_observer().map_err(|_| problem())
}

impl Prepared {
    pub(super) fn from_programs(
        corpus: &Path,
        programs: &[OnlineProgramDefinition],
    ) -> Result<Self, CorpusProblem> {
        let candidates = programs
            .iter()
            .filter(|program| program.name == "COACTVWC")
            .collect::<Vec<_>>();
        let [definition] = candidates.as_slice() else {
            return Err(problem());
        };
        Self::new(corpus, definition)
    }
    pub(super) fn new(
        corpus: &Path,
        definition: &OnlineProgramDefinition,
    ) -> Result<Self, CorpusProblem> {
        // The caller retains the exact actual definition used by the separately
        // verified complete source/publisher/installed-artifact proof.
        require(definition.name == "COACTVWC")?;
        let source = read_corpus_file(corpus, &corpus.join("app/cbl/COACTVWC.cbl"))?;
        require(format!("{:x}", Sha256::digest(&source)) == SOURCE)?;
        source_artifact_binding::verify_pinned_sources(corpus)?;
        let record = source_artifact_binding::publisher_record(definition)?;
        let module =
            decode_binary(&record.payload, CodecLimits::default()).map_err(|_| problem())?;
        let root = env::var_os("CARDDEMO_READ_COPYBACK_ORACLE_DIR").ok_or_else(problem)?;
        let root = Path::new(&root);
        let mut expected = Vec::with_capacity(3);
        let mut slots = Vec::with_capacity(3);
        let mut selections = Vec::with_capacity(3);
        for (into, name, width, expected_hash, _) in ORACLES {
            // Frozen externally before the collector exists; no machine/result
            // codec, observed response or terminal field supplies expected bytes.
            expected.push(load_expected(root, name, width, expected_hash)?);
            let targets = [
                selected_slot(&module, into, width)?,
                selected_slot(&module, RESPONSE, 4)?,
                selected_slot(&module, RESPONSE2, 4)?,
            ];
            let select = |target: &Slot| {
                CicsReadCopybackSlotSelection::new(target.descriptor.clone(), target.length)
                    .map_err(|_| problem())
            };
            selections.push(CicsReadCopybackSelection::new(
                select(&targets[0])?,
                Some(select(&targets[1])?),
                Some(select(&targets[2])?),
            ));
            slots.push(targets);
        }
        Ok(Self {
            record,
            plan: CicsReadCopybackPlan::new(selections).map_err(|_| problem())?,
            slots,
            expected,
        })
    }

    pub(super) fn arm(
        &self,
        manager: &CicsReadObserverManager,
        session: &str,
    ) -> Result<ArmedReadCopybackObservation, CorpusProblem> {
        let limits = InvocationLimits::default();
        let profile = SourceBoundReadCopybackProfile::new(
            SessionId::new(session, limits.max_binding_bytes).map_err(|_| problem())?,
            Selector::new("program:COACTVWC", limits).map_err(|_| problem())?,
            PrincipalId::new("WEBUSER", limits).map_err(|_| problem())?,
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

    pub(super) fn compare(
        &self,
        completed: &CompletedReadCopyback,
        terminal: &serde_json::Value,
        session: &str,
        installed: &BTreeMap<String, ArtifactRef>,
        store: &dyn PlatformStore,
    ) -> Result<(), CorpusProblem> {
        // Called only after the unchanged current-stage READ request+key proof.
        // This binds actual opaque capture to the same returned run and installed
        // source artifact; final fixture drains/removal still occur in the owner.
        let archive = completed.archive();
        let origin = archive.origin();
        let execution = store
            .get_execution(origin.execution())
            .map_err(|_| problem())?
            .ok_or_else(problem)?;
        require_at(
            terminal_matches(terminal, session, origin.run().as_str())
                && completed.session().as_str() == session
                && completed.asserted_source_digest() == &digest(SOURCE)?
                && completed.payload_digest() == &self.record.payload_digest
                && completed.semantic_identity()
                    == self
                        .record
                        .executable
                        .as_ref()
                        .ok_or_else(problem)?
                        .semantic_identity
                && origin.artifact() == &self.record.artifact
                && installed.get("COACTVWC") == Some(origin.artifact())
                && origin.selector().as_str() == "program:COACTVWC"
                && origin.principal().as_str() == "WEBUSER"
                && execution.execution_id == *origin.execution()
                && execution.run_unit_id == *origin.run()
                && execution.principal == *origin.principal()
                && execution.selector == *origin.selector()
                && execution.artifact == *origin.artifact()
                && execution.attempt == origin.attempt()
                && execution.state == ExecutionState::Completed
                && execution.version == completed.terminal_version()
                && execution.terminal_tick == Some(completed.terminal_tick())
                && execution.owner_lease.is_none()
                && execution.lease_expiry_tick.is_none(),
            RefusalStage::OwnerBinding,
            None,
            None,
        )?;
        let reads = archive
            .reads()
            .iter()
            .map(|read| ReadData {
                sequence: read.effect().sequence(),
                resource: read.effect().resource(),
                cells: read
                    .cells()
                    .iter()
                    .map(|cell| CellData {
                        name: cell.slot().qualified_layout_name.clone(),
                        id: cell.slot().storage.get(),
                        base: cell.base(),
                        offset: cell.offset(),
                        bytes: cell.bytes().to_vec(),
                    })
                    .collect(),
                eib: read.eib().to_vec(),
            })
            .collect::<Vec<_>>();
        let slots = self
            .slots
            .iter()
            .map(|targets| {
                targets
                    .iter()
                    .map(|slot| CellSelection {
                        name: slot.descriptor.qualified_layout_name.clone(),
                        id: slot.descriptor.storage.get(),
                        base: slot.base,
                        offset: slot.offset,
                        length: slot.length,
                    })
                    .collect()
            })
            .collect::<Vec<_>>();
        compare_data(&reads, &slots, &self.expected)
    }
}

#[derive(Clone)]
struct CellData {
    name: String,
    id: u32,
    base: usize,
    offset: usize,
    bytes: Vec<u8>,
}
#[derive(Clone)]
struct CellSelection {
    name: String,
    id: u32,
    base: usize,
    offset: usize,
    length: usize,
}
#[derive(Clone)]
struct ReadData {
    sequence: u64,
    resource: AuditResourceDigest,
    cells: Vec<CellData>,
    eib: Vec<(&'static str, CicsReadCopybackValue)>,
}

fn expected_eib() -> Vec<(&'static str, CicsReadCopybackValue)> {
    use CicsReadCopybackValue::{Bytes, Decimal};
    vec![
        (
            "EIBRESP",
            Decimal {
                coefficient: 0,
                scale: 0,
            },
        ),
        (
            "EIBRESP2",
            Decimal {
                coefficient: 0,
                scale: 0,
            },
        ),
        ("EIBFN", Bytes(vec![0x06, 0x02])),
        ("EIBAID", Bytes(vec![0x7d])),
        (
            "EIBCALEN",
            Decimal {
                coefficient: 2000,
                scale: 0,
            },
        ),
        ("EIBTRNID", Bytes(b"CAVW".to_vec())),
    ]
}

fn compare_data(
    reads: &[ReadData],
    slots: &[Vec<CellSelection>],
    expected: &[Vec<u8>],
) -> Result<(), CorpusProblem> {
    require_at(
        reads.len() == 3
            && slots.len() == 3
            && expected.len() == 3
            && reads
                .windows(2)
                .all(|pair| pair[0].sequence < pair[1].sequence)
            && reads[0].sequence > 0,
        RefusalStage::ReadChain,
        None,
        None,
    )?;
    for (index, ((read, selected), bytes)) in reads.iter().zip(slots).zip(expected).enumerate() {
        require_at(
            bytes.len() == ORACLES[index].2,
            RefusalStage::OracleWidth,
            Some(index),
            None,
        )?;
        require_at(
            read.cells.len() == 3 && selected.len() == 3,
            RefusalStage::CellMembership,
            Some(index),
            None,
        )?;
        require_at(
            read.resource.format == AuditResourceDigestFormat::CanonicalHostResourceV1
                && read.resource.value == digest(ORACLES[index].4)?,
            RefusalStage::Resource,
            Some(index),
            None,
        )?;
        let expected_context = expected_eib();
        if read.eib != expected_context {
            let field = (read.eib.len() == expected_context.len())
                .then(|| {
                    read.eib
                        .iter()
                        .zip(&expected_context)
                        .position(|(actual, expected)| actual != expected)
                })
                .flatten();
            return Err(refusal(RefusalStage::Eib, Some(index), None, field));
        }
        for (role, (cell, selection)) in read.cells.iter().zip(selected).enumerate() {
            let expected_bytes = if role == 0 { bytes.as_slice() } else { &[0; 4] };
            require_at(
                cell.name == selection.name
                    && cell.id == selection.id
                    && cell.base == selection.base
                    && cell.offset == selection.offset
                    && cell.bytes.len() == selection.length,
                RefusalStage::CellGeometry,
                Some(index),
                Some(role),
            )?;
            require_at(
                cell.bytes == expected_bytes,
                RefusalStage::CellBytes,
                Some(index),
                Some(role),
            )?;
        }
    }
    Ok(())
}

fn terminal_matches(terminal: &serde_json::Value, session: &str, run: &str) -> bool {
    terminal["schema_version"] == "mainframe-env.cics-terminal@1"
        && terminal["session"] == session
        && terminal["principal"] == "WEBUSER"
        && terminal["run_unit"] == run
        && terminal["connected"] == true
        && terminal["suspended"] == false
        && terminal["mapset"] == "COACTVW"
        && terminal["map"] == "CACTVWA"
        && terminal["aid"] == 125
        && terminal["transaction"] == "CAVW"
        && terminal["version"]
            .as_u64()
            .is_some_and(|version| version > 0)
}
