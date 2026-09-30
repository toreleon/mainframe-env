use super::tests::edited_test_layout;
use super::{Decimal, decimal_exceeds_picture, encode_edited};

#[test]
fn edited_arithmetic_capacity_uses_aligned_absolute_digits() {
    for (picture, digits, scale, coefficient, overflow) in [
        ("ZZZ9", 4, 0, 12345, true),
        ("Z,ZZ9", 4, 0, 12345, true),
        ("$$$9", 3, 0, 12345, true),
        ("++++9", 4, 0, 12345, true),
        ("----9", 4, 0, -12345, true),
        ("ZZ9.99", 5, 2, 123456, true),
        ("$$B$$9.9", 5, 1, 123456, true),
        ("ZZZ9", 4, 0, 1234, false),
        ("ZZZ9", 4, 0, -1234, false),
    ] {
        let layout = edited_test_layout(picture, digits, scale, false);
        assert_eq!(
            decimal_exceeds_picture(&layout, Decimal { coefficient, scale }),
            overflow,
            "{picture}: {coefficient}"
        );
    }
}

#[test]
fn embedded_simple_insertions_edit_floating_strings() {
    // GnuCOBOL 3.2.0 with -std=ibm is an unlicensed reference.
    for (picture, digits, scale, coefficient, expected) in [
        ("$$B$$9.9", 5, 1, 123450, b"$2 345.0".as_slice()),
        ("$$0$$9", 4, 0, 12345, b"$20345".as_slice()),
        ("$$/$$9", 4, 0, 12345, b"$2/345".as_slice()),
        ("++B++9", 4, 0, 12345, b"+2 345".as_slice()),
        ("--,--9.99", 6, 2, -1234500, b"-2,345.00".as_slice()),
        ("$$$B99", 4, 0, 12345, b"$23 45".as_slice()),
    ] {
        let layout = edited_test_layout(picture, digits, scale, false);
        assert_eq!(
            encode_edited(&layout, Decimal { coefficient, scale }),
            Ok(expected.to_vec()),
            "{picture}"
        );
    }
}

#[test]
fn embedded_decimal_keeps_floating_sign_to_its_left() {
    // GnuCOBOL 3.2.0 with -std=ibm is an unlicensed reference.
    let layout = edited_test_layout("+++.+++", 5, 3, false);
    assert_eq!(
        encode_edited(
            &layout,
            Decimal {
                coefficient: 120,
                scale: 3
            }
        ),
        Ok(b"  +.120".to_vec())
    );
}
