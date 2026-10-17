//! Passive actual suspended checkpoint comparison: no drive/resume/dispatch.
//! Selected local first-party storage intent only, with zero additional tokens.
use super::*;
use mainframe_env_cics::{CicsLimits, CicsReplayRetentionState, describe_cics_replay_row};
use mainframe_env_interpreter::ReferenceMachine;
use mainframe_env_ir::CodecLimits;
use mainframe_env_store::LocalArtifactStore;
use mainframe_env_store_api::{ArtifactStore, PlatformStore};

const MAX_CHECKPOINT: usize = 2 * 1024 * 1024;

const PREFIX_ASCII: &[u8] = b"\x43\x43\x4c\x49\x43\x4f\x43\x52\x44\x4c\x49\x43\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x55\x53\x45\x52\x30\x30\x30\x31\x55\x30\x30\x30\x30\x30\x30\x30\x30\x30\x30\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x20\x30\x30\x30\x30\x30\x30\x30\x30\x30\x30\x30\x20\x30\x30\x30\x30\x30\x30\x30\x30\x30\x30\x30\x30\x30\x30\x30\x30\x43\x43\x52\x44\x4c\x49\x41\x43\x4f\x43\x52\x44\x4c\x49";
const PREFIX_CP037: &[u8] = b"\xc3\xc3\xd3\xc9\xc3\xd6\xc3\xd9\xc4\xd3\xc9\xc3\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\xe4\xe2\xc5\xd9\xf0\xf0\xf0\xf1\xe4\xf0\xf0\xf0\xf0\xf0\xf0\xf0\xf0\xf0\xf0\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\x40\xf0\xf0\xf0\xf0\xf0\xf0\xf0\xf0\xf0\xf0\xf0\x40\xf0\xf0\xf0\xf0\xf0\xf0\xf0\xf0\xf0\xf0\xf0\xf0\xf0\xf0\xf0\xf0\xc3\xc3\xd9\xc4\xd3\xc9\xc1\xc3\xd6\xc3\xd9\xc4\xd3\xc9";

// No Debug/Serialize: raw first-party storage stays in this bounded comparison.
struct Facts {
    dfh: Vec<u8>,
    application: Vec<u8>,
    program: Vec<u8>,
    mapped_aid: Vec<u8>,
    calen: Vec<u8>,
    aid: Vec<u8>,
    transaction: Vec<u8>,
}

fn compare(facts: &Facts) -> Result<(), CorpusProblem> {
    require(
        facts.dfh.len() == 2000
            && facts.application.len() == 160
            && facts.program.len() == 254
            && (facts.application.as_slice() == PREFIX_ASCII
                || facts.application.as_slice() == PREFIX_CP037)
            && facts.dfh[..160] == facts.application
            && facts.dfh[160..414] == facts.program
            && (facts.mapped_aid == b"PFK08" || facts.mapped_aid == b"\xd7\xc6\xd2\xf0\xf8")
            && facts.calen == b"2000"
            && facts.aid == [0xf8]
            && facts.transaction == b"CCLI",
    )?;
    // Whole160 independently expected application prefix. Remaining selected
    // page2 context slices are frozen separately; unknown/padding bytes outside
    // those slices are exact source COPY retention, not independent full parity.
    oracles::commarea(&facts.dfh, 2)
}

fn invocation(owner: &evidence::Owner, priority: u8) -> Result<Invocation, CorpusProblem> {
    let limits = InvocationLimits::default();
    let value = Invocation::new(
        RequestId::new("passive-carddemo-checkpoint-request", limits).map_err(|_| problem())?,
        ExecutionId::new(&owner.execution, limits).map_err(|_| problem())?,
        RunUnitId::new(&owner.run, limits).map_err(|_| problem())?,
        None,
        Selector::new("program:COCRDLIC", limits).map_err(|_| problem())?,
        ArtifactRef::new(&owner.artifact, limits).map_err(|_| problem())?,
        Principal::new(
            PrincipalId::new("WEBUSER", limits).map_err(|_| problem())?,
            BTreeSet::new(),
            limits,
        )
        .map_err(|_| problem())?,
        ServiceClass::Interactive,
        priority,
        1, // Nonzero constructor qualification only; never a live task deadline.
        TraceId::new("passive-carddemo-checkpoint-trace", limits).map_err(|_| problem())?,
        IdempotencyKey::new("passive-carddemo-checkpoint", limits).map_err(|_| problem())?,
        owner.attempt,
        ResourceLimits::default(),
        BTreeMap::new(), // Do not initialize COMMAREA/EIB from expected values.
        limits,
    )
    .map_err(|_| problem())?;
    require(
        value.bindings.is_empty()
            && value.principal.grants().is_empty()
            && value.provider_generations.is_empty()
            && value.cancellation.is_none()
            && value.limits == ResourceLimits::default(),
    )?;
    Ok(value)
}

fn current_receive_sequence(
    store: &dyn PlatformStore,
    owner: &evidence::Owner,
) -> Result<u64, CorpusProblem> {
    let rows = store
        .list_provider_state_bounded("cics-effect-replay-v1", 513, MAX_CHECKPOINT)
        .map_err(|_| problem())?;
    require(rows.len() <= 512)?;
    let mut sequence = None;
    for row in &rows {
        let key =
            IdempotencyKey::new(&row.key, InvocationLimits::default()).map_err(|_| problem())?;
        let effect = store
            .effect(&key)
            .map_err(|_| problem())?
            .ok_or_else(problem)?;
        let descriptor = describe_cics_replay_row(row, Some(&effect), CicsLimits::default())
            .map_err(|_| problem())?;
        if descriptor.owner_execution.as_deref() != Some(owner.execution.as_str()) {
            continue;
        }
        require(
            descriptor.owner_run_unit.as_deref() == Some(owner.run.as_str())
                && descriptor.retention == CicsReplayRetentionState::Terminal
                && descriptor.request_digest == oracles::receive_digest(effect.sequence, key)?
                && sequence.is_none(),
        )?;
        sequence = Some(effect.sequence);
    }
    sequence.ok_or_else(problem)
}

pub(super) fn observe(
    store: &dyn PlatformStore,
    artifact_root: &Path,
    session: &str,
    owner: &evidence::Owner,
) -> Result<(), CorpusProblem> {
    // The owning caller has already proved actual source RECEIVE suspension,
    // current core-completed replay, actor-specific audits and final handoff.
    // Recheck exact retained owner/MEOM4 and source-validated artifact here.
    require(evidence::owner_digest(store, owner)? == owner.retained_digest)?;
    let before = store
        .get_provider_state("online-machine-continuation", session)
        .map_err(|_| problem())?
        .ok_or_else(problem)?;
    let artifact =
        ArtifactRef::new(&owner.artifact, InvocationLimits::default()).map_err(|_| problem())?;
    evidence::machine(store, session, &artifact)?;
    let (artifact_digest, installed) = evidence::artifacts(store, artifact_root)?;
    require(installed == artifact)?;
    let artifact_store =
        LocalArtifactStore::open(artifact_root, 64 * 1024 * 1024).map_err(|_| problem())?;
    let record = artifact_store
        .get_artifact(&artifact)
        .map_err(|_| problem())?
        .ok_or_else(problem)?;
    let payload_digest: [u8; 32] = Sha256::digest(&record.payload).into();
    require(
        record.artifact == artifact
            && record.payload_digest == payload_digest
            && record
                .executable
                .as_ref()
                .is_some_and(|meta| meta.validate() && meta.validates_payload(&payload_digest)),
    )?;
    let fields = evidence::envelope(
        &before.payload,
        b"MEOM4",
        &[128, 256, 4096, 1, 128, MAX_CHECKPOINT, 0],
    )?;
    let diagnostic = invocation(owner, fields[3][0])?;
    let mut machine =
        ReferenceMachine::from_binary(&record.payload, diagnostic, CodecLimits::default())
            .map_err(|_| problem())?;
    let checkpoint = BoundedPayload::new(
        "mainframe-env.reference-machine-checkpoint@12",
        fields[5].to_vec(),
        InvocationLimits {
            max_payload_bytes: MAX_CHECKPOINT,
            ..InvocationLimits::default()
        },
    )
    .map_err(|_| problem())?;
    machine
        .restore_checkpoint(&checkpoint)
        .map_err(|_| problem())?;
    let snapshot = machine.snapshot();
    require(
        snapshot.schema_version == 12
            && snapshot.effect_sequence == current_receive_sequence(store, owner)?
            && ["EIBCALEN", "EIBAID", "EIBTRNID"]
                .iter()
                .all(|name| snapshot.implicit_values.contains_key(*name)),
    )?;
    let read = |name| {
        machine
            .variable(name)
            .map(|value| value.bytes().to_vec())
            .ok_or_else(problem)
    };
    // variable() exposes the physical DFH group; EIBCALEN/source/MECC1 bind the
    // selected first2000 bytes. Never claim the unused physical tail is logical
    // COMMAREA or an independent language/storage oracle.
    let dfh = read("DFHCOMMAREA")?;
    require(dfh.len() >= 2000 && dfh.len() <= 32767)?;
    compare(&Facts {
        dfh: dfh[..2000].to_vec(),
        application: read("CARDDEMO-COMMAREA")?,
        program: read("WS-THIS-PROGCOMMAREA")?,
        mapped_aid: read("CCARD-AID")?,
        calen: read("EIBCALEN")?,
        aid: read("EIBAID")?,
        transaction: read("EIBTRNID")?,
    })?;
    require(
        store
            .get_provider_state("online-machine-continuation", session)
            .map_err(|_| problem())?
            .as_ref()
            == Some(&before)
            && evidence::owner_digest(store, owner)? == owner.retained_digest
            && evidence::artifacts(store, artifact_root)?.0 == artifact_digest,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts() -> Facts {
        let mut dfh = vec![0; 2000];
        dfh[..160].copy_from_slice(PREFIX_ASCII);
        for (offset, ascii, _) in oracles::CONTEXT2 {
            dfh[*offset..*offset + ascii.len()].copy_from_slice(ascii);
        }
        Facts {
            application: dfh[..160].to_vec(),
            program: dfh[160..414].to_vec(),
            dfh,
            mapped_aid: b"PFK08".to_vec(),
            calen: b"2000".to_vec(),
            aid: vec![0xf8],
            transaction: b"CCLI".to_vec(),
        }
    }

    #[test]
    fn passive_application_prefix_refuses_every_source_byte_width_and_alias_drift() {
        assert!(compare(&facts()).is_ok());
        let mut cp037 = facts();
        cp037.dfh[..160].copy_from_slice(PREFIX_CP037);
        for (offset, _, bytes) in oracles::CONTEXT2 {
            cp037.dfh[*offset..*offset + bytes.len()].copy_from_slice(bytes);
        }
        cp037.application = cp037.dfh[..160].to_vec();
        cp037.program = cp037.dfh[160..414].to_vec();
        cp037.mapped_aid = b"\xd7\xc6\xd2\xf0\xf8".to_vec();
        assert!(compare(&cp037).is_ok());
        for at in 0..160 {
            let mut corrupt = facts();
            corrupt.application[at] ^= 1;
            assert!(compare(&corrupt).is_err(), "application byte {at}");
        }
        let mut corrupt = facts();
        corrupt.application.pop();
        assert!(compare(&corrupt).is_err());
        let mut corrupt = facts();
        corrupt.application.push(0);
        assert!(compare(&corrupt).is_err());
        let mut corrupt = facts();
        corrupt.dfh[25] ^= 1;
        assert!(compare(&corrupt).is_err());
        let mut corrupt = facts();
        corrupt.program[0] ^= 1;
        assert!(compare(&corrupt).is_err());
    }

    #[test]
    fn passive_eib_context_refuses_wrong_aid_transaction_calen_and_mapping() {
        for mode in 0..8 {
            let mut corrupt = facts();
            match mode {
                0 => {
                    corrupt.calen = b"160".to_vec();
                }
                1 => {
                    corrupt.calen = b"2000.00".to_vec();
                }
                2 => {
                    corrupt.calen = vec![0x07, 0xd0];
                }
                3 => {
                    corrupt.aid = vec![0x7d];
                }
                4 => {
                    corrupt.aid.clear();
                }
                5 => {
                    corrupt.transaction = b"CM00".to_vec();
                }
                6 => {
                    corrupt.transaction = b"CCLI ".to_vec();
                }
                _ => {
                    corrupt.mapped_aid = b"ENTER".to_vec();
                }
            }
            assert!(compare(&corrupt).is_err(), "context mode {mode}");
        }
    }

    #[test]
    fn passive_page2_storage_refuses_independent_slice_and_copy_extent_drift() {
        for (offset, ascii, _) in oracles::CONTEXT2 {
            let mut corrupt = facts();
            let at = *offset + ascii.len() - 1;
            corrupt.dfh[at] ^= 1;
            if at >= 160 {
                corrupt.program[at - 160] ^= 1;
            }
            assert!(compare(&corrupt).is_err(), "page2 storage {at}");
        }
        let mut corrupt = facts();
        corrupt.dfh.pop();
        assert!(compare(&corrupt).is_err());
        let mut corrupt = facts();
        corrupt.program.pop();
        assert!(compare(&corrupt).is_err());
    }
}
