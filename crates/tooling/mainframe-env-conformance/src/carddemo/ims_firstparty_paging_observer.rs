//! Actual per-AID actor/replay/audit/UOW attribution; no provider count promotion.
use super::*;
use mainframe_env_execution_api::{AuditRecord, AuditResourceDigest};
use mainframe_env_store_api::ExecutionRecord;
#[path = "ims_firstparty_paging_actor.rs"]
mod actor;

pub(super) struct PageContext<'a> {
    pub(super) store: &'a dyn PlatformStore,
    pub(super) record: &'a ArtifactRecord,
    pub(super) terminal: &'a serde_json::Value,
    pub(super) session: &'a str,
    pub(super) before_cics: &'a BTreeMap<String, ProviderStateRecord>,
    pub(super) before_ims: &'a BTreeMap<String, ProviderStateRecord>,
    pub(super) receipt: &'a mut serde_json::Value,
}
// One fixed-size technical cursor; the existing outer failure save retains it.
// This records no actor IDs, request/result bytes, credentials or extra store reads.
fn hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn observation_stage(receipt: &mut serde_json::Value, phase: &'static str, ordinal: Option<usize>) {
    receipt["observer_stage"] = serde_json::json!({"phase": phase, "ordinal": ordinal});
}
// The actual observer has already enforced its unchanged <=512 audit bound.
fn nested_audit_matches(
    audits: &[AuditRecord],
    owner: &ExecutionRecord,
    run: &RunUnitId,
    sequence: u64,
    invocation: &IdempotencyKey,
    resource: &AuditResourceDigest,
) -> bool {
    let selected = audits
        .iter()
        .filter(|audit| {
            audit.effect_sequence == sequence
                && audit.capability.as_str() == "host.ims.write"
                && audit.resource == *resource
        })
        .collect::<Vec<_>>();
    matches!(selected.as_slice(), [audit] if audit.execution_id == owner.execution_id
        && audit.run_unit_id == *run && audit.principal == owner.principal
        && audit.attempt == owner.attempt && audit.invocation_key == *invocation
        && audit.resource == *resource && audit.decision == AuditDecision::Success)
}

// Exact current generation for the three pinned COPAUS0C DATASET READs.
// Operands come from source7ab6 and the already admitted independent paging
// oracle0e7, never from a sampled provider row, trace or result. Other operations
// retain the exact V3 rule; matching request bytes with a foreign audit refuses.
fn paging_plain_read_requests(
    account: &[u8],
    customer: &[u8],
) -> Result<Vec<HostRequest>, CorpusProblem> {
    need(account.len() == 11 && customer.len() == 9)?;
    let response = "WS-VARIABLES.WS-RESP-CD";
    let response2 = "WS-VARIABLES.WS-REAS-CD";
    let mut requests = Vec::new();
    for (logical, key, length, into) in [
        ("CXACAIX ", account, 50_usize, "CARD-XREF-RECORD"),
        ("ACCTDAT ", account, 300, "ACCOUNT-RECORD"),
        ("CUSTDAT ", customer, 500, "CUSTOMER-RECORD"),
    ] {
        let mut arguments = BTreeMap::new();
        for (name, schema, value) in [
            (
                "DATASET",
                "mainframe-env.cics.storage-value@1",
                logical.as_bytes().to_vec(),
            ),
            ("RIDFLD", "mainframe-env.cics.storage-value@1", key.to_vec()),
            (
                "KEYLENGTH",
                "mainframe-env.cics.decimal@1",
                key.len().to_string().into_bytes(),
            ),
            (
                "LENGTH",
                "mainframe-env.cics.decimal@1",
                length.to_string().into_bytes(),
            ),
            (
                "INTO",
                "mainframe-env.cics.argument@1",
                into.as_bytes().to_vec(),
            ),
            (
                "RESP",
                "mainframe-env.cics.argument@1",
                response.as_bytes().to_vec(),
            ),
            (
                "RESP2",
                "mainframe-env.cics.argument@1",
                response2.as_bytes().to_vec(),
            ),
        ] {
            arguments.insert(
                name.into(),
                BoundedPayload::new(schema, value, InvocationLimits::default())
                    .map_err(|_| refused())?,
            );
        }
        requests.push(HostRequest::Cics(CicsRequest {
            operation: CicsOperation::Read,
            arguments,
            condition_policy: CicsConditionPolicy::Respond {
                response_field: response.into(),
                response2_field: Some(response2.into()),
            },
            mutation: None,
        }));
    }
    Ok(requests)
}
fn expected_paging_cics_codec(
    effect: &mainframe_env_store_api::EffectRecord,
    account: &[u8],
    customer: &[u8],
    expected_reads: usize,
) -> Result<CicsReplayCodecVersion, CorpusProblem> {
    if expected_reads == 0 {
        return Ok(CicsReplayCodecVersion::RetentionV3);
    }
    need(expected_reads == 3)?;
    for request in paging_plain_read_requests(account, customer)? {
        let HostRequest::Cics(plain) = &request else {
            return Err(refused());
        };
        if plain.is_completed_plain_read_replay()
            && effect.request_digest
                == mainframe_env_host_api::canonical_request_digest(&request)
                    .map_err(|_| refused())?
        {
            need(
                effect.intent.audit_resource
                    == Some(mainframe_env_host_api::canonical_audit_resource_digest(
                        &request,
                    )),
            )?;
            return Ok(CicsReplayCodecVersion::PlainReadV4);
        }
    }
    Ok(CicsReplayCodecVersion::RetentionV3)
}
// The complete provider row/core/result decoder precedes this private gate.
// Exactly one independently selected codec is required, together with every
// original terminal and execution/run owner condition.
fn current_terminal_cics_owner(
    descriptor: &mainframe_env_cics::CicsReplayRowDescriptor,
    expected_codec: Option<CicsReplayCodecVersion>,
    execution: &str,
    run: &str,
) -> bool {
    expected_codec == Some(descriptor.codec)
        && matches!(
            descriptor.codec,
            CicsReplayCodecVersion::RetentionV3 | CicsReplayCodecVersion::PlainReadV4
        )
        && descriptor.retention == CicsReplayRetentionState::Terminal
        && descriptor.owner_execution.as_deref() == Some(execution)
        && descriptor.owner_run_unit.as_deref() == Some(run)
}
fn codec_name(codec: CicsReplayCodecVersion) -> &'static str {
    match codec {
        CicsReplayCodecVersion::LegacyV1 => "LegacyV1",
        CicsReplayCodecVersion::AttributionV2 => "AttributionV2",
        CicsReplayCodecVersion::RetentionV3 => "RetentionV3",
        CicsReplayCodecVersion::PlainReadV4 => "PlainReadV4",
    }
}
pub(super) fn observe_page(
    context: PageContext<'_>,
    oracle: &PagingOracle,
    index: usize,
) -> Result<String, CorpusProblem> {
    let PageContext {
        store,
        record,
        terminal,
        session,
        before_cics,
        before_ims,
        receipt,
    } = context;
    observation_stage(receipt, "terminal-contract", None);
    need(
        terminal["schema_version"] == "mainframe-env.cics-terminal@1"
            && terminal["session"] == session
            && terminal["principal"] == "WEBADM"
            && terminal["connected"] == true
            && terminal["suspended"] == false
            && terminal["mapset"] == "COPAU00"
            && terminal["map"] == "COPAU0A",
    )?;
    observation_stage(receipt, "terminal-run-unit", None);
    let run = RunUnitId::new(
        terminal["run_unit"].as_str().ok_or_else(refused)?,
        InvocationLimits::default(),
    )
    .map_err(|_| refused())?;
    observation_stage(receipt, "terminal-field-oracle", None);
    oracle.compare_fields(index, &online_screen_fields(terminal)?)?;
    observation_stage(receipt, "oracle-frame", None);
    let frame = oracle.frames.get(index).ok_or_else(refused)?;
    observation_stage(receipt, "cics-replay-read", None);
    let after_cics = rows(store, "cics-effect-replay-v1")?;
    observation_stage(receipt, "ims-replay-read", None);
    let after_ims = rows(store, "ims-v1-replay")?;
    observation_stage(receipt, "prior-row-preservation", None);
    need(
        before_cics
            .iter()
            .all(|(key, row)| after_cics.get(key) == Some(row))
            && before_ims
                .iter()
                .all(|(key, row)| after_ims.get(key) == Some(row)),
    )?;
    let mut owner = None;
    let mut cics_count = 0;
    let mut cics_effects = BTreeMap::new();
    for (row_index, (key, row)) in after_cics
        .iter()
        .filter(|(key, _)| !before_cics.contains_key(*key))
        .enumerate()
    {
        observation_stage(receipt, "cics-effect", Some(row_index));
        let effect = store
            .effect(&IdempotencyKey::new(key, InvocationLimits::default()).map_err(|_| refused())?)
            .map_err(|_| refused())?
            .ok_or_else(refused)?;
        observation_stage(receipt, "cics-replay-descriptor", Some(row_index));
        let descriptor = describe_cics_replay_row(row, Some(&effect), CicsLimits::default())
            .map_err(|_| refused())?;
        observation_stage(receipt, "cics-terminal-retention-owner", Some(row_index));
        let expected_codec = expected_paging_cics_codec(
            &effect,
            oracle.account.as_bytes(),
            frame.fields.get("CUSTID").ok_or_else(refused)?,
            frame.dataset_reads,
        )
        .ok();
        if !current_terminal_cics_owner(
            &descriptor,
            expected_codec,
            effect.execution_id.as_str(),
            run.as_str(),
        ) {
            // One bounded failure record; no request, actor identity or payload.
            receipt["cics_replay_refusal"] = serde_json::json!({
                "ordinal": row_index,
                "codec": codec_name(descriptor.codec),
                "expected_codec": expected_codec.map(codec_name),
                "canonical_request_and_audit_rule_valid": expected_codec.is_some(),
                "codec_matches_expected": expected_codec == Some(descriptor.codec),
                "terminal": descriptor.retention == CicsReplayRetentionState::Terminal,
                "owner_execution_matches": descriptor.owner_execution.as_deref()
                    == Some(effect.execution_id.as_str()),
                "owner_run_unit_matches": descriptor.owner_run_unit.as_deref() == Some(run.as_str()),
            });
            return Err(refused());
        }
        observation_stage(receipt, "cics-actor", Some(row_index));
        let found =
            actor::check_paging_actor(store, &effect, record, &run, "host.cics.execute", receipt)?;
        observation_stage(receipt, "cics-owner-consistency", Some(row_index));
        need(owner.as_ref().is_none_or(|old| old == &found))?;
        owner = Some(found);
        cics_count += 1;
        observation_stage(receipt, "cics-key-unique", Some(row_index));
        need(cics_effects.insert(key.clone(), effect).is_none())?;
    }
    observation_stage(receipt, "cics-effect-count", None);
    need(cics_count > 0)?;
    observation_stage(receipt, "cics-owner-present", None);
    let owner = owner.ok_or_else(refused)?;
    let mut results = BTreeMap::<
        u64,
        (
            String,
            Vec<SegmentImage>,
            mainframe_env_store_api::EffectRecord,
        ),
    >::new();
    let mut nested_commits = 0;
    for (row_index, (key, row)) in after_ims
        .iter()
        .filter(|(key, _)| !before_ims.contains_key(*key))
        .enumerate()
    {
        observation_stage(receipt, "ims-effect", Some(row_index));
        let effect = store
            .effect(&IdempotencyKey::new(key, InvocationLimits::default()).map_err(|_| refused())?)
            .map_err(|_| refused())?;
        observation_stage(receipt, "ims-replay-descriptor", Some(row_index));
        let descriptor = describe_ims_replay_row(row, effect.as_ref(), ImsLimits::default())
            .map_err(|_| refused())?;
        observation_stage(receipt, "ims-terminal-owner", Some(row_index));
        let terminal_owner = descriptor.retention == ImsReplayRetentionState::Terminal
            && descriptor.owner_execution.as_deref() == Some(owner.execution_id.as_str())
            && descriptor.owner_run_unit.as_deref() == Some(run.as_str());
        if !terminal_owner {
            receipt["ims_terminal_owner_failure"] = serde_json::json!({
                "terminal": descriptor.retention == ImsReplayRetentionState::Terminal,
                "execution": descriptor.owner_execution.as_deref() == Some(owner.execution_id.as_str()),
                "run": descriptor.owner_run_unit.as_deref() == Some(run.as_str())
            });
        }
        need(terminal_owner)?;
        // Decode only after the provider's complete strict result/retention codec validation.
        observation_stage(receipt, "ims-result-decode", Some(row_index));
        let payload: serde_json::Value =
            serde_json::from_slice(&row.payload).map_err(|_| refused())?;
        let value = &payload["value"];
        observation_stage(receipt, "ims-result-shape", Some(row_index));
        let shape = value["affected_segments"] == 0
            && value["checkpoint_id"].is_null()
            && value["system"].is_null()
            && value["gsam"].is_null()
            && value["pcb_feedback_v1"].is_null();
        if !shape {
            receipt["ims_result_shape_failure"] = serde_json::json!({
                "zero_affected": value["affected_segments"] == 0,
                "no_checkpoint": value["checkpoint_id"].is_null(),
                "no_system": value["system"].is_null(), "no_gsam": value["gsam"].is_null(),
                "no_feedback": value["pcb_feedback_v1"].is_null(),
                "payload_bytes": row.payload.len(), "payload_sha256": hex(&row.payload)
            });
        }
        need(shape)?;
        observation_stage(receipt, "ims-status-decode", Some(row_index));
        let status = value["status"].as_str().ok_or_else(refused)?.to_string();
        observation_stage(receipt, "ims-segments-decode", Some(row_index));
        let segments: Vec<SegmentImage> =
            serde_json::from_value(value["segments"].clone()).map_err(|_| refused())?;
        match descriptor.dependency {
            Some(ImsReplayDependency::CoreEffect) => {
                observation_stage(receipt, "ims-core-effect", Some(row_index));
                let effect = effect.ok_or_else(refused)?;
                observation_stage(receipt, "ims-core-identity-actor", Some(row_index));
                let identity = descriptor.sequence == Some(effect.sequence)
                    && descriptor.request_digest == effect.request_digest
                    && effect.digest_format
                        == mainframe_env_store_api::EffectDigestFormat::CanonicalHostV1;
                if !identity {
                    receipt["ims_core_identity_failure"] = serde_json::json!({
                        "sequence": descriptor.sequence == Some(effect.sequence),
                        "request_digest": descriptor.request_digest == effect.request_digest,
                        "canonical_format": effect.digest_format
                            == mainframe_env_store_api::EffectDigestFormat::CanonicalHostV1,
                        "descriptor_request_digest_sha256": hex(&descriptor.request_digest),
                        "core_request_digest_sha256": hex(&effect.request_digest)
                    });
                }
                // Original short circuit: no actor/store reads if identity already fails.
                need(identity)?;
                let found = actor::check_paging_actor(
                    store,
                    &effect,
                    record,
                    &run,
                    "host.ims.write",
                    receipt,
                )?;
                if found != owner {
                    receipt["ims_core_identity_failure"] = serde_json::json!({"same_owner": false});
                }
                need(found == owner)?;
                observation_stage(receipt, "ims-sequence-unique", Some(row_index));
                need(
                    results
                        .insert(effect.sequence, (status, segments, effect))
                        .is_none(),
                )?;
            }
            Some(ImsReplayDependency::CicsNested {
                run_unit,
                sequence,
                outer_effect_key,
            }) => {
                // The source's SYNCPOINT has a nested IMS commit, not an invented same-key core.
                observation_stage(receipt, "ims-nested-identity-result", Some(row_index));
                need(
                    effect.is_none()
                        && run_unit == run.as_str()
                        && descriptor.sequence == Some(sequence)
                        && descriptor.outer_effect_key.as_deref()
                            == Some(outer_effect_key.as_str())
                        && status == "  "
                        && segments.is_empty()
                        && value["affected_segments"] == 0,
                )?;
                observation_stage(receipt, "ims-nested-outer-present", Some(row_index));
                let outer = cics_effects.get(&outer_effect_key).ok_or_else(refused)?;
                observation_stage(receipt, "ims-nested-outer-actor", Some(row_index));
                let found = actor::check_paging_actor(
                    store,
                    outer,
                    record,
                    &run,
                    "host.cics.execute",
                    receipt,
                )?;
                if found != owner {
                    receipt["ims_nested_outer_actor_failure"] =
                        serde_json::json!({"same_owner": false});
                }
                need(found == owner)?;
                observation_stage(receipt, "ims-nested-commit-request", Some(row_index));
                let commit = HostRequest::Ims(ImsRequest {
                    operation: ImsOperation::Commit,
                    psb: None,
                    pcb: 1,
                    segments: Vec::new(),
                    data: Vec::new(),
                    qualifiers: Vec::new(),
                    checkpoint_id: None,
                    max_segments: 1,
                    mutation: Some(Mutation {
                        sequence,
                        idempotency_key: IdempotencyKey::new(key, InvocationLimits::default())
                            .map_err(|_| refused())?,
                        transaction: Some("CPVS".into()),
                    }),
                    system: None,
                    q_class: None,
                });
                observation_stage(receipt, "ims-nested-request-digest", Some(row_index));
                need(
                    mainframe_env_host_api::canonical_request_digest(&commit)
                        .map_err(|_| refused())?
                        == descriptor.request_digest,
                )?;
                let resource = mainframe_env_host_api::canonical_audit_resource_digest(&commit);
                observation_stage(receipt, "ims-nested-audit-invocation", Some(row_index));
                let invocation = outer
                    .intent
                    .audit_invocation_key
                    .as_ref()
                    .ok_or_else(refused)?;
                observation_stage(receipt, "ims-nested-audit-read", Some(row_index));
                let audits = store
                    .audit_records(&owner.execution_id, 0, MAX_ROWS + 1)
                    .map_err(|_| refused())?;
                observation_stage(receipt, "ims-nested-audit-limit", Some(row_index));
                need(audits.len() <= MAX_ROWS)?;
                observation_stage(receipt, "ims-nested-audit-match", Some(row_index));
                let matches =
                    nested_audit_matches(&audits, &owner, &run, sequence, invocation, &resource);
                if !matches {
                    // Failure-only fixed counts/booleans from the same <=512 rows.
                    // No IDs, payloads, extra query, expected bytes or success credit.
                    let coarse = audits
                        .iter()
                        .filter(|audit| {
                            audit.effect_sequence == sequence
                                && audit.capability.as_str() == "host.ims.write"
                        })
                        .collect::<Vec<_>>();
                    let exact = coarse
                        .iter()
                        .filter(|audit| audit.resource == resource)
                        .collect::<Vec<_>>();
                    let singleton = match exact.as_slice() {
                        [audit] => Some(serde_json::json!({
                            "execution": audit.execution_id == owner.execution_id,
                            "run": audit.run_unit_id == run,
                            "principal": audit.principal == owner.principal,
                            "attempt": audit.attempt == owner.attempt,
                            "invocation_key": audit.invocation_key == *invocation,
                            "resource": audit.resource == resource,
                            "success": audit.decision == AuditDecision::Success
                        })),
                        _ => None,
                    };
                    receipt["ims_nested_audit_failure"] = serde_json::json!({
                        "ordinal": row_index, "audit_rows": audits.len(),
                        "sequence_capability_rows": coarse.len(),
                        "canonical_resource_rows": exact.len(),
                        "singleton_predicates": singleton
                    });
                }
                need(matches)?;
                observation_stage(receipt, "cics-uow-read", Some(row_index));
                let uow = store
                    .get_provider_state("cics-uow", &outer_effect_key)
                    .map_err(|_| refused())?
                    .ok_or_else(refused)?;
                observation_stage(receipt, "cics-uow-undo-read", Some(row_index));
                let undo = store
                    .get_provider_state("cics-uow-undo", run.as_str())
                    .map_err(|_| refused())?;
                observation_stage(receipt, "cics-uow-decode", Some(row_index));
                let uow = describe_cics_uow_row(&uow, undo.as_ref()).map_err(|_| refused())?;
                observation_stage(receipt, "cics-uow-committed-owner", Some(row_index));
                need(
                    uow.state == CicsUowState::Committed
                        && uow.dependency == CicsUowDependencyState::Clear
                        && uow.transaction == "CPVS"
                        && uow.effect_key.as_deref() == Some(outer_effect_key.as_str())
                        && uow.owner_execution.as_deref() == Some(owner.execution_id.as_str())
                        && uow.owner_run_unit.as_deref() == Some(run.as_str()),
                )?;
                nested_commits += 1;
            }
            None => {
                observation_stage(receipt, "ims-dependency-present", Some(row_index));
                return Err(refused());
            }
        }
    }
    observation_stage(receipt, "ims-nested-count", None);
    need(nested_commits == usize::from(!frame.calls.is_empty()))?;
    observation_stage(receipt, "ims-call-count", None);
    need(results.len() == frame.calls.len())?;
    for (call_index, (call, (status, segments, effect))) in
        frame.calls.iter().zip(results.into_values()).enumerate()
    {
        observation_stage(receipt, "call-audit-resource", Some(call_index));
        let resource = effect.intent.audit_resource.as_ref().ok_or_else(refused)?;
        observation_stage(receipt, "call-oracle", Some(call_index));
        let compared = call.compare(
            effect.sequence,
            &effect.key,
            effect.request_digest,
            resource,
            &status,
            &segments,
        );
        if compared.is_err() {
            receipt["ims_call_failure"] = call.failure_metadata(
                effect.sequence,
                &effect.key,
                effect.request_digest,
                resource,
                &status,
                &segments,
            );
        }
        compared?;
        if index == 0 {
            // Preserve the original eight actual request operands independently
            // of the new seven-child result oracle and later qualified paging.
            observation_stage(receipt, "initial-call-request", Some(call_index));
            let expected = super::super::expected_request(
                call_index,
                effect.sequence,
                &effect.key,
                &oracle.root[..6],
            )?;
            observation_stage(receipt, "initial-call-request-digest", Some(call_index));
            let compared =
                super::super::compare_request(&expected, effect.request_digest, resource);
            if compared.is_err() {
                receipt["initial_request_failure"] = serde_json::json!({
                    "request_digest": mainframe_env_host_api::canonical_request_digest(&expected).ok()
                        == Some(effect.request_digest),
                    "resource": mainframe_env_host_api::canonical_audit_resource_digest(&expected) == *resource,
                    "actual_request_digest_sha256": hex(&effect.request_digest),
                    "actual_resource_digest_sha256": hex(&resource.value)
                });
            }
            compared?;
        }
    }
    observation_stage(receipt, "complete", None);
    Ok(format!(
        "{:x}",
        Sha256::digest(owner.execution_id.as_str().as_bytes())
    ))
}

#[cfg(test)]
mod current_codec_tests {
    use super::*;
    use mainframe_env_cics::CicsReplayRowDescriptor;
    use mainframe_env_store_api::{
        EffectDigestFormat, EffectIntentMetadata, EffectRecord, EffectState,
    };
    const ACCOUNT: &[u8] = b"00000000050";
    const CUSTOMER: &[u8] = b"000000050";

    // Gate-only synthetic descriptors, not invented persisted V4 receipts.
    // Complete byte/core/result validation is the unchanged provider decoder
    // invoked first by the actual observer. Exact request operands are separate.
    fn descriptor(codec: CicsReplayCodecVersion) -> CicsReplayRowDescriptor {
        CicsReplayRowDescriptor {
            namespace: "cics-effect-replay-v1".into(),
            key: "paging-current-row".into(),
            row_version: 2,
            payload_digest: [1; 32],
            codec,
            retention: CicsReplayRetentionState::Terminal,
            owner_execution: Some("execution".into()),
            owner_run_unit: Some("run".into()),
            sequence: Some(1),
            deadline_tick: Some(1),
            resolution_tick: Some(2),
            terminal_tick: Some(2),
            request_digest: [2; 32],
            result_digest: [3; 32],
        }
    }
    fn effect(request: &HostRequest) -> EffectRecord {
        let limits = InvocationLimits::default();
        EffectRecord {
            execution_id: ExecutionId::new("execution", limits).unwrap(),
            run_unit_id: RunUnitId::new("run", limits).unwrap(),
            sequence: 1,
            key: IdempotencyKey::new("paging-current-row", limits).unwrap(),
            digest_format: EffectDigestFormat::CanonicalHostV1,
            request_digest: mainframe_env_host_api::canonical_request_digest(request).unwrap(),
            state: EffectState::Completed,
            result_digest: Some([1; 32]),
            resolved_tick: Some(2),
            intent: EffectIntentMetadata {
                owner: ExecutionId::new("execution", limits).unwrap(),
                attempt: 1,
                capability: Some(CapabilityId::new("host.cics.execute", limits).unwrap()),
                audit_resource: Some(mainframe_env_host_api::canonical_audit_resource_digest(
                    request,
                )),
                audit_invocation_key: None,
                created_tick: 1,
                recovery_after_tick: 100,
                epoch: 1,
                recovery_lease: None,
            },
        }
    }
    #[test]
    fn three_pinned_reads_require_v4_and_exact_canonical_audit() {
        for request in paging_plain_read_requests(ACCOUNT, CUSTOMER).unwrap() {
            let mut observed = effect(&request);
            let expected = expected_paging_cics_codec(&observed, ACCOUNT, CUSTOMER, 3).unwrap();
            assert_eq!(expected, CicsReplayCodecVersion::PlainReadV4);
            assert!(current_terminal_cics_owner(
                &descriptor(expected),
                Some(expected),
                "execution",
                "run"
            ));
            assert!(!current_terminal_cics_owner(
                &descriptor(CicsReplayCodecVersion::RetentionV3),
                Some(expected),
                "execution",
                "run"
            ));
            observed.intent.audit_resource.as_mut().unwrap().value[0] ^= 1;
            assert!(expected_paging_cics_codec(&observed, ACCOUNT, CUSTOMER, 3).is_err());
            observed.intent.audit_resource = None;
            assert!(expected_paging_cics_codec(&observed, ACCOUNT, CUSTOMER, 3).is_err());
        }
    }
    #[test]
    fn unselected_forms_and_nonreading_frames_keep_exact_v3() {
        for request in paging_plain_read_requests(ACCOUNT, CUSTOMER).unwrap() {
            assert_eq!(
                expected_paging_cics_codec(&effect(&request), ACCOUNT, CUSTOMER, 0).unwrap(),
                CicsReplayCodecVersion::RetentionV3
            );
            let HostRequest::Cics(original) = request else {
                unreachable!()
            };
            for variant in 0..5 {
                let mut changed = original.clone();
                match variant {
                    0 => {
                        let value = changed.arguments.remove("DATASET").unwrap();
                        changed.arguments.insert("FILE".into(), value);
                    }
                    1 => changed.operation = CicsOperation::ReadNext,
                    2 => {
                        changed.arguments.insert(
                            "OPTION.UPDATE".into(),
                            BoundedPayload::new(
                                "mainframe-env.cics.option@1",
                                b"1".to_vec(),
                                InvocationLimits::default(),
                            )
                            .unwrap(),
                        );
                    }
                    3 => {
                        changed.arguments.get_mut("RIDFLD").unwrap().clone_from(
                            &BoundedPayload::new(
                                "mainframe-env.cics.storage-value@1",
                                b"other-key".to_vec(),
                                InvocationLimits::default(),
                            )
                            .unwrap(),
                        );
                    }
                    4 => {
                        changed.arguments.insert(
                            "LENGTH".into(),
                            BoundedPayload::new(
                                "mainframe-env.cics.decimal@1",
                                b"49".to_vec(),
                                InvocationLimits::default(),
                            )
                            .unwrap(),
                        );
                    }
                    _ => unreachable!(),
                }
                let expected = expected_paging_cics_codec(
                    &effect(&HostRequest::Cics(changed)),
                    ACCOUNT,
                    CUSTOMER,
                    3,
                )
                .unwrap();
                assert_eq!(expected, CicsReplayCodecVersion::RetentionV3);
                assert!(current_terminal_cics_owner(
                    &descriptor(expected),
                    Some(expected),
                    "execution",
                    "run"
                ));
                assert!(!current_terminal_cics_owner(
                    &descriptor(CicsReplayCodecVersion::PlainReadV4),
                    Some(expected),
                    "execution",
                    "run"
                ));
            }
        }
    }
    #[test]
    fn pending_and_legacy_retention_are_refused_for_both_current_codecs() {
        for codec in [
            CicsReplayCodecVersion::RetentionV3,
            CicsReplayCodecVersion::PlainReadV4,
        ] {
            for retention in [
                CicsReplayRetentionState::PendingProtected,
                CicsReplayRetentionState::LegacyProtected,
            ] {
                let mut value = descriptor(codec);
                value.retention = retention;
                assert!(!current_terminal_cics_owner(
                    &value,
                    Some(codec),
                    "execution",
                    "run"
                ));
            }
        }
    }
    #[test]
    fn legacy_wrong_and_unclassified_generations_are_refused() {
        for codec in [
            CicsReplayCodecVersion::LegacyV1,
            CicsReplayCodecVersion::AttributionV2,
        ] {
            for expected in [
                CicsReplayCodecVersion::RetentionV3,
                CicsReplayCodecVersion::PlainReadV4,
            ] {
                assert!(!current_terminal_cics_owner(
                    &descriptor(codec),
                    Some(expected),
                    "execution",
                    "run"
                ));
            }
        }
        for codec in [
            CicsReplayCodecVersion::RetentionV3,
            CicsReplayCodecVersion::PlainReadV4,
        ] {
            assert!(!current_terminal_cics_owner(
                &descriptor(codec),
                None,
                "execution",
                "run"
            ));
        }
    }
    #[test]
    fn missing_or_foreign_execution_and_run_owners_are_refused() {
        for codec in [
            CicsReplayCodecVersion::RetentionV3,
            CicsReplayCodecVersion::PlainReadV4,
        ] {
            for variant in 0..4 {
                let mut value = descriptor(codec);
                match variant {
                    0 => value.owner_execution = None,
                    1 => value.owner_execution = Some("foreign-execution".into()),
                    2 => value.owner_run_unit = None,
                    3 => value.owner_run_unit = Some("foreign-run".into()),
                    _ => unreachable!(),
                }
                assert!(!current_terminal_cics_owner(
                    &value,
                    Some(codec),
                    "execution",
                    "run"
                ));
            }
        }
        // The same nested-audit predicate used by the actual observer must
        // tolerate a different canonical resource at the same sequence/capability.
        let limits = InvocationLimits::default();
        let owner = ExecutionRecord {
            execution_id: ExecutionId::new("execution", limits).unwrap(),
            run_unit_id: RunUnitId::new("run", limits).unwrap(),
            selector: Selector::new("program:COPAUS0C", limits).unwrap(),
            artifact: ArtifactRef::new("synthetic-paging-artifact", limits).unwrap(),
            principal: PrincipalId::new("WEBADM", limits).unwrap(),
            state: mainframe_env_store_api::ExecutionState::Completed,
            attempt: 1,
            version: 1,
            owner_lease: None,
            lease_expiry_tick: None,
            terminal_tick: Some(1),
        };
        let invocation = IdempotencyKey::new("nested-outer-invocation", limits).unwrap();
        let commit = HostRequest::Ims(ImsRequest {
            operation: ImsOperation::Commit,
            psb: None,
            pcb: 1,
            segments: Vec::new(),
            data: Vec::new(),
            qualifiers: Vec::new(),
            checkpoint_id: None,
            max_segments: 1,
            mutation: Some(Mutation {
                sequence: 7,
                idempotency_key: IdempotencyKey::new("nested-commit", limits).unwrap(),
                transaction: Some("CPVS".into()),
            }),
            system: None,
            q_class: None,
        });
        let resource = mainframe_env_host_api::canonical_audit_resource_digest(&commit);
        let valid = AuditRecord {
            execution_id: owner.execution_id.clone(),
            run_unit_id: owner.run_unit_id.clone(),
            attempt: owner.attempt,
            effect_sequence: 7,
            observed_tick: 1,
            principal: owner.principal.clone(),
            invocation_key: invocation.clone(),
            capability: CapabilityId::new("host.ims.write", limits).unwrap(),
            resource,
            decision: AuditDecision::Success,
        };
        assert!(nested_audit_matches(
            std::slice::from_ref(&valid),
            &owner,
            &owner.run_unit_id,
            7,
            &invocation,
            &resource,
        ));
        let mut different = commit;
        let HostRequest::Ims(request) = &mut different else {
            unreachable!()
        };
        request.operation = ImsOperation::Rollback;
        let mut unrelated = valid.clone();
        unrelated.resource = mainframe_env_host_api::canonical_audit_resource_digest(&different);
        assert_ne!(unrelated.resource, resource);
        assert!(nested_audit_matches(
            &[valid.clone(), unrelated.clone()],
            &owner,
            &owner.run_unit_id,
            7,
            &invocation,
            &resource,
        ));
        assert!(actor::audit_matches(
            &[valid.clone(), unrelated.clone()],
            &owner,
            &owner.run_unit_id,
            7,
            &invocation,
            &resource,
            "host.ims.write",
        ));
        assert!(!actor::audit_matches(
            &[valid.clone(), valid.clone()],
            &owner,
            &owner.run_unit_id,
            7,
            &invocation,
            &resource,
            "host.ims.write",
        ));
        assert!(!nested_audit_matches(
            &[valid.clone(), valid.clone()],
            &owner,
            &owner.run_unit_id,
            7,
            &invocation,
            &resource,
        ));
        for variant in 0..9 {
            let mut changed = valid.clone();
            match variant {
                0 => changed = unrelated.clone(),
                1 => changed.decision = AuditDecision::Deny,
                2 => changed.execution_id = ExecutionId::new("foreign-execution", limits).unwrap(),
                3 => changed.run_unit_id = RunUnitId::new("foreign-run", limits).unwrap(),
                4 => changed.principal = PrincipalId::new("FOREIGN", limits).unwrap(),
                5 => changed.attempt += 1,
                6 => changed.invocation_key = IdempotencyKey::new("foreign-key", limits).unwrap(),
                7 => changed.effect_sequence += 1,
                _ => changed.capability = CapabilityId::new("host.ims.read", limits).unwrap(),
            }
            assert!(!nested_audit_matches(
                std::slice::from_ref(&changed),
                &owner,
                &owner.run_unit_id,
                7,
                &invocation,
                &resource,
            ));
            assert!(!actor::audit_matches(
                std::slice::from_ref(&changed),
                &owner,
                &owner.run_unit_id,
                7,
                &invocation,
                &resource,
                "host.ims.write",
            ));
        }
    }
    #[test]
    fn unknown_raw_codec_is_refused_before_any_typed_gate() {
        let row = ProviderStateRecord {
            namespace: "cics-effect-replay-v1".into(),
            key: "paging-current-row".into(),
            version: 1,
            payload: b"MECER999".to_vec(),
        };
        assert!(describe_cics_replay_row(&row, None, CicsLimits::default()).is_err());
    }
}
