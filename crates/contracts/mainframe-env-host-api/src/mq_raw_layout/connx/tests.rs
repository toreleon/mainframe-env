use super::*;
use crate::mq_mqi::MqMqiCall;
use crate::mq_raw_layout::*;

fn encoding(big: bool) -> MqRawStructureEncoding {
    MqRawStructureEncoding {
        numbers: if big {
            MqRawNumberEncoding::NormalBigEndian
        } else {
            MqRawNumberEncoding::ReversedLittleEndian
        },
        characters: MqRawCharacterEncoding::AsciiCompatible,
    }
}

// Independent reviewed prefix: MQCHAR4, MQLONG, MQLONG. Suffix is not a pointer.
fn fixture(options: i32, big: bool, suffix: usize) -> Vec<u8> {
    let mut bytes = vec![0xa5; 12 + suffix];
    bytes[..4].copy_from_slice(b"CNO ");
    bytes[4..8].copy_from_slice(&if big {
        1_i32.to_be_bytes()
    } else {
        1_i32.to_le_bytes()
    });
    bytes[8..12].copy_from_slice(&if big {
        options.to_be_bytes()
    } else {
        options.to_le_bytes()
    });
    bytes
}

fn capture(options: i32, big: bool, suffix: usize) -> MqRawCapture {
    MqRawCapture::capture(
        MqRawLayoutKind::Cno1,
        &fixture(options, big, suffix),
        encoding(big),
    )
    .unwrap()
}

fn context() -> MqRawWritebackContext {
    MqRawWritebackContext {
        call: MqMqiCall::ConnectExtended,
        platform: MqRawPlatform::Zos,
        single_queue: true,
        dynamic_model_open: false,
    }
}

#[test]
fn reviewed_prefix_has_exact_order_width_initials_and_no_later_slots() {
    let layout = mq_raw_layout(MqRawLayoutKind::Cno1);
    assert_eq!((layout.version, layout.prefix_bytes), (1, 12));
    assert_eq!(
        layout
            .fields
            .iter()
            .map(|f| (f.name, f.offset, f.width, f.kind))
            .collect::<Vec<_>>(),
        vec![
            ("StrucId", 0, 4, MqRawFieldKind::Characters),
            ("Version", 4, 4, MqRawFieldKind::Long),
            ("Options", 8, 4, MqRawFieldKind::Long)
        ]
    );
    assert_eq!(layout.fields[1].initial, MqRawInitialValue::Long(1));
    assert_eq!(layout.fields[2].initial, MqRawInitialValue::Long(0));
    assert_eq!(
        capture(0, true, 64).field("ClientConnOffset"),
        Err(MqRawProblem::Field)
    );
    assert_eq!(
        capture(0, true, 64).field("ClientConnPtr"),
        Err(MqRawProblem::Field)
    );
}

#[test]
fn default_and_explicit_nonshared_preserve_all_input_and_checked_manager() {
    for options in [0, 32] {
        for big in [false, true] {
            for suffix in [0, 1, 64] {
                let input = fixture(options, big, suffix);
                let c = capture(options, big, suffix);
                assert_eq!(c.prefix(), &input[..12]);
                assert_eq!(c.capacity(), input.len());
                assert_eq!(c.field("Options"), Ok(MqRawFieldValue::Long(options)));
                for manager in [None, Some(MqRouteName::new("QM1").unwrap())] {
                    let result = c
                        .decode_connx_default(
                            MqConnxProfile::OrdinaryOwnedNonshared,
                            manager.clone(),
                        )
                        .unwrap();
                    assert_eq!(result.manager, manager);
                    assert_eq!(result.sharing, MqHandleSharing::NonShared);
                    assert_eq!(result.options, MqMqiOptions::ContractDefault);
                }
                assert_eq!(c.prefix(), &input[..12]);
            }
        }
    }
}

#[test]
fn every_short_capacity_invalid_id_and_unsupported_version_reject_without_mutation() {
    let input = fixture(0, true, 32);
    for size in 0..12 {
        assert_eq!(
            MqRawCapture::capture(MqRawLayoutKind::Cno1, &input[..size], encoding(true)),
            Err(MqRawProblem::Capacity)
        );
    }
    for version in [-1_i32, 0, 2, 8, i32::MAX] {
        let mut bad = input.clone();
        bad[4..8].copy_from_slice(&version.to_be_bytes());
        let before = bad.clone();
        assert_eq!(
            MqRawCapture::capture(MqRawLayoutKind::Cno1, &bad, encoding(true)),
            Err(MqRawProblem::Version)
        );
        assert_eq!(bad, before);
    }
    for id in [*b"CNO\0", *b"cno ", *b"CNOX", [0xff; 4]] {
        let mut bad = input.clone();
        bad[..4].copy_from_slice(&id);
        let before = bad.clone();
        assert_eq!(
            MqRawCapture::capture(MqRawLayoutKind::Cno1, &bad, encoding(true)),
            Err(MqRawProblem::StructureIdentifier)
        );
        assert_eq!(bad, before);
    }
}

#[test]
fn structure_encoding_is_explicit_and_cp037_id_retains_blank_bytes() {
    let mut input = fixture(32, true, 8);
    input[..4].copy_from_slice(&[0xc3, 0xd5, 0xd6, 0x40]);
    let e = MqRawStructureEncoding {
        characters: MqRawCharacterEncoding::OwnedCp037,
        ..encoding(true)
    };
    let c = MqRawCapture::capture(MqRawLayoutKind::Cno1, &input, e).unwrap();
    assert_eq!(
        c.field("StrucId"),
        Ok(MqRawFieldValue::Characters(&[0xc3, 0xd5, 0xd6, 0x40]))
    );
    assert!(
        c.decode_connx_default(MqConnxProfile::OrdinaryOwnedNonshared, None)
            .is_ok()
    );
    assert_eq!(
        MqRawCapture::capture(MqRawLayoutKind::Cno1, &input, encoding(true)),
        Err(MqRawProblem::StructureIdentifier)
    );
    for e in [
        MqRawStructureEncoding {
            numbers: MqRawNumberEncoding::Unsupported,
            ..e
        },
        MqRawStructureEncoding {
            characters: MqRawCharacterEncoding::Unsupported,
            ..e
        },
    ] {
        assert_eq!(
            MqRawCapture::capture(MqRawLayoutKind::Cno1, &input, e),
            Err(MqRawProblem::UnsupportedEncoding)
        );
    }
    assert_eq!(
        MqRawCapture::capture(MqRawLayoutKind::Cno1, &fixture(0, true, 0), encoding(false)),
        Err(MqRawProblem::Version)
    );
}

#[test]
fn known_unrepresented_flags_and_combinations_never_become_defaults() {
    // Fixed source-reviewed expectations, independently of the generated table.
    for options in [
        1,
        2,
        4,
        8,
        16,
        64,
        128,
        256,
        512,
        1024,
        2048,
        4096,
        8192,
        16384,
        32768,
        65536,
        262144,
        524288,
        1048576,
        16777216,
        33554432,
        67108864,
        134217728,
        268435456,
        32 | 64,
        64 | 128,
        32 | 2048,
        1024 | 2048,
    ] {
        let c = capture(options, true, 16);
        let before = c.clone();
        assert_eq!(
            c.decode_connx_default(MqConnxProfile::OrdinaryOwnedNonshared, None),
            Err(MqConnxProblem::UnsupportedOptions)
        );
        assert_eq!(c, before);
    }
    for options in [131072, 2097152, 536870912, 32 | 131072] {
        assert_eq!(
            capture(options, true, 0)
                .decode_connx_default(MqConnxProfile::OrdinaryOwnedNonshared, None),
            Err(MqConnxProblem::UnknownBits)
        );
    }
}

#[test]
fn negative_and_wide_numbers_reject_but_raw_observations_are_exact() {
    for number in [
        -1_i64,
        -1000000000,
        1000000000,
        i64::MIN,
        i64::MAX,
        i64::from(i32::MAX),
    ] {
        assert_eq!(checked_options(number), Err(MqConnxProblem::NumericRange));
    }
    for options in [i32::MIN, -1, i32::MAX] {
        let c = capture(options, false, 0);
        assert_eq!(c.field("Options"), Ok(MqRawFieldValue::Long(options)));
        assert_eq!(
            c.decode_connx_default(MqConnxProfile::OrdinaryOwnedNonshared, None),
            Err(MqConnxProblem::NumericRange)
        );
    }
}

#[test]
fn application_options_cannot_select_host_or_sharing_profile() {
    for profile in [
        MqConnxProfile::MtsSharingDefaultPending,
        MqConnxProfile::ClientOrFallbackPending,
        MqConnxProfile::ImplicitConnectionPending,
        MqConnxProfile::BindingOrSecurityPending,
        MqConnxProfile::UnknownPending,
    ] {
        for options in [0, 32] {
            assert_eq!(
                capture(options, true, 0).decode_connx_default(profile, None),
                Err(MqConnxProblem::ProfilePending)
            );
        }
    }
    let mut md = vec![0; 324];
    md[..4].copy_from_slice(b"MD  ");
    md[4..8].copy_from_slice(&1_i32.to_be_bytes());
    let c = MqRawCapture::capture(MqRawLayoutKind::Md1, &md, encoding(true)).unwrap();
    assert_eq!(
        c.decode_connx_default(MqConnxProfile::OrdinaryOwnedNonshared, None),
        Err(MqConnxProblem::WrongLayout)
    );
}

#[test]
fn no_output_fabrication_and_noop_suffix_preservation_are_atomic() {
    let input = fixture(32, true, 8);
    let c = capture(32, true, 8);
    let mut target = input.clone();
    let unchanged = MqRawObservedField {
        field: "Options",
        observation: MqRawObservation::Unchanged,
    };
    let undefined = MqRawObservedField {
        field: "StrucId",
        observation: MqRawObservation::Undefined,
    };
    c.writeback(context(), &[unchanged, undefined], &mut target)
        .unwrap();
    assert_eq!(target, input);
    // Options can be an IBM output, but its conditional binding observation is not represented here.
    for field in ["Options", "Version"] {
        let update = MqRawObservedField {
            field,
            observation: MqRawObservation::Observed(MqRawFieldValue::Long(0)),
        };
        assert_eq!(
            c.writeback(context(), &[undefined, update], &mut target),
            Err(MqRawProblem::OutputPending)
        );
        assert_eq!(target, input);
    }
    let update = MqRawObservedField {
        field: "StrucId",
        observation: MqRawObservation::Observed(MqRawFieldValue::Characters(b"CNO ")),
    };
    assert_eq!(
        c.writeback(context(), &[update], &mut target),
        Err(MqRawProblem::OutputPending)
    );
    assert_eq!(target, input);
    // Suffix is outside the v1 prefix; even a separately changed suffix is never overwritten.
    target[12..].fill(0xff);
    let before = target.clone();
    c.writeback(context(), &[], &mut target).unwrap();
    assert_eq!(target, before);
}

#[test]
fn stale_capacity_wrong_call_duplicate_and_unknown_batches_never_mutate() {
    let input = fixture(0, true, 8);
    let c = capture(0, true, 8);
    let mut target = input.clone();
    let item = MqRawObservedField {
        field: "Options",
        observation: MqRawObservation::Undefined,
    };
    assert_eq!(
        c.writeback(context(), &[item, item], &mut target),
        Err(MqRawProblem::DuplicateField)
    );
    assert_eq!(
        c.writeback(context(), &[item; 4], &mut target),
        Err(MqRawProblem::ObservationCount)
    );
    let foreign = MqRawObservedField {
        field: "ClientConnPtr",
        observation: MqRawObservation::Undefined,
    };
    assert_eq!(
        c.writeback(context(), &[item, foreign], &mut target),
        Err(MqRawProblem::Field)
    );
    assert_eq!(target, input);
    let mut wrong = context();
    wrong.call = MqMqiCall::Connect;
    assert_eq!(
        c.writeback(wrong, &[], &mut target),
        Err(MqRawProblem::OutputPending)
    );
    assert_eq!(target, input);
    target[8] ^= 1;
    let stale = target.clone();
    assert_eq!(
        c.writeback(context(), &[], &mut target),
        Err(MqRawProblem::StaleCapture)
    );
    assert_eq!(target, stale);
    let mut short = input[..12].to_vec();
    let before = short.clone();
    assert_eq!(
        c.writeback(context(), &[], &mut short),
        Err(MqRawProblem::Capacity)
    );
    assert_eq!(short, before);
}

#[test]
fn numeric_aliases_and_old_status_counts_stay_explicit() {
    let identities = mq_connx_numeric_identities();
    assert_eq!(identities.len(), 29);
    for pair in [
        ("MQCNO_NONE", 0),
        ("MQCNO_STANDARD_BINDING", 0),
        ("MQCNO_RECONNECT_AS_DEF", 0),
        ("MQCNO_HANDLE_SHARE_NONE", 32),
        ("MQCNO_HANDLE_SHARE_BLOCK", 64),
        ("MQCNO_VERSION_1", 1),
    ] {
        assert!(identities.contains(&pair));
    }
    assert_eq!(crate::mq_status::MQ_STATUS_PAIR_COUNT, 1030);
    assert_eq!(crate::mq_status::MQ_STATUS_PENDING_COUNT, 10);
}
