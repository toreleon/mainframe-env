use super::tests::edited_test_layout;
use super::{Decimal, encode_edited};

#[test]
fn embedded_simple_insertions_edit_floating_strings() {
    // GnuCOBOL 3.2.0 with -std=ibm is an unlicensed reference.
    for (picture, digits, scale, coefficient, expected) in [
        ("$$B$$9.9", 5, 1, 123450, b"$2 345.0".as_slice()),
        ("$$0$$9", 4, 0, 12345, b"$20345".as_slice()),
        ("$$/$$9", 4, 0, 12345, b"$2/345".as_slice()),
        ("++B++9", 5, 0, 12345, b"+2 345".as_slice()),
        ("--,--9.99", 7, 2, -1234500, b"-2,345.00".as_slice()),
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
    let layout = edited_test_layout("+++.+++", 6, 3, false);
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
