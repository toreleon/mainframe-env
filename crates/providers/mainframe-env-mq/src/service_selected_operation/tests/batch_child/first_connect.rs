//! Provider-private same-store fixtures, not actual installed host proof.
use super::*;

#[path = "first_connect/failures.rs"]
mod failures;
#[path = "first_connect/restart.rs"]
mod restart;

fn request() -> MqMqiRequest {
    MqMqiRequest::Connect(MqMqiConnect {
        manager: None,
        sharing: MqHandleSharing::NonShared,
        options: MqMqiOptions::ContractDefault,
    })
}

fn connect(f: &Fixture, child: &Child) -> MqHconn {
    let reply = child.call(f, 10, request());
    let MqMqiOutput::Connected(c) = output(reply) else {
        panic!("child connection")
    };
    *f.connection.lock().unwrap() = Some(c);
    c
}

#[test]
fn memory_sqlite_next_same_task_child_uses_child_created_connection_without_reassigning_origin() {
    for sqlite in [false, true] {
        for back in [false, true] {
            let f = Fixture::new(sqlite);
            let child = Child::new(&f);
            let c = connect(&f, &child);
            let o = open(&f, &child, c);
            let unit = f.unit();
            child.call(
                &f,
                12,
                MqMqiRequest::Put {
                    connection: c,
                    object: o,
                    put: put(MqMqiUnitOfWork::Local { unit }),
                },
            );
            let owner = f
                .store
                .get_provider_state(ownership::UOW_NAMESPACE, &unit.to_string())
                .unwrap();
            let rows = f.rows();
            let audits = f
                .store
                .audit_records(&child.inv.execution_id, 0, 128)
                .unwrap();
            f.service
                .return_selected_batch_child(child.binding.frame(), &child.inv)
                .unwrap();
            assert_eq!(f.rows(), rows);
            assert_eq!(
                f.store
                    .audit_records(&child.inv.execution_id, 0, 128)
                    .unwrap(),
                audits
            );
            assert!(
                f.service
                    .selected_batch_owner(child.binding.frame(), &child.inv)
                    .is_err()
            );
            let next = Child::named(&f, "next-child");
            assert_eq!(
                f.service
                    .selected_local_unit(next.binding.frame(), &next.inv, c)
                    .unwrap(),
                MqMqiUnitOfWork::Local { unit }
            );
            assert_eq!(
                f.store
                    .get_provider_state(ownership::UOW_NAMESPACE, &unit.to_string())
                    .unwrap(),
                owner
            );
            let request = if back {
                MqMqiRequest::Back {
                    connection: c,
                    unit,
                }
            } else {
                MqMqiRequest::Commit {
                    connection: c,
                    unit,
                }
            };
            next.call(&f, 30, request);
            assert_eq!(f.depth(), usize::from(!back));
            let new_unit = f.unit();
            assert_ne!(new_unit, unit);
            let row = f
                .store
                .get_provider_state(ownership::UOW_NAMESPACE, &new_unit.to_string())
                .unwrap()
                .unwrap();
            let value: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
            assert_eq!(value["value"]["execution"], f.inv.execution_id.as_str());
            assert_eq!(value["value"]["connection_key"], "child-effect-10");
            next.call(
                &f,
                31,
                MqMqiRequest::Put {
                    connection: c,
                    object: o,
                    put: put(MqMqiUnitOfWork::Local { unit: new_unit }),
                },
            );
            next.call(&f, 32, MqMqiRequest::Disconnect { connection: c });
            // Explicit local MQDISC commits this pending unit; CALL return did not.
            assert_eq!(f.depth(), usize::from(!back) + 1);
            assert!(f.service.selected_local_unit(f.frame, &f.inv, c).is_err());
        }
    }
}
fn open(f: &Fixture, child: &Child, c: MqHconn) -> MqHobj {
    let reply = child.call(
        f,
        11,
        MqMqiRequest::Open(
            MqObjectOpenRequest::new(
                c,
                lookup(),
                &[
                    MqRouteOpenAccess::InputShared,
                    MqRouteOpenAccess::Output,
                    MqRouteOpenAccess::Browse,
                ],
                Default::default(),
            )
            .unwrap(),
        ),
    );
    let MqMqiOutput::Opened {
        object,
        dynamic: None,
    } = output(reply)
    else {
        panic!("child object")
    };
    object
}

#[test]
fn memory_sqlite_child_first_connect_pending_put_return_keeps_parent_handles_and_unit() {
    for sqlite in [false, true] {
        let f = Fixture::new(sqlite);
        let child = Child::new(&f);
        let c = connect(&f, &child);
        let o = open(&f, &child, c);
        let unit = f.unit();
        assert_eq!(
            f.service
                .selected_local_unit(child.binding.frame(), &child.inv, c)
                .unwrap(),
            MqMqiUnitOfWork::Local { unit }
        );
        child.call(
            &f,
            12,
            MqMqiRequest::Put {
                connection: c,
                object: o,
                put: put(MqMqiUnitOfWork::Local { unit }),
            },
        );
        assert_eq!(f.depth(), 0);
        assert_pending(&f, unit);
        let row = f
            .store
            .get_provider_state(ownership::UOW_NAMESPACE, &unit.to_string())
            .unwrap()
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
        assert_eq!(value["value"]["execution"], f.inv.execution_id.as_str());
        assert_eq!(value["value"]["run"], f.inv.run_unit_id.as_str());
        assert_eq!(value["value"]["principal"], f.inv.principal.id().as_str());
        assert_eq!(value["value"]["connection_key"], "child-effect-10");
        let rows = f.rows();
        f.service
            .return_selected_batch_child(child.binding.frame(), &child.inv)
            .unwrap();
        assert_eq!(f.rows(), rows);
        assert_pending(&f, unit);
        f.call(
            20,
            MqMqiRequest::Commit {
                connection: c,
                unit,
            },
        );
        assert_eq!(f.depth(), 1);
        assert_ne!(f.unit(), unit);
        f.call(
            21,
            get(c, o, MqMqiUnitOfWork::NoSyncpoint, 1024, MqGetMode::Remove),
        );
        assert_eq!(f.depth(), 0);
        f.call(22, MqMqiRequest::Disconnect { connection: c });
    }
}
