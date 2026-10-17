//! Real original COPAUS1C failure branch, with an intentionally uninstalled LINK target.
//! This is a diagnostic, not a successful fraud/UOW application observation.
use super::reference::Reference;
use super::*;
use crate::carddemo::ims_firstparty_readonly::{dataset_images, rows};
use crate::carddemo::owned_source_receipt::{ReceiptOwner, require_owned_directory};
use mainframe_env_cics::{
    CicsLimits, CicsReplayCodecVersion, CicsReplayRetentionState, CicsUowDependencyState,
    CicsUowState, describe_cics_replay_row, describe_cics_uow_row,
};
use mainframe_env_db2::{
    Db2Limits, Db2ReplayDependency, Db2ReplayRetentionState, describe_db2_replay_row,
};
use mainframe_env_execution_api::{AuditDecision, AuditResourceDigestFormat, LifecycleEventKind};
use mainframe_env_host_api::HostRequest;
use mainframe_env_ims::{ImsReplayDependency, ImsReplayRetentionState, describe_ims_replay_row};
use mainframe_env_store::LocalArtifactStore;
use mainframe_env_store_api::{
    ArtifactStore, EffectDigestFormat, EffectRecord, EffectState, ExecutionRecord, ExecutionState,
    PlatformStore, ProviderStateRecord,
};
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
const MAX_ROWS: usize = 512;
fn require(value: bool) -> Result<(), CorpusProblem> {
    need(value, "real rollback diagnostic differs")
}
fn refused() -> CorpusProblem {
    problem("actual compiled fraud actor differs")
}
fn actor(
    store: &dyn PlatformStore,
    effect: &mainframe_env_store_api::EffectRecord,
    record: &ArtifactRecord,
    run: &RunUnitId,
    capability: &str,
) -> Result<ExecutionRecord, CorpusProblem> {
    let owner = store
        .get_execution(&effect.execution_id)
        .map_err(|_| problem("actual compiled fraud actor differs"))?
        .ok_or_else(refused)?;
    require(
        owner.selector.as_str() == "program:COPAUS1C"
            && owner.artifact == record.artifact
            && owner.run_unit_id == *run
            && owner.principal.as_str() == "WEBADM"
            && owner.attempt > 0
            && owner.state == ExecutionState::Completed
            && owner.terminal_tick.is_some_and(|tick| tick > 0)
            && owner.owner_lease.is_none()
            && owner.lease_expiry_tick.is_none()
            && effect.execution_id == owner.execution_id
            && effect.run_unit_id == *run
            && effect.intent.owner == owner.execution_id
            && effect.intent.attempt == owner.attempt
            && effect.intent.capability.as_ref().map(CapabilityId::as_str) == Some(capability)
            && effect.state == EffectState::Completed
            && effect.result_digest.is_some(),
    )?;
    let events = store
        .events(&owner.execution_id, owner.version, 2)
        .map_err(|_| problem("actual compiled fraud actor differs"))?;
    require(matches!(events.as_slice(),[event] if event.validate()
        && event.execution_id==owner.execution_id && event.run_unit_id==*run
        && event.sequence==owner.version && event.attempt==owner.attempt
        && Some(event.tick)==owner.terminal_tick && event.tick>0
        && matches!(event.kind,LifecycleEventKind::Completed {return_code:0})))?;
    let resource = effect.intent.audit_resource.as_ref().ok_or_else(refused)?;
    let invocation = effect
        .intent
        .audit_invocation_key
        .as_ref()
        .ok_or_else(refused)?;
    require(resource.format == AuditResourceDigestFormat::CanonicalHostResourceV1)?;
    let audits = store
        .audit_records(&owner.execution_id, 0, MAX_ROWS + 1)
        .map_err(|_| problem("actual compiled fraud actor differs"))?;
    require(audits.len() <= MAX_ROWS)?;
    let matches = audits
        .iter()
        .filter(|audit| {
            audit.effect_sequence == effect.sequence && audit.capability.as_str() == capability
        })
        .collect::<Vec<_>>();
    require(
        matches!(matches.as_slice(),[audit] if audit.execution_id==owner.execution_id
        && audit.run_unit_id==*run && audit.principal==owner.principal
        && audit.attempt==owner.attempt && audit.invocation_key==*invocation
        && audit.resource==*resource && audit.decision==AuditDecision::Success),
    )?;
    Ok(owner)
}
fn installed(
    store: &dyn PlatformStore,
    artifacts: &Path,
    prepared: &Prepared,
) -> Result<(), CorpusProblem> {
    let local = LocalArtifactStore::open(artifacts, 64 * 1024 * 1024)
        .map_err(|_| problem("artifact authority unavailable"))?;
    for name in ["COPAUS0C", "COPAUS1C"] {
        let record = &prepared.records[name];
        let row = store
            .get_provider_state("online-program", name)
            .map_err(|_| problem("program lookup"))?
            .ok_or_else(|| problem("program absent"))?;
        require(
            row.namespace == "online-program"
                && row.key == name
                && row.version == 1
                && row.payload == record.artifact.as_str().as_bytes()
                && local
                    .get_artifact(&record.artifact)
                    .map_err(|_| problem("artifact lookup"))?
                    .as_ref()
                    == Some(record),
        )?;
    }
    // The one deliberate failure injection is an unavailable compiled target.
    // No fake response/host hook/SQL result or provider-state write supplies failure.
    require(
        store
            .get_provider_state("online-program", "COPAUS2C")
            .map_err(|_| problem("injected target lookup"))?
            .is_none()
            && store
                .get_provider_state("cics-program", "COPAUS2C")
                .map_err(|_| problem("injected CICS target lookup"))?
                .is_none(),
    )?;
    for (transaction, program) in [("CPVS", "COPAUS0C"), ("CPVD", "COPAUS1C")] {
        let row = store
            .get_provider_state("online-transaction", transaction)
            .map_err(|_| problem("transaction lookup"))?
            .ok_or_else(|| problem("transaction absent"))?;
        require(
            row.namespace == "online-transaction"
                && row.key == transaction
                && row.version == 1
                && row.payload == program.as_bytes(),
        )?;
    }
    Ok(())
}
fn hierarchy(server: &ProductServer, reference: &Reference) -> Result<(), CorpusProblem> {
    let result = server
        .ims_service()
        .execute(
            &ims_invocation("fraud-diagnostic-image", true, None)?,
            &ims_request(
                ImsOperation::Unload,
                91_003,
                Some("DBPAUTP0"),
                &[],
                vec![],
                vec![],
                None,
            )?,
        )
        .map_err(|_| problem("hierarchy unload"))?;
    let actual = result
        .segments
        .into_iter()
        .map(|segment| (segment.name, segment.parent_key, segment.data))
        .collect::<Vec<_>>();
    require(
        result.status == "  "
            && result.affected_segments == 0
            && actual
                == vec![
                    ("PAUTSUM0".into(), None, reference.root.clone()),
                    (
                        "PAUTDTL1".into(),
                        Some(reference.root[..6].to_vec()),
                        reference.original_child.clone(),
                    ),
                    (
                        "PAUTDTL1".into(),
                        Some(reference.root[..6].to_vec()),
                        reference.unchanged_neighbor.clone(),
                    ),
                ],
    )?;
    require(
        server
            .db2_service()
            .table_rows("CARDDEMO.AUTHFRDS")
            .map_err(|_| problem("AUTHFRDS read"))?
            .is_empty()
            && server
                .db2_service()
                .pending_units()
                .map_err(|_| problem("pending Db2 UOW read"))?
                == 0,
    )
}
fn unchanged(
    before: &BTreeMap<String, ProviderStateRecord>,
    after: &BTreeMap<String, ProviderStateRecord>,
) -> Result<(), CorpusProblem> {
    require(before.iter().all(|(key, row)| after.get(key) == Some(row)))
}
fn mutation(effect: &EffectRecord) -> Mutation {
    Mutation {
        sequence: effect.sequence,
        idempotency_key: effect.key.clone(),
        transaction: Some("CPVD".into()),
    }
}
fn request_matches(effect: &EffectRecord, request: &HostRequest) -> Result<bool, CorpusProblem> {
    Ok(effect.request_digest
        == mainframe_env_host_api::canonical_request_digest(request)
            .map_err(|_| problem("expected request codec"))?
        && effect.intent.audit_resource.as_ref()
            == Some(&mainframe_env_host_api::canonical_audit_resource_digest(
                request,
            )))
}
fn arg(schema: &str, bytes: Vec<u8>) -> Result<BoundedPayload, CorpusProblem> {
    BoundedPayload::new(schema, bytes, InvocationLimits::default())
        .map_err(|_| problem("expected argument"))
}
fn observe(
    store: &dyn PlatformStore,
    prepared: &Prepared,
    reference: &Reference,
    terminal: &serde_json::Value,
    session: &str,
    before: &[BTreeMap<String, ProviderStateRecord>; 3],
) -> Result<String, CorpusProblem> {
    require(
        terminal["schema_version"] == "mainframe-env.cics-terminal@1"
            && terminal["session"] == session
            && terminal["principal"] == "WEBADM"
            && terminal["transaction"] == "CPVD"
            && terminal["connected"] == true
            && terminal["suspended"] == false
            && terminal["mapset"] == "COPAU01"
            && terminal["map"] == "COPAU1A",
    )?;
    reference.screen("disabled-link-rollback", &online_screen_fields(terminal)?)?;
    let run = RunUnitId::new(
        terminal["run_unit"]
            .as_str()
            .ok_or_else(|| problem("run absent"))?,
        InvocationLimits::default(),
    )
    .map_err(|_| problem("run invalid"))?;
    let after = [
        rows(store, "cics-effect-replay-v1")?,
        rows(store, "ims-v1-replay")?,
        rows(store, "db2-v1-replay")?,
    ];
    for (old, new) in before.iter().zip(&after) {
        unchanged(old, new)?;
    }
    let mut effects = BTreeMap::new();
    let mut owner = None;
    for (key, row) in after[0]
        .iter()
        .filter(|(key, _)| !before[0].contains_key(*key))
    {
        let effect = store
            .effect(
                &IdempotencyKey::new(key, InvocationLimits::default())
                    .map_err(|_| problem("effect key"))?,
            )
            .map_err(|_| problem("effect lookup"))?
            .ok_or_else(|| problem("effect absent"))?;
        let description = describe_cics_replay_row(row, Some(&effect), CicsLimits::default())
            .map_err(|_| problem("CICS replay codec"))?;
        require(
            description.codec == CicsReplayCodecVersion::RetentionV3
                && description.retention == CicsReplayRetentionState::Terminal
                && description.owner_execution.as_deref() == Some(effect.execution_id.as_str())
                && description.owner_run_unit.as_deref() == Some(run.as_str())
                && effect.digest_format == EffectDigestFormat::CanonicalHostV1,
        )?;
        let found = actor(
            store,
            &effect,
            &prepared.records["COPAUS1C"],
            &run,
            "host.cics.execute",
        )?;
        require(owner.as_ref().is_none_or(|old| old == &found))?;
        owner = Some(found);
        require(effects.insert(effect.sequence, effect).is_none())?;
    }
    let owner = owner.ok_or_else(|| problem("compiled actor absent"))?;
    let mut offered = b"00000000050".to_vec();
    offered.extend_from_slice(&reference.root[6..15]);
    let mut selected = reference.original_child.clone();
    selected[174] = b'F';
    offered.extend_from_slice(&selected);
    offered.push(b'F');
    offered.extend_from_slice(&[b' '; 51]);
    require(offered.len() == 272)?;
    let mut link = None;
    let mut rollback = None;
    for effect in effects.values() {
        let expected = HostRequest::Cics(CicsRequest {
            operation: CicsOperation::Link,
            arguments: BTreeMap::from([
                (
                    "PROGRAM".into(),
                    arg("mainframe-env.cics.storage-value@1", b"COPAUS2C".to_vec())?,
                ),
                (
                    "COMMAREA".into(),
                    arg("mainframe-env.cics.storage-value@1", offered.clone())?,
                ),
                (
                    "OPTION.NOHANDLE".into(),
                    arg("mainframe-env.cics.option@1", Vec::new())?,
                ),
            ]),
            condition_policy: CicsConditionPolicy::NoHandle,
            mutation: Some(mutation(effect)),
        });
        if request_matches(effect, &expected)? {
            require(link.replace(effect).is_none())?;
        }
        let expected = HostRequest::Cics(CicsRequest {
            operation: CicsOperation::Syncpoint,
            arguments: BTreeMap::from([(
                "OPTION.ROLLBACK".into(),
                arg("mainframe-env.cics.option@1", Vec::new())?,
            )]),
            condition_policy: CicsConditionPolicy::Default,
            mutation: Some(mutation(effect)),
        });
        if request_matches(effect, &expected)? {
            require(rollback.replace(effect).is_none())?;
        }
    }
    let link = link.ok_or_else(|| problem("exact original LINK not observed"))?;
    let rollback = rollback.ok_or_else(|| problem("exact original ROLLBACK not observed"))?;
    require(link.sequence < rollback.sequence)?;
    let uow = store
        .get_provider_state("cics-uow", rollback.key.as_str())
        .map_err(|_| problem("UOW lookup"))?
        .ok_or_else(|| problem("UOW absent"))?;
    let undo = store
        .get_provider_state("cics-uow-undo", run.as_str())
        .map_err(|_| problem("undo lookup"))?;
    let uow = describe_cics_uow_row(&uow, undo.as_ref()).map_err(|_| problem("UOW codec"))?;
    require(
        uow.state == CicsUowState::RolledBack
            && uow.dependency == CicsUowDependencyState::Clear
            && uow.transaction == "CPVD"
            && uow.effect_key.as_deref() == Some(rollback.key.as_str())
            && uow.owner_execution.as_deref() == Some(owner.execution_id.as_str())
            && uow.owner_run_unit.as_deref() == Some(run.as_str()),
    )?;
    let mut nested_ims = 0;
    let mut core_ims = BTreeMap::new();
    for (key, row) in after[1]
        .iter()
        .filter(|(key, _)| !before[1].contains_key(*key))
    {
        let id = IdempotencyKey::new(key, InvocationLimits::default())
            .map_err(|_| problem("IMS key"))?;
        let effect = store
            .effect(&id)
            .map_err(|_| problem("IMS effect lookup"))?;
        let d = describe_ims_replay_row(row, effect.as_ref(), ImsLimits::default())
            .map_err(|_| problem("IMS codec"))?;
        require(
            d.retention == ImsReplayRetentionState::Terminal
                && d.owner_execution.as_deref() == Some(owner.execution_id.as_str())
                && d.owner_run_unit.as_deref() == Some(run.as_str()),
        )?;
        // Provider result bytes are decoded only after the full strict codec.
        let payload: serde_json::Value =
            serde_json::from_slice(&row.payload).map_err(|_| problem("IMS result envelope"))?;
        require(payload["value"]["status"] == "  ")?;
        match d.dependency {
            Some(ImsReplayDependency::CoreEffect) => {
                let effect = effect.ok_or_else(|| problem("IMS core absent"))?;
                require(
                    actor(
                        store,
                        &effect,
                        &prepared.records["COPAUS1C"],
                        &run,
                        "host.ims.write",
                    )? == owner,
                )?;
                require(core_ims.insert(effect.sequence, effect).is_none())?;
            }
            Some(ImsReplayDependency::CicsNested {
                run_unit,
                sequence,
                outer_effect_key,
            }) => {
                require(
                    effect.is_none()
                        && run_unit == run.as_str()
                        && outer_effect_key == rollback.key.as_str()
                        && payload["value"]["affected_segments"] == 0
                        && payload["value"]["segments"]
                            .as_array()
                            .is_some_and(Vec::is_empty),
                )?;
                let request = HostRequest::Ims(ImsRequest {
                    operation: ImsOperation::Rollback,
                    psb: None,
                    pcb: 1,
                    segments: vec![],
                    data: vec![],
                    qualifiers: vec![],
                    checkpoint_id: None,
                    max_segments: 1,
                    mutation: Some(Mutation {
                        sequence,
                        idempotency_key: id,
                        transaction: Some("CPVD".into()),
                    }),
                    system: None,
                    q_class: None,
                });
                require(
                    d.request_digest
                        == mainframe_env_host_api::canonical_request_digest(&request)
                            .map_err(|_| problem("nested IMS request"))?,
                )?;
                nested_audit(
                    store,
                    &owner,
                    rollback,
                    sequence,
                    "host.ims.write",
                    &request,
                )?;
                nested_ims += 1;
            }
            None => return Err(problem("IMS dependency absent")),
        }
    }
    require(core_ims.len() == 3 && nested_ims == 1)?;
    for (index, effect) in core_ims.into_values().enumerate() {
        let request = HostRequest::Ims(ImsRequest {
            operation: match index {
                0 => ImsOperation::Schedule,
                1 => ImsOperation::GetUnique,
                _ => ImsOperation::GetNextParent,
            },
            psb: (index == 0).then(|| "PSBPAUTB".into()),
            pcb: 1,
            segments: match index {
                0 => vec![],
                1 => vec!["PAUTSUM0".into()],
                _ => vec!["PAUTDTL1".into()],
            },
            data: vec![],
            qualifiers: match index {
                0 => vec![],
                1 => vec![ImsQualifier {
                    segment: "PAUTSUM0".into(),
                    field: "ACCNTID".into(),
                    value: reference.root[..6].to_vec(),
                }],
                _ => vec![ImsQualifier {
                    segment: "PAUTDTL1".into(),
                    field: "PAUT9CTS".into(),
                    value: reference.original_child[..8].to_vec(),
                }],
            },
            checkpoint_id: None,
            max_segments: 1,
            mutation: Some(Mutation {
                sequence: effect.sequence,
                idempotency_key: effect.key.clone(),
                transaction: None,
            }),
            system: None,
            q_class: None,
        });
        require(request_matches(&effect, &request)?)?;
    }
    let mut nested_db2 = 0;
    for (key, row) in after[2]
        .iter()
        .filter(|(key, _)| !before[2].contains_key(*key))
    {
        let id = IdempotencyKey::new(key, InvocationLimits::default())
            .map_err(|_| problem("Db2 key"))?;
        require(
            store
                .effect(&id)
                .map_err(|_| problem("Db2 effect lookup"))?
                .is_none(),
        )?;
        let d = describe_db2_replay_row(row, None, Db2Limits::default())
            .map_err(|_| problem("Db2 codec"))?;
        let payload: serde_json::Value =
            serde_json::from_slice(&row.payload).map_err(|_| problem("Db2 result envelope"))?;
        require(
            payload["value"]["sqlcode"] == 0
                && payload["value"]["sqlstate"] == "00000"
                && payload["value"]["affected_rows"] == 0
                && payload["value"]["rows"]
                    .as_array()
                    .is_some_and(Vec::is_empty),
        )?;
        require(
            d.retention == Db2ReplayRetentionState::Terminal
                && d.owner_execution.as_deref() == Some(owner.execution_id.as_str())
                && d.owner_run_unit.as_deref() == Some(run.as_str()),
        )?;
        let Some(Db2ReplayDependency::CicsNested {
            run_unit,
            sequence,
            outer_effect_key,
        }) = d.dependency
        else {
            return Err(problem("Db2 nested dependency"));
        };
        require(run_unit == run.as_str() && outer_effect_key == rollback.key.as_str())?;
        let request = HostRequest::Db2(Db2Request {
            operation: Db2Operation::Rollback,
            statement: String::new(),
            cursor: None,
            inputs: BTreeMap::new(),
            outputs: vec![],
            max_rows: 0,
            mutation: Some(Mutation {
                sequence,
                idempotency_key: id,
                transaction: Some("CPVD".into()),
            }),
        });
        require(
            d.request_digest
                == mainframe_env_host_api::canonical_request_digest(&request)
                    .map_err(|_| problem("nested Db2 request"))?,
        )?;
        nested_audit(
            store,
            &owner,
            rollback,
            sequence,
            "host.db2.write",
            &request,
        )?;
        nested_db2 += 1;
    }
    require(nested_db2 == 1)?;
    Ok(format!(
        "{:x}",
        Sha256::digest(owner.execution_id.as_str().as_bytes())
    ))
}
fn nested_audit(
    store: &dyn PlatformStore,
    owner: &ExecutionRecord,
    outer: &EffectRecord,
    sequence: u64,
    capability: &str,
    request: &HostRequest,
) -> Result<(), CorpusProblem> {
    let audits = store
        .audit_records(&owner.execution_id, 0, MAX_ROWS + 1)
        .map_err(|_| problem("nested audit read"))?;
    require(audits.len() <= MAX_ROWS)?;
    let found = audits
        .iter()
        .filter(|audit| {
            audit.effect_sequence == sequence && audit.capability.as_str() == capability
        })
        .collect::<Vec<_>>();
    require(
        matches!(found.as_slice(),[audit] if audit.execution_id==owner.execution_id && audit.run_unit_id==owner.run_unit_id
        && audit.principal==owner.principal && audit.attempt==owner.attempt
        && Some(&audit.invocation_key)==outer.intent.audit_invocation_key.as_ref()
        && audit.resource==mainframe_env_host_api::canonical_audit_resource_digest(request) && audit.decision==AuditDecision::Success),
    )
}

// Failure-only private evidence; never promotes or retries a failed exchange.
// Namespace reads preflight bytes. Existing trace/checkpoint APIs materialize
// before caller checks. This does not recover a fault PC absent from the store.
fn failure_blob(
    owned: &ReceiptOwner,
    name: &str,
    bytes: &[u8],
    remaining: &mut usize,
) -> Result<serde_json::Value, CorpusProblem> {
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    if bytes.len() > *remaining {
        return Err(problem("failure observation byte budget"));
    }
    owned.check()?;
    let path = owned.evidence.join(name);
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .map_err(|_| problem("failure observation exclusive file"))?;
    file.write_all(bytes)
        .map_err(|_| problem("failure observation write"))?;
    file.sync_all()
        .map_err(|_| problem("failure observation sync"))?;
    let handle = file
        .metadata()
        .map_err(|_| problem("failure observation handle"))?;
    let found = fs::symlink_metadata(&path).map_err(|_| problem("failure observation path"))?;
    if !found.is_file()
        || found.file_type().is_symlink()
        || found.nlink() != 1
        || found.permissions().mode() & 0o777 != 0o600
        || (found.dev(), found.ino()) != (handle.dev(), handle.ino())
        || found.len() != bytes.len() as u64
    {
        return Err(problem("failure observation file identity"));
    }
    owned.check()?;
    *remaining -= bytes.len();
    Ok(
        serde_json::json!({"file":name,"bytes":bytes.len(),"device":handle.dev(),
        "inode":handle.ino(),"sha256":format!("{:x}",Sha256::digest(bytes))}),
    )
}
fn failure_observation(
    store: &dyn PlatformStore,
    server: &ProductServer,
    owned: &ReceiptOwner,
    exchange: Option<(&str, &str)>,
    error: &CorpusProblem,
) -> Result<serde_json::Value, CorpusProblem> {
    let Some((label, session)) = exchange else {
        return Err(problem("failure observation has no actual exchange"));
    };
    let mut remaining = 8 * 1024 * 1024;
    let mut report = serde_json::json!({"schema":"mainframe-env.fraud-failure-observation@1",
        "exchange":label,"session":session,"problem_code":error.code,"problem_detail":error.detail,
        "application_credit":0,"guest_fault_pc":null,"guest_fault_subscript":null,
        "checkpoint_is_durable_state_not_a_claim_of_fault_PC":true,"rows":[],"execution":null,
        "checkpoint":null,"events":[],"trace":[],"incomplete":[]});
    let mut exchange_value = None;
    for (namespace, name) in [
        ("online-exchange-v1", "failure-online-exchange.bin"),
        (
            "online-machine-continuation",
            "failure-online-continuation.bin",
        ),
    ] {
        let page = store
            .list_provider_state_bounded(namespace, MAX_ROWS + 1, 2 * 1024 * 1024)
            .map_err(|_| problem("failure observation bounded namespace read"))?;
        if page.len() > MAX_ROWS {
            return Err(problem("failure observation row budget"));
        }
        let selected = page
            .iter()
            .filter(|row| row.key == session)
            .collect::<Vec<_>>();
        match selected.as_slice() {
            [row] if row.namespace == namespace => {
                let blob = failure_blob(owned, name, &row.payload, &mut remaining)?;
                report["rows"]
                    .as_array_mut()
                    .ok_or_else(refused)?
                    .push(serde_json::json!({
                    "namespace":namespace,"key":row.key,"version":row.version,"payload":blob}));
                if namespace == "online-exchange-v1" {
                    exchange_value = Some(
                        serde_json::from_slice::<serde_json::Value>(&row.payload)
                            .map_err(|_| problem("failure observation exchange JSON"))?,
                    );
                }
            }
            [] => report["incomplete"]
                .as_array_mut()
                .ok_or_else(refused)?
                .push(serde_json::json!({"missing_namespace":namespace})),
            _ => return Err(problem("failure observation ambiguous session row")),
        }
    }
    if let Some(value) = exchange_value {
        let id = value["execution_id"]
            .as_str()
            .ok_or_else(|| problem("failure execution ID absent"))?;
        let id = ExecutionId::new(id, InvocationLimits::default())
            .map_err(|_| problem("failure execution ID malformed"))?;
        if let Some(actor) = store
            .get_execution(&id)
            .map_err(|_| problem("failure execution read"))?
        {
            if actor.execution_id != id
                || Some(actor.run_unit_id.as_str()) != value["run_unit_id"].as_str()
                || Some(actor.selector.as_str()) != value["selector"].as_str()
                || Some(actor.artifact.as_str()) != value["artifact"].as_str()
                || Some(actor.principal.as_str()) != value["principal"].as_str()
                || Some(u64::from(actor.attempt)) != value["attempt"].as_u64()
            {
                return Err(problem(
                    "failure execution does not bind actual session row",
                ));
            }
            report["execution"] = serde_json::json!({"execution_id":id.as_str(),
                "run_unit_id":actor.run_unit_id.as_str(),"selector":actor.selector.as_str(),
                "artifact":actor.artifact.as_str(),"principal":actor.principal.as_str(),
                "attempt":actor.attempt,"version":actor.version,"state":format!("{:?}",actor.state),
                "terminal_tick":actor.terminal_tick,"owner_lease":actor.owner_lease,
                "lease_expiry_tick":actor.lease_expiry_tick,"program_from_session_row":value["program"]});
            let events = store
                .events(&id, actor.version.saturating_sub(3), 5)
                .map_err(|_| problem("failure lifecycle read"))?;
            if events.len() > 4 {
                return Err(problem("failure lifecycle budget"));
            }
            for event in events {
                if event.execution_id != id || event.run_unit_id != actor.run_unit_id {
                    return Err(problem("failure lifecycle actor mismatch"));
                }
                report["events"]
                    .as_array_mut()
                    .ok_or_else(refused)?
                    .push(serde_json::json!({
                    "sequence":event.sequence,"attempt":event.attempt,"tick":event.tick,
                    "kind":format!("{:?}",event.kind)}));
            }
            match store
                .get_checkpoint(&id)
                .map_err(|_| problem("failure checkpoint read"))?
            {
                Some(checkpoint) => {
                    if checkpoint.execution_id != id
                        || checkpoint.run_unit_id != actor.run_unit_id
                        || checkpoint.payload.len() > 2 * 1024 * 1024
                        || checkpoint.payload_size != checkpoint.payload.len() as u64
                    {
                        return Err(problem("failure checkpoint identity or byte budget"));
                    }
                    let blob = failure_blob(
                        owned,
                        "failure-execution-checkpoint.bin",
                        &checkpoint.payload,
                        &mut remaining,
                    )?;
                    report["checkpoint"] = serde_json::json!({"schema_version":checkpoint.schema_version,
                        "machine_schema_version":checkpoint.machine_schema_version,
                        "artifact":checkpoint.artifact.as_str(),"effect_sequence":checkpoint.effect_sequence,
                        "session_id":checkpoint.session_id,"payload_digest":checkpoint.payload_digest,
                        "provider_generation":checkpoint.provider_generation,"payload":blob});
                }
                None => report["incomplete"]
                    .as_array_mut()
                    .ok_or_else(refused)?
                    .push(serde_json::json!("no durable execution checkpoint")),
            }
        } else {
            report["incomplete"]
                .as_array_mut()
                .ok_or_else(refused)?
                .push(serde_json::json!("actual session execution absent"));
        }
    }
    let trace = server
        .online_trace(session)
        .map_err(|_| problem("failure trace read"))?;
    if trace.len() > MAX_ROWS {
        return Err(problem("failure trace row budget"));
    }
    for row in trace {
        report["trace"]
            .as_array_mut()
            .ok_or_else(refused)?
            .push(serde_json::json!({
            "operation":format!("{:?}",row.operation),"outcome":row.outcome,
            "response":row.response,"response2":row.response2,"payload_bytes":row.payload_bytes}));
    }
    let bytes =
        serde_json::to_vec_pretty(&report).map_err(|_| problem("failure observation encoding"))?;
    if bytes.len() > 1024 * 1024 {
        return Err(problem("failure observation metadata budget"));
    }
    failure_blob(owned, "failure-observation.json", &bytes, &mut remaining)
}

async fn exercise(
    corpus: &Path,
    reference: &Reference,
    owned: &mut ReceiptOwner,
    receipt: &mut serde_json::Value,
) -> Result<(), CorpusProblem> {
    owned.check()?;
    sources(corpus)?;
    let config = ServerConfig {
        store_profile: StoreProfile::Memory,
        artifact_root: owned.artifacts.clone(),
        cobol_current_date: Some(reference.cobol_current_date.clone()),
        tls: TlsConfig {
            enabled: false,
            certificate_path: None,
            private_key_reference: None,
        },
        ..ServerConfig::default()
    };
    let store = Arc::new(MemoryStore::new(Default::default()));
    let secrets = Arc::new(MemorySecretResolver::default());
    let server = ProductServer::open_with_package_trust(
        config.clone(),
        store.clone(),
        secrets.clone(),
        default_program_router(),
        carddemo_package_trust()?,
    )
    .map_err(|_| problem("server open"))?;
    let app = server.router();
    let mut retained = None;
    let mut last_exchange: Option<(&str, String)> = None;
    let probe=async {
        let mut prepared=Prepared::new(corpus)?;
        // Deliberate install-time absence, not a fabricated LINK reply. Compile the
        // original target for source qualification but do not make it executable.
        prepared.online.programs.retain(|program|program.name!="COPAUS2C");
        require(prepared.online.programs.len()==2)?;
        install_base_online_authorities(&server,corpus,&prepared.online)?;
        server.bootstrap_user("IBMUSER",b"TESTPASS").map_err(|_|problem("bootstrap"))?;
        let racf=server.racf_service();
        for (class,name) in [("IMSPSB","PSBPAUTB"),("IMSDB","DBPAUTP0")] {
            racf.define_profile(class,name,"IBMUSER",Some(AccessIntent::Control)).map_err(|_|problem("IMS profile"))?;
            for principal in ["IBMUSER","WEBADM"] {racf.permit(class,name,principal,AccessIntent::Control).map_err(|_|problem("IMS permit"))?;}
        }
        racf.define_profile("IMSUOW","CURRENT","IBMUSER",None).map_err(|_|problem("UOW profile"))?;
        for principal in ["IBMUSER","WEBADM"] {racf.permit("IMSUOW","CURRENT",principal,AccessIntent::Update).map_err(|_|problem("UOW permit"))?;}
        // Authorization succeeds independently of the intentional not-installed
        // program, so this tests PGMIDERR rather than an authorization denial.
        racf.define_profile("FACILITY","CICS.PROGRAM.COPAUS2C","IBMUSER",None).map_err(|_|problem("LINK facility"))?;
        racf.permit("FACILITY","CICS.PROGRAM.COPAUS2C","WEBADM",AccessIntent::Execute).map_err(|_|problem("LINK execute permit"))?;
        server.install_online_application(prepared.online.clone()).map_err(|_|problem("online install"))?;
        installed(store.as_ref(),&owned.artifacts,&prepared)?;
        let ims=server.ims_service();ims.install_metadata(ims_packages::metadata(corpus)?).map_err(|_|problem("IMS metadata"))?;
        let seed=ims_invocation("fraud-diagnostic-seed",true,None)?;
        let image=ImsLoadImage {database:"DBPAUTP0".into(),roots:vec![ImsLoadRoot {data:reference.root.clone(),
            children:vec![reference.original_child.clone(),reference.unchanged_neighbor.clone()]}]};
        let result=ims.execute(&seed,&ims_request(ImsOperation::Load,91_001,None,&[],serde_json::to_vec(&image).map_err(|_|problem("seed encode"))?,vec![],None)?)
            .map_err(|_|problem("IMS seed"))?;require(result.status=="  " && result.affected_segments==3)?;
        let result=ims.execute(&seed,&ims_request(ImsOperation::Commit,91_002,None,&[],vec![],vec![],None)?)
            .map_err(|_|problem("IMS seed commit"))?;require(result.status=="  ")?;
        let ddl=String::from_utf8(read_corpus_file(corpus,&corpus.join("app/app-authorization-ims-db2-mq/ddl/AUTHFRDS.ddl"))?)
            .map_err(|_|problem("DDL encoding"))?;
        let admin=authorization_invocation("fraud-diagnostic-ddl",true,ServiceClass::Interactive)?;
        let result=server.db2_service().execute(&admin,&authorization_db2_request(Db2Operation::ExecuteScript,91_004,&ddl,BTreeMap::new())?)
            .map_err(|_|problem("original AUTHFRDS DDL"))?;require(result.sqlcode==0)?;
        hierarchy(&server,reference)?;dataset_images(&server,corpus)?;
        receipt["stage"]=serde_json::json!("originals-installed-target-deliberately-unavailable");
        receipt["source_identities"]=serde_json::json!(prepared.source_ids);owned.save(receipt)?;
        let mut headers=BTreeMap::from([("authorization".into(),format!("Basic {}",base64::engine::general_purpose::STANDARD.encode("WEBADM:admin-transport-password"))),
            ("x-csrf-zosmf-header".into(),"true".into())]);
        let launch=terminal_http(&app,Method::POST,"/mainframe-env/cics/v1/sessions",headers.clone(),br#"{"transaction":"CPVS"}"#.to_vec())
            .await.map_err(|_|problem("launch transport"))?;require(launch.0==StatusCode::CREATED)?;
        let initial:serde_json::Value=serde_json::from_slice(&launch.1).map_err(|_|problem("launch JSON"))?;
        let session=initial["session"].as_str().ok_or_else(||problem("session absent"))?.to_string();
        require(initial["terminal"]["transaction"]=="CPVS" && initial["terminal"]["mapset"]=="COPAU00"
            && initial["terminal"]["map"]=="COPAU0A" && initial["terminal"]["principal"]=="WEBADM"
            && initial["terminal"]["connected"]==true && initial["terminal"]["suspended"]==false)?;
        headers.insert("x-csrf-token".into(),initial["csrf_token"].as_str().ok_or_else(||problem("CSRF absent"))?.to_string());
        let mut fields=BTreeMap::from([("ACCTID".into(),"00000000050".into()),("SEL0001".into()," ".into()),
            ("SEL0002".into()," ".into()),("SEL0003".into()," ".into()),("SEL0004".into()," ".into()),("SEL0005".into()," ".into())]);
        last_exchange=Some(("account-enter",session.clone()));
        let page=carddemo_terminal_exchange(&app,&session,&headers,0x7d,fields.clone()).await?;
        require(page["mapset"]=="COPAU00" && page["map"]=="COPAU0A")?;
        fields.insert("SEL0001".into(),"S".into());
        last_exchange=Some(("select-first-detail",session.clone()));
        let detail=carddemo_terminal_exchange(&app,&session,&headers,0x7d,fields).await?;
        require(detail["mapset"]=="COPAU01" && detail["map"]=="COPAU1A")?;
        reference.screen("initial-low",&online_screen_fields(&detail)?)?;
        hierarchy(&server,reference)?;dataset_images(&server,corpus)?;
        let before=[rows(store.as_ref(),"cics-effect-replay-v1")?,rows(store.as_ref(),"ims-v1-replay")?,rows(store.as_ref(),"db2-v1-replay")?];
        let trace_before=server.online_trace(&session).map_err(|_|problem("trace before"))?;require(trace_before.len()<=MAX_ROWS)?;
        receipt["stage"]=serde_json::json!("before-real-PF5-source-LINK-failure");owned.save(receipt)?;
        last_exchange=Some(("mark-fraud-pf5",session.clone()));
        let terminal=carddemo_terminal_exchange(&app,&session,&headers,0xf5,BTreeMap::new()).await?;
        let execution=observe(store.as_ref(),&prepared,reference,&terminal,&session,&before)?;
        let trace=server.online_trace(&session).map_err(|_|problem("trace after"))?;
        require(trace.len()<=MAX_ROWS && trace.starts_with(&trace_before))?;
        let delta=&trace[trace_before.len()..];
        let links=delta.iter().filter(|row|row.operation==CicsOperation::Link).collect::<Vec<_>>();
        let syncpoints=delta.iter().filter(|row|row.operation==CicsOperation::Syncpoint).collect::<Vec<_>>();
        require(matches!(links.as_slice(),[row] if row.outcome=="PGMIDERR" && row.response==27 && row.response2==1)
            && matches!(syncpoints.as_slice(),[row] if row.outcome=="NORMAL" && row.response==0 && row.response2==0))?;
        hierarchy(&server,reference)?;dataset_images(&server,corpus)?;sources(corpus)?;
        installed(store.as_ref(),&owned.artifacts,&prepared)?;
        receipt["actual_execution_sha256"]=serde_json::json!(execution);
        receipt["actual_source_failure_branch_compared"]=serde_json::json!(true);
        receipt["actual_empty_AUTHFRDS_and_full_unchanged_IMS_image_compared"]=serde_json::json!(true);
        receipt["actual_canonical_LINK_then_ROLLBACK_and_nested_audits_compared"]=serde_json::json!(true);
        let uri=format!("/mainframe-env/cics/v1/sessions/{session}");
        require(terminal_http(&app,Method::DELETE,&uri,headers.clone(),Vec::new()).await.map_err(|_|problem("disconnect"))?.0==StatusCode::NO_CONTENT)?;
        require(terminal_http(&app,Method::GET,&uri,headers,Vec::new()).await.map_err(|_|problem("disconnect absence"))?.0==StatusCode::NOT_FOUND)?;
        receipt["session_disconnect_checked"]=serde_json::json!(true);retained=Some(prepared);Ok::<_,CorpusProblem>(())
    }.await;
    if let Err(error) = &probe {
        receipt["problem_code"] = serde_json::json!(error.code);
        receipt["last_exchange"] = last_exchange
            .as_ref()
            .map(|(label, session)| serde_json::json!({"exchange":label,"session":session}))
            .unwrap_or(serde_json::Value::Null);
        // Best effort only: keep the original probe Result and all later drains.
        // The inherited Rust panic hook and OOM/abort are not suppressed.
        receipt["failure_observation"] =
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                failure_observation(
                    store.as_ref(),
                    &server,
                    owned,
                    last_exchange
                        .as_ref()
                        .map(|(label, session)| (*label, session.as_str())),
                    error,
                )
            })) {
                Ok(Ok(blob)) => {
                    serde_json::json!({"status":"captured-diagnostic-only","manifest":blob})
                }
                Ok(Err(error)) => serde_json::json!({"status":"incomplete","reason":error.detail}),
                Err(_) => serde_json::json!({"status":"incomplete","reason":"observer-panicked"}),
            };
    }
    let before_save = owned.save(receipt);
    drop(app);
    let first = server.graceful_shutdown().await;
    receipt["first_drain"] = serde_json::json!(first);
    drop(server);
    let after_save = owned.save(receipt);
    before_save?;
    after_save?;
    require(first)?;
    owned.check()?;
    let reopened = ProductServer::open_with_package_trust(
        config,
        store.clone(),
        secrets,
        default_program_router(),
        carddemo_package_trust()?,
    )
    .map_err(|_| problem("reopen"))?;
    let reopening = (|| {
        sources(corpus)?;
        installed(
            store.as_ref(),
            &owned.artifacts,
            retained
                .as_ref()
                .ok_or_else(|| problem("retained originals absent"))?,
        )?;
        hierarchy(&reopened, reference)?;
        dataset_images(&reopened, corpus)?;
        receipt["reopen_compared"] = serde_json::json!(true);
        Ok::<_, CorpusProblem>(())
    })();
    let before_save = owned.save(receipt);
    let second = reopened.graceful_shutdown().await;
    receipt["second_drain"] = serde_json::json!(second);
    drop(reopened);
    let after_save = owned.save(receipt);
    probe?;
    reopening?;
    before_save?;
    after_save?;
    require(second)?;
    owned.check()?;
    fs::remove_dir_all(&owned.artifacts).map_err(|_| problem("owned disposal"))?;
    require(
        matches!(fs::symlink_metadata(&owned.artifacts),Err(error) if error.kind()==std::io::ErrorKind::NotFound),
    )?;
    owned.artifacts_removed = true;
    owned.check()?;
    receipt["cleanup_verified"] = serde_json::json!(true);
    receipt["stage"] =
        serde_json::json!("local-source-failure-diagnostic-pass-zero-application-credit");
    owned.save(receipt)
}
pub(super) fn verify_unavailable_link_from_env() -> Result<(), CorpusProblem> {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .map_err(|_| problem("repository root"))?;
    let inventory = repository.join("conformance/profiles/carddemo/inventory/carddemo-corpus.json");
    let corpus = verify_carddemo_corpus_from_env(&inventory)?;
    let corpus_dir =
        PathBuf::from(env::var_os(CORPUS_ENV).ok_or_else(|| problem("explicit corpus required"))?);
    sources(&corpus_dir)?;
    let reference = Reference::independent()?;
    let parent = env::temp_dir()
        .canonicalize()
        .map_err(|_| problem("temporary parent"))?;
    require(!parent.starts_with(&repository))?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| problem("nonce"))?
        .as_nanos();
    let evidence = parent.join(format!(
        "mainframe-env-carddemo-fraud-link-diagnostic-{}-{nonce}",
        std::process::id()
    ));
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&evidence)
        .map_err(|_| problem("exclusive evidence root"))?;
    let m = fs::symlink_metadata(&evidence).map_err(|_| problem("evidence stat"))?;
    let identity = (m.dev(), m.ino());
    require_owned_directory(&evidence, identity)?;
    let artifacts = evidence.join("owned-artifacts");
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&artifacts)
        .map_err(|_| problem("exclusive artifacts root"))?;
    let m = fs::symlink_metadata(&artifacts).map_err(|_| problem("artifact stat"))?;
    let mut owned = ReceiptOwner::open(&evidence, identity, &artifacts, (m.dev(), m.ino()))?;
    let mut receipt = serde_json::json!({"schema":"mainframe-env.carddemo-original-fraud-failure-diagnostic@1","corpus_commit":corpus.commit,
        "backend":"memory","physical_store_restart":false,"injected_failure":"COPAUS2C deliberately not installed; no fake provider response",
        "no_staged_DB2_write_in_this_case":true,"application_credit":0,"issue_credit":0,"vendor_credit":0,"acceptance_credit":0,
        "first_drain":false,"second_drain":false,"reopen_compared":false,"cleanup_verified":false,"session_disconnect_checked":false,
        "artifact_root":artifacts,"artifact_identity":(m.dev(),m.ino()),"stage":"before-open"});
    owned.save(&receipt)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| problem("runtime"))?;
    let result = runtime.block_on(exercise(&corpus_dir, &reference, &mut owned, &mut receipt));
    if let Err(error) = &result {
        receipt["problem_code"] = serde_json::json!(error.code);
    }
    let saved = owned.save(&receipt);
    result?;
    saved
}
