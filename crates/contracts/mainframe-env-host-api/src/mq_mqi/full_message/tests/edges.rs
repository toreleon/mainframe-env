use super::*;

#[test]
fn zero_length_messages_capacity_and_exact_property_count_remain_bounded() {
    for v2 in [false, true] {
        let mut p = put(v2);
        p.message.body.clear();
        p.message.properties = (0..128)
            .map(|i| MqMessageProperty {
                name: format!("p{i}"),
                kind: MqPropertyType::Null,
                value: vec![],
            })
            .collect();
        let e = envelope(MqMqiRequest::FullPut {
            connection: connection(),
            object: object(),
            put: p.clone(),
        });
        assert_eq!(e.validate(), Ok(()));
        assert_eq!(e.review(), Ok(MqMqiPending::StructureAndWireMapping));
        p.message.properties.push(MqMessageProperty {
            name: "extra".into(),
            kind: MqPropertyType::Null,
            value: vec![],
        });
        assert!(p.message.validate(MqMessageLimits::default()).is_err());
        let mut g = get(v2);
        g.buffer_capacity = 0;
        let e = envelope(MqMqiRequest::FullGet(g));
        let output = MqMqiOutput::FullGot {
            disposition: MqGetDisposition::Message(MqTruncationDisposition::Complete { length: 0 }),
            message: Some(MqFullMessage {
                descriptor: value(v2, false),
                body: vec![],
                properties: vec![],
            }),
            data_length: Some(0),
            cursor: None,
        };
        let status =
            MqReviewedStatus::from_symbols(MqMqiCall::Get, "MQCC_OK", "MQRC_NONE").unwrap();
        assert!(MqMqiResult::reviewed_output(status, output.clone(), &e).is_ok());
        let mut bad = e.clone();
        let MqMqiRequest::FullGet(g) = &mut bad.request else {
            unreachable!()
        };
        g.options = MqMqiOptions::PendingStructure {
            requested_version: Some(1),
        };
        assert!(MqMqiResult::reviewed_output(status, output.clone(), &bad).is_err());
        for wrong in [value(!v2, false), value(v2, true)] {
            let mut bad = output.clone();
            let MqMqiOutput::FullGot {
                message: Some(m), ..
            } = &mut bad
            else {
                unreachable!()
            };
            m.descriptor = wrong;
            assert!(MqMqiResult::reviewed_output(status, bad, &e).is_err());
        }
    }
}
