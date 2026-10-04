use super::*;
use crate::mq_md_value::tests::{encoding, kind, raw, value};

fn context() -> MqRawWritebackContext {
    MqRawWritebackContext {
        call: MqMqiCall::Get,
        platform: MqRawPlatform::Zos,
        single_queue: true,
        dynamic_model_open: false,
    }
}

// Diagnostic value/independent declaration-order fixture, not native legality.
fn returned(v2: bool, cp037: bool) -> MqMdValue {
    let mut md = value(v2, cp037);
    let fields = match &mut md {
        MqMdValue::V1 { fields, .. } | MqMdValue::V2 { fields, .. } => fields,
    };
    fields.report = -999_999_999;
    fields.feedback = 999_999_999;
    if let MqMdValue::V2 { extension, .. } = &mut md {
        extension.offset = -17;
        extension.msg_flags = 19;
    }
    md
}

fn expected(v2: bool, big: bool, cp037: bool) -> Vec<u8> {
    let mut bytes = raw(v2, big, cp037);
    // Independent MQMD declaration offsets, not the generated writer catalog.
    for (offset, number) in [(8, -999_999_999_i32), (20, 999_999_999)] {
        bytes[offset..offset + 4].copy_from_slice(&if big {
            number.to_be_bytes()
        } else {
            number.to_le_bytes()
        });
    }
    if v2 {
        for (offset, number) in [(352, -17_i32), (356, 19)] {
            bytes[offset..offset + 4].copy_from_slice(&if big {
                number.to_be_bytes()
            } else {
                number.to_le_bytes()
            });
        }
    }
    bytes
}

fn input(v2: bool, big: bool, cp037: bool) -> Vec<u8> {
    let mut bytes = raw(v2, big, cp037);
    bytes[8..].fill(0xa5);
    bytes.extend_from_slice(&[0, 0xff, 0x40, 0x20, 0]);
    bytes
}

#[test]
fn every_complete_field_matches_independent_bytes_in_all_eight_profiles() {
    for v2 in [false, true] {
        for big in [false, true] {
            for cp037 in [false, true] {
                let before = input(v2, big, cp037);
                let capture =
                    MqRawCapture::capture(kind(v2), &before, encoding(big, cp037)).unwrap();
                let mut after = before.clone();
                let md = returned(v2, cp037);
                capture
                    .writeback_full_get_md(context(), &md, &mut after)
                    .unwrap();
                let mut wanted = expected(v2, big, cp037);
                wanted.extend_from_slice(&before[capture.layout.prefix_bytes..]);
                assert_eq!(after, wanted);
                assert_eq!(&after[..8], &before[..8]);
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

#[test]
fn last_v2_scalar_failure_preserves_every_destination_byte() {
    let before = input(true, true, false);
    let capture = MqRawCapture::capture(kind(true), &before, encoding(true, false)).unwrap();
    let mut md = returned(true, false);
    let MqMdValue::V2 { extension, .. } = &mut md else {
        unreachable!()
    };
    extension.original_length = i32::MAX;
    let mut after = before.clone();
    assert_eq!(
        capture.writeback_full_get_md(context(), &md, &mut after),
        Err(MqRawProblem::CobolLongRange)
    );
    assert_eq!(after, before);
}

#[test]
fn common_scalar_failure_preserves_every_destination_byte() {
    let before = input(false, false, true);
    let capture = MqRawCapture::capture(kind(false), &before, encoding(false, true)).unwrap();
    let mut md = returned(false, true);
    let MqMdValue::V1 { fields, .. } = &mut md else {
        unreachable!()
    };
    fields.backout_count = i32::MIN;
    let mut after = before.clone();
    assert_eq!(
        capture.writeback_full_get_md(context(), &md, &mut after),
        Err(MqRawProblem::CobolLongRange)
    );
    assert_eq!(after, before);
}

#[test]
fn foreign_version_characters_identifier_and_call_refuse_without_writes() {
    let before = input(false, true, false);
    let capture = MqRawCapture::capture(kind(false), &before, encoding(true, false)).unwrap();
    for (md, problem) in [
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
            capture.writeback_full_get_md(context(), &md, &mut after),
            Err(problem)
        );
        assert_eq!(after, before);
    }
    for call in [MqMqiCall::Put, MqMqiCall::PutOne, MqMqiCall::Open] {
        let mut after = before.clone();
        let context = MqRawWritebackContext { call, ..context() };
        assert_eq!(
            capture.writeback_full_get_md(context, &returned(false, false), &mut after),
            Err(MqRawProblem::OutputPending)
        );
        assert_eq!(after, before);
    }
}

#[test]
fn stale_prefix_and_capacity_refuse_but_suffix_is_never_owned() {
    let before = input(true, true, false);
    let capture = MqRawCapture::capture(kind(true), &before, encoding(true, false)).unwrap();
    let md = returned(true, false);
    let mut stale = before.clone();
    stale[300] ^= 1;
    let wanted = stale.clone();
    assert_eq!(
        capture.writeback_full_get_md(context(), &md, &mut stale),
        Err(MqRawProblem::StaleCapture)
    );
    assert_eq!(stale, wanted);
    let mut short = before[..before.len() - 1].to_vec();
    let wanted = short.clone();
    assert_eq!(
        capture.writeback_full_get_md(context(), &md, &mut short),
        Err(MqRawProblem::Capacity)
    );
    assert_eq!(short, wanted);
    let mut suffix = before.clone();
    suffix[364..].fill(0x7f);
    capture
        .writeback_full_get_md(context(), &md, &mut suffix)
        .unwrap();
    assert_eq!(&suffix[364..], &[0x7f; 5]);
}

#[test]
fn non_descriptor_capture_cannot_be_repurposed_as_md() {
    let mut bytes = vec![0; 72];
    bytes[..4].copy_from_slice(b"GMO ");
    bytes[4..8].copy_from_slice(&1_i32.to_be_bytes());
    let capture =
        MqRawCapture::capture(MqRawLayoutKind::Gmo1, &bytes, encoding(true, false)).unwrap();
    let before = bytes.clone();
    assert_eq!(
        capture.writeback_full_get_md(context(), &returned(false, false), &mut bytes),
        Err(MqRawProblem::FieldKind)
    );
    assert_eq!(bytes, before);
}
