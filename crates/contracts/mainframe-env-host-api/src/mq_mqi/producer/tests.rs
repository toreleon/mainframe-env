use super::*;
use crate::mq_md_value::tests::value;
use crate::mq_md_value::{MqMdFields, MqMdV2Fields};
pub(crate) fn input(v2: bool, cp: bool) -> MqMqiFullPut {
    let mut md = value(v2, cp);
    let blank = if cp { 0x40 } else { b' ' };
    let f = match &mut md {
        MqMdValue::V1 { fields, .. } | MqMdValue::V2 { fields, .. } => fields,
    };
    *f = MqMdFields {
        struc_id: f.struc_id,
        report: 0,
        msg_type: 8,
        expiry: -1,
        feedback: 0,
        encoding: i32::MIN,
        coded_char_set_id: 819,
        format: [blank; 8],
        priority: 0,
        persistence: 1,
        msg_id: [0xa3; 24],
        correl_id: [0; 24],
        backout_count: i32::MIN,
        reply_to_q: [blank; 48],
        reply_to_q_mgr: [blank; 48],
        user_identifier: [0; 12],
        accounting_token: [0xff; 32],
        appl_identity_data: [0; 32],
        put_appl_type: i32::MIN,
        put_appl_name: [0; 28],
        put_date: [0; 8],
        put_time: [0; 8],
        appl_origin_data: [0; 4],
    };
    if let MqMdValue::V2 { extension, .. } = &mut md {
        *extension = MqMdV2Fields {
            group_id: [0; 24],
            msg_seq_number: 1,
            offset: 0,
            msg_flags: 0,
            original_length: -1,
        };
    }
    MqMqiFullPut {
        message: MqFullMessage {
            descriptor: md,
            body: vec![0, 255],
            properties: vec![],
        },
        message_handle: None,
        context: MqMqiMessageContext::NoContext,
        options: MqMqiOptions::PutV1Synchronous,
        unit: MqMqiUnitOfWork::NoSyncpoint,
    }
}
fn result(put: &MqMqiFullPut) -> MqMqiProduced {
    let mut md = put.message.descriptor.clone();
    let blank = if md.characters() == MqMdCharacterEncoding::OwnedCp037 {
        0x40
    } else {
        b' '
    };
    let f = match &mut md {
        MqMdValue::V1 { fields, .. } | MqMdValue::V2 { fields, .. } => fields,
    };
    f.user_identifier = [blank; 12];
    f.accounting_token = [0; 32];
    f.appl_identity_data = [blank; 32];
    f.put_appl_type = 0;
    f.put_appl_name = [blank; 28];
    f.put_date = [blank; 8];
    f.put_time = [blank; 8];
    f.appl_origin_data = [blank; 4];
    MqMqiProduced {
        descriptor: md,
        outcome: MqDeliveryOutcome::Accepted,
        resolved_queue: [b'Q'; 48],
        resolved_manager: [b'M'; 48],
        known_dest_count: MqMqiDestinationCount::UndefinedZos,
        unknown_dest_count: MqMqiDestinationCount::UndefinedZos,
        invalid_dest_count: MqMqiDestinationCount::UndefinedZos,
        backout_count: MqMqiIgnoredCounter::PreservedIgnoredInput,
    }
}
#[test]
fn finite_producer_all_signed_ignored_counters_and_old_default_pending_are_distinct() {
    for v2 in [false, true] {
        for cp in [false, true] {
            for counter in [i32::MIN, -1, 0, 255, i32::MAX] {
                let mut p = input(v2, cp);
                match &mut p.message.descriptor {
                    MqMdValue::V1 { fields, .. } | MqMdValue::V2 { fields, .. } => {
                        fields.backout_count = counter
                    }
                }
                p.validate_producer_profile().unwrap();
                let r = result(&p);
                r.validate(Default::default()).unwrap();
                r.bind(&p).unwrap();
                assert_eq!(r.descriptor.fields().backout_count, counter);
                p.options = MqMqiOptions::ContractDefault;
                assert!(p.validate_producer_profile().is_err());
            }
        }
    }
}
#[test]
fn producer_coherent_output_mutations_and_invalid_input_profiles_refused() {
    let p = input(true, false);
    for change in 0..8 {
        let mut r = result(&p);
        let f = match &mut r.descriptor {
            MqMdValue::V1 { fields, .. } | MqMdValue::V2 { fields, .. } => fields,
        };
        match change {
            0 => f.backout_count = 0,
            1 => f.correl_id[0] = 1,
            2 => f.format[0] = 0,
            3 => f.encoding = 0,
            4 => f.put_date[0] = 0,
            5 => f.user_identifier[0] = 1,
            6 => r.outcome = MqDeliveryOutcome::Pending,
            _ => f.msg_id[0] = 0,
        }
        assert!(r.bind(&p).is_err());
    }
    for change in 0..6 {
        let mut p = input(true, false);
        let f = match &mut p.message.descriptor {
            MqMdValue::V1 { fields, .. } | MqMdValue::V2 { fields, .. } => fields,
        };
        match change {
            0 => f.msg_id = [0; 24],
            1 => f.coded_char_set_id = 500,
            2 => f.priority = -2,
            3 => f.report = 1,
            4 => f.persistence = 3,
            _ => f.expiry = 0,
        }
        assert!(p.validate_producer_profile().is_err());
    }
}

#[test]
fn additive_producer_tags_frozen_and_feedback_fields_bind_full_host_digest() {
    use crate::canonical::encode;
    fn text(out: &mut Vec<u8>, value: &str) {
        out.push(1);
        out.extend_from_slice(&(value.len() as u64).to_le_bytes());
        out.extend_from_slice(value.as_bytes());
    }
    fn tag(ty: &str, name: &str) -> Vec<u8> {
        let mut out = b"producer-test\0".to_vec();
        out.push(0x41);
        text(&mut out, ty);
        text(&mut out, name);
        out.extend_from_slice(&0u64.to_le_bytes());
        out
    }
    let mut actual = vec![];
    encode(
        &MqMqiOptions::PutV1Synchronous,
        b"producer-test\0",
        4096,
        &mut |b| actual.extend_from_slice(b),
    )
    .unwrap();
    assert_eq!(actual, tag("MqMqiOptions", "PutV1Synchronous"));
    actual.clear();
    encode(
        &MqMqiMessageContext::NoContext,
        b"producer-test\0",
        4096,
        &mut |b| actual.extend_from_slice(b),
    )
    .unwrap();
    assert_eq!(actual, tag("MqMqiMessageContext", "NoContext"));
    actual.clear();
    encode(
        &MqMqiIgnoredCounter::PreservedIgnoredInput,
        b"producer-test\0",
        4096,
        &mut |b| actual.extend_from_slice(b),
    )
    .unwrap();
    assert_eq!(actual, tag("MqMqiIgnoredCounter", "PreservedIgnoredInput"));
    let p = result(&input(true, false));
    let digest = |p: MqMqiProduced| {
        crate::canonical_result_digest(&Ok(crate::HostResult::MqMqi(crate::MqMqiHostResult {
            limits: Default::default(),
            result: MqMqiResult {
                call: MqMqiCall::Put,
                outcome: MqMqiOutcome::StatusPending {
                    output: MqMqiOutput::Produced(p),
                },
            },
        })))
        .unwrap()
    };
    let original = digest(p.clone());
    for changed_field in 0..4 {
        let mut changed = p.clone();
        match changed_field {
            0 => changed.resolved_queue[0] = b'R',
            1 => changed.resolved_manager[0] = b'N',
            2 => changed.outcome = MqDeliveryOutcome::Pending,
            _ => match &mut changed.descriptor {
                MqMdValue::V1 { fields, .. } | MqMdValue::V2 { fields, .. } => {
                    fields.backout_count = 77
                }
            },
        };
        assert_ne!(digest(changed), original);
    }
}

#[test]
fn producer_queue_policy_inputs_remain_exact_not_effective_application_output() {
    for (priority, persistence) in [(-1, 2), (7, 0), (i32::MAX, 1)] {
        let mut p = input(true, false);
        let MqMdValue::V2 { fields, .. } = &mut p.message.descriptor else {
            panic!()
        };
        fields.priority = priority;
        fields.persistence = persistence;
        p.validate_producer_profile().unwrap();
        let r = result(&p);
        r.bind(&p).unwrap();
        assert_eq!(r.descriptor.fields().priority, priority);
        assert_eq!(r.descriptor.fields().persistence, persistence);
        for which in [true, false] {
            let mut changed = r.clone();
            let MqMdValue::V2 { fields, .. } = &mut changed.descriptor else {
                panic!()
            };
            if which {
                fields.priority = 3
            } else {
                fields.persistence = 0
            };
            if changed.descriptor != r.descriptor {
                assert!(changed.bind(&p).is_err());
            }
        }
    }
}
