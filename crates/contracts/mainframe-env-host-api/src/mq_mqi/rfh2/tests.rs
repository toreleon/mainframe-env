use super::*;
use crate::MqPropertyType as T;

#[test]
fn direct_result_assembly_binds_exact_request_capacity_descriptor_call_and_reviewed_status() {
    let (c, h) = {
        let mut registry = crate::MqHandleRegistry::new(1, 8).unwrap();
        let owner = crate::MqHandleOwner {
            environment: crate::MqHostEnvironment::ZosBatch,
            host_id: 1,
            process_id: 2,
            thread_id: 3,
            task_id: 4,
            syncpoint_epoch: 5,
        };
        let c = registry
            .connect(owner, crate::MqHandleSharing::NonShared)
            .unwrap();
        let h = registry.create_message(owner, c).unwrap();
        (c, h)
    };
    let request = MqMqiRequest::Rfh2(MqMqiRfh2Request::HandleToBuffer {
        connection: c,
        handle: h,
        profile: MqRfh2Profile::ZosBatchUtf8NativeV1,
        options: MqRfh2Options::checked(MqMqiCall::HandleToBuffer, 1, 1).unwrap(),
        descriptor: md(),
        name: name(),
        buffer_capacity: 4096,
    });
    let bytes = mq_rfh2_encode(
        &name(),
        &MqPropertyDescriptor::source_default(),
        &data(T::String, b" abc "),
        MqMqiLimits::default(),
    )
    .unwrap();
    let status =
        MqReviewedStatus::from_symbols(MqMqiCall::HandleToBuffer, "MQCC_OK", "MQRC_NONE").unwrap();
    let observation = MqRfh2Observation {
        descriptor: Some(mq_rfh2_outer_descriptor(&md()).unwrap()),
        data_length: Some(bytes.len() as i32),
        buffer: MqRfh2BufferObservation::WrittenPrefix(bytes),
    };
    let valid = MqMqiResult {
        call: MqMqiCall::HandleToBuffer,
        outcome: MqMqiOutcome::ReviewedOutput {
            status,
            output: MqMqiOutput::Rfh2Observation(observation.clone()),
        },
    };
    assert!(valid.validate(MqMqiLimits::default()).is_ok());
    assert!(valid.validate_reviewed_output_for(&request).is_ok());
    for case in 0..6 {
        let mut result = valid.clone();
        let mut original = request.clone();
        let MqMqiOutcome::ReviewedOutput { output, .. } = &mut result.outcome else {
            panic!()
        };
        let MqMqiOutput::Rfh2Observation(value) = output else {
            panic!()
        };
        match case {
            0 => {
                let MqMdValue::V1 { fields, .. } = value.descriptor.as_mut().unwrap() else {
                    panic!()
                };
                fields.msg_id = [0; 24];
            }
            1 => {
                let MqMqiRequest::Rfh2(MqMqiRfh2Request::HandleToBuffer {
                    buffer_capacity, ..
                }) = &mut original
                else {
                    panic!()
                };
                *buffer_capacity = 0;
            }
            2 => value.data_length = Some(1),
            3 => result.call = MqMqiCall::BufferToHandle,
            4 => result.outcome = MqMqiOutcome::ReviewedStatus { status },
            _ => {
                result.outcome = MqMqiOutcome::Completed {
                    status: MqMqiStatus::OkNone,
                    output: MqMqiOutput::Rfh2Observation(observation.clone()),
                }
            }
        }
        assert!(
            result.validate(MqMqiLimits::default()).is_err()
                || result.validate_reviewed_output_for(&original).is_err(),
            "case {case}"
        );
    }
}

#[test]
fn new_observation_has_independent_full_host_canonical_framing() {
    use crate::canonical::{Canonical, Encoder, RESULT_DIGEST_DOMAIN, encode};
    use crate::{
        HostProblem, HostResult, MAX_CANONICAL_EFFECT_BYTES, MqMqiHostResult,
        canonical_result_digest, canonical_result_size,
    };
    use sha2::{Digest, Sha256};
    struct Reference {
        limits: MqMqiLimits,
        status: MqReviewedStatus,
    }
    impl Canonical for Reference {
        fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
            out.variant("HostResult", "MqMqi", 1)?;
            out.text("0")?;
            out.object("MqMqiHostResult", 2)?;
            out.text("limits")?;
            self.limits.encode(out)?;
            out.text("result")?;
            out.object("MqMqiResult", 2)?;
            out.text("call")?;
            MqMqiCall::HandleToBuffer.encode(out)?;
            out.text("outcome")?;
            out.variant("MqMqiOutcome", "ReviewedOutput", 2)?;
            out.text("output")?;
            out.variant("MqMqiOutput", "Rfh2Observation", 1)?;
            out.text("0")?;
            out.object("MqRfh2Observation", 3)?;
            out.text("buffer")?;
            out.variant("MqRfh2BufferObservation", "Unchanged", 0)?;
            out.text("data_length")?;
            Some(116i32).encode(out)?;
            out.text("descriptor")?;
            Option::<MqMdValue>::None.encode(out)?;
            out.text("status")?;
            self.status.encode(out)
        }
    }
    let limits = MqMqiLimits::default();
    let status = MqReviewedStatus::from_symbols(
        MqMqiCall::HandleToBuffer,
        "MQCC_FAILED",
        "MQRC_PROPERTY_VALUE_TOO_BIG",
    )
    .unwrap();
    let value: MqMqiResult = MqMqiResult {
        call: MqMqiCall::HandleToBuffer,
        outcome: MqMqiOutcome::ReviewedOutput {
            status,
            output: MqMqiOutput::Rfh2Observation(MqRfh2Observation {
                descriptor: None,
                data_length: Some(116),
                buffer: MqRfh2BufferObservation::Unchanged,
            }),
        },
    };
    let actual: Result<_, HostProblem> = Ok(HostResult::MqMqi(MqMqiHostResult {
        result: value,
        limits,
    }));
    let expected: Result<_, HostProblem> = Ok(Reference { limits, status });
    let mut bytes = Vec::new();
    let size = encode(
        &expected,
        RESULT_DIGEST_DOMAIN,
        MAX_CANONICAL_EFFECT_BYTES,
        &mut |p| bytes.extend_from_slice(p),
    )
    .unwrap();
    assert_eq!(
        canonical_result_size(&actual, MAX_CANONICAL_EFFECT_BYTES),
        Ok(size)
    );
    assert_eq!(
        canonical_result_digest(&actual).unwrap(),
        <[u8; 32]>::from(Sha256::digest(&bytes))
    );
}

fn md() -> MqMdValue {
    let mut md = mq_property_initial_descriptor();
    let MqMdValue::V1 { fields, .. } = &mut md else {
        panic!()
    };
    fields.encoding = 785;
    fields.coded_char_set_id = 1208;
    fields.msg_id = [255; 24];
    fields.correl_id = [128; 24];
    md
}
fn name() -> MqPropertyName {
    MqPropertyName::checked("invoice.id".into(), MqMessageLimits::default()).unwrap()
}
fn data(kind: T, bytes: &[u8]) -> MqPropertyData {
    MqPropertyData {
        kind,
        encoding: 785,
        ccsid: 1208,
        bytes: bytes.to_vec(),
    }
}
fn fixture(xml: &[u8], tail: &[u8]) -> Vec<u8> {
    let n = (xml.len() + 3) & !3;
    let length = 40 + n;
    let mut bytes = b"RFH ".to_vec();
    for number in [2, length as i32, 785, -2] {
        bytes.extend(number.to_be_bytes());
    }
    bytes.extend(b"        ");
    bytes.extend(0i32.to_be_bytes());
    bytes.extend(1208i32.to_be_bytes());
    bytes.extend((n as i32).to_be_bytes());
    bytes.extend(xml);
    bytes.resize(length, b' ');
    bytes.extend(tail);
    bytes
}
#[test]
fn source_fixed_header_owned_xml_scalar_and_length_fixtures() {
    for (kind, bytes, lexical, expected) in [
        (T::Null, vec![], " xsi:nil='true'", ""),
        (T::ByteString, vec![0, 255, 128], " dt='bin.hex'", "00FF80"),
        (T::ByteString, vec![], " dt='bin.hex'", ""),
        (T::Int8, vec![128], " dt='i1'", "-128"),
        (
            T::Int16,
            (-32768i16).to_be_bytes().to_vec(),
            " dt='i2'",
            "-32768",
        ),
        (
            T::Int32,
            i32::MAX.to_be_bytes().to_vec(),
            " dt='i4'",
            "2147483647",
        ),
        (
            T::Int64,
            i64::MIN.to_be_bytes().to_vec(),
            " dt='i8'",
            "-9223372036854775808",
        ),
        (T::String, vec![], " dt='string'", ""),
        (
            T::String,
            b"  <a&b>  ".to_vec(),
            " dt='string'",
            "  &lt;a&amp;b>  ",
        ),
    ] {
        let value = data(kind, &bytes);
        let pd = MqPropertyDescriptor::source_default();
        let xml = format!("<invoice content='properties'><id{lexical}>{expected}</id></invoice>");
        let fixed = fixture(xml.as_bytes(), &[]);
        let n = mq_rfh2_required_length(&name(), &pd, &value, MqMqiLimits::default()).unwrap();
        assert_eq!(n, fixed.len());
        assert_eq!(
            mq_rfh2_encode(&name(), &pd, &value, MqMqiLimits::default()).unwrap(),
            fixed
        );
        let imported = mq_rfh2_decode(
            &mq_rfh2_outer_descriptor(&md()).unwrap(),
            &fixed,
            MqMqiLimits::default(),
        )
        .unwrap();
        assert_eq!(imported.properties, vec![(name(), pd, value)]);
        assert!(imported.tail.is_empty());
    }
}

#[test]
fn unvalidated_limits_hex_signs_xml_delimiters_and_preallocation_limits_fail_closed() {
    let outer = mq_rfh2_outer_descriptor(&md()).unwrap();
    let invalid = MqMqiLimits {
        buffer_bytes: usize::MAX,
        ..Default::default()
    };
    assert_eq!(
        mq_rfh2_decode(&md(), &[], invalid),
        Err(MqPropertyProblem::Capacity)
    );
    assert_eq!(
        mq_rfh2_required_length(
            &name(),
            &MqPropertyDescriptor::source_default(),
            &data(T::Null, &[]),
            invalid
        ),
        Err(MqPropertyProblem::Capacity)
    );
    for value in ["+A", "GG", "0", "-1"] {
        let xml = format!("<invoice content='properties'><id dt='bin.hex'>{value}</id></invoice>");
        assert_eq!(
            mq_rfh2_decode(
                &outer,
                &fixture(xml.as_bytes(), &[]),
                MqMqiLimits::default()
            ),
            Err(MqPropertyProblem::Value)
        );
    }
    let value = data(T::String, b"]]> &");
    let expected = fixture(
        b"<invoice content='properties'><id dt='string'>]]&gt; &amp;</id></invoice>",
        &[],
    );
    assert_eq!(
        mq_rfh2_encode(
            &name(),
            &MqPropertyDescriptor::source_default(),
            &value,
            MqMqiLimits::default()
        )
        .unwrap(),
        expected
    );
    assert_eq!(
        mq_rfh2_required_length(
            &name(),
            &MqPropertyDescriptor::source_default(),
            &value,
            MqMqiLimits::default()
        )
        .unwrap(),
        expected.len()
    );
    assert_eq!(
        mq_rfh2_decode(&outer, &expected, MqMqiLimits::default())
            .unwrap()
            .properties[0]
            .2,
        value
    );
    let limits = MqMqiLimits {
        message: MqMessageLimits {
            property_value_bytes: 1,
            ..Default::default()
        },
        ..Default::default()
    };
    assert_eq!(
        mq_rfh2_decode(&outer, &expected, limits),
        Err(MqPropertyProblem::Capacity)
    );
}
#[test]
fn opaque_tail_ids_complete_md_and_zero_import_are_preserved() {
    let original = md();
    let outer = mq_rfh2_outer_descriptor(&original).unwrap();
    let mut restored = outer.clone();
    let MqMdValue::V1 { fields, .. } = &mut restored else {
        panic!()
    };
    fields.format = *b"        ";
    assert_eq!(restored, original);
    let bytes = fixture(
        b"<invoice content='properties'><id>  a  </id></invoice>",
        &[0, 255, 128, b'<', b'&'],
    );
    let decoded = mq_rfh2_decode(&outer, &bytes, MqMqiLimits::default()).unwrap();
    assert_eq!(decoded.tail, vec![0, 255, 128, b'<', b'&']);
    assert_eq!(decoded.properties[0].2.bytes, b"  a  ");
    assert!(
        mq_rfh2_decode(&original, &[], MqMqiLimits::default())
            .unwrap()
            .properties
            .is_empty()
    );
    assert_eq!(original, md());
}
#[test]
fn malformed_and_recognized_pending_forms_are_distinct_and_bounded() {
    let md = mq_rfh2_outer_descriptor(&md()).unwrap();
    let valid = fixture(
        b"<invoice content='properties'><id dt='i1'>127</id></invoice>",
        &[],
    );
    for n in 1..36 {
        assert_eq!(
            mq_rfh2_decode(&md, &valid[..n], MqMqiLimits::default()),
            Err(MqPropertyProblem::Value)
        );
    }
    for (offset, number) in [(8, -1i32), (8, 37), (36, -4), (36, 3)] {
        let mut bytes = valid.clone();
        bytes[offset..offset + 4].copy_from_slice(&number.to_be_bytes());
        assert_eq!(
            mq_rfh2_decode(&md, &bytes, MqMqiLimits::default()),
            Err(MqPropertyProblem::Value)
        );
    }
    for xml in [
        "<invoice content='properties'><id dt='i1'>128</id></invoice>",
        "<invoice content='properties'><id dt='i8'>9223372036854775808</id></invoice>",
        "<invoice content='properties'><id dt='bin.hex'>F</id></invoice>",
    ] {
        assert_eq!(
            mq_rfh2_decode(&md, &fixture(xml.as_bytes(), &[]), MqMqiLimits::default()),
            Err(MqPropertyProblem::Value)
        );
    }
    for xml in [
        "<invoice content='properties'><id dt='boolean'>1</id></invoice>",
        "<invoice content='properties'><id context='user'>a</id></invoice>",
        "<invoice content='properties'><id>&external;</id></invoice>",
        "<!DOCTYPE invoice><invoice content='properties'><id>a</id></invoice>",
        "<invoice content='properties'><id xsi:nil='false'></id></invoice>",
        "<invoice content='properties'><id><nested>a</nested></id></invoice>",
    ] {
        assert_eq!(
            mq_rfh2_decode(&md, &fixture(xml.as_bytes(), &[]), MqMqiLimits::default()),
            Err(MqPropertyProblem::Unsupported)
        );
    }
    let mut small = MqMqiLimits::default();
    small.buffer_bytes = valid.len() - 1;
    assert_eq!(
        mq_rfh2_decode(&md, &valid, small),
        Err(MqPropertyProblem::Capacity)
    );
}
#[test]
fn numeric_profile_descriptor_and_name_refusals_do_not_infer_defaults() {
    assert!(MqRfh2Options::checked(MqMqiCall::BufferToHandle, 1, 0).is_ok());
    for call in [MqMqiCall::BufferToHandle, MqMqiCall::HandleToBuffer] {
        for bits in [-1, 4, i32::MAX] {
            assert_eq!(
                MqRfh2Options::checked(call, 1, bits),
                Err(MqPropertyProblem::Options)
            );
        }
        assert_eq!(
            MqRfh2Options::checked(call, 2, 0),
            Err(MqPropertyProblem::Unsupported)
        );
    }
    assert_eq!(
        MqRfh2Options::checked(MqMqiCall::BufferToHandle, 1, 1),
        Err(MqPropertyProblem::Unsupported)
    );
    assert_eq!(
        MqRfh2Options::checked(MqMqiCall::HandleToBuffer, 1, 0),
        Err(MqPropertyProblem::Unsupported)
    );
    for bits in [1, 3] {
        assert_eq!(
            MqRfh2Options::checked(MqMqiCall::HandleToBuffer, 1, bits)
                .unwrap()
                .deletes(),
            bits == 3
        );
    }
    let mut bad = md();
    let MqMdValue::V1 { fields, .. } = &mut bad else {
        panic!()
    };
    fields.report = 1;
    assert_eq!(validate_md(&bad), Err(MqPropertyProblem::Unsupported));
    for name in [
        "invoice.a.b",
        "invoice.xmlName",
        "invoice.JmsId",
        "Root.MQMD.Format",
    ] {
        if let Ok(name) = MqPropertyName::checked(name.into(), MqMessageLimits::default()) {
            assert_eq!(
                custom_name(&name, MqMessageLimits::default()),
                Err(MqPropertyProblem::Unsupported)
            );
        }
    }
    assert_eq!(
        mq_rfh2_required_length(
            &name(),
            &MqPropertyDescriptor::source_default(),
            &data(T::String, b"embedded\0"),
            MqMqiLimits::default()
        ),
        Err(MqPropertyProblem::Unsupported)
    );
}
