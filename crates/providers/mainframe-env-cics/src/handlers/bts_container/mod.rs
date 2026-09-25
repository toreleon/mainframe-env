//! Private, owner-scoped command-data read port for BTS and task channels.

mod browse_read;
mod scope;
mod state;

pub(super) use browse_read::{ContainerReadError, ReadReply, ReadRequest};
pub(super) use scope::ContainerSelector;

use crate::service::{CicsService, Run};
use mainframe_env_host_api::AccessIntent;

impl CicsService {
    /// Read command data only for this authenticated invocation. The nested
    /// SAF request persists its allow/deny audit through the shared host path.
    pub(in crate::service::handlers) fn read_bts_container(
        &self,
        run: &mut Run,
        selector: ContainerSelector<'_>,
        request: ReadRequest<'_>,
    ) -> Result<ReadReply, ContainerReadError> {
        let run_unit = run.invocation.run_unit_id.as_str().to_owned();
        let execution = run.invocation.execution_id.as_str().to_owned();
        let principal = run.invocation.principal.id().as_str().to_owned();
        let identity = scope::OwnerIdentity {
            run_unit: &run_unit,
            execution: &execution,
            principal: &principal,
        };
        let mut authorize =
            |class: &str, resource: &str| self.authorize(run, class, resource, AccessIntent::Read);
        browse_read::ReadPort::new(self.store.as_ref(), identity, &mut authorize)
            .read(selector, request)
    }
}

#[cfg(test)]
mod tests {
    use super::{browse_read::*, scope::*, state::*};
    use crate::service::handlers::bts_lifecycle::{BtsLifecycleStore, BtsProcess};
    use mainframe_env_store::{MemoryStore, SqliteStateStore};
    use mainframe_env_store_api::ProviderStateStore;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_SQLITE: AtomicU64 = AtomicU64::new(1);

    fn define(store: &dyn ProviderStateStore, uow: &str) -> String {
        let lifecycle = BtsLifecycleStore::new(store);
        let root = BtsLifecycleStore::root_id("TYPE", "ORDER", uow).unwrap();
        lifecycle
            .define_process(
                BtsProcess::new("TYPE", "ORDER", &root, "MAIN", "BTS1", "USER", uow).unwrap(),
                uow,
                "EXEC",
                "USER",
            )
            .unwrap();
        root
    }

    fn owner<'a>(uow: &'a str) -> OwnerIdentity<'a> {
        OwnerIdentity {
            run_unit: uow,
            execution: "EXEC",
            principal: "USER",
        }
    }

    fn seed_process(store: &dyn ProviderStateStore, root: &str, name: &str, bytes: &[u8]) {
        let scope = ContainerOwner::Process {
            process_type: "TYPE".into(),
            process_name: "ORDER".into(),
            root_activity_id: root.into(),
        };
        store
            .put_provider_state(
                container_record(
                    &scope,
                    name,
                    &ContainerValue {
                        datatype: ContainerDatatype::Character,
                        ccsid: Some(37),
                        read_only: false,
                        bytes: bytes.into(),
                    },
                )
                .unwrap(),
                None,
            )
            .unwrap();
    }

    #[test]
    fn bts_browse_read_port_lists_exists_and_reads_metadata() {
        let store = MemoryStore::new(Default::default());
        let root = define(&store, "UOW1");
        seed_process(&store, &root, "ZETA", b"z");
        seed_process(&store, &root, "ALPHA", b"a");
        let audit = std::cell::RefCell::new(Vec::new());
        let mut authorize = |class: &str, resource: &str| {
            audit
                .borrow_mut()
                .push((class.to_owned(), resource.to_owned()));
            Ok(())
        };
        let mut port = ReadPort::new(&store, owner("UOW1"), &mut authorize);
        let target = ContainerSelector::Process {
            process_type: "TYPE",
            process_name: "ORDER",
            epoch: 1,
        };
        assert_eq!(
            port.read(target, ReadRequest::Names { max: 2 }).unwrap(),
            ReadReply::Names(vec!["ALPHA".into(), "ZETA".into()])
        );
        assert_eq!(
            port.read(target, ReadRequest::Exists("MISSING")).unwrap(),
            ReadReply::Exists(false)
        );
        assert_eq!(
            port.read(target, ReadRequest::Value("ALPHA")).unwrap(),
            ReadReply::Value(Some(ContainerValue {
                datatype: ContainerDatatype::Character,
                ccsid: Some(37),
                read_only: false,
                bytes: b"a".to_vec()
            }))
        );
        assert_eq!(audit.borrow().len(), 3);
        assert!(audit.borrow().iter().all(|(class, _)| class == "BTSLIFE"));
    }

    #[test]
    fn bts_container_cross_owner_and_stale_epoch_are_typed_denials() {
        let store = MemoryStore::new(Default::default());
        let root = define(&store, "UOW1");
        seed_process(&store, &root, "SECRET", b"private");
        let mut allow = |_: &str, _: &str| Ok(());
        let target = ContainerSelector::Process {
            process_type: "TYPE",
            process_name: "ORDER",
            epoch: 1,
        };
        let mut other = ReadPort::new(
            &store,
            OwnerIdentity {
                principal: "OTHER",
                ..owner("UOW1")
            },
            &mut allow,
        );
        assert_eq!(
            other.read(target, ReadRequest::Value("SECRET")),
            Err(ContainerReadError::Unauthorized)
        );
        BtsLifecycleStore::new(&store)
            .finish_uow("UOW1", "EXEC", "USER", true)
            .unwrap();
        let mut stale = ReadPort::new(&store, owner("UOW1"), &mut allow);
        assert_eq!(
            stale.read(target, ReadRequest::Value("SECRET")),
            Err(ContainerReadError::StaleEpoch)
        );
        BtsLifecycleStore::new(&store)
            .acquire("UOW1", "EXEC", "USER", "TYPE", "ORDER", &root)
            .unwrap();
        assert_eq!(
            stale.read(target, ReadRequest::Value("SECRET")),
            Err(ContainerReadError::StaleEpoch)
        );
    }

    #[test]
    fn bts_container_names_are_bounded() {
        let store = MemoryStore::new(Default::default());
        let root = define(&store, "UOW1");
        seed_process(&store, &root, "A", b"a");
        seed_process(&store, &root, "B", b"b");
        let mut allow = |_: &str, _: &str| Ok(());
        let mut port = ReadPort::new(&store, owner("UOW1"), &mut allow);
        let target = ContainerSelector::Process {
            process_type: "TYPE",
            process_name: "ORDER",
            epoch: 1,
        };
        assert_eq!(
            port.read(target, ReadRequest::Names { max: 1 }),
            Err(ContainerReadError::Bounds)
        );
        assert_eq!(
            port.read(target, ReadRequest::Names { max: 0 }),
            Err(ContainerReadError::Bounds)
        );
    }

    #[test]
    fn bts_container_maximum_binary_payload_fits_bounded_row() {
        let owner = ContainerOwner::Channel {
            execution: "EXEC".into(),
            principal: "USER".into(),
            run_unit: "UOW1".into(),
            channel: "INPUT".into(),
        };
        let value = ContainerValue {
            datatype: ContainerDatatype::Bit,
            ccsid: None,
            read_only: false,
            bytes: vec![0xff; 65_536],
        };
        let row = container_record(&owner, "PAYLOAD", &value).unwrap();
        assert!(row.payload.len() < 262_144);
        assert_eq!(decode_container(&row, &owner).unwrap(), value);
        let mut oversized = value;
        oversized.bytes.push(0xff);
        assert_eq!(
            container_record(&owner, "PAYLOAD", &oversized),
            Err(mainframe_env_host_api::HostProblem::Malformed)
        );
    }

    #[test]
    fn bts_container_task_channel_does_not_read_another_owner() {
        let store = MemoryStore::new(Default::default());
        let channel_owner = ContainerOwner::Channel {
            execution: "EXEC".into(),
            principal: "USER".into(),
            run_unit: "UOW1".into(),
            channel: "INPUT".into(),
        };
        store
            .put_provider_state(channel_record(&channel_owner).unwrap(), None)
            .unwrap();
        store
            .put_provider_state(
                container_record(
                    &channel_owner,
                    "PAYLOAD",
                    &ContainerValue {
                        datatype: ContainerDatatype::Bit,
                        ccsid: None,
                        read_only: true,
                        bytes: b"owned".to_vec(),
                    },
                )
                .unwrap(),
                None,
            )
            .unwrap();
        let audit = std::cell::RefCell::new(Vec::new());
        let mut authorize = |class: &str, resource: &str| {
            audit
                .borrow_mut()
                .push((class.to_owned(), resource.to_owned()));
            Ok(())
        };
        let mut port = ReadPort::new(&store, owner("UOW1"), &mut authorize);
        assert_eq!(
            port.read(
                ContainerSelector::Channel("INPUT"),
                ReadRequest::Names { max: 256 }
            )
            .unwrap(),
            ReadReply::Names(vec!["PAYLOAD".into()])
        );
        let mut other = ReadPort::new(
            &store,
            OwnerIdentity {
                principal: "OTHER",
                ..owner("UOW1")
            },
            &mut authorize,
        );
        assert_eq!(
            other.read(
                ContainerSelector::Channel("INPUT"),
                ReadRequest::Value("PAYLOAD")
            ),
            Err(ContainerReadError::NotFound)
        );
        assert_eq!(
            *audit.borrow(),
            vec![
                ("CICSCHAN".into(), "CICS.CHANNEL.INPUT".into()),
                ("CICSCHAN".into(), "CICS.CHANNEL.INPUT".into()),
            ]
        );
    }

    #[test]
    fn bts_browse_process_and_activity_lanes_are_distinct() {
        let store = MemoryStore::new(Default::default());
        let root = define(&store, "UOW1");
        let activity_owner = ContainerOwner::Activity {
            process_type: "TYPE".into(),
            process_name: "ORDER".into(),
            root_activity_id: root.clone(),
            activity_id: root.clone(),
        };
        store
            .put_provider_state(
                container_record(
                    &activity_owner,
                    "LOCAL",
                    &ContainerValue {
                        datatype: ContainerDatatype::Bit,
                        ccsid: None,
                        read_only: false,
                        bytes: b"activity".to_vec(),
                    },
                )
                .unwrap(),
                None,
            )
            .unwrap();
        let mut allow = |_: &str, _: &str| Ok(());
        let mut port = ReadPort::new(&store, owner("UOW1"), &mut allow);
        assert_eq!(
            port.read(
                ContainerSelector::Process {
                    process_type: "TYPE",
                    process_name: "ORDER",
                    epoch: 1,
                },
                ReadRequest::Names { max: 256 },
            )
            .unwrap(),
            ReadReply::Names(vec![])
        );
        assert_eq!(
            port.read(
                ContainerSelector::Activity {
                    process_type: "TYPE",
                    process_name: "ORDER",
                    activity_id: &root,
                    epoch: 1,
                },
                ReadRequest::Names { max: 256 },
            )
            .unwrap(),
            ReadReply::Names(vec!["LOCAL".into()])
        );
    }

    #[test]
    fn bts_container_authorization_denial_precedes_contents() {
        let store = MemoryStore::new(Default::default());
        let root = define(&store, "UOW1");
        seed_process(&store, &root, "SECRET", b"private");
        let mut calls = 0;
        let mut deny = |_: &str, _: &str| {
            calls += 1;
            Err(mainframe_env_host_api::HostProblem::Unauthorized)
        };
        let mut port = ReadPort::new(&store, owner("UOW1"), &mut deny);
        assert_eq!(
            port.read(
                ContainerSelector::Process {
                    process_type: "TYPE",
                    process_name: "ORDER",
                    epoch: 1
                },
                ReadRequest::Value("SECRET")
            ),
            Err(ContainerReadError::Unauthorized)
        );
        assert_eq!(calls, 1);
    }

    #[test]
    fn bts_container_contract_fences_process_and_container_rows_together() {
        let store = MemoryStore::new(Default::default());
        let old_root = define(&store, "UOW1");
        seed_process(&store, &old_root, "SECRET", b"old");
        BtsLifecycleStore::new(&store)
            .finish_uow("UOW1", "EXEC", "USER", false)
            .unwrap();
        let new_root = define(&store, "UOW2");
        assert_ne!(old_root, new_root);
        let mut allow = |_: &str, _: &str| Ok(());
        let mut port = ReadPort::new(&store, owner("UOW2"), &mut allow);
        let target = ContainerSelector::Process {
            process_type: "TYPE",
            process_name: "ORDER",
            epoch: 1,
        };
        assert_eq!(
            port.read(target, ReadRequest::Names { max: 256 }).unwrap(),
            ReadReply::Names(vec![])
        );
        assert_eq!(
            port.read(target, ReadRequest::Value("SECRET")).unwrap(),
            ReadReply::Value(None)
        );
    }

    #[test]
    fn bts_browse_read_port_survives_sqlite_reopen() {
        let directory = std::env::temp_dir().join(format!(
            "mainframe-env-bts-container-read-{}-{}",
            std::process::id(),
            NEXT_SQLITE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let url = format!("sqlite://{}?mode=rwc", directory.join("state.db").display());
        {
            let store = SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap();
            let root = define(&store, "UOW1");
            seed_process(&store, &root, "ALPHA", b"retained");
        }
        {
            let store = SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap();
            let mut allow = |_: &str, _: &str| Ok(());
            let mut port = ReadPort::new(&store, owner("UOW1"), &mut allow);
            let target = ContainerSelector::Process {
                process_type: "TYPE",
                process_name: "ORDER",
                epoch: 1,
            };
            assert_eq!(
                port.read(target, ReadRequest::Names { max: 256 }).unwrap(),
                ReadReply::Names(vec!["ALPHA".into()])
            );
        }
        std::fs::remove_dir_all(directory).unwrap();
    }
}
