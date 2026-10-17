//! Unit codec/fence controls. These DTO fixtures are not installed-source owner credit.
use super::*;
use mainframe_env_store_api::EffectIntentMetadata;

fn replay(with_alias: bool) -> CicsEffectReplay {
    let response = CicsResponse {
        disposition: CicsDisposition::Complete,
        condition: "NORMAL".into(),
        response: 0,
        response2: 0,
        applid: "ME01".into(),
        sysid: "S001".into(),
        transaction: "AC01".into(),
        aid: 0x7d,
        target: None,
        next_transaction: None,
        payload: BoundedPayload::new(
            "mainframe-env.cics.record@1",
            b"AA11".to_vec(),
            InvocationLimits::default(),
        )
        .unwrap(),
        outputs: BTreeMap::new(),
        unit_of_work: None,
    };
    let mut replay = CicsEffectReplay {
        effect_key: Some("read:1".into()),
        owner_execution: Some("execution".into()),
        owner_run_unit: Some("run".into()),
        sequence: Some(1),
        deadline_tick: Some(100),
        resolution_tick: Some(300),
        request_digest: [1; 32],
        result_digest: Some(
            mainframe_env_host_api::canonical_result_digest(&Ok(HostResult::Cics(
                response.clone(),
            )))
            .unwrap(),
        ),
        binding_digest: None,
        read_alias: with_alias.then(|| ProviderStateRecord {
            namespace: "cics-file-alias".into(),
            key: "ACCOUNT".into(),
            version: 1,
            payload: encode_file_definition(&CicsFileDefinition {
                dataset: DatasetName::new("TEST.ACCOUNT", 128).unwrap(),
                ccsid: None,
            })
            .unwrap(),
        }),
        response,
    };
    replay.binding_digest = Some(cics_effect_replay_binding_digest(&replay));
    replay
}
fn effect(replay: &CicsEffectReplay) -> EffectRecord {
    let limits = InvocationLimits::default();
    EffectRecord {
        execution_id: ExecutionId::new("execution", limits).unwrap(),
        run_unit_id: RunUnitId::new("run", limits).unwrap(),
        sequence: 1,
        key: IdempotencyKey::new("read:1", limits).unwrap(),
        digest_format: EffectDigestFormat::CanonicalHostV1,
        request_digest: replay.request_digest,
        state: EffectState::Completed,
        result_digest: replay.result_digest,
        resolved_tick: Some(50),
        intent: EffectIntentMetadata {
            owner: ExecutionId::new("execution", limits).unwrap(),
            attempt: 1,
            capability: Some(CapabilityId::new("host.cics.execute", limits).unwrap()),
            audit_resource: None,
            audit_invocation_key: Some(IdempotencyKey::new("original", limits).unwrap()),
            created_tick: 1,
            recovery_after_tick: 100,
            epoch: 1,
            recovery_lease: None,
        },
    }
}
fn row(replay: &CicsEffectReplay) -> ProviderStateRecord {
    ProviderStateRecord {
        namespace: "cics-effect-replay-v1".into(),
        key: "read:1".into(),
        version: 2,
        payload: encode_cics_effect_replay(replay).unwrap(),
    }
}
#[test]
fn plain_read_v4_binds_exact_alias_and_refuses_corrupt_current_codec() {
    let original = replay(true);
    let effect = effect(&original);
    let good = row(&original);
    let desc =
        crate::describe_cics_replay_row(&good, Some(&effect), CicsLimits::default()).unwrap();
    assert_eq!(desc.codec, crate::CicsReplayCodecVersion::PlainReadV4);
    assert_eq!(desc.retention, crate::CicsReplayRetentionState::Terminal);
    assert_eq!(
        decode_cics_effect_replay(&good.payload, CicsLimits::default())
            .unwrap()
            .read_alias,
        original.read_alias
    );
    // Changed but syntactically valid alias keeps the old binding digest: it must
    // not become delivery proof for the replacement definition or CAS version.
    let mut changed = replay(true);
    changed.read_alias.as_mut().unwrap().version += 1;
    assert!(
        crate::describe_cics_replay_row(&row(&changed), Some(&effect), CicsLimits::default())
            .is_err()
    );
    let mut changed = replay(true);
    changed.read_alias.as_mut().unwrap().payload = encode_file_definition(&CicsFileDefinition {
        dataset: DatasetName::new("OTHER.ACCOUNT", 128).unwrap(),
        ccsid: None,
    })
    .unwrap();
    assert!(
        crate::describe_cics_replay_row(&row(&changed), Some(&effect), CicsLimits::default())
            .is_err()
    );
    for bytes in [
        good.payload[..good.payload.len() - 1].to_vec(),
        [good.payload.as_slice(), b"x"].concat(),
    ] {
        assert!(decode_cics_effect_replay(&bytes, CicsLimits::default()).is_err());
    }
    let mut changed = effect.clone();
    changed.state = EffectState::Intent;
    assert!(crate::describe_cics_replay_row(&good, Some(&changed), CicsLimits::default()).is_err());
    let mut changed = effect.clone();
    changed.result_digest = Some([7; 32]);
    assert!(crate::describe_cics_replay_row(&good, Some(&changed), CicsLimits::default()).is_err());
}
#[test]
fn plain_read_alias_codec_refuses_missing_identity_bounds_and_malformed_definition() {
    for bad in [
        ProviderStateRecord {
            namespace: "foreign".into(),
            ..replay(true).read_alias.unwrap()
        },
        ProviderStateRecord {
            key: "lowercase".into(),
            ..replay(true).read_alias.unwrap()
        },
        ProviderStateRecord {
            version: 0,
            ..replay(true).read_alias.unwrap()
        },
        ProviderStateRecord {
            payload: b"MEFA1".to_vec(),
            ..replay(true).read_alias.unwrap()
        },
    ] {
        let mut offered = replay(true);
        offered.read_alias = Some(bad);
        assert!(encode_cics_effect_replay(&offered).is_err());
    }
    let mut offered = replay(true);
    offered.effect_key = None;
    assert!(encode_cics_effect_replay(&offered).is_err());
}
#[test]
fn prior_v3_codec_and_binding_remain_exact_without_plain_read_alias() {
    let mut old = replay(false);
    old.result_digest = Some([2; 32]);
    old.binding_digest = Some([3; 32]);
    let bytes = encode_cics_effect_replay(&old).unwrap();
    assert_eq!(
        hex_fixture(&bytes),
        "4d4543455230303300000006726561643a3100000009657865637574696f6e0000000372756e00000000000000010000000000000064000000000000012c01010101010101010101010101010101010101010101010101010101010101010202020202020202020202020202020202020202020202020202020202020202030303030303030303030303030303030303030303030303030303030303030301000000064e4f524d414c0000000000000000000000044d453031000000045330303100000004414330317d00000000001b6d61696e6672616d652d656e762e636963732e7265636f7264403100000004414131310000000000"
    );
    let decoded = decode_cics_effect_replay(&bytes, CicsLimits::default()).unwrap();
    assert!(decoded.read_alias.is_none());
    assert_eq!(encode_cics_effect_replay(&decoded).unwrap(), bytes);
    assert_eq!(
        cics_effect_replay_binding_digest(&old),
        [
            221, 233, 119, 1, 148, 232, 170, 59, 85, 69, 102, 242, 166, 44, 191, 220, 177, 217, 50,
            69, 156, 167, 44, 1, 127, 105, 247, 198, 167, 86, 125, 102
        ]
    );
}
fn hex_fixture(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
