use mainframe_env_host_api::{ImsSsaCommand, ImsSsaLimits, ImsSsaProblem, parse_ims_ssa};

fn fields(segment: &str, field: &str) -> Option<usize> {
    (segment == "ROOT" && field == "KEY").then_some(2)
}

#[test]
fn literal_null_slots_preserve_command_format_without_active_behavior() {
    for raw in [b"ROOT    *- ".as_slice(), b"ROOT    *--- "] {
        let parsed = parse_ims_ssa(raw, ImsSsaLimits::default(), &fields).unwrap();
        assert!(parsed.command_format);
        assert!(parsed.commands.is_empty());
        assert!(parsed.predicates.is_empty());
    }
    let parsed = parse_ims_ssa(
        b"ROOT    *-D--P-(KEY     EQA1)",
        ImsSsaLimits::default(),
        &fields,
    )
    .unwrap();
    assert_eq!(
        parsed.commands,
        [
            ImsSsaCommand {
                code: b'D',
                subset_pointer: None
            },
            ImsSsaCommand {
                code: b'P',
                subset_pointer: None
            },
        ]
    );
    assert_eq!(parsed.predicates[0].value, b"A1");
}

#[test]
fn null_slots_do_not_bypass_bounds_or_accept_subset_digits() {
    let limits = ImsSsaLimits {
        max_command_codes: 2,
        ..ImsSsaLimits::default()
    };
    assert!(parse_ims_ssa(b"ROOT    *-- ", limits, &fields).is_ok());
    for raw in [b"ROOT    *--- ".as_slice(), b"ROOT    *-D- "] {
        assert_eq!(
            parse_ims_ssa(raw, limits, &fields),
            Err(ImsSsaProblem::ResourceExhausted)
        );
    }
    for raw in [b"ROOT    *-1 ".as_slice(), b"ROOT    *D-1 "] {
        assert_eq!(
            parse_ims_ssa(raw, ImsSsaLimits::default(), &fields),
            Err(ImsSsaProblem::InvalidSubsetPointer)
        );
    }
    assert_eq!(
        parse_ims_ssa(b"ROOT    *-", ImsSsaLimits::default(), &fields),
        Err(ImsSsaProblem::MissingCommandTerminator)
    );
    assert_eq!(
        parse_ims_ssa(b"ROOT    *-B ", ImsSsaLimits::default(), &fields),
        Err(ImsSsaProblem::InvalidCommandCode)
    );
}
