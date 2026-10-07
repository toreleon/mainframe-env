use super::*;
use crate::mq_raw_layout::*;

pub(crate) fn array<const N: usize>(seed: u8) -> [u8; N] {
    std::array::from_fn(|i| seed.wrapping_add(i as u8))
}
pub(crate) fn value(v2: bool, cp037: bool) -> MqMdValue {
    let characters = if cp037 {
        MqMdCharacterEncoding::OwnedCp037
    } else {
        MqMdCharacterEncoding::AsciiCompatible
    };
    let fields = MqMdFields {
        struc_id: if cp037 {
            [0xd4, 0xc4, 0x40, 0x40]
        } else {
            *b"MD  "
        },
        report: i32::MIN,
        msg_type: 123,
        expiry: -42,
        feedback: i32::MAX,
        encoding: -7,
        coded_char_set_id: 1208,
        format: array(0xf8),
        priority: -3,
        persistence: 77,
        msg_id: array(0),
        correl_id: array(24),
        backout_count: -99,
        reply_to_q: array(0xe0),
        reply_to_q_mgr: array(0x20),
        user_identifier: array(0x40),
        accounting_token: array(0x70),
        appl_identity_data: array(0),
        put_appl_type: -1234,
        put_appl_name: array(0x80),
        put_date: *b"        ",
        put_time: [0; 8],
        appl_origin_data: [0xff, 0, 0x40, 0x20],
    };
    if v2 {
        MqMdValue::V2 {
            characters,
            fields,
            extension: MqMdV2Fields {
                group_id: array(0xc0),
                msg_seq_number: -31,
                offset: i32::MAX,
                msg_flags: i32::MIN,
                original_length: -1,
            },
        }
    } else {
        MqMdValue::V1 { characters, fields }
    }
}

/// Independent declaration-order byte fixture, never rendered from a value,
/// catalog offsets or decoder output. Scalar/array literals are reviewed inputs.
pub(crate) fn raw(v2: bool, big: bool, cp037: bool) -> Vec<u8> {
    let mut raw = Vec::new();
    raw.extend_from_slice(if cp037 {
        &[0xd4, 0xc4, 0x40, 0x40]
    } else {
        b"MD  "
    });
    fn long(raw: &mut Vec<u8>, big: bool, n: i32) {
        raw.extend_from_slice(&if big {
            n.to_be_bytes()
        } else {
            n.to_le_bytes()
        });
    }
    for n in [
        if v2 { 2 } else { 1 },
        i32::MIN,
        123,
        -42,
        i32::MAX,
        -7,
        1208,
    ] {
        long(&mut raw, big, n);
    }
    raw.extend_from_slice(&array::<8>(0xf8));
    for n in [-3, 77] {
        long(&mut raw, big, n);
    }
    raw.extend_from_slice(&array::<24>(0));
    raw.extend_from_slice(&array::<24>(24));
    long(&mut raw, big, -99);
    raw.extend_from_slice(&array::<48>(0xe0));
    raw.extend_from_slice(&array::<48>(0x20));
    raw.extend_from_slice(&array::<12>(0x40));
    raw.extend_from_slice(&array::<32>(0x70));
    raw.extend_from_slice(&array::<32>(0));
    long(&mut raw, big, -1234);
    raw.extend_from_slice(&array::<28>(0x80));
    raw.extend_from_slice(b"        ");
    raw.extend_from_slice(&[0; 8]);
    raw.extend_from_slice(&[0xff, 0, 0x40, 0x20]);
    if v2 {
        raw.extend_from_slice(&array::<24>(0xc0));
        for n in [-31, i32::MAX, i32::MIN, -1] {
            long(&mut raw, big, n);
        }
    }
    assert_eq!(raw.len(), if v2 { 364 } else { 324 });
    raw
}
pub(crate) fn encoding(big: bool, cp037: bool) -> MqRawStructureEncoding {
    MqRawStructureEncoding {
        numbers: if big {
            MqRawNumberEncoding::NormalBigEndian
        } else {
            MqRawNumberEncoding::ReversedLittleEndian
        },
        characters: if cp037 {
            MqRawCharacterEncoding::OwnedCp037
        } else {
            MqRawCharacterEncoding::AsciiCompatible
        },
    }
}
pub(crate) fn kind(v2: bool) -> MqRawLayoutKind {
    if v2 {
        MqRawLayoutKind::Md2
    } else {
        MqRawLayoutKind::Md1
    }
}

#[test]
fn every_v1_v2_field_is_lossless_across_numeric_and_structure_character_profiles() {
    for v2 in [false, true] {
        for big in [false, true] {
            for cp037 in [false, true] {
                let input = raw(v2, big, cp037);
                let before = input.clone();
                let capture =
                    MqRawCapture::capture(kind(v2), &input, encoding(big, cp037)).unwrap();
                let actual = capture.to_full_md_value().unwrap();
                assert_eq!(actual, value(v2, cp037));
                assert_eq!(actual.version(), if v2 { 2 } else { 1 });
                assert_eq!(actual.validate_representation(), Ok(()));
                assert_eq!(capture.prefix(), before);
                assert_eq!(input, before);
                assert_eq!(
                    capture.try_typed_descriptor(),
                    Err(MqRawProblem::DescriptorRepresentationPending)
                );
                // Out-of-PIC-range and unrecognized flags/persistence are exact
                // observations, not a claim that a put/get accepts those values.
                assert_eq!(
                    crate::mq_raw_layout::mq_raw_cobol_long(i64::from(actual.fields().report)),
                    Err(MqRawProblem::CobolLongRange)
                );
                assert_eq!(actual.fields().encoding, -7);
                assert_eq!(actual.fields().coded_char_set_id, 1208);
                assert_eq!(actual.fields().persistence, 77);
            }
        }
    }
}
#[test]
fn suffix_capacity_and_raw_byte_order_do_not_enter_the_complete_value() {
    for v2 in [false, true] {
        let mut input = raw(v2, true, false);
        input.extend_from_slice(&[0xaa, 0xbb, 0xcc]);
        let before = input.clone();
        let capture = MqRawCapture::capture(kind(v2), &input, encoding(true, false)).unwrap();
        assert_eq!(capture.capacity(), before.len());
        assert_eq!(capture.to_full_md_value(), Ok(value(v2, false)));
        let little =
            MqRawCapture::capture(kind(v2), &raw(v2, false, false), encoding(false, false))
                .unwrap();
        assert_eq!(capture.to_full_md_value(), little.to_full_md_value());
        assert_eq!(input, before);
    }
}
#[test]
fn incomplete_later_version_wrong_identity_encoding_and_layout_fail_closed() {
    for v2 in [false, true] {
        let input = raw(v2, true, false);
        for length in 0..input.len() {
            assert_eq!(
                MqRawCapture::capture(kind(v2), &input[..length], encoding(true, false)),
                Err(MqRawProblem::Capacity)
            );
        }
        for version in [-1_i32, 0, 3, i32::MAX, if v2 { 1 } else { 2 }] {
            let mut bad = input.clone();
            bad[4..8].copy_from_slice(&version.to_be_bytes());
            assert_eq!(
                MqRawCapture::capture(kind(v2), &bad, encoding(true, false)),
                Err(MqRawProblem::Version)
            );
        }
        let mut bad = input.clone();
        bad[0] = 0;
        assert_eq!(
            MqRawCapture::capture(kind(v2), &bad, encoding(true, false)),
            Err(MqRawProblem::StructureIdentifier)
        );
        let mut unsupported = encoding(true, false);
        unsupported.characters = MqRawCharacterEncoding::Unsupported;
        assert_eq!(
            MqRawCapture::capture(kind(v2), &input, unsupported),
            Err(MqRawProblem::UnsupportedEncoding)
        );
        unsupported = encoding(true, false);
        unsupported.numbers = MqRawNumberEncoding::Unsupported;
        assert_eq!(
            MqRawCapture::capture(kind(v2), &input, unsupported),
            Err(MqRawProblem::UnsupportedEncoding)
        );
    }
    let mut od = vec![0; 168];
    od[..4].copy_from_slice(b"OD  ");
    od[4..8].copy_from_slice(&1_i32.to_be_bytes());
    let capture = MqRawCapture::capture(MqRawLayoutKind::Od1, &od, encoding(true, false)).unwrap();
    assert_eq!(capture.to_full_md_value(), Err(MqRawProblem::FieldKind));
}

#[test]
fn projection_helpers_check_generated_kind_and_width_instead_of_coercing() {
    // Projection helper tests live in its owning module through the catalog API;
    // public raw field observations also retain exact character/byte distinction.
    let capture =
        MqRawCapture::capture(kind(false), &raw(false, true, false), encoding(true, false))
            .unwrap();
    assert!(matches!(capture.field("Format"), Ok(MqRawFieldValue::Characters(v)) if v.len() == 8));
    assert!(matches!(capture.field("MsgId"), Ok(MqRawFieldValue::Bytes(v)) if v.len() == 24));
    assert!(matches!(
        capture.field("Report"),
        Ok(MqRawFieldValue::Long(i32::MIN))
    ));
    assert_eq!(capture.field("GroupId"), Err(MqRawProblem::Field));
}
