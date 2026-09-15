use super::*;

pub(super) fn seed_control_library(
    server: &ProductServer,
    name: &str,
    members: Vec<(String, Vec<Vec<u8>>)>,
    sequence: &mut u64,
) -> Result<(), CorpusProblem> {
    let dataset = DatasetName::new(name, 128)
        .map_err(|_| CorpusProblem::new("carddemo.utility.dataset", "dataset name is invalid"))?;
    let mut definition =
        mainframe_env_host_api::DatasetDefinition::compatibility(DatasetAttributes {
            organization: DatasetOrganization::Partitioned,
            record_format: RecordFormat::Variable,
            logical_record_length: 4_096,
            key_offset: None,
            key_length: None,
            ccsid: Some(1208),
        });
    definition.allocation.directory_blocks = u32::try_from(members.len().div_ceil(6).max(1))
        .map_err(|_| CorpusProblem::new("carddemo.utility.dataset", "too many control members"))?;
    let idempotency_key = IdempotencyKey::new(
        format!("utility-create-{sequence}"),
        InvocationLimits::default(),
    )
    .map_err(|_| CorpusProblem::new("carddemo.utility.dataset", "mutation is invalid"))?;
    server
        .dataset_service()
        .invoke(DatasetRequest::Define {
            dataset,
            definition: Box::new(definition),
            mutation: Mutation {
                sequence: *sequence,
                idempotency_key,
                transaction: Some("CD-021".into()),
            },
        })
        .map_err(terminal_problem)?;
    *sequence += 1;
    for (member, records) in members {
        utility_write_dataset(server, name, Some(&member), records, sequence)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Issue #201: DB2 control-library seeding must fit all seven members.
    #[test]
    fn db2_control_library_seeds_all_seven_members() {
        let artifact_root = env::temp_dir().join(format!(
            "mainframe-env-control-library-{}",
            std::process::id()
        ));
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let server = ProductServer::open(
                ServerConfig {
                    store_profile: StoreProfile::Memory,
                    artifact_root: artifact_root.clone(),
                    tls: TlsConfig {
                        enabled: false,
                        certificate_path: None,
                        private_key_reference: None,
                    },
                    ..ServerConfig::default()
                },
                Arc::new(MemoryStore::new(Default::default())),
                Arc::new(MemorySecretResolver::default()),
                default_program_router(),
            )
            .unwrap();
            let expected = (1..=7).map(|n| format!("CTRL{n:04}")).collect::<Vec<_>>();
            let members = expected
                .iter()
                .map(|name| (name.clone(), vec![format!("CONTROL {name}").into_bytes()]))
                .collect();
            seed_control_library(&server, "AWS.M2.CARDDEMO.CNTL", members, &mut 40_000)
                .expect("all seven control members fit");
            let result = server
                .dataset_service()
                .invoke(DatasetRequest::ListMembers {
                    dataset: DatasetName::new("AWS.M2.CARDDEMO.CNTL", 128).unwrap(),
                    start: None,
                    max_items: 8,
                })
                .unwrap();
            let DatasetResult::Members { names, more } = result else {
                panic!("expected control members");
            };
            assert!(!more);
            assert_eq!(
                names.iter().map(|name| name.as_str()).collect::<Vec<_>>(),
                expected
            );
            assert!(server.graceful_shutdown().await);
        });
        let _ = fs::remove_dir_all(artifact_root);
    }
}
