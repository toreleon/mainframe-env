use super::*;
use crate::mq_md_value::tests::{array, encoding, kind, raw, value};
use crate::mq_raw_layout::{MqRawCapture, mq_raw_layout};

fn text(out: &mut Vec<u8>, s: &str) {
    out.push(1);
    out.extend_from_slice(&(s.len() as u64).to_le_bytes());
    out.extend_from_slice(s.as_bytes());
}
fn object(out: &mut Vec<u8>, s: &str, n: u64) {
    out.push(0x40);
    text(out, s);
    out.extend_from_slice(&n.to_le_bytes());
}
fn variant(out: &mut Vec<u8>, s: &str, v: &str, n: u64) {
    out.push(0x41);
    text(out, s);
    text(out, v);
    out.extend_from_slice(&n.to_le_bytes());
}
fn bytes(out: &mut Vec<u8>, name: &str, b: &[u8]) {
    text(out, name);
    out.push(2);
    out.extend_from_slice(&(b.len() as u64).to_le_bytes());
    out.extend_from_slice(b);
}
fn long(out: &mut Vec<u8>, name: &str, n: i32) {
    text(out, name);
    out.push(0x1a);
    out.extend_from_slice(&n.to_le_bytes());
}
/// Independent fixed canonical fixture; neither Encoder nor decode builds it.
fn golden(v2: bool) -> Vec<u8> {
    let mut b = b"mainframe-env.mq-md-value@1\0".to_vec();
    text(&mut b, "mainframe-env.mq-md-value@1");
    variant(
        &mut b,
        "MqMdValue",
        if v2 { "V2" } else { "V1" },
        if v2 { 3 } else { 2 },
    );
    text(&mut b, "characters");
    variant(&mut b, "MqMdCharacterEncoding", "AsciiCompatible", 0);
    if v2 {
        text(&mut b, "extension");
        object(&mut b, "MqMdV2Fields", 5);
        bytes(&mut b, "group_id", &array::<24>(0xc0));
        long(&mut b, "msg_flags", i32::MIN);
        long(&mut b, "msg_seq_number", -31);
        long(&mut b, "offset", i32::MAX);
        long(&mut b, "original_length", -1);
    }
    text(&mut b, "fields");
    object(&mut b, "MqMdFields", 23);
    bytes(&mut b, "accounting_token", &array::<32>(0x70));
    bytes(&mut b, "appl_identity_data", &array::<32>(0));
    bytes(&mut b, "appl_origin_data", &[255, 0, 64, 32]);
    long(&mut b, "backout_count", -99);
    long(&mut b, "coded_char_set_id", 1208);
    bytes(&mut b, "correl_id", &array::<24>(24));
    long(&mut b, "encoding", -7);
    long(&mut b, "expiry", -42);
    long(&mut b, "feedback", i32::MAX);
    bytes(&mut b, "format", &array::<8>(248));
    bytes(&mut b, "msg_id", &array::<24>(0));
    long(&mut b, "msg_type", 123);
    long(&mut b, "persistence", 77);
    long(&mut b, "priority", -3);
    bytes(&mut b, "put_appl_name", &array::<28>(128));
    long(&mut b, "put_appl_type", -1234);
    bytes(&mut b, "put_date", b"        ");
    bytes(&mut b, "put_time", &[0; 8]);
    bytes(&mut b, "reply_to_q", &array::<48>(224));
    bytes(&mut b, "reply_to_q_mgr", &array::<48>(32));
    long(&mut b, "report", i32::MIN);
    bytes(&mut b, "struc_id", b"MD  ");
    bytes(&mut b, "user_identifier", &array::<12>(64));
    b
}
#[test]
fn complete_codec_has_independent_fixed_preimages_digests_and_lossless_roundtrips() {
    for (v2, size, digest) in [
        (
            false,
            1126,
            "cccb7ae1988d94d21a9b5f54b866fdad622bda2bfd019cb04fdf3f06ce409c0c",
        ),
        (
            true,
            1324,
            "8b1e8981fd3a2693a8481074113c9b9572320bb9ea16874513428439de7e624f",
        ),
    ] {
        let expected = golden(v2);
        assert_eq!(expected.len(), size);
        assert_eq!(format!("{:x}", Sha256::digest(&expected)), digest);
        for cp037 in [false, true] {
            let input = value(v2, cp037);
            let actual = mq_md_value_bytes(&input, MQ_MD_VALUE_MAX_BYTES).unwrap();
            assert_eq!(
                mq_md_value_size(&input, MQ_MD_VALUE_MAX_BYTES),
                Ok(actual.len())
            );
            assert_eq!(mq_md_value_decode(&actual, actual.len()), Ok(input.clone()));
            assert_eq!(
                mq_md_value_digest(&input, MQ_MD_VALUE_MAX_BYTES).unwrap(),
                <[u8; 32]>::from(Sha256::digest(&actual))
            );
            if !cp037 {
                assert_eq!(actual, expected);
            }
        }
    }
}
#[test]
fn every_catalog_field_is_represented_and_changes_the_digest_without_silent_narrowing() {
    for v2 in [false, true] {
        let raw = raw(v2, true, false);
        let original = value(v2, false);
        let digest = mq_md_value_digest(&original, 2048).unwrap();
        for field in mq_raw_layout(kind(v2)).fields {
            if matches!(field.name, "StrucId" | "Version") {
                continue;
            }
            let mut changed = raw.clone();
            changed[field.offset + field.width - 1] ^= 1;
            let projected = MqRawCapture::capture(kind(v2), &changed, encoding(true, false))
                .unwrap()
                .to_full_md_value()
                .unwrap();
            assert_ne!(projected, original, "{}", field.name);
            assert_ne!(
                mq_md_value_digest(&projected, 2048).unwrap(),
                digest,
                "{}",
                field.name
            );
            let encoded = mq_md_value_bytes(&projected, 2048).unwrap();
            assert_eq!(mq_md_value_decode(&encoded, 2048), Ok(projected));
        }
        assert_ne!(mq_md_value_digest(&value(v2, true), 2048).unwrap(), digest);
    }
    assert_ne!(
        mq_md_value_digest(&value(false, false), 2048),
        mq_md_value_digest(&value(true, false), 2048)
    );
}
fn find(bytes: &[u8], needle: &[u8]) -> usize {
    bytes
        .windows(needle.len())
        .position(|w| w == needle)
        .unwrap()
}
#[test]
fn strict_schema_type_order_width_version_count_and_trailing_mutations_are_rejected() {
    for v2 in [false, true] {
        let good = golden(v2);
        let mut mutations = vec![];
        for needle in [
            b"mainframe-env.mq-md-value@1".as_slice(),
            b"MqMdValue",
            b"MqMdFields",
            b"characters",
            b"accounting_token",
            b"user_identifier",
        ] {
            let mut bad = good.clone();
            let offset = find(&bad, needle);
            bad[offset] ^= 1;
            mutations.push(bad);
        }
        for needle in [
            b"mainframe-env.mq-md-value@1".as_slice(),
            b"accounting_token",
        ] {
            let mut bad = good.clone();
            let length_at =
                MQ_MD_VALUE_DOMAIN.len() + find(&bad[MQ_MD_VALUE_DOMAIN.len()..], needle) - 8;
            bad[length_at..length_at + 8].copy_from_slice(&u64::MAX.to_le_bytes());
            mutations.push(bad);
        }
        let mut bad = good.clone();
        let offset = find(&bad, if v2 { b"V2" } else { b"V1" }) + 1;
        bad[offset] = b'3';
        mutations.push(bad);
        let mut bad = good.clone();
        let offset = find(&bad, b"AsciiCompatible");
        bad[offset] = b'?';
        mutations.push(bad);
        let object_at = find(&good, b"MqMdFields") + b"MqMdFields".len();
        for count in [0_u64, 22, 24, u64::MAX] {
            let mut bad = good.clone();
            bad[object_at..object_at + 8].copy_from_slice(&count.to_le_bytes());
            mutations.push(bad);
        }
        let field_at = find(&good, b"accounting_token") + b"accounting_token".len();
        for tag in [0, 1, 0x30, 0x1a] {
            let mut bad = good.clone();
            bad[field_at] = tag;
            mutations.push(bad);
        }
        for width in [0_u64, 31, 33, u64::MAX] {
            let mut bad = good.clone();
            bad[field_at + 1..field_at + 9].copy_from_slice(&width.to_le_bytes());
            mutations.push(bad);
        }
        let long_at = find(&good, b"backout_count") + b"backout_count".len();
        let mut bad = good.clone();
        bad[long_at] = 0x1b;
        mutations.push(bad);
        let mut bad = good.clone();
        bad.extend_from_slice(&[0]);
        mutations.push(bad);
        // Duplicate first field where the next required field must begin.
        let mut bad = good.clone();
        let next = field_at + 9 + 32;
        bad.splice(next..next, good[object_at + 8..next].iter().copied());
        mutations.push(bad);
        // Missing required first field, preserving the declared field count.
        let mut bad = good.clone();
        bad.drain(object_at + 8..next);
        mutations.push(bad);
        for bad in mutations {
            assert!(mq_md_value_decode(&bad, 2048).is_err());
        }
        for size in 0..good.len() {
            assert!(
                mq_md_value_decode(&good[..size], 2048).is_err(),
                "len={size}"
            );
        }
    }
}
#[test]
fn bounded_codec_rejects_oversize_narrow_limits_and_bad_structure_identity() {
    for v2 in [false, true] {
        let value = value(v2, false);
        let good = golden(v2);
        for limit in [0, 1, good.len() - 1] {
            assert_eq!(
                mq_md_value_bytes(&value, limit),
                Err(MqMdValueProblem::CanonicalLimit)
            );
            assert_eq!(
                mq_md_value_digest(&value, limit),
                Err(MqMdValueProblem::CanonicalLimit)
            );
            assert_eq!(
                mq_md_value_decode(&good, limit),
                Err(MqMdValueProblem::CanonicalLimit)
            );
        }
        assert_eq!(mq_md_value_bytes(&value, good.len()).unwrap(), good);
        assert_eq!(
            mq_md_value_decode(&vec![0; 2049], usize::MAX),
            Err(MqMdValueProblem::CanonicalLimit)
        );
        let mut corrupted = value.clone();
        match &mut corrupted {
            MqMdValue::V1 { fields, .. } | MqMdValue::V2 { fields, .. } => fields.struc_id[0] = 0,
        }
        assert_eq!(
            mq_md_value_bytes(&corrupted, 2048),
            Err(MqMdValueProblem::StructureIdentifier)
        );
    }
}
