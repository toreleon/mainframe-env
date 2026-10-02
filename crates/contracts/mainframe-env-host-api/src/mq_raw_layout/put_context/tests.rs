use super::*;
use crate::mq_md_value::tests::{array, encoding, kind, raw, value};

fn context(call: MqMqiCall) -> MqRawWritebackContext {
    MqRawWritebackContext {
        call,
        platform: MqRawPlatform::Zos,
        single_queue: true,
        dynamic_model_open: false,
    }
}

// Diagnostic byte observations, not native legality, host identity or GMT proof.
fn returned(v2: bool, cp037: bool) -> MqMdValue {
    let mut md = value(v2, cp037);
    let fields = match &mut md {
        MqMdValue::V1 { fields, .. } | MqMdValue::V2 { fields, .. } => fields,
    };
    fields.user_identifier = array(0xf8);
    fields.accounting_token = array(0xe8);
    fields.appl_identity_data = array(0x08);
    fields.put_appl_type = -999_999_999;
    fields.put_appl_name = array(0x30);
    fields.put_date = array(0x50);
    fields.put_time = array(0x60);
    fields.appl_origin_data = array(0x80);
    md
}

fn expected(v2: bool, big: bool, cp037: bool) -> Vec<u8> {
    let mut bytes = raw(v2, big, cp037);
    // Independent MQMD declaration-order fixture, not generated catalog offsets.
    bytes[196..208].copy_from_slice(&array::<12>(0xf8));
    bytes[208..240].copy_from_slice(&array::<32>(0xe8));
    bytes[240..272].copy_from_slice(&array::<32>(0x08));
    bytes[272..276].copy_from_slice(&if big {
        (-999_999_999_i32).to_be_bytes()
    } else {
        (-999_999_999_i32).to_le_bytes()
    });
    bytes[276..304].copy_from_slice(&array::<28>(0x30));
    bytes[304..312].copy_from_slice(&array::<8>(0x50));
    bytes[312..320].copy_from_slice(&array::<8>(0x60));
    bytes[320..324].copy_from_slice(&array::<4>(0x80));
    bytes
}

#[test]
fn eight_profiles_and_both_put_calls_write_only_independent_context_bytes() {
    for v2 in [false, true] {
        for big in [false, true] {
            for cp037 in [false, true] {
                for call in [MqMqiCall::Put, MqMqiCall::PutOne] {
                    let mut before = raw(v2, big, cp037);
                    before.extend_from_slice(&[0, 0xff, 0x40, 0x20]);
                    let capture =
                        MqRawCapture::capture(kind(v2), &before, encoding(big, cp037)).unwrap();
                    let mut after = before.clone();
                    let md = returned(v2, cp037);
                    capture
                        .writeback_put_context_md(context(call), &md, &mut after)
                        .unwrap();
                    let mut wanted = expected(v2, big, cp037);
                    wanted.extend_from_slice(&before[capture.layout.prefix_bytes..]);
                    assert_eq!(after, wanted);
                    assert_eq!(&after[..196], &before[..196]);
                    assert_eq!(&after[324..], &before[324..]);
                    assert_eq!(
                        MqRawCapture::capture(kind(v2), &after, encoding(big, cp037))
                            .unwrap()
                            .to_full_md_value()
                            .unwrap(),
                        md
                    );
                }
            }
        }
    }
}

#[test]
fn actual_no_context_bytes_preserve_arbitrary_ignored_signed_input() {
    for big in [false, true] {
        for cp037 in [false, true] {
            let mut before = raw(true, big, cp037);
            before[96..100].copy_from_slice(&if big {
                i32::MAX.to_be_bytes()
            } else {
                i32::MAX.to_le_bytes()
            });
            let capture = MqRawCapture::capture(kind(true), &before, encoding(big, cp037)).unwrap();
            let mut md = capture.to_full_md_value().unwrap();
            let MqMdValue::V2 { fields, .. } = &mut md else {
                unreachable!()
            };
            let blank = if cp037 { 0x40 } else { b' ' };
            fields.user_identifier.fill(blank);
            fields.accounting_token.fill(0);
            fields.appl_identity_data.fill(blank);
            fields.put_appl_type = 0;
            fields.put_appl_name.fill(blank);
            fields.put_date.fill(blank);
            fields.put_time.fill(blank);
            fields.appl_origin_data.fill(blank);
            let mut wanted = before.clone();
            wanted[196..324].fill(blank);
            wanted[208..240].fill(0);
            wanted[272..276].fill(0);
            let mut after = before.clone();
            capture
                .writeback_put_context_md(context(MqMqiCall::Put), &md, &mut after)
                .unwrap();
            assert_eq!(after, wanted);
            assert_eq!(&after[96..100], &before[96..100]);
        }
    }
}

#[test]
fn changed_ids_ignored_counter_body_or_last_v2_field_refuse_all_writes() {
    let before = raw(true, true, false);
    let capture = MqRawCapture::capture(kind(true), &before, encoding(true, false)).unwrap();
    for n in 0..6 {
        let mut md = returned(true, false);
        let MqMdValue::V2 {
            fields, extension, ..
        } = &mut md
        else {
            unreachable!()
        };
        match n {
            0 => fields.msg_id[0] ^= 1,
            1 => fields.correl_id[0] ^= 1,
            2 => fields.backout_count = 0,
            3 => fields.coded_char_set_id += 1,
            4 => fields.reply_to_q_mgr[0] ^= 1,
            _ => extension.original_length += 1,
        }
        let mut after = before.clone();
        assert_eq!(
            capture.writeback_put_context_md(context(MqMqiCall::Put), &md, &mut after),
            Err(MqRawProblem::OutputPending)
        );
        assert_eq!(after, before);
    }
}

#[test]
fn output_scalar_range_failure_preserves_every_destination_byte() {
    let before = raw(false, false, true);
    let capture = MqRawCapture::capture(kind(false), &before, encoding(false, true)).unwrap();
    let mut md = returned(false, true);
    let MqMdValue::V1 { fields, .. } = &mut md else {
        unreachable!()
    };
    fields.put_appl_type = i32::MIN;
    let mut after = before.clone();
    assert_eq!(
        capture.writeback_put_context_md(context(MqMqiCall::PutOne), &md, &mut after),
        Err(MqRawProblem::CobolLongRange)
    );
    assert_eq!(after, before);
}

#[test]
fn foreign_version_characters_identifier_or_non_md_capture_refuse() {
    let before = raw(false, true, false);
    let capture = MqRawCapture::capture(kind(false), &before, encoding(true, false)).unwrap();
    for (md, error) in [
        (returned(true, false), MqRawProblem::Version),
        (returned(false, true), MqRawProblem::UnsupportedEncoding),
        (
            {
                let mut md = returned(false, false);
                let MqMdValue::V1 { fields, .. } = &mut md else {
                    unreachable!()
                };
                fields.struc_id = *b"BAD!";
                md
            },
            MqRawProblem::StructureIdentifier,
        ),
    ] {
        let mut after = before.clone();
        assert_eq!(
            capture.writeback_put_context_md(context(MqMqiCall::Put), &md, &mut after),
            Err(error)
        );
        assert_eq!(after, before);
    }
    let mut bytes = vec![0; 72];
    bytes[..4].copy_from_slice(b"GMO ");
    bytes[4..8].copy_from_slice(&1_i32.to_be_bytes());
    let capture =
        MqRawCapture::capture(MqRawLayoutKind::Gmo1, &bytes, encoding(true, false)).unwrap();
    let before = bytes.clone();
    assert_eq!(
        capture.writeback_put_context_md(
            context(MqMqiCall::Put),
            &returned(false, false),
            &mut bytes
        ),
        Err(MqRawProblem::FieldKind)
    );
    assert_eq!(bytes, before);
}

#[test]
fn stale_prefix_and_capacity_refuse_but_changed_suffix_remains_unowned() {
    let mut before = raw(true, true, false);
    before.extend_from_slice(&[1, 2, 3]);
    let capture = MqRawCapture::capture(kind(true), &before, encoding(true, false)).unwrap();
    let md = returned(true, false);
    let mut stale = before.clone();
    stale[320] ^= 1;
    let wanted = stale.clone();
    assert_eq!(
        capture.writeback_put_context_md(context(MqMqiCall::Put), &md, &mut stale),
        Err(MqRawProblem::StaleCapture)
    );
    assert_eq!(stale, wanted);
    let mut short = before[..before.len() - 1].to_vec();
    let wanted = short.clone();
    assert_eq!(
        capture.writeback_put_context_md(context(MqMqiCall::Put), &md, &mut short),
        Err(MqRawProblem::Capacity)
    );
    assert_eq!(short, wanted);
    let mut after = before.clone();
    after[364..].fill(0x7f);
    capture
        .writeback_put_context_md(context(MqMqiCall::Put), &md, &mut after)
        .unwrap();
    assert_eq!(&after[364..], &[0x7f; 3]);
}

#[test]
fn wrong_call_platform_topology_and_generic_policy_stay_closed() {
    let before = raw(false, true, false);
    let capture = MqRawCapture::capture(kind(false), &before, encoding(true, false)).unwrap();
    for policy in [
        context(MqMqiCall::Get),
        MqRawWritebackContext {
            platform: MqRawPlatform::Other,
            ..context(MqMqiCall::Put)
        },
        MqRawWritebackContext {
            single_queue: false,
            ..context(MqMqiCall::Put)
        },
        MqRawWritebackContext {
            dynamic_model_open: true,
            ..context(MqMqiCall::Put)
        },
    ] {
        let mut after = before.clone();
        assert_eq!(
            capture.writeback_put_context_md(policy, &returned(false, false), &mut after),
            Err(MqRawProblem::OutputPending)
        );
        assert_eq!(after, before);
    }
    let mut after = before.clone();
    assert_eq!(
        capture.writeback(
            context(MqMqiCall::Put),
            &[observed(
                "PutDate",
                MqRawFieldValue::Characters(b"20261003")
            )],
            &mut after
        ),
        Err(MqRawProblem::OutputPending)
    );
    assert_eq!(after, before);
}
