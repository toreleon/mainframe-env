use super::*;
use std::error::Error;

#[test]
fn every_ascii_value_is_an_exact_identity_including_all_controls_and_del() {
    let expected: Vec<u8> = (0..=127).collect();
    let input: String = expected.iter().copied().map(char::from).collect();
    assert_eq!(encode_ascii(&input, 128).unwrap(), expected);
    for byte in 0_u8..=127 {
        assert_eq!(
            encode_ascii(&char::from(byte).to_string(), 1).unwrap(),
            [byte]
        );
    }
}

#[test]
fn empty_input_and_zero_bound_reserve_no_output() {
    for bound in [0, 1, usize::MAX] {
        let output = encode_ascii("", bound).unwrap();
        assert!(output.is_empty());
        assert_eq!(output.capacity(), 0);
    }
    assert_eq!(
        encode_ascii("\0", 0),
        Err(AsciiEncodingProblem::OutputLimit)
    );
}

#[test]
fn ordinary_strings_spaces_and_controls_are_owned_without_replacement() {
    for (input, expected) in [
        ("HELLO 1234, WORLD!", &b"HELLO 1234, WORLD!"[..]),
        ("  A  ", &b"  A  "[..]),
        ("\0\t\n\r\x1b\x7f", &[0, 9, 10, 13, 27, 127][..]),
    ] {
        assert_eq!(encode_ascii(input, expected.len()).unwrap(), expected);
        assert_eq!(encode_ascii(input, usize::MAX).unwrap(), expected);
    }
    let mut input = String::from("owned");
    let output = encode_ascii(&input, 5).unwrap();
    input.clear();
    assert_eq!(output, b"owned");
}

#[test]
fn exact_bound_succeeds_but_short_bound_rejects_the_whole_output() {
    assert_eq!(encode_ascii("ABCD", 4).unwrap(), b"ABCD");
    for bound in 0..4 {
        assert_eq!(
            encode_ascii("ABCD", bound),
            Err(AsciiEncodingProblem::OutputLimit)
        );
    }
    // A large caller bound does not become the allocation request.
    assert_eq!(encode_ascii("ABCD", usize::MAX).unwrap(), b"ABCD");
}

#[test]
fn invalid_or_over_limit_input_never_reaches_reservation() {
    fn unexpected_reservation(_: &mut Vec<u8>, _: usize) -> Result<(), AsciiEncodingProblem> {
        panic!("invalid input reached output reservation")
    }
    assert_eq!(
        encode_with_reservation("AB", 1, unexpected_reservation),
        Err(AsciiEncodingProblem::OutputLimit)
    );
    assert_eq!(
        encode_with_reservation("é", 0, unexpected_reservation),
        Err(AsciiEncodingProblem::NonAscii)
    );
    assert_eq!(
        encode_with_reservation("AéZ", 1, unexpected_reservation),
        Err(AsciiEncodingProblem::NonAscii)
    );
}

#[test]
fn unicode_and_mixed_input_are_never_transliterated_or_truncated() {
    for input in ["é", "€", "😀", "AéZ", "e\u{301}", "ASCII\0非ASCII"] {
        assert_eq!(
            encode_ascii(input, usize::MAX),
            Err(AsciiEncodingProblem::NonAscii)
        );
        // Non-ASCII wins even when an ASCII prefix fits or the bound is zero.
        for bound in [0, 1, input.len().saturating_sub(1)] {
            assert_eq!(
                encode_ascii(input, bound),
                Err(AsciiEncodingProblem::NonAscii)
            );
        }
    }
}

#[test]
fn errors_have_distinct_display_and_standard_error_identity() {
    for (problem, message) in [
        (
            AsciiEncodingProblem::NonAscii,
            "input contains a non-ASCII character",
        ),
        (
            AsciiEncodingProblem::OutputLimit,
            "ASCII output exceeds the caller's byte limit",
        ),
        (
            AsciiEncodingProblem::Allocation,
            "ASCII output capacity could not be reserved",
        ),
    ] {
        assert_eq!(problem.to_string(), message);
        let error: &dyn Error = &problem;
        assert!(error.source().is_none());
        assert_eq!(error.downcast_ref::<AsciiEncodingProblem>(), Some(&problem));
    }
    assert_ne!(
        AsciiEncodingProblem::Allocation,
        AsciiEncodingProblem::OutputLimit
    );
    assert_ne!(
        AsciiEncodingProblem::NonAscii,
        AsciiEncodingProblem::OutputLimit
    );
}

#[test]
fn actual_fallible_reserve_maps_capacity_overflow_without_mutating_output() {
    let mut output = vec![7, 8];
    let before = (output.clone(), output.capacity());
    // len + usize::MAX cannot be a Vec capacity: try_reserve_exact rejects
    // capacity overflow before any allocation. This is not physical-OOM proof.
    assert_eq!(
        reserve_output(&mut output, usize::MAX),
        Err(AsciiEncodingProblem::Allocation)
    );
    assert_eq!((output.clone(), output.capacity()), before);
}
