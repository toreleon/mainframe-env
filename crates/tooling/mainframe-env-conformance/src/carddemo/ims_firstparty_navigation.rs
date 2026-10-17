//! One real CPVS navigation predicate; it does not certify the complete J17 journey.
use super::ims_firstparty_readonly::{Oracle, exercise_observed, sources};
use super::journey_closure::AuthorityKind;
use super::owned_source_receipt::{ReceiptOwner, require_owned_directory};
use super::*;
use mainframe_env_execution_api::AuditResourceDigest;
use mainframe_env_host_api::HostRequest;
use mainframe_env_ims::{ImsReplayDependency, ImsReplayRetentionState, describe_ims_replay_row};
use mainframe_env_store_api::{EffectDigestFormat, PlatformStore, ProviderStateRecord};
use std::os::unix::fs::{DirBuilderExt, MetadataExt};

#[path = "ims_firstparty_paging.rs"]
mod paging;

// An explicitly offered paging input selects the independent complete paging
// scenario. A malformed/mismatched offer refuses; it never falls back to six-b71.
// Without that offer, the original fixed readonly route and owner stay unchanged.
pub(super) fn verify_pages(inventory_path: &Path) -> Result<RouteObservations, CorpusProblem> {
    select_input_route(
        env::var_os("CARDDEMO_IMS_PAGING_ORACLE").is_some(),
        || paging::verify_from_env(inventory_path),
        || verify_from_env(inventory_path),
    )
}

fn select_input_route<T>(
    paging_offered: bool,
    paging: impl FnOnce() -> Result<T, CorpusProblem>,
    legacy: impl FnOnce() -> Result<T, CorpusProblem>,
) -> Result<T, CorpusProblem> {
    if paging_offered { paging() } else { legacy() }
}

fn refused() -> CorpusProblem {
    CorpusProblem::new(
        "carddemo.ims.firstparty_navigation_drift",
        "the exact installed guest navigation requests or owned completion differ",
    )
}

// COPAUS0C's pinned initial ENTER schedules PSBPAUTB, selects its packed account
// root, then issues six unqualified GNPs (five complete children and exact GE).
// Sequence/key are from each actual core effect; all remaining request operands
// are independently fixed by the pinned source and admitted private oracle.
fn expected_request(
    index: usize,
    sequence: u64,
    key: &IdempotencyKey,
    account: &[u8],
) -> Result<HostRequest, CorpusProblem> {
    if index >= 8 || sequence == 0 || account != [0, 0, 0, 0, 5, 0x0c] {
        return Err(refused());
    }
    Ok(HostRequest::Ims(ImsRequest {
        operation: match index {
            0 => ImsOperation::Schedule,
            1 => ImsOperation::GetUnique,
            _ => ImsOperation::GetNextParent,
        },
        psb: (index == 0).then(|| "PSBPAUTB".into()),
        pcb: 1,
        segments: match index {
            0 => Vec::new(),
            1 => vec!["PAUTSUM0".into()],
            _ => vec!["PAUTDTL1".into()],
        },
        data: Vec::new(),
        qualifiers: if index == 1 {
            vec![ImsQualifier {
                segment: "PAUTSUM0".into(),
                field: "ACCNTID".into(),
                value: account.to_vec(),
            }]
        } else {
            Vec::new()
        },
        checkpoint_id: None,
        max_segments: 1,
        mutation: Some(Mutation {
            sequence,
            idempotency_key: key.clone(),
            transaction: None,
        }),
        system: None,
        q_class: None,
    }))
}

fn compare_request(
    expected: &HostRequest,
    request_digest: [u8; 32],
    resource: &AuditResourceDigest,
) -> Result<(), CorpusProblem> {
    if mainframe_env_host_api::canonical_request_digest(expected).map_err(|_| refused())?
        != request_digest
        || mainframe_env_host_api::canonical_audit_resource_digest(expected) != *resource
    {
        return Err(refused());
    }
    Ok(())
}

// Called only after the unchanged readonly actor/result/audit/UOW comparator
// succeeds. This binds operation identity too; result counts never mint a token.
pub(super) fn check_requests(
    store: &dyn PlatformStore,
    before: &BTreeMap<String, ProviderStateRecord>,
    oracle: &Oracle,
) -> Result<(), CorpusProblem> {
    let rows = super::ims_firstparty_readonly::rows(store, "ims-v1-replay")?;
    let mut effects = BTreeMap::new();
    for row in rows.values().filter(|row| !before.contains_key(&row.key)) {
        let key =
            IdempotencyKey::new(&row.key, InvocationLimits::default()).map_err(|_| refused())?;
        let effect = store.effect(&key).map_err(|_| refused())?;
        let descriptor = describe_ims_replay_row(row, effect.as_ref(), ImsLimits::default())
            .map_err(|_| refused())?;
        if descriptor.retention != ImsReplayRetentionState::Terminal {
            return Err(refused());
        }
        if descriptor.dependency == Some(ImsReplayDependency::CoreEffect) {
            let effect = effect.ok_or_else(refused)?;
            if effect.digest_format != EffectDigestFormat::CanonicalHostV1
                || descriptor.request_digest != effect.request_digest
                || effects.insert(effect.sequence, effect).is_some()
            {
                return Err(refused());
            }
        }
    }
    if effects.len() != 8 {
        return Err(refused());
    }
    for (index, effect) in effects.into_values().enumerate() {
        let expected = expected_request(index, effect.sequence, &effect.key, &oracle.root[..6])?;
        compare_request(
            &expected,
            effect.request_digest,
            effect.intent.audit_resource.as_ref().ok_or_else(refused)?,
        )?;
    }
    Ok(())
}

pub(super) fn verify_from_env(inventory_path: &Path) -> Result<RouteObservations, CorpusProblem> {
    let corpus = verify_carddemo_corpus_from_env(inventory_path)?;
    let corpus_dir = PathBuf::from(env::var_os(CORPUS_ENV).ok_or_else(refused)?);
    sources(&corpus_dir)?;
    let oracle_path =
        PathBuf::from(env::var_os("CARDDEMO_IMS_READONLY_ORACLE").ok_or_else(refused)?);
    let oracle = Oracle::read(&oracle_path)?;
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .map_err(|_| refused())?;
    let parent = env::temp_dir().canonicalize().map_err(|_| refused())?;
    if parent.starts_with(&repository) {
        return Err(refused());
    }
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| refused())?
        .as_nanos();
    let evidence = parent.join(format!(
        "mainframe-env-carddemo-ims-navigation-{}-{nonce}",
        std::process::id()
    ));
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&evidence)
        .map_err(|_| refused())?;
    let metadata = fs::symlink_metadata(&evidence).map_err(|_| refused())?;
    let identity = (metadata.dev(), metadata.ino());
    require_owned_directory(&evidence, identity)?;
    let artifacts = evidence.join("owned-artifacts");
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&artifacts)
        .map_err(|_| refused())?;
    let metadata = fs::symlink_metadata(&artifacts).map_err(|_| refused())?;
    let artifact_identity = (metadata.dev(), metadata.ino());
    let mut owned = ReceiptOwner::open(&evidence, identity, &artifacts, artifact_identity)?;
    let mut receipt = serde_json::json!({
        "schema":"mainframe-env.carddemo-ims-navigation-observation@1",
        "corpus_commit":corpus.commit,"backend":"memory","physical_store_restart":false,
        "source_program":"COPAUS0C","transaction":"CPVS","stage":"before-open",
        "first_drain":false,"second_drain":false,"reopen_compared":false,"cleanup_verified":false,
        "session_disconnect_checked":false,"actual_installed_guest_compared":false,"actual_ims_calls":0,
        "whole_seed_hierarchy_compared":false,"fixed_named_fields_compared":0,
        "root_seed_bytes":100,"child_seed_bytes":200,"child_seed_count":5,
        "whole_private_guest_storage_postimage_claimed":false,"raw_storage_retained":false,
        "original_j15_j16_j17_j18_criteria_unchanged":true,"application_credit":0,"vendor_credit":0,
        "acceptance_credit":0,"artifact_root":artifacts,"artifact_identity":artifact_identity,
        "controls":"default config/limits/router/date; original public HTTP and process bounds"});
    owned.save(&receipt)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| refused())?;
    let result = runtime.block_on(exercise_observed(
        &corpus_dir,
        &oracle,
        &mut owned,
        &mut receipt,
    ));
    if let Err(problem) = &result {
        receipt["producer_problem_code"] = serde_json::json!(problem.code);
    }
    let retained = owned.save(&receipt);
    result?;
    retained?;
    // No token escapes until both original drains, complete reopen comparison,
    // session disconnect and exact owned artifact absence have succeeded.
    let mut observed = RouteObservations::default();
    observed.compare(
        AuthorityKind::Journey,
        "CD.J17",
        "IMS GU/GNP navigation",
        false,
        || Ok(refused()),
    )?;
    Ok(observed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> IdempotencyKey {
        IdempotencyKey::new("synthetic-request-control", InvocationLimits::default()).unwrap()
    }

    #[test]
    fn navigation_requests_bind_operation_order_account_and_complete_operands() {
        for index in 0..8 {
            let expected = expected_request(index, 41, &key(), &[0, 0, 0, 0, 5, 0x0c]).unwrap();
            let digest = mainframe_env_host_api::canonical_request_digest(&expected).unwrap();
            let resource = mainframe_env_host_api::canonical_audit_resource_digest(&expected);
            compare_request(&expected, digest, &resource).unwrap();
            for variant in 0..9 {
                let mut changed = expected.clone();
                let HostRequest::Ims(request) = &mut changed else {
                    panic!("IMS fixture")
                };
                match variant {
                    0 => request.operation = ImsOperation::GetNext,
                    1 => request.psb = Some("OTHERPSB".into()),
                    2 => request.pcb = 2,
                    3 => request.segments = vec!["OTHERSEG".into()],
                    4 => request.data = vec![0],
                    5 => {
                        request.qualifiers = vec![ImsQualifier {
                            segment: "PAUTSUM0".into(),
                            field: "ACCNTID".into(),
                            value: vec![0; 6],
                        }]
                    }
                    6 => request.max_segments = 2,
                    7 => request.mutation.as_mut().unwrap().sequence += 1,
                    _ => request.mutation.as_mut().unwrap().transaction = Some("CPVS".into()),
                }
                assert!(compare_request(&changed, digest, &resource).is_err());
            }
        }
        assert!(expected_request(8, 41, &key(), &[0, 0, 0, 0, 5, 0x0c]).is_err());
        assert!(expected_request(1, 0, &key(), &[0, 0, 0, 0, 5, 0x0c]).is_err());
        assert!(expected_request(1, 41, &key(), &[0; 6]).is_err());
    }

    #[test]
    fn navigation_request_and_audit_cannot_be_substituted_by_result_metadata() {
        let request = expected_request(1, 41, &key(), &[0, 0, 0, 0, 5, 0x0c]).unwrap();
        let digest = mainframe_env_host_api::canonical_request_digest(&request).unwrap();
        let resource = mainframe_env_host_api::canonical_audit_resource_digest(&request);
        let other = expected_request(2, 42, &key(), &[0, 0, 0, 0, 5, 0x0c]).unwrap();
        let other_resource = mainframe_env_host_api::canonical_audit_resource_digest(&other);
        assert!(compare_request(&request, [0; 32], &resource).is_err());
        assert!(compare_request(&request, digest, &other_resource).is_err());
        assert!(compare_request(&other, digest, &resource).is_err());
    }
    #[test]
    #[ignore = "real installed COPAUS0C navigation route; exact pinned corpus/private oracle; one partial J17 predicate only; no complete journey or issue credit"]
    fn pinned_installed_navigation_emits_only_request_bound_gu_gnp_observation() {
        let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let observed = verify_from_env(
            &repository.join("conformance/profiles/carddemo/inventory/carddemo-corpus.json"),
        )
        .expect("exact source, independent oracle, real requests and owned teardown required");
        assert_eq!(
            observed.journeys(),
            &[("CD.J17".into(), vec!["IMS GU/GNP navigation".into()])]
        );
        assert!(observed.issues().is_empty());
    }
}
