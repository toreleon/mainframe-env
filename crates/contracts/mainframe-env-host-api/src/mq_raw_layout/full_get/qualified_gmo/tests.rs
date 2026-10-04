//! Independent value/byte fixtures, not installed/native GET or live authority.
use super::*;
use crate::mq_md_value::tests::{encoding, kind, raw, value};
use crate::mq_mqi::*;
use crate::mq_status::MqReviewedStatus;
use crate::*;

fn context() -> MqRawWritebackContext {
    MqRawWritebackContext {
        call: MqMqiCall::Get,
        platform: MqRawPlatform::Zos,
        single_queue: true,
        dynamic_model_open: false,
    }
}

// Literal MQGMO1 declaration order/widths, not generated offsets or a decoder.
fn original(big: bool, cp: bool) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend(if cp {
        [0xc7, 0xd4, 0xd6, 0x40]
    } else {
        *b"GMO "
    });
    for number in [1_i32, 0x01234567, i32::MIN] {
        bytes.extend(if big {
            number.to_be_bytes()
        } else {
            number.to_le_bytes()
        });
    }
    bytes.extend([0x11, 0x22, 0, 0xff]); // ignored Signal1, never dereferenced
    bytes.extend(if big {
        i32::MAX.to_be_bytes()
    } else {
        i32::MAX.to_le_bytes()
    });
    bytes.extend([0xa5; 48]);
    bytes.extend([0, 0xff, 0x40, 0x20, 0, 0x17, 0x91, 0]); // caller suffix/reserved
    assert_eq!(bytes.len(), 80);
    bytes
}
fn name(cp: bool) -> [u8; 48] {
    let mut bytes = [if cp { 0x40 } else { b' ' }; 48];
    // Exact bytes including padding, null and non-text observations must survive.
    bytes[..8].copy_from_slice(&[0, 0xff, if cp { 0xd8 } else { b'Q' }, 0x20, 0x40, 0, 7, 0]);
    bytes
}
fn request(v2: bool, cp: bool, truncate: MqTruncation) -> MqMqiRequestEnvelope {
    let object: MqHandleObservation = serde_json::from_str(
        r#"{"role":"Object","registry":77,"slot":2,"generation":3,"epoch":4}"#,
    )
    .unwrap();
    let connection: MqHandleObservation = serde_json::from_str(
        r#"{"role":"Connection","registry":77,"slot":1,"generation":3,"epoch":4}"#,
    )
    .unwrap();
    MqMqiRequestEnvelope {
        context: MqMqiContext {
            owner: MqHandleOwner {
                environment: MqHostEnvironment::ZosBatch,
                host_id: 1,
                process_id: 2,
                thread_id: 3,
                task_id: 4,
                syncpoint_epoch: 5,
            },
            syncpoint_owner: MqSyncpointOwner::QueueManager,
        },
        limits: MqMqiLimits::default(),
        request: MqMqiRequest::QualifiedFullGet(MqMqiFullGet {
            connection: connection.historical_connection().unwrap(), // no live authority
            object: object.historical_object().unwrap(),             // no executable permission
            descriptor: value(v2, cp),
            mode: MqGetMode::Remove,
            wait: MqWait::NoWait,
            truncation: truncate,
            buffer_capacity: 3,
            message_handle: None,
            options: MqMqiOptions::ContractDefault,
            unit: MqMqiUnitOfWork::NoSyncpoint,
        }),
    }
}
fn returned(v2: bool, cp: bool, which: usize) -> MqMqiResult {
    let (disposition, reason, completion, length, meaningful, resolved) = match which {
        0 => (
            MqGetDisposition::Message(MqTruncationDisposition::Complete { length: 3 }),
            "MQRC_NONE",
            "MQCC_OK",
            Some(3),
            true,
            Some(name(cp)),
        ),
        1 => (
            MqGetDisposition::Message(MqTruncationDisposition::AcceptedRemoved {
                required: 5,
                copied: 3,
            }),
            "MQRC_TRUNCATED_MSG_ACCEPTED",
            "MQCC_WARNING",
            Some(5),
            true,
            Some(name(cp)),
        ),
        2 => (
            MqGetDisposition::Message(MqTruncationDisposition::RejectedRetained {
                required: 5,
                copied: 3,
            }),
            "MQRC_TRUNCATED_MSG_FAILED",
            "MQCC_WARNING",
            Some(5),
            true,
            None,
        ),
        3 => (
            MqGetDisposition::NoMessage,
            "MQRC_NO_MSG_AVAILABLE",
            "MQCC_FAILED",
            None,
            false,
            None,
        ),
        _ => unreachable!(),
    };
    MqMqiResult {
        call: MqMqiCall::Get,
        outcome: MqMqiOutcome::ReviewedOutput {
            status: MqReviewedStatus::from_symbols(MqMqiCall::Get, completion, reason).unwrap(),
            output: MqMqiOutput::QualifiedFullGot(MqMqiQualifiedGot {
                characters: value(v2, cp).characters(),
                disposition,
                message: meaningful.then(|| MqFullMessage {
                    descriptor: value(v2, cp),
                    body: vec![0, 255, 10],
                    properties: vec![],
                }),
                data_length: length,
                cursor: None,
                resolved_queue: resolved,
            }),
        },
    }
}
fn qvalue(result: &mut MqMqiResult) -> &mut MqMqiQualifiedGot {
    let MqMqiOutcome::ReviewedOutput {
        output: MqMqiOutput::QualifiedFullGot(v),
        ..
    } = &mut result.outcome
    else {
        panic!()
    };
    v
}

#[test]
fn exact_qname_only_in_independent_gmo1_all_structure_and_md_profiles() {
    for big in [false, true] {
        for cp in [false, true] {
            for v2 in [false, true] {
                for which in 0..4 {
                    let original = original(big, cp);
                    let capture =
                        MqRawCapture::capture(MqRawLayoutKind::Gmo1, &original, encoding(big, cp))
                            .unwrap();
                    let request = request(
                        v2,
                        cp,
                        if which == 1 {
                            MqTruncation::Accept
                        } else {
                            MqTruncation::Reject
                        },
                    );
                    let result = returned(v2, cp, which);
                    let untouched_result = result.clone();
                    let mut scratch = original.clone();
                    capture
                        .stage_qualified_full_get_gmo(
                            context(),
                            &request,
                            &result,
                            &original,
                            &mut scratch,
                        )
                        .unwrap();
                    let mut expected = original.clone();
                    if which < 2 {
                        expected[24..72].copy_from_slice(&name(cp));
                    }
                    assert_eq!(scratch, expected);
                    assert_eq!(result, untouched_result);
                    assert_eq!(capture.prefix(), &original[..72]);
                    assert_eq!(&scratch[..24], &original[..24]);
                    assert_eq!(&scratch[72..], &original[72..]);
                    // Diagnostic full signed MD observations are not narrowed or
                    // written by this separate GMO helper, regardless of PIC range.
                    assert!(mq_raw_cobol_long(i64::from(i32::MIN)).is_err());
                }
            }
        }
    }
}

#[test]
fn all_captured_bytes_and_suffix_late_edits_refuse_without_partial_writes() {
    for big in [false, true] {
        for cp in [false, true] {
            let original = original(big, cp);
            let capture =
                MqRawCapture::capture(MqRawLayoutKind::Gmo1, &original, encoding(big, cp)).unwrap();
            let request = request(true, cp, MqTruncation::Reject);
            let result = returned(true, cp, 0);
            for index in 0..original.len() {
                let mut scratch = original.clone();
                scratch[index] ^= 1;
                let before = scratch.clone();
                assert_eq!(
                    capture.stage_qualified_full_get_gmo(
                        context(),
                        &request,
                        &result,
                        &original,
                        &mut scratch
                    ),
                    Err(MqRawProblem::StaleCapture)
                );
                assert_eq!(scratch, before);
                let mut captured = original.clone();
                captured[index] ^= 1;
                let mut scratch = original.clone();
                assert_eq!(
                    capture.stage_qualified_full_get_gmo(
                        context(),
                        &request,
                        &result,
                        &captured,
                        &mut scratch
                    ),
                    Err(MqRawProblem::StaleCapture)
                );
                assert_eq!(scratch, original);
            }
            for n in [0, 24, 71, 72, 79, 81] {
                let mut scratch = vec![0xa5; n];
                let before = scratch.clone();
                assert_eq!(
                    capture.stage_qualified_full_get_gmo(
                        context(),
                        &request,
                        &result,
                        &original,
                        &mut scratch
                    ),
                    Err(MqRawProblem::Capacity)
                );
                assert_eq!(scratch, before);
            }
        }
    }
}

#[test]
fn wrong_kind_version_encoding_and_context_are_not_writeback_permission() {
    let original = original(true, false);
    let request = request(false, false, MqTruncation::Reject);
    let result = returned(false, false, 0);
    let capture =
        MqRawCapture::capture(MqRawLayoutKind::Gmo1, &original, encoding(true, false)).unwrap();
    for context in [
        MqRawWritebackContext {
            call: MqMqiCall::Put,
            ..context()
        },
        MqRawWritebackContext {
            platform: MqRawPlatform::Other,
            ..context()
        },
        MqRawWritebackContext {
            single_queue: false,
            ..context()
        },
        MqRawWritebackContext {
            dynamic_model_open: true,
            ..context()
        },
    ] {
        let mut scratch = original.clone();
        assert_eq!(
            capture.stage_qualified_full_get_gmo(
                context,
                &request,
                &result,
                &original,
                &mut scratch
            ),
            Err(MqRawProblem::OutputPending)
        );
        assert_eq!(scratch, original);
    }
    let cp_request = self::request(true, true, MqTruncation::Reject);
    let cp_result = returned(true, true, 0);
    let mut scratch = original.clone();
    assert_eq!(
        capture.stage_qualified_full_get_gmo(
            context(),
            &cp_request,
            &cp_result,
            &original,
            &mut scratch
        ),
        Err(MqRawProblem::UnsupportedEncoding)
    );
    assert_eq!(scratch, original);
    for version in [0_i32, 2, i32::MAX] {
        let mut bad = original.clone();
        bad[4..8].copy_from_slice(&version.to_be_bytes());
        assert_eq!(
            MqRawCapture::capture(MqRawLayoutKind::Gmo1, &bad, encoding(true, false)),
            Err(MqRawProblem::Version)
        );
    }
    for n in 0..72 {
        assert!(
            MqRawCapture::capture(MqRawLayoutKind::Gmo1, &original[..n], encoding(true, false))
                .is_err()
        );
    }
    for encoding in [
        MqRawStructureEncoding {
            numbers: MqRawNumberEncoding::Unsupported,
            ..encoding(true, false)
        },
        MqRawStructureEncoding {
            characters: MqRawCharacterEncoding::Unsupported,
            ..encoding(true, false)
        },
    ] {
        assert_eq!(
            MqRawCapture::capture(MqRawLayoutKind::Gmo1, &original, encoding),
            Err(MqRawProblem::UnsupportedEncoding)
        );
    }
    for v2 in [false, true] {
        let md_raw = raw(v2, true, false);
        let md = MqRawCapture::capture(kind(v2), &md_raw, encoding(true, false)).unwrap();
        let mut scratch = md_raw.clone();
        assert_eq!(
            md.stage_qualified_full_get_gmo(context(), &request, &result, &md_raw, &mut scratch),
            Err(MqRawProblem::FieldKind)
        );
        assert_eq!(scratch, md_raw);
    }
}

#[test]
fn malformed_status_class_profile_shape_limits_and_unknown_refuse_exactly() {
    let original = original(true, false);
    let capture =
        MqRawCapture::capture(MqRawLayoutKind::Gmo1, &original, encoding(true, false)).unwrap();
    let request = request(false, false, MqTruncation::Reject);
    let good = returned(false, false, 0);
    let mut bad_values = Vec::new();
    let mut bad = good.clone();
    bad.call = MqMqiCall::Put;
    bad_values.push(bad);
    let mut bad = good.clone();
    qvalue(&mut bad).resolved_queue = None;
    bad_values.push(bad);
    let mut bad = good.clone();
    qvalue(&mut bad).message = None;
    bad_values.push(bad);
    let mut bad = good.clone();
    qvalue(&mut bad).data_length = Some(4);
    bad_values.push(bad);
    let mut bad = good.clone();
    qvalue(&mut bad).characters = MqMdCharacterEncoding::OwnedCp037;
    bad_values.push(bad);
    let mut bad = good.clone();
    let v = qvalue(&mut bad).clone();
    bad.outcome = MqMqiOutcome::ReviewedOutput {
        status: MqReviewedStatus::from_symbols(MqMqiCall::Get, "MQCC_OK", "MQRC_NONE").unwrap(),
        output: MqMqiOutput::FullGot {
            disposition: v.disposition,
            message: v.message,
            data_length: v.data_length,
            cursor: v.cursor,
        },
    };
    bad_values.push(bad);
    let mut bad = returned(false, false, 2);
    qvalue(&mut bad).resolved_queue = Some(name(false));
    bad_values.push(bad);
    let mut bad = returned(false, false, 3);
    qvalue(&mut bad).resolved_queue = Some(name(false));
    bad_values.push(bad);
    let mut bad = good.clone();
    let MqMqiOutcome::ReviewedOutput { status, .. } = &mut bad.outcome else {
        panic!()
    };
    *status =
        MqReviewedStatus::from_symbols(MqMqiCall::Get, "MQCC_FAILED", "MQRC_NO_MSG_AVAILABLE")
            .unwrap();
    bad_values.push(bad);
    for outcome in [
        MqMqiOutcome::UnknownOutcome,
        MqMqiOutcome::DuplicatePossible,
        MqMqiOutcome::StatusPending {
            output: MqMqiOutput::QualifiedFullGot(MqMqiQualifiedGot {
                characters: MqMdCharacterEncoding::AsciiCompatible,
                disposition: MqGetDisposition::UnknownOutcome,
                message: None,
                data_length: None,
                cursor: None,
                resolved_queue: None,
            }),
        },
        MqMqiOutcome::ReviewedStatus {
            status: MqReviewedStatus::from_symbols(MqMqiCall::Get, "MQCC_OK", "MQRC_NONE").unwrap(),
        },
    ] {
        bad_values.push(MqMqiResult {
            call: MqMqiCall::Get,
            outcome,
        });
    }
    for result in bad_values {
        let mut scratch = original.clone();
        assert_eq!(
            capture.stage_qualified_full_get_gmo(
                context(),
                &request,
                &result,
                &original,
                &mut scratch
            ),
            Err(MqRawProblem::OutputPending)
        );
        assert_eq!(scratch, original);
    }
    for which in 0..4 {
        let mut bad_request = request.clone();
        match which {
            0 => {
                let MqMqiRequest::QualifiedFullGet(v) = bad_request.request else {
                    panic!()
                };
                bad_request.request = MqMqiRequest::FullGet(v);
            }
            1 => bad_request.context.owner.process_id = 0,
            2 => bad_request.limits.message.destination_bytes = 47,
            3 => {
                let MqMqiRequest::QualifiedFullGet(v) = &mut bad_request.request else {
                    panic!()
                };
                v.descriptor = value(true, false);
            }
            _ => unreachable!(),
        }
        let mut scratch = original.clone();
        assert_eq!(
            capture.stage_qualified_full_get_gmo(
                context(),
                &bad_request,
                &good,
                &original,
                &mut scratch
            ),
            Err(MqRawProblem::OutputPending)
        );
        assert_eq!(scratch, original);
    }
    // Existing known-success outcome is also a checked actual observation.
    let mut success = good.clone();
    let observation = qvalue(&mut success).clone();
    success.outcome = MqMqiOutcome::Completed {
        status: MqMqiStatus::OkNone,
        output: MqMqiOutput::QualifiedFullGot(observation),
    };
    let mut scratch = original.clone();
    capture
        .stage_qualified_full_get_gmo(context(), &request, &success, &original, &mut scratch)
        .unwrap();
    assert_eq!(&scratch[24..72], &name(false));
}

#[test]
fn complete_group_budget_and_foreign_context_fail_before_staging() {
    let original = original(true, false);
    let capture =
        MqRawCapture::capture(MqRawLayoutKind::Gmo1, &original, encoding(true, false)).unwrap();
    let result = returned(false, false, 0);
    for which in 0..5 {
        let mut request = request(false, false, MqTruncation::Reject);
        match which {
            0 => request.limits.buffer_bytes = original.len() - 1,
            1 => request.limits.buffer_bytes = MqMqiLimits::default().buffer_bytes + 1,
            2 => request.limits.buffer_bytes = 0,
            3 => request.context.owner.environment = MqHostEnvironment::ZosCics,
            4 => request.context.syncpoint_owner = MqSyncpointOwner::HostCoordinator,
            _ => unreachable!(),
        }
        let mut scratch = original.clone();
        assert_eq!(
            capture.stage_qualified_full_get_gmo(
                context(),
                &request,
                &result,
                &original,
                &mut scratch
            ),
            Err(if which == 0 {
                MqRawProblem::Capacity
            } else {
                MqRawProblem::OutputPending
            })
        );
        assert_eq!(scratch, original);
    }
    let mut request = request(false, false, MqTruncation::Reject);
    request.limits.buffer_bytes = original.len();
    let mut scratch = original.clone();
    capture
        .stage_qualified_full_get_gmo(context(), &request, &result, &original, &mut scratch)
        .unwrap();
    assert_eq!(&scratch[24..72], &name(false));
}
