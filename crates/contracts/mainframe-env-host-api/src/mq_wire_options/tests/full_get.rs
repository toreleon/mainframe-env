use super::*;

fn full_input(v2: bool, cp037: bool, options: i64) -> MqWireFullGet {
    let (connection, object) = handles();
    MqWireFullGet {
        connection,
        object,
        descriptor: crate::mq_md_value::tests::value(v2, cp037),
        gmo_version: 1,
        options,
        wait_milliseconds: 0,
        buffer_capacity: 8,
    }
}

#[test]
fn complete_wire_adapter_preserves_every_diagnostic_descriptor_observation() {
    for v2 in [false, true] {
        for cp037 in [false, true] {
            for flags in [0, 4, 64, 4 | 64] {
                let input = full_input(v2, cp037, flags);
                let expected = input.descriptor.clone();
                let actual = get_full(input, &Bindings::default(), Default::default()).unwrap();
                assert_eq!(actual.descriptor, expected);
                assert_eq!(actual.mode, MqGetMode::Remove);
                assert_eq!(actual.wait, MqWait::NoWait);
                assert_eq!(actual.unit, MqMqiUnitOfWork::NoSyncpoint);
                assert_eq!(actual.buffer_capacity, 8);
                assert_eq!(actual.message_handle, None);
                assert_eq!(actual.options, MqMqiOptions::ContractDefault);
                assert_eq!(
                    actual.truncation,
                    if flags & 64 != 0 {
                        MqTruncation::Accept
                    } else {
                        MqTruncation::Reject
                    }
                );
            }
        }
    }
}

#[test]
fn complete_wire_adapter_delegates_default_and_explicit_unit_rules() {
    let bindings = Bindings {
        zos: true,
        unit: Some(MqMqiUnitOfWork::Local { unit: 19 }),
        ..Default::default()
    };
    for flags in [0, 2, 2 | 64] {
        assert_eq!(
            get_full(
                full_input(true, false, flags),
                &bindings,
                Default::default()
            )
            .unwrap()
            .unit,
            MqMqiUnitOfWork::Local { unit: 19 }
        );
    }
    assert_eq!(
        get_full(full_input(false, false, 4), &bindings, Default::default())
            .unwrap()
            .unit,
        MqMqiUnitOfWork::NoSyncpoint
    );
    assert_eq!(
        get_full(full_input(false, false, 6), &bindings, Default::default()),
        Err(MqWireProblem::IllegalCombination)
    );
    let missing = Bindings {
        zos: true,
        unit: None,
        ..Default::default()
    };
    assert_eq!(
        get_full(full_input(false, false, 0), &missing, Default::default()),
        Err(MqWireProblem::MissingUnit)
    );
    let external = Bindings {
        zos: true,
        unit: Some(MqMqiUnitOfWork::ExternalPending { unit: 19 }),
        ..Default::default()
    };
    assert_eq!(
        get_full(full_input(false, false, 2), &external, Default::default()),
        Err(MqWireProblem::PendingExternalUnit)
    );
}

#[test]
fn complete_wire_adapter_refuses_pending_controls_and_unknown_versions() {
    for flags in [1, 8, 16, 32, 256, 4096, 16384, 67108864] {
        assert!(matches!(
            get_full(
                full_input(true, false, flags),
                &Bindings::default(),
                Default::default()
            ),
            Err(MqWireProblem::PendingOptions {
                family: MqWireFamily::Get,
                ..
            })
        ));
    }
    for (version, expected) in [
        (
            2,
            MqWireProblem::PendingVersion {
                family: MqWireFamily::Get,
                version: 2,
            },
        ),
        (
            9,
            MqWireProblem::UnreviewedVersion {
                family: MqWireFamily::Get,
                version: 9,
            },
        ),
    ] {
        let mut input = full_input(true, false, 4);
        input.gmo_version = version;
        assert_eq!(
            get_full(input, &Bindings::default(), Default::default()),
            Err(expected)
        );
    }
    let mut input = full_input(false, false, 4);
    input.wait_milliseconds = 1;
    assert_eq!(
        get_full(input, &Bindings::default(), Default::default()),
        Err(MqWireProblem::PendingFields)
    );
}

#[test]
fn complete_wire_adapter_checks_policy_descriptor_and_total_capacity() {
    let policy = Bindings {
        policy: false,
        ..Default::default()
    };
    assert_eq!(
        get_full(full_input(false, false, 4), &policy, Default::default()),
        Err(MqWireProblem::PendingFields)
    );
    let mut input = full_input(false, false, 4);
    let crate::mq_md_value::MqMdValue::V1 { fields, .. } = &mut input.descriptor else {
        unreachable!()
    };
    fields.struc_id = [0; 4];
    assert_eq!(
        get_full(input, &Bindings::default(), Default::default()),
        Err(MqWireProblem::PendingFields)
    );
    for capacity in [-1, i64::MAX] {
        let mut input = full_input(true, false, 4);
        input.buffer_capacity = capacity;
        assert_eq!(
            get_full(input, &Bindings::default(), Default::default()),
            Err(MqWireProblem::MqlongRange)
        );
    }
    for capacity in [0, 8] {
        let mut input = full_input(true, false, 4);
        input.buffer_capacity = capacity;
        assert_eq!(
            get_full(input, &Bindings::default(), Default::default())
                .unwrap()
                .buffer_capacity,
            capacity as usize
        );
    }
    let narrow = MqMessageLimits {
        identifier_bytes: 23,
        ..Default::default()
    };
    assert_eq!(
        get_full(full_input(false, false, 4), &Bindings::default(), narrow),
        Err(MqWireProblem::Message(MqMessageProblem::Limits))
    );
}
