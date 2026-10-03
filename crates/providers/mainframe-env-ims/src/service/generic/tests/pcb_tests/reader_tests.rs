use super::*;
use serde_json::{Value, json};

fn prepared() -> (Arc<MemoryStore>, Arc<ImsService>) {
    let store = Arc::new(MemoryStore::new(Default::default()));
    let service = ImsService::open(store.clone(), Default::default()).unwrap();
    seed(&service);
    let run = "pcb-reader";
    call(
        &service,
        run,
        &request(run, ImsOperation::Schedule, 1, &[], b""),
    );
    call(
        &service,
        run,
        &selected(run, ImsOperation::GetHoldUnique, 2, 1, &["ROOT"], b""),
    );
    call(
        &service,
        run,
        &selected(run, ImsOperation::GetHoldUnique, 3, 2, &["ROOT"], b""),
    );
    (store, service)
}

#[test]
fn historical_session_without_map_retains_scheduled_pcb_position_and_hold() {
    let (store, service) = prepared();
    let expected = service.lock().unwrap().state.sessions["pcb-reader"]
        .position
        .clone();
    drop(service);
    let mut row = store
        .get_provider_state(SESSION_NAMESPACE, "pcb-reader")
        .unwrap()
        .unwrap();
    let mut value: Value = serde_json::from_slice(&row.payload).unwrap();
    value["value"]
        .as_object_mut()
        .unwrap()
        .remove("pcb_positions");
    row.payload = serde_json::to_vec(&value).unwrap();
    let prior = row.version;
    row.version += 1;
    store.put_provider_state(row, Some(prior)).unwrap();
    let service = ImsService::open(store, Default::default()).unwrap();
    let session = service.lock().unwrap().state.sessions["pcb-reader"].clone();
    assert_eq!(session.pcb, 1);
    assert_eq!(session.position, expected);
    assert!(session.position.is_held());
    assert!(session.pcb_positions.is_empty());
    assert_eq!(
        call(
            &service,
            "pcb-reader",
            &selected("pcb-reader", ImsOperation::GetNext, 4, 2, &["ROOT"], b"")
        )
        .segments[0]
            .data,
        b"B1X"
    );
}

#[test]
fn corrupt_pcb_map_keys_positions_and_oversize_fail_closed_on_open() {
    let default = serde_json::to_value(PcbPosition::default()).unwrap();
    for map in [
        json!({"0":default}),
        json!({"1":default}),
        json!({"3":default}),
        json!({"02":default}),
        json!({"2":{"current":999,"parentage":null,"held":null,"after_end":false}}),
        json!({"2":{"current":1,"parentage":1,"held":{"id":2,"version":1},"after_end":false}}),
        json!({"2":{"current":0,"parentage":null,"held":null,"after_end":false}}),
        json!({"2":{"current":1,"parentage":1,"held":null,"after_end":true}}),
        json!({"2":{"current":null,"parentage":null,"held":null,"after_end":false,"unknown":true}}),
        Value::Null,
        json!([]),
    ] {
        let (store, service) = prepared();
        drop(service);
        corrupt_row(
            store.as_ref(),
            SESSION_NAMESPACE,
            "pcb-reader",
            "pcb_positions",
            map,
        );
        assert!(matches!(
            ImsService::open(store, Default::default()),
            Err(HostProblem::InfrastructureFailure)
        ));
    }
    let (store, service) = prepared();
    drop(service);
    assert!(matches!(
        ImsService::open(
            store,
            ImsLimits {
                max_pcbs: 1,
                ..Default::default()
            }
        ),
        Err(HostProblem::InfrastructureFailure)
    ));
}

#[test]
fn duplicate_pcb_keys_are_rejected_instead_of_last_entry_winning() {
    let (store, service) = prepared();
    drop(service);
    let mut row = store
        .get_provider_state(SESSION_NAMESPACE, "pcb-reader")
        .unwrap()
        .unwrap();
    let mut value: Value = serde_json::from_slice(&row.payload).unwrap();
    value["value"]["pcb_positions"] = json!({});
    let body = serde_json::to_string(&value).unwrap();
    let position = serde_json::to_string(&PcbPosition::default()).unwrap();
    row.payload = body
        .replace(
            "\"pcb_positions\":{}",
            &format!("\"pcb_positions\":{{\"2\":{position},\"2\":{position}}}"),
        )
        .into_bytes();
    let prior = row.version;
    row.version += 1;
    store.put_provider_state(row, Some(prior)).unwrap();
    assert!(matches!(
        ImsService::open(store, Default::default()),
        Err(HostProblem::InfrastructureFailure)
    ));
}
