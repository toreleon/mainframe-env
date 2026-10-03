use super::*;
use crate::MqPropertyType;
use crate::mq_mqi::property::MqPropertyDescriptor;
use crate::mq_status::MqCompletion;

const ENCODING: MqRawStructureEncoding = MqRawStructureEncoding {
    numbers: MqRawNumberEncoding::NormalBigEndian,
    characters: MqRawCharacterEncoding::AsciiCompatible,
};
const IMPO: MqRawPropertyKind = MqRawPropertyKind::Impo1NullSlot4AsciiNormal;
const CHARV: MqRawPropertyKind = MqRawPropertyKind::CharvNullSlot4AsciiNormal;

// Independent reviewed byte vectors, not built using generated field offsets.
fn group(start: usize, size: i32) -> Vec<u8> {
    let mut v = vec![0xa5; start + 80];
    v[start..start + 4].copy_from_slice(b"IMPO");
    for (offset, value) in [
        (4, 1_i32),
        (8, 0),
        (12, i32::MIN),
        (16, -3),
        (20, 17),
        (24, 31),
        (36, 64),
        (40, size),
        (44, 0),
        (48, -3),
    ] {
        v[start + offset..start + offset + 4].copy_from_slice(&value.to_be_bytes());
    }
    v[start + 28..start + 32].copy_from_slice(&[b' ', 0, 0xff, b' ']);
    v[start + 32..start + 36].fill(0);
    v[start + 52..start + 60].copy_from_slice(&[0, 255, b' ', b'x', b'y', 0, b' ', 127]);
    v
}
fn capture(v: &[u8], start: usize) -> MqRawPropertyCapture {
    MqRawPropertyCapture::capture(IMPO, v, start, ENCODING).unwrap()
}
fn status(completion: MqCompletion, reason: &str) -> MqReviewedStatus {
    MqReviewedStatus::from_symbols(MqMqiCall::InquireProperty, completion.symbol(), reason).unwrap()
}
fn inquiry(kind: MqPropertyType, name: &[u8], length: i32) -> MqPropertyObservation {
    MqPropertyObservation::Inquired(MqPropertyInquiryObservation {
        descriptor: MqPropertyDescriptor::source_default(),
        kind,
        returned_encoding: 785,
        returned_ccsid: (kind == MqPropertyType::String).then_some(1208),
        returned_name: name.to_vec(),
        name_length: length,
        name_ccsid: 1208,
        data_length: 0,
        copied_value: vec![],
    })
}
fn plan(v: &[u8], start: usize, observation: &MqPropertyObservation) -> MqRawPropertyWriteback {
    capture(v, start)
        .prepare_inquiry(
            observation,
            status(MqCompletion::Ok, "MQRC_NONE"),
            MqMqiLimits::default(),
            &[],
        )
        .unwrap()
}

#[test]
fn complete_reconciled_fields_and_fixed_offsets() {
    let c = capture(&group(0, 8), 0);
    let expected = [
        ("StrucId", 0, 4),
        ("Version", 4, 4),
        ("Options", 8, 4),
        ("RequestedEncoding", 12, 4),
        ("RequestedCCSID", 16, 4),
        ("ReturnedEncoding", 20, 4),
        ("ReturnedCCSID", 24, 4),
        ("Reserved1", 28, 4),
        ("ReturnedName.VSPtr", 32, 4),
        ("ReturnedName.VSOffset", 36, 4),
        ("ReturnedName.VSBufSize", 40, 4),
        ("ReturnedName.VSLength", 44, 4),
        ("ReturnedName.VSCCSID", 48, 4),
        ("TypeString", 52, 8),
    ];
    assert_eq!(c.layout.prefix_bytes, 60);
    assert_eq!(
        c.layout
            .fields
            .iter()
            .map(|f| (f.name, f.offset, f.width))
            .collect::<Vec<_>>(),
        expected
    );
    for f in c.layout.fields {
        c.value(f.name).unwrap();
    }
    assert_eq!(c.long("RequestedEncoding"), Ok(i32::MIN));
    assert_eq!(
        c.value("Reserved1"),
        Ok(MqRawPropertyValue::Characters(&[b' ', 0, 255, b' ']))
    );
    assert_eq!(
        c.value("TypeString"),
        Ok(MqRawPropertyValue::Characters(&[
            0, 255, b' ', b'x', b'y', 0, b' ', 127
        ]))
    );
    assert_eq!(
        c.value("ReturnedName.VSPtr"),
        Ok(MqRawPropertyValue::NullSlot(&[0; 4]))
    );
    assert_eq!(c.long("Reserved1"), Err(MqRawPropertyProblem::FieldKind));
    assert_eq!(c.value("VSBuffer"), Err(MqRawPropertyProblem::Field));
}

#[test]
fn standalone_charv_and_offset_origins_are_distinct() {
    let mut v = vec![0xcc; 40];
    v[..4].fill(0);
    for (offset, value) in [(4, 24_i32), (8, 4), (12, 2), (16, 1208)] {
        v[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
    }
    v[24..28].copy_from_slice(&[b'a', 0, 255, b' ']);
    let c = MqRawPropertyCapture::capture(CHARV, &v, 0, ENCODING).unwrap();
    assert_eq!(c.layout.prefix_bytes, 20);
    assert_eq!(c.charv_region(&[]), Ok(24..28));
    assert_eq!(c.charv_bytes(&[]).unwrap(), &[b'a', 0, 255, b' ']);
    assert_eq!(c.original(), v);
    let v = group(8, 8);
    let c = capture(&v, 8);
    assert_eq!(c.charv_region(&[]), Ok(72..80));
    assert_eq!(c.prefix(), &v[8..68]);
    assert_eq!(c.original(), v);
    assert_eq!(c.capacity(), 88);
    assert_eq!(c.encoding(), ENCODING);
}

#[test]
fn all_prefix_truncations_and_alignment_capacity_overflow_reject_unchanged() {
    for length in 0..60 {
        let v = group(0, 8);
        assert_eq!(
            MqRawPropertyCapture::capture(IMPO, &v[..length], 0, ENCODING),
            Err(MqRawPropertyProblem::Capacity)
        );
    }
    for length in 0..20 {
        assert_eq!(
            MqRawPropertyCapture::capture(CHARV, &vec![0; length], 0, ENCODING),
            Err(MqRawPropertyProblem::Capacity)
        );
    }
    let v = group(0, 8);
    for start in [1, usize::MAX, usize::MAX - 1] {
        assert_eq!(
            MqRawPropertyCapture::capture(IMPO, &v, start, ENCODING),
            Err(MqRawPropertyProblem::Capacity)
        );
    }
    let mut huge = v.clone();
    huge.resize(65537, 0);
    assert_eq!(
        MqRawPropertyCapture::capture(IMPO, &huge, 0, ENCODING),
        Err(MqRawPropertyProblem::Capacity)
    );
    huge.truncate(65536);
    assert_eq!(capture(&huge, 0).capacity(), 65536);
}

#[test]
fn invalid_id_versions_pointer_and_encoding_are_not_guessed() {
    let original = group(0, 8);
    for (offset, bytes, expected) in [
        (0, *b"IPMO", MqRawPropertyProblem::StructureIdentifier),
        (4, (-1_i32).to_be_bytes(), MqRawPropertyProblem::Version),
        (4, 2_i32.to_be_bytes(), MqRawPropertyProblem::Version),
        (32, [0, 0, 0, 1], MqRawPropertyProblem::Pointer),
    ] {
        let mut v = original.clone();
        v[offset..offset + 4].copy_from_slice(&bytes);
        let before = v.clone();
        assert_eq!(
            MqRawPropertyCapture::capture(IMPO, &v, 0, ENCODING),
            Err(expected)
        );
        assert_eq!(v, before);
    }
    for encoding in [
        MqRawStructureEncoding {
            numbers: MqRawNumberEncoding::ReversedLittleEndian,
            ..ENCODING
        },
        MqRawStructureEncoding {
            numbers: MqRawNumberEncoding::Unsupported,
            ..ENCODING
        },
        MqRawStructureEncoding {
            characters: MqRawCharacterEncoding::OwnedCp037,
            ..ENCODING
        },
        MqRawStructureEncoding {
            characters: MqRawCharacterEncoding::Unsupported,
            ..ENCODING
        },
    ] {
        assert_eq!(
            MqRawPropertyCapture::capture(IMPO, &original, 0, encoding),
            Err(MqRawPropertyProblem::UnsupportedEncoding)
        );
    }
}

#[test]
fn absent_negative_and_overlapping_offset_capacity_are_raw_then_fail_resolution() {
    for (offset, size, error) in [
        (0_i32, 8, MqRawPropertyProblem::Offset),
        (-1, 8, MqRawPropertyProblem::Offset),
        (32, 8, MqRawPropertyProblem::Offset),
        (64, 0, MqRawPropertyProblem::BufferCapacity),
        (64, -1, MqRawPropertyProblem::BufferCapacity),
        (i32::MAX, 1, MqRawPropertyProblem::BufferCapacity),
        (64, i32::MAX, MqRawPropertyProblem::BufferCapacity),
    ] {
        let mut v = group(0, size);
        v[36..40].copy_from_slice(&offset.to_be_bytes());
        let c = capture(&v, 0);
        assert_eq!(c.long("ReturnedName.VSOffset"), Ok(offset));
        assert_eq!(c.charv_region(&[]), Err(error));
    }
    let c = capture(&group(0, 8), 0);
    assert_eq!(
        c.charv_region(&[63..66]),
        Err(MqRawPropertyProblem::Overlap)
    );
    assert_eq!(
        c.charv_region(&[79..81]),
        Err(MqRawPropertyProblem::Overlap)
    );
    assert_eq!(c.charv_region(&[5..4]), Err(MqRawPropertyProblem::Overlap));
    assert_eq!(c.charv_region(&[0..60, 72..80]), Ok(64..72));
    assert_eq!(
        c.charv_region(&vec![0..0; 9]),
        Err(MqRawPropertyProblem::Overlap)
    );
}

#[test]
fn standard_string_writeback_only_changes_defined_touched_fields() {
    let mut v = group(8, 8);
    let original = v.clone();
    plan(&v, 8, &inquiry(MqPropertyType::String, b"abc", 3))
        .commit(&mut v, ENCODING)
        .unwrap();
    let mut expected = original.clone();
    for (offset, value) in [(20, 785_i32), (24, 1208), (44, 3), (48, 1208)] {
        expected[8 + offset..8 + offset + 4].copy_from_slice(&value.to_be_bytes());
    }
    expected[72..75].copy_from_slice(b"abc");
    assert_eq!(v, expected);
    assert_eq!(&v[36..40], &original[36..40]); // four Reserved1 characters
    assert_eq!(&v[60..68], &original[60..68]); // all eight TypeString bytes
    assert_eq!(&v[75..], &original[75..]);
}

#[test]
fn nonstring_ccsid_and_typestring_remain_undefined_and_unchanged() {
    for kind in [MqPropertyType::Null, MqPropertyType::ByteString] {
        let mut v = group(0, 8);
        let original = v.clone();
        plan(&v, 0, &inquiry(kind, b"abc", 3))
            .commit(&mut v, ENCODING)
            .unwrap();
        assert_eq!(&v[24..32], &original[24..32]);
        assert_eq!(&v[52..60], &original[52..60]);
    }
}

#[test]
fn failed_short_name_preserves_copied_prefix_and_complete_vslength() {
    let mut v = group(0, 2);
    capture(&v, 0)
        .prepare_inquiry(
            &inquiry(MqPropertyType::Null, b"ab", 5),
            status(MqCompletion::Failed, "MQRC_PROPERTY_NAME_TOO_BIG"),
            MqMqiLimits::default(),
            &[],
        )
        .unwrap()
        .commit(&mut v, ENCODING)
        .unwrap();
    assert_eq!(&v[64..66], b"ab");
    assert_eq!(&v[66..], &[0xa5; 14]);
    assert_eq!(i32::from_be_bytes(v[44..48].try_into().unwrap()), 5);
}

#[test]
fn failed_short_value_still_writes_complete_impo_name() {
    let mut v = group(0, 8);
    let mut o = inquiry(MqPropertyType::ByteString, b"abc", 3);
    if let MqPropertyObservation::Inquired(value) = &mut o {
        value.data_length = 5;
        value.copied_value = vec![0, 255];
    }
    capture(&v, 0)
        .prepare_inquiry(
            &o,
            status(MqCompletion::Failed, "MQRC_PROPERTY_VALUE_TOO_BIG"),
            MqMqiLimits::default(),
            &[],
        )
        .unwrap()
        .commit(&mut v, ENCODING)
        .unwrap();
    assert_eq!(&v[64..67], b"abc");
    assert_eq!(i32::from_be_bytes(v[44..48].try_into().unwrap()), 3);
}

#[test]
fn unavailable_has_no_defined_writes_even_when_no_name_buffer_is_supplied() {
    let mut v = group(0, 0);
    let original = v.clone();
    capture(&v, 0)
        .prepare_inquiry(
            &MqPropertyObservation::Absent,
            status(MqCompletion::Failed, "MQRC_PROPERTY_NOT_AVAILABLE"),
            MqMqiLimits::default(),
            &[],
        )
        .unwrap()
        .commit(&mut v, ENCODING)
        .unwrap();
    assert_eq!(v, original);
}

#[test]
fn unsupported_output_wrong_call_and_mismatched_status_do_not_prepare() {
    let v = group(0, 8);
    let wrong_call =
        MqReviewedStatus::from_symbols(MqMqiCall::SetProperty, "MQCC_OK", "MQRC_NONE").unwrap();
    for (o, s) in [
        (inquiry(MqPropertyType::Null, b"abc", 3), wrong_call),
        (
            inquiry(MqPropertyType::Null, b"ab", 3),
            status(MqCompletion::Ok, "MQRC_NONE"),
        ),
        (
            inquiry(MqPropertyType::Boolean, b"abc", 3),
            status(MqCompletion::Ok, "MQRC_NONE"),
        ),
        (
            inquiry(MqPropertyType::Null, b"abc", 3),
            status(MqCompletion::Warning, "MQRC_PROP_TYPE_NOT_SUPPORTED"),
        ),
        (
            MqPropertyObservation::PropertyDeleted,
            status(MqCompletion::Ok, "MQRC_NONE"),
        ),
    ] {
        assert!(matches!(
            capture(&v, 0).prepare_inquiry(&o, s, MqMqiLimits::default(), &[]),
            Err(MqRawPropertyProblem::OutputPending)
        ));
    }
    let mut malformed = inquiry(MqPropertyType::Null, b"abc", 3);
    if let MqPropertyObservation::Inquired(value) = &mut malformed {
        value.name_length = -1;
    }
    assert!(matches!(
        capture(&v, 0).prepare_inquiry(
            &malformed,
            status(MqCompletion::Ok, "MQRC_NONE"),
            MqMqiLimits::default(),
            &[]
        ),
        Err(MqRawPropertyProblem::OutputPending)
    ));
    assert_eq!(v, group(0, 8));
}

#[test]
fn exact_prefix_count_and_protected_scalar_ranges_are_checked_before_write() {
    let v = group(0, 2);
    assert!(matches!(
        capture(&v, 0).prepare_inquiry(
            &inquiry(MqPropertyType::Null, b"abc", 3),
            status(MqCompletion::Ok, "MQRC_NONE"),
            MqMqiLimits::default(),
            &[]
        ),
        Err(MqRawPropertyProblem::BufferCapacity)
    ));
    let v = group(0, 8);
    assert!(matches!(
        capture(&v, 0).prepare_inquiry(
            &inquiry(MqPropertyType::Null, b"abc", 3),
            status(MqCompletion::Ok, "MQRC_NONE"),
            MqMqiLimits::default(),
            &[20..24]
        ),
        Err(MqRawPropertyProblem::Overlap)
    ));
}

#[test]
fn stale_prefix_suffix_capacity_and_profile_fail_without_partial_write() {
    for location in [0, 8, 28, 32, 52, 64, 79] {
        let original = group(0, 8);
        let p = plan(&original, 0, &inquiry(MqPropertyType::Null, b"abc", 3));
        let mut v = original;
        v[location] ^= 1;
        let before = v.clone();
        assert_eq!(
            p.commit(&mut v, ENCODING),
            Err(MqRawPropertyProblem::StaleCapture)
        );
        assert_eq!(v, before);
    }
    let mut v = group(0, 8);
    let p = plan(&v, 0, &inquiry(MqPropertyType::Null, b"abc", 3));
    v.push(0);
    let before = v.clone();
    assert_eq!(
        p.commit(&mut v, ENCODING),
        Err(MqRawPropertyProblem::StaleCapture)
    );
    assert_eq!(v, before);
    let p = plan(&before, 0, &inquiry(MqPropertyType::Null, b"abc", 3));
    assert_eq!(
        p.commit(
            &mut v,
            MqRawStructureEncoding {
                numbers: MqRawNumberEncoding::ReversedLittleEndian,
                ..ENCODING
            }
        ),
        Err(MqRawPropertyProblem::StaleCapture)
    );
    assert_eq!(v, before);
}

#[test]
fn batch_late_stale_and_overlap_reject_all_touched_writes() {
    let mut v = group(0, 8);
    let original = v.clone();
    let a = plan(&v, 0, &inquiry(MqPropertyType::Null, b"abc", 3));
    let b = plan(&v, 0, &inquiry(MqPropertyType::Null, b"abc", 3));
    assert_eq!(
        MqRawPropertyWriteback::commit_batch(&[a, b], &mut v, ENCODING),
        Err(MqRawPropertyProblem::Overlap)
    );
    assert_eq!(v, original);
    let a = plan(&v, 0, &inquiry(MqPropertyType::Null, b"abc", 3));
    let mut stale = v.clone();
    stale[79] ^= 1;
    let b = plan(&stale, 0, &inquiry(MqPropertyType::Null, b"abc", 3));
    assert_eq!(
        MqRawPropertyWriteback::commit_batch(&[a, b], &mut v, ENCODING),
        Err(MqRawPropertyProblem::StaleCapture)
    );
    assert_eq!(v, original);
    assert_eq!(
        MqRawPropertyWriteback::commit_batch(&[], &mut v, ENCODING),
        Err(MqRawPropertyProblem::BatchCapacity)
    );
}

#[test]
fn disjoint_impo_prefixes_commit_as_one_bounded_batch() {
    let mut v = group(0, 8);
    v.resize(160, 0);
    v[80..160].copy_from_slice(&group(0, 8));
    let mut expected = v.clone();
    for start in [0, 80] {
        for (offset, value) in [(20, 785_i32), (44, 3), (48, 1208)] {
            expected[start + offset..start + offset + 4].copy_from_slice(&value.to_be_bytes());
        }
        expected[start + 64..start + 67].copy_from_slice(b"abc");
    }
    let plans = [
        plan(&v, 0, &inquiry(MqPropertyType::Null, b"abc", 3)),
        plan(&v, 80, &inquiry(MqPropertyType::Null, b"abc", 3)),
    ];
    MqRawPropertyWriteback::commit_batch(&plans, &mut v, ENCODING).unwrap();
    assert_eq!(v, expected);
}

#[test]
fn scalar_pic_range_is_distinct_from_lossless_raw_capture() {
    let v = group(0, 8);
    let c = capture(&v, 0);
    assert_eq!(c.long("RequestedEncoding"), Ok(i32::MIN));
    for value in [i32::MIN, -1_000_000_000, 1_000_000_000, i32::MAX] {
        assert!(matches!(
            c.long_patch("ReturnedEncoding", value, &mut vec![]),
            Err(MqRawPropertyProblem::CobolLongRange)
        ));
    }
    for value in [-999_999_999, 0, 999_999_999] {
        c.long_patch("ReturnedEncoding", value, &mut vec![])
            .unwrap();
    }
    for name in [
        "Options",
        "RequestedEncoding",
        "ReturnedName.VSOffset",
        "ReturnedName.VSBufSize",
        "TypeString",
        "Reserved1",
    ] {
        assert_eq!(
            c.long_patch(name, 0, &mut vec![]),
            Err(MqRawPropertyProblem::OutputPending)
        );
    }
}

#[test]
fn cross_plan_name_output_cannot_overwrite_other_input_prefix() {
    let mut v = group(0, 110);
    v.resize(240, 0);
    v[80..160].copy_from_slice(&group(0, 8));
    let original = v.clone();
    let a = plan(&v, 0, &inquiry(MqPropertyType::Null, &[b'x'; 100], 100));
    let b = plan(&v, 80, &inquiry(MqPropertyType::Null, b"abc", 3));
    assert_eq!(
        MqRawPropertyWriteback::commit_batch(&[a, b], &mut v, ENCODING),
        Err(MqRawPropertyProblem::Overlap)
    );
    assert_eq!(v, original);
}

#[test]
fn finite_batch_limit_rejects_even_no_write_plans() {
    let mut v = group(0, 0);
    let original = v.clone();
    let plans = (0..9)
        .map(|_| {
            capture(&v, 0)
                .prepare_inquiry(
                    &MqPropertyObservation::Absent,
                    status(MqCompletion::Failed, "MQRC_PROPERTY_NOT_AVAILABLE"),
                    MqMqiLimits::default(),
                    &[],
                )
                .unwrap()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        MqRawPropertyWriteback::commit_batch(&plans, &mut v, ENCODING),
        Err(MqRawPropertyProblem::BatchCapacity)
    );
    assert_eq!(v, original);
}
