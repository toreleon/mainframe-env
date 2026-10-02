use super::*;
use crate::mq_status::MqReviewedStatus;
use crate::*;

fn owner() -> MqHandleOwner {
    MqHandleOwner {
        environment: MqHostEnvironment::ZosBatch,
        host_id: 1,
        process_id: 2,
        thread_id: 3,
        task_id: 4,
        syncpoint_epoch: 5,
    }
}
fn fixture() -> (MqHandleRegistry, MqHconn, MqHmsg) {
    let mut r = MqHandleRegistry::new(1, 4).unwrap();
    let c = r.connect(owner(), MqHandleSharing::NonShared).unwrap();
    let h = r.create_message(owner(), c).unwrap();
    (r, c, h)
}
fn options(call: MqMqiCall) -> MqPropertyOptions {
    MqPropertyOptions::checked(call, 1, 0).unwrap()
}
fn envelope(request: MqPropertyRequest) -> MqMqiRequestEnvelope {
    MqMqiRequestEnvelope {
        context: MqMqiContext {
            owner: owner(),
            syncpoint_owner: MqSyncpointOwner::QueueManager,
        },
        limits: Default::default(),
        request: MqMqiRequest::Property(request),
    }
}
fn named(s: &str) -> MqPropertyName {
    MqPropertyName::checked(s.into(), MqMessageLimits::default()).unwrap()
}
fn inquiry(c: MqHconn, h: MqHmsg) -> MqMqiRequestEnvelope {
    envelope(MqPropertyRequest::Inquire {
        connection: c,
        handle: h,
        options: options(MqMqiCall::InquireProperty),
        name: named("invoice.id"),
        requested_type: 0,
        name_capacity: 10,
        value_capacity: 2,
    })
}
fn short() -> MqMqiOutput {
    MqMqiOutput::PropertyObservation(MqPropertyObservation::Inquired(
        MqPropertyInquiryObservation {
            descriptor: MqPropertyDescriptor {
                struc_id: *b"PD  ",
                version: 1,
                options: 0,
                support: 1,
                context: 0,
                copy_options: 22,
            },
            kind: MqPropertyType::ByteString,
            returned_encoding: 785,
            returned_ccsid: None,
            returned_name: b"invoice.id".to_vec(),
            name_length: 10,
            name_ccsid: 1208,
            data_length: 4,
            copied_value: vec![0, 255],
        },
    ))
}

#[test]
fn exact_named_numeric_fixtures_and_all_option_families_fail_closed() {
    for (call, id, known) in [
        (MqMqiCall::CreateMessageHandle, *b"CMHO", 1),
        (MqMqiCall::DeleteMessageHandle, *b"DMHO", 0),
        (MqMqiCall::SetProperty, *b"SMPO", 4),
        (MqMqiCall::InquireProperty, *b"IMPO", 32),
        (MqMqiCall::DeleteProperty, *b"DMPO", 1),
    ] {
        assert_eq!(options(call).structure_id(), id);
        assert_eq!(
            MqPropertyOptions::checked(call, 2, 0),
            Err(MqPropertyProblem::Unsupported)
        );
        assert_eq!(
            MqPropertyOptions::checked(call, 1, -1),
            Err(MqPropertyProblem::Options)
        );
        assert_eq!(
            MqPropertyOptions::checked(call, 1, 1 << 30),
            Err(MqPropertyProblem::Options)
        );
        if known != 0 {
            assert_eq!(
                MqPropertyOptions::checked(call, 1, known),
                Err(MqPropertyProblem::Unsupported)
            );
        }
    }
    assert_eq!(
        MqPropertyOptions::checked(MqMqiCall::CallbackFunction, 1, 0),
        Err(MqPropertyProblem::Call)
    );
    let pd = MqPropertyDescriptor::source_default();
    assert_eq!(pd.support, 1);
    assert_eq!(pd.copy_options, 22);
    let mut wrong = pd.clone();
    wrong.support = 0;
    assert!(wrong.validate_input().is_err());
    let mut wrong = pd;
    wrong.copy_options = 0;
    assert!(wrong.validate_input().is_err());
    for (number, kind, bytes) in [
        (2, MqPropertyType::Null, vec![]),
        (8, MqPropertyType::ByteString, vec![0, 255]),
        (16, MqPropertyType::Int8, vec![128]),
        (32, MqPropertyType::Int16, vec![255, 254]),
        (64, MqPropertyType::Int32, (-123i32).to_be_bytes().to_vec()),
        (128, MqPropertyType::Int64, i64::MIN.to_be_bytes().to_vec()),
        (1024, MqPropertyType::String, b"x\0  ".to_vec()),
    ] {
        let v = MqPropertyData::from_numeric(
            number,
            785,
            1208,
            bytes.clone(),
            MqMessageLimits::default(),
        )
        .unwrap();
        assert_eq!(v.kind, kind);
        assert_eq!(v.bytes, bytes);
    }
    for kind in [-1, 0, 4, 256, 512, i32::MAX] {
        assert!(
            MqPropertyData::from_numeric(kind, 785, 1208, vec![], MqMessageLimits::default())
                .is_err()
        );
    }
    assert!(
        MqPropertyData::from_numeric(64, 785, 1208, vec![0; 3], MqMessageLimits::default())
            .is_err()
    );
    assert!(
        MqPropertyData::from_numeric(1024, 785, 1208, vec![255], MqMessageLimits::default())
            .is_err()
    );
    for (encoding, ccsid) in [(786, 1208), (785, -3), (0, 1208)] {
        assert!(
            MqPropertyData::from_numeric(8, encoding, ccsid, vec![], MqMessageLimits::default())
                .is_err()
        );
    }
}
#[test]
fn names_are_exact_bounded_and_descriptor_fields_cannot_be_narrowed() {
    for s in [
        "",
        ".a",
        "a.",
        "a..b",
        "1a",
        "a-b",
        "NOT",
        "null",
        "a.*",
        "Root.MQMD.StrucId",
        "Root.MQMD.Version",
        "root.MQMD.MsgId",
        "jms.X",
        "usr.JMSX",
        "a\0b",
        "é",
    ] {
        assert!(
            MqPropertyName::checked(s.into(), MqMessageLimits::default()).is_err(),
            "{s}"
        );
    }
    assert_ne!(named("a"), named("A"));
    assert_eq!(named("Root.MQMD.MsgId").descriptor_field(), Some("MsgId"));
    let (_, c, h) = fixture();
    let mut req = envelope(MqPropertyRequest::Set {
        connection: c,
        handle: h,
        options: options(MqMqiCall::SetProperty),
        name: named("Root.MQMD.MsgId"),
        descriptor: MqPropertyDescriptor::source_default(),
        value: MqPropertyData {
            kind: MqPropertyType::ByteString,
            encoding: 785,
            ccsid: 1208,
            bytes: vec![255; 24],
        },
    });
    req.validate().unwrap();
    let MqMqiRequest::Property(MqPropertyRequest::Set { value, .. }) = &mut req.request else {
        panic!()
    };
    value.bytes.pop();
    assert!(req.validate().is_err());
    let mut md = mq_property_initial_descriptor();
    let before = md.clone();
    assert!(mq_property_set_descriptor_bytes(&mut md, "MsgId", &[1; 23]).is_err());
    assert_eq!(md, before);
    assert!(mq_property_set_descriptor_bytes(&mut md, "StrucId", &[0; 4]).is_err());
    assert_eq!(md, before);
    for field in mq_property_md_fields() {
        assert_eq!(
            mq_property_descriptor_bytes(&md, field.name).unwrap().len(),
            field.width
        );
    }
}
#[test]
fn partial_failure_output_is_request_bound_and_numeric_alias_admission_cannot_bypass_it() {
    let (_, c, h) = fixture();
    let request = inquiry(c, h);
    let status = MqReviewedStatus::from_wire_pair(MqMqiCall::InquireProperty, 2, 2469).unwrap();
    let good = MqMqiResult::reviewed_output(status, short(), &request).unwrap();
    assert_eq!(status.wire_pair(), (2, 2469));
    assert_eq!(good.call, MqMqiCall::InquireProperty);
    for case in 0..11 {
        let mut output = short();
        let MqMqiOutput::PropertyObservation(MqPropertyObservation::Inquired(v)) = &mut output
        else {
            panic!()
        };
        match case {
            0 => v.data_length = -1,
            1 => v.copied_value.push(1),
            2 => v.name_length = 9,
            3 => v.returned_name[0] = b'X',
            4 => v.returned_ccsid = Some(1208),
            5 => v.returned_encoding = 1,
            6 => v.descriptor.support = 0,
            7 => v.kind = MqPropertyType::Int64,
            8 => v.data_length = 1,
            9 => v.name_ccsid = -3,
            _ => v.descriptor.copy_options = 0,
        }
        assert!(
            MqMqiResult::reviewed_output(status, output, &request).is_err(),
            "case {case}"
        );
    }
    let ok =
        MqReviewedStatus::from_symbols(MqMqiCall::InquireProperty, "MQCC_OK", "MQRC_NONE").unwrap();
    assert!(MqMqiResult::reviewed_output(ok, short(), &request).is_err());
    assert!(MqReviewedStatus::from_wire_pair(MqMqiCall::SetProperty, 2, 2469).is_err());
    assert!(
        MqReviewedStatus::from_symbols(
            MqMqiCall::InquireProperty,
            "MQCC_OK",
            "MQRC_PROPERTY_VALUE_TOO_BIG"
        )
        .is_err()
    );
    let mut legacy = request.clone();
    legacy.request = MqMqiRequest::InquireProperty(MqMqiPropertyInquiry {
        connection: c,
        handle: h,
        query: MqPropertyQuery::Exact("invoice.id".into()),
        after: None,
        requested_type: None,
        name_capacity: 10,
        value_capacity: 2,
        options: MqMqiOptions::ContractDefault,
    });
    assert!(MqMqiResult::reviewed_output(status, short(), &legacy).is_err());
    let full = Ok(HostResult::MqMqi(MqMqiHostResult {
        limits: request.limits,
        result: good,
    }));
    let bytes = canonical_result_size(&full, MAX_CANONICAL_EFFECT_BYTES).unwrap();
    assert_eq!(canonical_result_size(&full, bytes).unwrap(), bytes);
    assert!(canonical_result_size(&full, bytes - 1).is_err());
    assert_ne!(
        canonical_result_digest(&full).unwrap(),
        canonical_result_digest(&Ok(HostResult::MqMqi(MqMqiHostResult {
            limits: request.limits,
            result: MqMqiResult {
                call: MqMqiCall::InquireProperty,
                outcome: MqMqiOutcome::ReviewedStatus { status }
            }
        })))
        .unwrap()
    );
}
