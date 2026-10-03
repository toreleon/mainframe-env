use super::*;

#[test]
fn structure_encoding_delegates_exact_ascii_with_fixed_name_bound() {
    let source = Source(invocation(0));
    assert_eq!(
        source.encode_structure("Q\0 ", MqMdCharacterEncoding::AsciiCompatible),
        Ok(vec![0x51, 0x00, 0x20])
    );
    assert_eq!(
        source.encode_structure("", MqMdCharacterEncoding::AsciiCompatible),
        Ok(vec![])
    );
    assert_eq!(
        source.encode_structure(&"Q".repeat(48), MqMdCharacterEncoding::AsciiCompatible),
        Ok(vec![0x51; 48])
    );
    for text in ["é", &"Q".repeat(49)] {
        assert_eq!(
            source.encode_structure(text, MqMdCharacterEncoding::AsciiCompatible),
            Err(HostProblem::Unsupported)
        );
    }
    assert_eq!(
        source.encode_structure("Q", MqMdCharacterEncoding::OwnedCp037),
        Err(HostProblem::Unsupported)
    );
}

#[test]
fn foundation_allocation_is_resource_failure_not_unsupported_input() {
    // Error mapping only; this does not claim a physical OOM experiment.
    assert_eq!(
        ascii_problem(AsciiEncodingProblem::Allocation),
        HostProblem::ResourceExhausted
    );
    assert_eq!(
        ascii_problem(AsciiEncodingProblem::NonAscii),
        HostProblem::Unsupported
    );
    assert_eq!(
        ascii_problem(AsciiEncodingProblem::OutputLimit),
        HostProblem::Unsupported
    );
}
