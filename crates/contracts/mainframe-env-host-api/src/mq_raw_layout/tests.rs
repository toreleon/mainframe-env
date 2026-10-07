use super::*;

const KINDS: [(MqRawLayoutKind, usize, i32, &[u8; 4]); 5] = [
    (MqRawLayoutKind::Od1, 168, 1, b"OD  "),
    (MqRawLayoutKind::Md1, 324, 1, b"MD  "),
    (MqRawLayoutKind::Md2, 364, 2, b"MD  "),
    (MqRawLayoutKind::Gmo1, 72, 1, b"GMO "),
    (MqRawLayoutKind::Pmo1, 128, 1, b"PMO "),
];

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

// Fixture widths/IDs/versions are independent reviewed expectations, not rendered data.
fn fixture(kind: MqRawLayoutKind, big: bool, suffix: usize) -> Vec<u8> {
    let (_, size, version, id) = KINDS.iter().find(|v| v.0 == kind).unwrap();
    let mut bytes: Vec<u8> = (0..size + suffix).map(|i| (i % 256) as u8).collect();
    bytes[..4].copy_from_slice(*id);
    bytes[4..8].copy_from_slice(&if big {
        version.to_be_bytes()
    } else {
        version.to_le_bytes()
    });
    bytes
}

fn context(call: MqMqiCall) -> MqRawWritebackContext {
    MqRawWritebackContext {
        call,
        platform: MqRawPlatform::Zos,
        single_queue: true,
        dynamic_model_open: false,
    }
}

fn observed<'a>(field: &'a str, value: MqRawFieldValue<'a>) -> MqRawObservedField<'a> {
    MqRawObservedField {
        field,
        observation: MqRawObservation::Observed(value),
    }
}

#[test]
fn every_prefix_has_complete_sequential_source_derived_fields() {
    for (kind, bytes, version, _) in KINDS {
        let layout = mq_raw_layout(kind);
        assert_eq!((layout.prefix_bytes, layout.version), (bytes, version));
        let mut offset = 0;
        for field in layout.fields {
            assert_eq!(field.offset, offset);
            if matches!(
                field.kind,
                MqRawFieldKind::Long | MqRawFieldKind::Alias | MqRawFieldKind::SignalSlot
            ) {
                assert_eq!(field.width, 4);
                assert_eq!(offset % 4, 0);
            }
            offset += field.width;
        }
        assert_eq!(offset, bytes);
    }
    assert_eq!(mq_raw_layout(MqRawLayoutKind::Md1).fields.len(), 24);
    assert_eq!(mq_raw_layout(MqRawLayoutKind::Md2).fields.len(), 29);
}

#[test]
fn all_prefixes_preserve_every_byte_and_actual_capacity_in_both_orders() {
    for (kind, size, version, _) in KINDS {
        for big in [false, true] {
            for suffix in [0, 1, 512] {
                let input = fixture(kind, big, suffix);
                let capture = MqRawCapture::capture(kind, &input, encoding(big)).unwrap();
                assert_eq!(capture.prefix(), &input[..size]);
                assert_eq!(capture.capacity(), input.len());
                assert_eq!(capture.field("Version"), Ok(MqRawFieldValue::Long(version)));
                let mut output = input.clone();
                let call = match kind {
                    MqRawLayoutKind::Od1 => MqMqiCall::Open,
                    MqRawLayoutKind::Pmo1 => MqMqiCall::Put,
                    _ => MqMqiCall::Get,
                };
                capture.writeback(context(call), &[], &mut output).unwrap();
                assert_eq!(output, input);
            }
        }
    }
}

#[test]
fn every_truncated_capacity_rejects_without_read_or_mutation() {
    for (kind, size, _, _) in KINDS {
        let input = fixture(kind, true, 0);
        for capacity in 0..size {
            let before = input[..capacity].to_vec();
            assert_eq!(
                MqRawCapture::capture(kind, &before, encoding(true)),
                Err(MqRawProblem::Capacity)
            );
            assert_eq!(&input[..capacity], before);
        }
    }
}

#[test]
fn negative_newer_wrong_version_and_bad_identifier_fail_closed() {
    for (kind, _, expected, _) in KINDS {
        for version in [-1_i32, 0, expected + 1, i32::MAX] {
            let mut input = fixture(kind, true, 8);
            input[4..8].copy_from_slice(&version.to_be_bytes());
            let before = input.clone();
            assert_eq!(
                MqRawCapture::capture(kind, &input, encoding(true)),
                Err(MqRawProblem::Version)
            );
            assert_eq!(input, before);
        }
        let mut input = fixture(kind, true, 0);
        input[3] = 0;
        assert_eq!(
            MqRawCapture::capture(kind, &input, encoding(true)),
            Err(MqRawProblem::StructureIdentifier)
        );
    }
}

#[test]
fn owned_cp037_identifiers_and_unsupported_encodings_are_explicit() {
    let ids = [
        [0xd6, 0xc4, 0x40, 0x40],
        [0xd4, 0xc4, 0x40, 0x40],
        [0xd4, 0xc4, 0x40, 0x40],
        [0xc7, 0xd4, 0xd6, 0x40],
        [0xd7, 0xd4, 0xd6, 0x40],
    ];
    for ((kind, _, _, _), id) in KINDS.into_iter().zip(ids) {
        let mut input = fixture(kind, true, 0);
        input[..4].copy_from_slice(&id);
        let e = MqRawStructureEncoding {
            characters: MqRawCharacterEncoding::OwnedCp037,
            ..encoding(true)
        };
        assert_eq!(
            MqRawCapture::capture(kind, &input, e).unwrap().prefix(),
            input
        );
        assert_eq!(
            MqRawCapture::capture(kind, &input, encoding(true)),
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
                MqRawCapture::capture(kind, &input, e),
                Err(MqRawProblem::UnsupportedEncoding)
            );
        }
    }
}

#[test]
fn md_encoding_is_body_metadata_and_never_changes_structure_decoder() {
    let mut input = fixture(MqRawLayoutKind::Md2, true, 0);
    input[24..28].copy_from_slice(&546_i32.to_be_bytes());
    let capture = MqRawCapture::capture(MqRawLayoutKind::Md2, &input, encoding(true)).unwrap();
    assert_eq!(capture.field("Encoding"), Ok(MqRawFieldValue::Long(546)));
    assert_eq!(capture.encoding(), encoding(true));
    assert_eq!(
        MqRawCapture::capture(MqRawLayoutKind::Md2, &input, encoding(false)),
        Err(MqRawProblem::Version)
    );
}

#[test]
fn source_offsets_distinguish_binary_ids_context_and_md2_tail() {
    let md = mq_raw_layout(MqRawLayoutKind::Md2);
    for (name, offset, width, kind) in [
        ("MsgId", 48, 24, MqRawFieldKind::Bytes),
        ("CorrelId", 72, 24, MqRawFieldKind::Bytes),
        ("AccountingToken", 208, 32, MqRawFieldKind::Bytes),
        ("Format", 32, 8, MqRawFieldKind::Characters),
        ("GroupId", 324, 24, MqRawFieldKind::Bytes),
        ("MsgSeqNumber", 348, 4, MqRawFieldKind::Long),
        ("Offset", 352, 4, MqRawFieldKind::Long),
        ("MsgFlags", 356, 4, MqRawFieldKind::Long),
        ("OriginalLength", 360, 4, MqRawFieldKind::Long),
    ] {
        let f = md.fields.iter().find(|f| f.name == name).unwrap();
        assert_eq!((f.offset, f.width, f.kind), (offset, width, kind));
    }
    let md1 = MqRawCapture::capture(
        MqRawLayoutKind::Md1,
        &fixture(MqRawLayoutKind::Md1, true, 64),
        encoding(true),
    )
    .unwrap();
    assert_eq!(md1.field("GroupId"), Err(MqRawProblem::Field));
}

#[test]
fn binary_zero_high_ids_and_char_blanks_are_never_trimmed_or_reencoded() {
    let mut input = fixture(MqRawLayoutKind::Md2, false, 2);
    let id: [u8; 24] = std::array::from_fn(|i| if i % 3 == 0 { 0 } else { 255 - i as u8 });
    input[48..72].copy_from_slice(&id);
    input[32..40].fill(b' ');
    input[324..348].fill(0);
    let capture = MqRawCapture::capture(MqRawLayoutKind::Md2, &input, encoding(false)).unwrap();
    assert_eq!(capture.field("MsgId"), Ok(MqRawFieldValue::Bytes(&id)));
    assert_eq!(
        capture.field("Format"),
        Ok(MqRawFieldValue::Characters(b"        "))
    );
    assert_eq!(
        capture.field("GroupId"),
        Ok(MqRawFieldValue::Bytes(&[0; 24]))
    );
    let id2 = [0xff; 24];
    let suffix = input[364..].to_vec();
    capture
        .writeback(
            context(MqMqiCall::Get),
            &[
                observed("MsgId", MqRawFieldValue::Bytes(&id2)),
                observed("Format", MqRawFieldValue::Characters(b"MQSTR   ")),
            ],
            &mut input,
        )
        .unwrap();
    assert_eq!(&input[48..72], &id2);
    assert_eq!(&input[32..40], b"MQSTR   ");
    assert_eq!(&input[364..], suffix);
}

#[test]
fn generated_initial_observations_do_not_initialize_missing_or_opaque_fields() {
    let md = mq_raw_layout(MqRawLayoutKind::Md2);
    for (name, value) in [
        ("Expiry", -1),
        ("Priority", -1),
        ("Persistence", 2),
        ("MsgType", 8),
        ("MsgSeqNumber", 1),
        ("OriginalLength", -1),
    ] {
        assert_eq!(
            md.fields.iter().find(|f| f.name == name).unwrap().initial,
            MqRawInitialValue::Long(value)
        );
    }
    assert_eq!(
        md.fields
            .iter()
            .find(|f| f.name == "Encoding")
            .unwrap()
            .initial,
        MqRawInitialValue::Environment
    );
    assert_eq!(
        mq_raw_layout(MqRawLayoutKind::Od1)
            .fields
            .iter()
            .find(|f| f.name == "DynamicQName")
            .unwrap()
            .initial,
        MqRawInitialValue::Environment
    );
    let input = fixture(MqRawLayoutKind::Md2, true, 0);
    assert_eq!(
        MqRawCapture::capture(MqRawLayoutKind::Md2, &input, encoding(true))
            .unwrap()
            .prefix(),
        input
    );
}

#[test]
fn every_md_get_output_field_can_be_observed_without_narrowing() {
    for kind in [MqRawLayoutKind::Md1, MqRawLayoutKind::Md2] {
        let input = fixture(kind, true, 5);
        let capture = MqRawCapture::capture(kind, &input, encoding(true)).unwrap();
        for field in &capture.layout().fields[2..] {
            let mut target = input.clone();
            let raw = vec![0xf0; field.width];
            let value = match field.kind {
                MqRawFieldKind::Long => MqRawFieldValue::Long(-1),
                MqRawFieldKind::Characters => MqRawFieldValue::Characters(&raw),
                MqRawFieldKind::Bytes => MqRawFieldValue::Bytes(&raw),
                _ => panic!("MD has no pointer/handle slot"),
            };
            capture
                .writeback(
                    context(MqMqiCall::Get),
                    &[observed(field.name, value)],
                    &mut target,
                )
                .unwrap();
            let mut expected = input.clone();
            expected[field.offset..field.offset + field.width].copy_from_slice(
                if field.kind == MqRawFieldKind::Long {
                    &[255; 4]
                } else {
                    &raw
                },
            );
            assert_eq!(target, expected);
        }
    }
}

#[test]
fn late_bad_field_kind_width_duplicate_or_input_rolls_back_every_byte() {
    let input = fixture(MqRawLayoutKind::Md2, true, 12);
    let capture = MqRawCapture::capture(MqRawLayoutKind::Md2, &input, encoding(true)).unwrap();
    for (bad, error) in [
        (
            observed("Foreign", MqRawFieldValue::Long(0)),
            MqRawProblem::Field,
        ),
        (
            observed("MsgId", MqRawFieldValue::Characters(&[0; 24])),
            MqRawProblem::FieldKind,
        ),
        (
            observed("MsgId", MqRawFieldValue::Bytes(&[0; 23])),
            MqRawProblem::FieldWidth,
        ),
        (
            observed("Version", MqRawFieldValue::Long(1)),
            MqRawProblem::OutputPending,
        ),
        (
            observed("Expiry", MqRawFieldValue::Long(4)),
            MqRawProblem::DuplicateField,
        ),
        (
            observed("Priority", MqRawFieldValue::Long(i32::MAX)),
            MqRawProblem::CobolLongRange,
        ),
    ] {
        let mut target = input.clone();
        let before = capture.clone();
        assert_eq!(
            capture.writeback(
                context(MqMqiCall::Get),
                &[observed("Expiry", MqRawFieldValue::Long(600)), bad],
                &mut target
            ),
            Err(error)
        );
        assert_eq!(target, input);
        assert_eq!(capture, before);
    }
}

#[test]
fn wrong_call_stale_prefix_and_changed_capacity_reject_atomically() {
    let input = fixture(MqRawLayoutKind::Gmo1, true, 1);
    let capture = MqRawCapture::capture(MqRawLayoutKind::Gmo1, &input, encoding(true)).unwrap();
    let mut target = input.clone();
    assert_eq!(
        capture.writeback(context(MqMqiCall::Put), &[], &mut target),
        Err(MqRawProblem::OutputPending)
    );
    assert_eq!(target, input);
    target[12] ^= 1;
    let stale = target.clone();
    assert_eq!(
        capture.writeback(context(MqMqiCall::Get), &[], &mut target),
        Err(MqRawProblem::StaleCapture)
    );
    assert_eq!(target, stale);
    target = input[..72].to_vec();
    let shorter = target.clone();
    assert_eq!(
        capture.writeback(context(MqMqiCall::Get), &[], &mut target),
        Err(MqRawProblem::Capacity)
    );
    assert_eq!(target, shorter);
}

#[test]
fn observation_ceiling_is_finite_and_duplicates_include_undefined() {
    let input = fixture(MqRawLayoutKind::Gmo1, true, 0);
    let capture = MqRawCapture::capture(MqRawLayoutKind::Gmo1, &input, encoding(true)).unwrap();
    let mut target = input.clone();
    let item = MqRawObservedField {
        field: "Signal1",
        observation: MqRawObservation::Undefined,
    };
    assert_eq!(
        capture.writeback(context(MqMqiCall::Get), &[item; 8], &mut target),
        Err(MqRawProblem::ObservationCount)
    );
    assert_eq!(
        capture.writeback(context(MqMqiCall::Get), &[item; 2], &mut target),
        Err(MqRawProblem::DuplicateField)
    );
    assert_eq!(target, input);
}

#[test]
fn undefined_and_unchanged_zos_pmo_counts_preserve_exact_inputs() {
    let input = fixture(MqRawLayoutKind::Pmo1, true, 9);
    let capture = MqRawCapture::capture(MqRawLayoutKind::Pmo1, &input, encoding(true)).unwrap();
    let mut target = input.clone();
    let fields = [
        MqRawObservedField {
            field: "KnownDestCount",
            observation: MqRawObservation::Undefined,
        },
        MqRawObservedField {
            field: "UnknownDestCount",
            observation: MqRawObservation::Unchanged,
        },
    ];
    capture
        .writeback(context(MqMqiCall::Put), &fields, &mut target)
        .unwrap();
    assert_eq!(target, input);
    for name in ["KnownDestCount", "UnknownDestCount", "InvalidDestCount"] {
        assert_eq!(
            capture.writeback(
                context(MqMqiCall::Put),
                &[observed(name, MqRawFieldValue::Long(1))],
                &mut target
            ),
            Err(MqRawProblem::OutputPending)
        );
        assert_eq!(target, input);
    }
    let mut other = context(MqMqiCall::Put);
    other.platform = MqRawPlatform::Other;
    assert_eq!(
        capture.writeback(
            other,
            &[observed("KnownDestCount", MqRawFieldValue::Long(-1))],
            &mut target,
        ),
        Err(MqRawProblem::OutputPending)
    );
    assert_eq!(target, input);
    capture
        .writeback(
            other,
            &[observed("KnownDestCount", MqRawFieldValue::Long(1))],
            &mut target,
        )
        .unwrap();
    assert_eq!(&target[20..24], &1_i32.to_be_bytes());
}

#[test]
fn resolved_names_require_real_single_queue_or_model_observation() {
    let names = [b' '; 48];
    for kind in [
        MqRawLayoutKind::Od1,
        MqRawLayoutKind::Gmo1,
        MqRawLayoutKind::Pmo1,
    ] {
        let input = fixture(kind, true, 0);
        let capture = MqRawCapture::capture(kind, &input, encoding(true)).unwrap();
        let mut target = input.clone();
        let (name, call) = match kind {
            MqRawLayoutKind::Od1 => ("ObjectName", MqMqiCall::Open),
            MqRawLayoutKind::Gmo1 => ("ResolvedQName", MqMqiCall::Get),
            _ => ("ResolvedQName", MqMqiCall::PutOne),
        };
        let mut c = context(call);
        if kind == MqRawLayoutKind::Od1 {
            assert_eq!(
                capture.writeback(
                    c,
                    &[observed(name, MqRawFieldValue::Characters(&names))],
                    &mut target
                ),
                Err(MqRawProblem::OutputPending)
            );
            c.dynamic_model_open = true;
        }
        if kind == MqRawLayoutKind::Pmo1 {
            c.single_queue = false;
            assert_eq!(
                capture.writeback(
                    c,
                    &[observed(name, MqRawFieldValue::Characters(&names))],
                    &mut target
                ),
                Err(MqRawProblem::OutputPending)
            );
            c.single_queue = true;
        }
        capture
            .writeback(
                c,
                &[observed(name, MqRawFieldValue::Characters(&names))],
                &mut target,
            )
            .unwrap();
    }
}

#[test]
fn unsupported_conditional_put_fields_and_signal_slots_remain_pending() {
    let input = fixture(MqRawLayoutKind::Md1, true, 0);
    let capture = MqRawCapture::capture(MqRawLayoutKind::Md1, &input, encoding(true)).unwrap();
    let mut target = input.clone();
    capture
        .writeback(
            context(MqMqiCall::PutOne),
            &[observed("MsgId", MqRawFieldValue::Bytes(&[0xff; 24]))],
            &mut target,
        )
        .unwrap();
    let mut multiple = context(MqMqiCall::PutOne);
    multiple.single_queue = false;
    let mut multiple_target = input.clone();
    assert_eq!(
        capture.writeback(
            multiple,
            &[observed("MsgId", MqRawFieldValue::Bytes(&[0xff; 24]))],
            &mut multiple_target,
        ),
        Err(MqRawProblem::OutputPending)
    );
    assert_eq!(multiple_target, input);
    for name in ["CorrelId", "UserIdentifier", "ApplIdentityData", "PutDate"] {
        let mut target = input.clone();
        assert_eq!(
            capture.writeback(
                context(MqMqiCall::PutOne),
                &[observed(name, MqRawFieldValue::Bytes(&[0; 24]))],
                &mut target
            ),
            Err(MqRawProblem::OutputPending)
        );
        assert_eq!(target, input);
    }
    let input = fixture(MqRawLayoutKind::Gmo1, true, 0);
    let capture = MqRawCapture::capture(MqRawLayoutKind::Gmo1, &input, encoding(true)).unwrap();
    let mut target = input.clone();
    assert_eq!(
        capture.field("Signal1"),
        Ok(MqRawFieldValue::SignalSlot(&input[16..20]))
    );
    assert_eq!(
        capture.writeback(
            context(MqMqiCall::Get),
            &[observed("Signal1", MqRawFieldValue::SignalSlot(&[0; 4]))],
            &mut target
        ),
        Err(MqRawProblem::OutputPending)
    );
    assert_eq!(target, input);
}

#[test]
fn scalar_capture_is_lossless_while_cobol_input_range_and_typed_projection_fail_closed() {
    for big in [true, false] {
        let mut input = fixture(MqRawLayoutKind::Md2, big, 0);
        input[12..16].copy_from_slice(&if big {
            i32::MIN.to_be_bytes()
        } else {
            i32::MIN.to_le_bytes()
        });
        let capture = MqRawCapture::capture(MqRawLayoutKind::Md2, &input, encoding(big)).unwrap();
        assert_eq!(
            capture.field("MsgType"),
            Ok(MqRawFieldValue::Long(i32::MIN))
        );
        assert_eq!(
            capture.try_typed_descriptor(),
            Err(MqRawProblem::DescriptorRepresentationPending)
        );
    }
    for number in [-999999999_i64, 999999999] {
        assert_eq!(mq_raw_cobol_long(number), Ok(number as i32));
    }
    for number in [-1000000000_i64, 1000000000, i64::MIN, i64::MAX] {
        assert_eq!(mq_raw_cobol_long(number), Err(MqRawProblem::CobolLongRange));
    }
    let input = fixture(MqRawLayoutKind::Pmo1, true, 0);
    let capture = MqRawCapture::capture(MqRawLayoutKind::Pmo1, &input, encoding(true)).unwrap();
    assert_eq!(
        capture.field("Context"),
        Ok(MqRawFieldValue::Alias(i32::from_be_bytes(
            input[16..20].try_into().unwrap()
        )))
    );
    assert_eq!(crate::mq_status::MQ_STATUS_PAIR_COUNT, 1030);
    assert_eq!(crate::mq_status::MQ_STATUS_PENDING_COUNT, 10);
}
