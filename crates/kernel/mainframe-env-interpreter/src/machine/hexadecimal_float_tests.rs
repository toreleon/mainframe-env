use super::tests::edited_test_layout;
use super::{Decimal, LayoutCategory, LayoutMetadata, decode_decimal, encode_decimal};

fn float_layout(category: LayoutCategory, length: usize) -> LayoutMetadata {
    let mut layout = edited_test_layout("", 0, 0, false);
    layout.category = category;
    layout.length = length;
    layout.element_length = length;
    layout
}

// Hand-derived from the format in #160: value = (-1)^sign * .fraction * 16^(E-64).
// 1 = .1 hexadecimal * 16^1, so E = 65 (0x41), fraction = 0x100000 / 0x10000000000000.
// -118.625 = -0x76.a = -.76a * 16^2, so sign|E = 0xc2, fraction = 0x76a000 /
// 0x76a00000000000. Extending the exact fraction with zeroes gives COMP-2 bytes.
const ONE_SHORT: [u8; 4] = [0x41, 0x10, 0x00, 0x00];
const ONE_LONG: [u8; 8] = [0x41, 0x10, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
const NEGATIVE_SHORT: [u8; 4] = [0xc2, 0x76, 0xa0, 0x00];
const NEGATIVE_LONG: [u8; 8] = [0xc2, 0x76, 0xa0, 0x00, 0x00, 0x00, 0x00, 0x00];

fn assert_hfp_encoding(category: LayoutCategory, one: &[u8], negative: &[u8]) {
    let layout = float_layout(category, one.len());
    for (coefficient, scale, expected) in [(1, 0, one), (-118625, 3, negative)] {
        assert_eq!(
            encode_decimal(&layout, Decimal { coefficient, scale }),
            Ok(expected.to_vec()),
            "z/OS COMP storage must use hexadecimal floating point"
        );
    }
}

fn assert_hfp_decoding(category: LayoutCategory, one: &[u8], negative: &[u8]) {
    let layout = float_layout(category, one.len());
    for (coefficient, scale, bytes) in [(1, 0, one), (-118625, 3, negative)] {
        assert_eq!(
            decode_decimal(&layout, bytes),
            Ok(Decimal { coefficient, scale }),
            "z/OS COMP storage must decode hexadecimal floating point"
        );
    }
}

#[test]
fn comp1_encodes_hexadecimal_floating_storage() {
    assert_hfp_encoding(LayoutCategory::FloatShort, &ONE_SHORT, &NEGATIVE_SHORT);
}

#[test]
fn comp2_encodes_hexadecimal_floating_storage() {
    assert_hfp_encoding(LayoutCategory::FloatLong, &ONE_LONG, &NEGATIVE_LONG);
}

#[test]
fn comp1_decodes_hexadecimal_floating_storage() {
    assert_hfp_decoding(LayoutCategory::FloatShort, &ONE_SHORT, &NEGATIVE_SHORT);
}

#[test]
fn comp2_decodes_hexadecimal_floating_storage() {
    assert_hfp_decoding(LayoutCategory::FloatLong, &ONE_LONG, &NEGATIVE_LONG);
}

use super::float_codec::{FloatProfile, Wide, decode_hex, encode_hex};
use super::{
    BoundedPayload, CodecLimits, InvocationLimits, Machine, MachineProblem, ReferenceMachine,
    StorageView,
};

fn wide(text: &str) -> Wide {
    text.parse().unwrap()
}

#[test]
fn hex_float_zero_fractions_sign_and_hexadecade_boundaries() {
    // .8 * 16^0 = .5; .1 * 16^0 = 1/16; .1 * 16^2 = 16.
    for (coefficient, scale, short) in [
        (0, 0, [0, 0, 0, 0]),
        (5, 1, [0x40, 0x80, 0, 0]),
        (-5, 1, [0xc0, 0x80, 0, 0]),
        (625, 4, [0x40, 0x10, 0, 0]),
        (16, 0, [0x42, 0x10, 0, 0]),
        (256, 0, [0x43, 0x10, 0, 0]),
    ] {
        for (category, width) in [
            (LayoutCategory::FloatShort, 4),
            (LayoutCategory::FloatLong, 8),
        ] {
            let mut expected = short.to_vec();
            expected.resize(width, 0);
            let layout = float_layout(category, width);
            let value = Decimal { coefficient, scale };
            assert_eq!(encode_decimal(&layout, value), Ok(expected.clone()));
            assert_eq!(decode_decimal(&layout, &expected), Ok(value));
        }
    }
    assert_eq!(encode_hex(4, wide("-0")), Ok(vec![0; 4]));
    assert_eq!(decode_hex(&[0xff, 0, 0, 0]), Ok(wide("0")));
    // Leading hexadecimal digit changes across 1: .ffffff * 16^0 vs .1 * 16^1.
    assert_eq!(
        encode_hex(4, wide("0.999999940395355224609375")),
        Ok(vec![0x40, 0xff, 0xff, 0xff])
    );
    assert_eq!(
        decode_hex(&[0x40, 0xff, 0xff, 0xff]),
        Ok(wide("0.999999940395355224609375"))
    );
    // Unnormalized raw aliases are legal HFP values: .01 * 16^2 = 1.
    assert_eq!(decode_hex(&[0x42, 0x01, 0, 0]), Ok(wide("1")));
}

#[test]
fn hex_float_decimal_fraction_truncates_without_binary64_double_rounding() {
    // UNVERIFIED ASSUMPTION (#160), pending licensed differential: truncation
    // toward zero. .1 decimal = .199999... hexadecimal * 16^0. Keep 6/14
    // hexadecimal digits, dropping the next 9. Negative uses the same fraction.
    for (category, expected) in [
        (LayoutCategory::FloatShort, vec![0x40, 0x19, 0x99, 0x99]),
        (
            LayoutCategory::FloatLong,
            vec![0x40, 0x19, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99],
        ),
    ] {
        let layout = float_layout(category, expected.len());
        assert_eq!(
            encode_decimal(
                &layout,
                Decimal {
                    coefficient: 1,
                    scale: 1
                }
            ),
            Ok(expected.clone())
        );
        let mut negative = expected.clone();
        negative[0] |= 0x80;
        assert_eq!(
            encode_decimal(
                &layout,
                Decimal {
                    coefficient: -1,
                    scale: 1
                }
            ),
            Ok(negative)
        );
        let decoded = decode_decimal(&layout, &expected).unwrap();
        assert_eq!(encode_decimal(&layout, decoded), Ok(expected));
    }
    // COMP-2 retains all 56 fraction bits: .10000000000001 * 16^1
    // = 1 + 2^-52, and .123456789abcdf * 16^1 exercises bits beyond f64.
    for bytes in [
        [0x41, 0x10, 0, 0, 0, 0, 0, 1],
        [0x41, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xdf],
    ] {
        let layout = float_layout(LayoutCategory::FloatLong, 8);
        assert_eq!(
            encode_decimal(&layout, decode_decimal(&layout, &bytes).unwrap()),
            Ok(bytes.to_vec())
        );
    }
}

#[test]
fn hex_float_full_storage_range_overflow_and_underflow() {
    // UNVERIFIED ASSUMPTIONS (#160), pending licensed differential: overflow
    // above the greatest finite magnitude -> SizeError; underflow -> true zero.
    // Min = .1 * 16^-64 = 16^-65 (E=0); max = .fff... * 16^63 (E=127).
    let mut context = dec::Context::<Wide>::default();
    let sixteen = Wide::from(16);
    let mut minimum = Wide::from(1);
    for _ in 0..65 {
        context.div(&mut minimum, &sixteen);
    }
    for width in [4, 8] {
        let mut min_bytes = vec![0; width];
        min_bytes[1] = 0x10;
        assert_eq!(encode_hex(width, minimum), Ok(min_bytes.clone()));
        assert_eq!(decode_hex(&min_bytes), Ok(minimum));
        let mut smaller = minimum;
        context.div(&mut smaller, &sixteen);
        assert_eq!(encode_hex(width, smaller), Ok(vec![0; width]));
        let mut negative_smaller = smaller;
        context.minus(&mut negative_smaller);
        assert_eq!(encode_hex(width, negative_smaller), Ok(vec![0; width]));
        let mut max_bytes = vec![0xff; width];
        max_bytes[0] = 0x7f;
        // Hand-derived maximum = (2^fraction_bits - 1) * 2^(252-fraction_bits).
        let fraction_bits = (width - 1) * 8;
        let mut maximum = Wide::from((1u64 << fraction_bits) - 1);
        for _ in 0..252 - fraction_bits {
            context.mul(&mut maximum, &Wide::from(2));
        }
        assert_eq!(decode_hex(&max_bytes), Ok(maximum));
        assert_eq!(encode_hex(width, maximum), Ok(max_bytes.clone()));
        let mut over = maximum;
        context.add(&mut over, &Wide::from(1));
        assert_eq!(encode_hex(width, over), Err(MachineProblem::SizeError));
        context.minus(&mut over);
        assert_eq!(encode_hex(width, over), Err(MachineProblem::SizeError));
        max_bytes[0] |= 0x80;
        let mut negative_maximum = maximum;
        context.minus(&mut negative_maximum);
        assert_eq!(encode_hex(width, negative_maximum), Ok(max_bytes));
        assert_eq!(
            encode_hex(width, wide("1E76")),
            Err(MachineProblem::SizeError)
        );
        assert_eq!(encode_hex(width, wide("1E-79")), Ok(vec![0; width]));
    }
    // Every normalized power-of-16 boundary: 16^k = .1 * 16^(k+1),
    // therefore stored E=k+65 and fraction begins with hexadecimal 1.
    let mut boundary = minimum;
    for k in -65..=62 {
        for width in [4, 8] {
            let mut expected = vec![0; width];
            expected[0] = (k + 65) as u8;
            expected[1] = 0x10;
            assert_eq!(encode_hex(width, boundary), Ok(expected.clone()));
            assert_eq!(decode_hex(&expected), Ok(boundary));
        }
        context.mul(&mut boundary, &sixteen);
    }
    assert_eq!(encode_hex(4, boundary), Err(MachineProblem::SizeError));
    assert_eq!(decode_hex(&[0; 3]), Err(MachineProblem::DataException));
}

#[test]
fn explicitly_named_ieee_profile_retains_legacy_encoding() {
    for (category, width, expected) in [
        (LayoutCategory::FloatShort, 4, vec![0x3f, 0x80, 0, 0]),
        (
            LayoutCategory::FloatLong,
            8,
            vec![0x3f, 0xf0, 0, 0, 0, 0, 0, 0],
        ),
    ] {
        let mut layout = float_layout(category, width);
        layout.float_profile = FloatProfile::Ieee;
        let one = Decimal {
            coefficient: 1,
            scale: 0,
        };
        assert_eq!(encode_decimal(&layout, one), Ok(expected.clone()));
        assert_eq!(decode_decimal(&layout, &expected), Ok(one));
    }
}

fn checkpoint_machine() -> ReferenceMachine {
    let mut machine = ReferenceMachine::from_binary(
        &super::tests::binary(),
        super::tests::invocation(),
        CodecLimits::default(),
    )
    .unwrap();
    let mut layout = float_layout(LayoutCategory::FloatShort, 4);
    layout.name = "FLOAT-X".into();
    layout.simple_name = "FLOAT-X".into();
    machine.layouts.insert("FLOAT-X".into(), layout);
    let base = machine.bases.len();
    machine.bases.push(ONE_SHORT.to_vec());
    machine.static_base_count += 1;
    machine.views.insert(
        "FLOAT-X".into(),
        StorageView {
            base,
            offset: 0,
            length: 4,
        },
    );
    machine
}

#[test]
fn float_checkpoints_preserve_hex_profiles_and_restore_legacy_bytes_as_ieee() {
    let machine = checkpoint_machine();
    let checkpoint = machine.checkpoint().unwrap();
    assert_eq!(
        checkpoint.schema(),
        "mainframe-env.reference-machine-checkpoint@13"
    );
    let mut restored = checkpoint_machine();
    restored.restore_checkpoint(&checkpoint).unwrap();
    assert_eq!(restored.layouts["FLOAT-X"].float_profile, FloatProfile::Hex);
    assert_eq!(
        decode_decimal(
            &restored.layouts["FLOAT-X"],
            &restored.read("FLOAT-X").unwrap()
        ),
        Ok(Decimal {
            coefficient: 1,
            scale: 0
        })
    );
    // Append-only migration: schema 12 omits the profile trailer and its bytes
    // are IEEE. Restoring it onto a new HEX layout selects IEEE for that layout.
    let mut snapshot = machine.snapshot();
    snapshot.schema_version = 12;
    snapshot.float_profiles.clear();
    *snapshot.base_storage.last_mut().unwrap() = vec![0x3f, 0x80, 0, 0];
    let bytes = super::snapshot_codec::encode_snapshot(&snapshot).unwrap();
    let legacy = BoundedPayload::new(
        "mainframe-env.reference-machine-checkpoint@12",
        bytes,
        InvocationLimits::default(),
    )
    .unwrap();
    restored.restore_checkpoint(&legacy).unwrap();
    assert_eq!(
        restored.layouts["FLOAT-X"].float_profile,
        FloatProfile::Ieee
    );
    assert_eq!(
        decode_decimal(
            &restored.layouts["FLOAT-X"],
            &restored.read("FLOAT-X").unwrap()
        ),
        Ok(Decimal {
            coefficient: 1,
            scale: 0
        })
    );
    let migrated = restored.checkpoint().unwrap();
    let mut reopened = checkpoint_machine();
    reopened.restore_checkpoint(&migrated).unwrap();
    assert_eq!(
        reopened.layouts["FLOAT-X"].float_profile,
        FloatProfile::Ieee
    );
    assert_eq!(
        decode_decimal(
            &reopened.layouts["FLOAT-X"],
            &reopened.read("FLOAT-X").unwrap()
        ),
        Ok(Decimal {
            coefficient: 1,
            scale: 0
        })
    );
    let mut invalid = reopened.snapshot();
    invalid
        .float_profiles
        .insert("FLOAT-X".into(), "bogus".into());
    assert_eq!(
        reopened.restore(invalid),
        Err(MachineProblem::IncompatibleSnapshot)
    );
}

#[test]
fn hex_float_ieee_ingress_truncation_and_decimal_range_adapter() {
    // UNVERIFIED ASSUMPTION (#160), pending licensed differential: truncate
    // the exact IEEE ingress fraction. Binary64 .1 is .1999999999999a hex;
    // decimal .1 above instead truncates to .19999999999999 hex.
    assert_eq!(
        encode_hex(
            8,
            wide("0.1000000000000000055511151231257827021181583404541015625")
        ),
        Ok(vec![0x40, 0x19, 0x99, 0x99, 0x99, 0x99, 0x99, 0x9a])
    );
    assert_eq!(
        encode_hex(
            4,
            wide("0.1000000000000000055511151231257827021181583404541015625")
        ),
        Ok(vec![0x40, 0x19, 0x99, 0x99])
    );
    let short = float_layout(LayoutCategory::FloatShort, 4);
    let minimum = [0, 0x10, 0, 0];
    let value = decode_decimal(&short, &minimum).unwrap();
    assert_eq!(encode_decimal(&short, value), Ok(minimum.to_vec()));
    assert_eq!(
        encode_decimal(
            &short,
            Decimal {
                coefficient: 1,
                scale: 118
            }
        ),
        Ok(vec![0; 4])
    );
    // Storage range does not enlarge the existing 34-digit decimal integer range.
    assert_eq!(
        decode_decimal(&short, &[0x7f, 0xff, 0xff, 0xff]),
        Err(MachineProblem::SizeError)
    );
}

#[test]
fn artifact_float_profile_is_append_only_and_absence_selects_ieee() {
    let module = super::decode_binary(&super::tests::binary(), CodecLimits::default()).unwrap();
    let mut definition = module
        .regions()
        .iter()
        .flat_map(|region| &region.blocks)
        .flat_map(|block| &block.operations)
        .next()
        .unwrap()
        .clone();
    definition.identity = mainframe_env_ir::cobol_layout_definition_identity();
    definition.attributes = super::BTreeMap::new();
    for (key, value) in [
        ("name", "FLOAT-X"),
        ("simple_name", "FLOAT-X"),
        ("category", "float_short"),
        ("picture", ""),
        ("parent", ""),
        ("condition_values", ""),
    ] {
        definition
            .attributes
            .insert(key.into(), super::Attribute::Text(value.into()));
    }
    for (key, value) in [
        ("digits", 0),
        ("scale", 0),
        ("signed", 0),
        ("sign_separate", 0),
        ("offset", 0),
        ("length", 4),
        ("element_length", 4),
        ("occurs", 1),
    ] {
        definition
            .attributes
            .insert(key.into(), super::Attribute::Integer(value));
    }
    for (profile, expected) in [
        (None, FloatProfile::Ieee),
        (Some(mainframe_env_ir::COBOL_FLOAT_HEX), FloatProfile::Hex),
        (Some(mainframe_env_ir::COBOL_FLOAT_IEEE), FloatProfile::Ieee),
    ] {
        definition.attributes.remove("float_profile");
        if let Some(profile) = profile {
            definition.attributes.insert(
                "float_profile".into(),
                super::Attribute::Text(profile.into()),
            );
        }
        let (layouts, _) = super::layout_metadata(std::slice::from_ref(&definition)).unwrap();
        assert_eq!(layouts.values().next().unwrap().float_profile, expected);
    }
    definition.attributes.insert(
        "float_profile".into(),
        super::Attribute::Text("unknown".into()),
    );
    assert!(matches!(
        super::layout_metadata(&[definition]),
        Err(MachineProblem::InvalidArtifact(_))
    ));
}
