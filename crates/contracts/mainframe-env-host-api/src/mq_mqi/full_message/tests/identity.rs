use super::*;
use crate::mq_md_value::tests::{encoding, kind, raw};
use crate::mq_raw_layout::{MqRawCapture, mq_raw_layout};

#[test]
fn every_descriptor_field_and_profile_changes_full_request_and_host_result_identity() {
    for v2 in [false, true] {
        let original = envelope(MqMqiRequest::FullPut {
            connection: connection(),
            object: object(),
            put: put(v2),
        });
        let observed = result(
            MqMqiCall::Put,
            MqMqiOutput::FullPut {
                descriptor: value(v2, false),
                outcome: MqDeliveryOutcome::Accepted,
            },
        );
        let host = |r| {
            Ok(HostResult::MqMqi(MqMqiHostResult {
                result: r,
                limits: MqMqiLimits::default(),
            }))
        };
        let request_digest = mq_mqi_request_digest(&original).unwrap();
        let result_digest = canonical_result_digest(&host(observed.clone())).unwrap();
        let mut mutations = vec![value(v2, true)];
        for field in mq_raw_layout(kind(v2)).fields {
            if matches!(field.name, "StrucId" | "Version") {
                continue;
            }
            let mut input = raw(v2, true, false);
            input[field.offset] ^= 1;
            mutations.push(
                MqRawCapture::capture(kind(v2), &input, encoding(true, false))
                    .unwrap()
                    .to_full_md_value()
                    .unwrap(),
            );
        }
        for descriptor in mutations {
            let mut request = original.clone();
            if let MqMqiRequest::FullPut { put, .. } = &mut request.request {
                put.message.descriptor = descriptor.clone();
            }
            let changed = result(
                MqMqiCall::Put,
                MqMqiOutput::FullPut {
                    descriptor,
                    outcome: MqDeliveryOutcome::Accepted,
                },
            );
            assert_ne!(mq_mqi_request_digest(&request).unwrap(), request_digest);
            assert_ne!(
                canonical_result_digest(&host(changed)).unwrap(),
                result_digest
            );
        }
    }
}

#[test]
fn body_properties_controls_limits_and_data_length_are_identity_not_hidden_metadata() {
    let original = envelope(MqMqiRequest::FullGet(get(true)));
    let digest = mq_mqi_request_digest(&original).unwrap();
    for case in 0..6 {
        let mut changed = original.clone();
        let MqMqiRequest::FullGet(g) = &mut changed.request else {
            unreachable!()
        };
        match case {
            0 => g.buffer_capacity += 1,
            1 => g.truncation = MqTruncation::Accept,
            2 => g.mode = MqGetMode::BrowseFirst,
            3 => g.unit = MqMqiUnitOfWork::Local { unit: 9 },
            4 => {
                g.options = MqMqiOptions::PendingStructure {
                    requested_version: Some(1),
                }
            }
            _ => changed.limits.message.body_bytes -= 1,
        }
        assert_ne!(mq_mqi_request_digest(&changed).unwrap(), digest);
    }
    let base = result(
        MqMqiCall::Get,
        got(
            true,
            MqGetDisposition::Message(MqTruncationDisposition::Complete { length: 3 }),
            Some(3),
        ),
    );
    let host = |r| {
        Ok(HostResult::MqMqi(MqMqiHostResult {
            result: r,
            limits: MqMqiLimits::default(),
        }))
    };
    let digest = canonical_result_digest(&host(base.clone())).unwrap();
    for case in 0..4 {
        let mut changed = base.clone();
        let MqMqiOutcome::StatusPending {
            output:
                MqMqiOutput::FullGot {
                    message: Some(m),
                    disposition,
                    data_length,
                    ..
                },
        } = &mut changed.outcome
        else {
            unreachable!()
        };
        match case {
            0 => m.body.reverse(),
            1 => m.properties[0].value.reverse(),
            2 => m.properties[0].name.push('x'),
            _ => {
                m.body.push(17);
                *disposition =
                    MqGetDisposition::Message(MqTruncationDisposition::Complete { length: 4 });
                *data_length = Some(4);
            }
        }
        changed.validate(MqMqiLimits::default()).unwrap();
        assert_ne!(canonical_result_digest(&host(changed)).unwrap(), digest);
    }
}
