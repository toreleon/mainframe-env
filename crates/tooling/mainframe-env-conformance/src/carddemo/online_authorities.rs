use super::*;

pub(super) fn install_base_online_authorities(
    server: &ProductServer,
    corpus_dir: &Path,
    definition: &OnlineApplicationDefinition,
) -> Result<(), CorpusProblem> {
    let dataset = server.dataset_service();
    let objects = carddemo_base_seed_objects(corpus_dir)?;
    dataset
        .install_seed_generation("CARDDEMO", "g1", objects.clone())
        .map_err(terminal_problem)?;
    let mutation = |sequence| Mutation {
        sequence,
        idempotency_key: IdempotencyKey::new(
            format!("carddemo-online-index-{sequence}"),
            InvocationLimits::default(),
        )
        .expect("static mutation key"),
        transaction: Some("CARDDEMO-INSTALL".into()),
    };
    for (sequence, (index, base, offset, length)) in [
        (
            "AWS.M2.CARDDEMO.CARDDATA.VSAM.AIX.PATH",
            "AWS.M2.CARDDEMO.CARDDATA.VSAM.KSDS",
            16,
            11,
        ),
        (
            "AWS.M2.CARDDEMO.CARDXREF.VSAM.AIX.PATH",
            "AWS.M2.CARDDEMO.CARDXREF.VSAM.KSDS",
            25,
            11,
        ),
        (
            "AWS.M2.CARDDEMO.TRANSACT.VSAM.AIX.PATH",
            "AWS.M2.CARDDEMO.TRANSACT.VSAM.KSDS",
            304,
            26,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        dataset
            .invoke(DatasetRequest::DefineAlternateIndex {
                base: DatasetName::new(base, 128).expect("static base"),
                index: DatasetName::new(index, 128).expect("static index"),
                key_offset: offset,
                key_length: length,
                allow_duplicates: true,
                upgrade: true,
                mutation: mutation(sequence as u64 + 1),
            })
            .map_err(terminal_problem)?;
    }
    let csd = String::from_utf8(read_corpus_file(
        corpus_dir,
        &corpus_dir.join("app/csd/CARDDEMO.CSD"),
    )?)
    .map_err(|_| CorpusProblem::new("carddemo.online.csd_invalid", "base CSD is not UTF-8"))?;
    let resources = parse_csd(&csd).map_err(package_problem)?;
    let aliases = resources
        .iter()
        .filter(|resource| resource.kind == "FILE")
        .map(|resource| {
            Ok((
                resource.name.clone(),
                DatasetName::new(
                    resource.properties.get("DSNAME").ok_or_else(|| {
                        CorpusProblem::new(
                            "carddemo.online.csd_invalid",
                            format!("{} DSNAME is missing", resource.name),
                        )
                    })?,
                    128,
                )
                .map_err(|_| {
                    CorpusProblem::new(
                        "carddemo.online.csd_invalid",
                        format!("{} DSNAME is invalid", resource.name),
                    )
                })?,
            ))
        })
        .collect::<Result<BTreeMap<_, _>, CorpusProblem>>()?;
    install_online_resources(server, definition, &objects, &aliases)
}

fn install_online_resources(
    server: &ProductServer,
    definition: &OnlineApplicationDefinition,
    objects: &[DatasetSeedObject],
    aliases: &BTreeMap<String, DatasetName>,
) -> Result<(), CorpusProblem> {
    server
        .cics_service()
        .register_file_definitions(
            &aliases
                .iter()
                .map(|(name, dataset)| {
                    (
                        name.clone(),
                        CicsFileDefinition {
                            dataset: dataset.clone(),
                            ccsid: Some(37),
                        },
                    )
                })
                .collect(),
        )
        .map_err(terminal_problem)?;

    server
        .bootstrap_identity("WEBUSER", b"transport-password")
        .map_err(terminal_problem)?;
    server
        .bootstrap_identity("WEBADM", b"admin-transport-password")
        .map_err(terminal_problem)?;
    let racf = server.racf_service();
    for transaction in definition.transactions.keys() {
        let resource = format!("CICS.{transaction}");
        racf.define_profile("TCICSTRN", &resource, "WEBADM", None)
            .map_err(terminal_problem)?;
        racf.permit("TCICSTRN", &resource, "WEBADM", AccessIntent::Execute)
            .map_err(terminal_problem)?;
        if transaction != "CA00"
            && !transaction.starts_with("CU")
            && (!transaction.starts_with("CT")
                || matches!(transaction.as_str(), "CT00" | "CT01" | "CT02"))
        {
            racf.permit("TCICSTRN", &resource, "WEBUSER", AccessIntent::Execute)
                .map_err(terminal_problem)?;
        }
    }
    for program in definition.programs.iter().map(|program| &program.name) {
        let resource = format!("CICS.PROGRAM.{program}");
        racf.define_profile("FACILITY", &resource, "WEBADM", None)
            .map_err(terminal_problem)?;
        racf.permit("FACILITY", &resource, "WEBADM", AccessIntent::Execute)
            .map_err(terminal_problem)?;
        if !program.starts_with("COADM")
            && !program.starts_with("COUSR")
            && !program.starts_with("COTRT")
        {
            racf.permit("FACILITY", &resource, "WEBUSER", AccessIntent::Execute)
                .map_err(terminal_problem)?;
        }
    }
    for name in objects
        .iter()
        .map(|object| object.dataset.as_str())
        .chain(aliases.values().map(DatasetName::as_str))
        .collect::<BTreeSet<_>>()
    {
        racf.define_profile("DATASET", name, "WEBADM", None)
            .map_err(terminal_problem)?;
        racf.permit("DATASET", name, "WEBADM", AccessIntent::Update)
            .map_err(terminal_problem)?;
        racf.permit("DATASET", name, "WEBUSER", AccessIntent::Update)
            .map_err(terminal_problem)?;
    }
    racf.define_profile("QUEUE", "CICS.TD.JOBS", "WEBADM", None)
        .map_err(terminal_problem)?;
    for principal in ["WEBADM", "WEBUSER"] {
        racf.permit("QUEUE", "CICS.TD.JOBS", principal, AccessIntent::Update)
            .map_err(terminal_problem)?;
    }
    Ok(())
}

pub(super) fn install_db2_authorities(server: &ProductServer) -> Result<(), CorpusProblem> {
    let racf = server.racf_service();
    for (table, access) in [
        ("CARDDEMO.TRANSACTION_TYPE", AccessIntent::Update),
        ("CARDDEMO.TRANSACTION_TYPE_CATEGORY", AccessIntent::Update),
        ("SYSIBM.SYSDUMMY1", AccessIntent::Read),
    ] {
        racf.define_profile("DB2TABLE", table, "IBMUSER", None)
            .map_err(terminal_problem)?;
        racf.permit("DB2TABLE", table, "IBMUSER", AccessIntent::Alter)
            .map_err(terminal_problem)?;
        racf.permit("DB2TABLE", table, "WEBADM", access)
            .map_err(terminal_problem)?;
    }
    Ok(())
}

pub(super) fn install_transaction_authorities(
    server: &ProductServer,
    corpus_dir: &Path,
    definition: &OnlineApplicationDefinition,
) -> Result<(), CorpusProblem> {
    let objects = [
        (
            "AWS.M2.CARDDEMO.USRSEC.PS",
            "AWS.M2.CARDDEMO.USRSEC.VSAM.KSDS",
            80,
            8,
        ),
        (
            "AWS.M2.CARDDEMO.CARDXREF.PS",
            "AWS.M2.CARDDEMO.CARDXREF.VSAM.KSDS",
            50,
            16,
        ),
        (
            "AWS.M2.CARDDEMO.DALYTRAN.PS.INIT",
            "AWS.M2.CARDDEMO.TRANSACT.VSAM.KSDS",
            350,
            16,
        ),
    ]
    .into_iter()
    .map(|(source, target, record_length, key_length)| {
        let relative = format!("app/data/EBCDIC/{source}");
        let bytes = read_corpus_file(corpus_dir, &corpus_dir.join(&relative))?;
        Ok(DatasetSeedObject {
            source_id: relative,
            dataset: DatasetName::new(target, 128).map_err(|_| {
                CorpusProblem::new("carddemo.online.seed_invalid", "seed target is invalid")
            })?,
            attributes: DatasetAttributes {
                organization: DatasetOrganization::KeySequenced,
                record_format: RecordFormat::Fixed,
                logical_record_length: record_length,
                key_offset: Some(0),
                key_length: Some(key_length),
                ccsid: Some(37),
            },
            record_length,
            sha256: format!("sha256:{:x}", Sha256::digest(&bytes)),
            bytes,
        })
    })
    .collect::<Result<Vec<_>, CorpusProblem>>()?;
    let dataset = server.dataset_service();
    dataset
        .install_seed_generation("CARDDEMO", "g1", objects.clone())
        .map_err(terminal_problem)?;
    for (sequence, index, base, offset, length) in [
        (
            2,
            "AWS.M2.CARDDEMO.CARDXREF.VSAM.AIX.PATH",
            "AWS.M2.CARDDEMO.CARDXREF.VSAM.KSDS",
            25,
            11,
        ),
        (
            3,
            "AWS.M2.CARDDEMO.TRANSACT.VSAM.AIX.PATH",
            "AWS.M2.CARDDEMO.TRANSACT.VSAM.KSDS",
            304,
            26,
        ),
    ] {
        dataset
            .invoke(DatasetRequest::DefineAlternateIndex {
                base: DatasetName::new(base, 128).expect("static base"),
                index: DatasetName::new(index, 128).expect("static index"),
                key_offset: offset,
                key_length: length,
                allow_duplicates: true,
                upgrade: true,
                mutation: Mutation {
                    sequence,
                    idempotency_key: IdempotencyKey::new(
                        format!("carddemo-online-index-{sequence}"),
                        InvocationLimits::default(),
                    )
                    .expect("static mutation key"),
                    transaction: Some("CARDDEMO-INSTALL".into()),
                },
            })
            .map_err(terminal_problem)?;
    }
    let csd = String::from_utf8(read_corpus_file(
        corpus_dir,
        &corpus_dir.join("app/csd/CARDDEMO.CSD"),
    )?)
    .map_err(|_| CorpusProblem::new("carddemo.online.csd_invalid", "base CSD is not UTF-8"))?;
    let resources = parse_csd(&csd).map_err(package_problem)?;
    let mut aliases = BTreeMap::new();
    for (alias, expected_dataset) in [
        ("USRSEC", "AWS.M2.CARDDEMO.USRSEC.VSAM.KSDS"),
        ("CCXREF", "AWS.M2.CARDDEMO.CARDXREF.VSAM.KSDS"),
        ("CXACAIX", "AWS.M2.CARDDEMO.CARDXREF.VSAM.AIX.PATH"),
        ("TRANSACT", "AWS.M2.CARDDEMO.TRANSACT.VSAM.KSDS"),
    ] {
        let matching = resources
            .iter()
            .filter(|resource| resource.kind == "FILE" && resource.name == alias)
            .collect::<Vec<_>>();
        if matching.len() != 1
            || matching[0].properties.get("DSNAME").map(String::as_str) != Some(expected_dataset)
        {
            return Err(CorpusProblem::new("carddemo.transaction.csd_drift", alias));
        }
        aliases.insert(
            alias.into(),
            DatasetName::new(expected_dataset, 128).expect("static dataset"),
        );
    }
    install_online_resources(server, definition, &objects, &aliases)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_host_api::{
        EnterpriseAuthorizer, EnterpriseResource, EnterpriseResourceClass,
    };

    /// Issue #215: maintenance tables admit WEBADM while WEBUSER stays denied.
    #[test]
    fn db2_maintenance_authorities_allow_admin_and_deny_regular_user() {
        let artifact_root = env::temp_dir().join(format!(
            "mainframe-env-db2-authorities-{}",
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
            server.bootstrap_user("IBMUSER", b"TESTPASS").unwrap();
            for user in ["WEBADM", "WEBUSER"] {
                server.bootstrap_identity(user, b"test-password").unwrap();
            }
            install_db2_authorities(&server).unwrap();
            let racf = server.racf_service();
            let principal = |user| PrincipalId::new(user, InvocationLimits::default()).unwrap();
            for table in [
                "CARDDEMO.TRANSACTION_TYPE",
                "CARDDEMO.TRANSACTION_TYPE_CATEGORY",
            ] {
                for intent in [AccessIntent::Read, AccessIntent::Update] {
                    let resource =
                        EnterpriseResource::new(EnterpriseResourceClass::Db2Table, table, intent)
                            .unwrap();
                    assert_eq!(
                        EnterpriseAuthorizer::authorize(&*racf, &principal("WEBADM"), &resource),
                        Ok(()),
                        "WEBADM {intent:?} {table}"
                    );
                    assert_eq!(
                        EnterpriseAuthorizer::authorize(&*racf, &principal("WEBUSER"), &resource),
                        Err(HostProblem::Unauthorized)
                    );
                    assert_eq!(
                        EnterpriseAuthorizer::authorize(&*racf, &principal("IBMUSER"), &resource),
                        Ok(()),
                        "batch installation must retain table access"
                    );
                }
            }
            for (user, intent, expected) in [
                ("WEBADM", AccessIntent::Read, Ok(())),
                (
                    "WEBADM",
                    AccessIntent::Update,
                    Err(HostProblem::Unauthorized),
                ),
                (
                    "WEBUSER",
                    AccessIntent::Read,
                    Err(HostProblem::Unauthorized),
                ),
                ("IBMUSER", AccessIntent::Read, Ok(())),
            ] {
                let resource = EnterpriseResource::new(
                    EnterpriseResourceClass::Db2Table,
                    "SYSIBM.SYSDUMMY1",
                    intent,
                )
                .unwrap();
                assert_eq!(
                    EnterpriseAuthorizer::authorize(&*racf, &principal(user), &resource),
                    expected,
                    "{user} {intent:?} SYSIBM.SYSDUMMY1"
                );
            }
            let unrelated = EnterpriseResource::new(
                EnterpriseResourceClass::Db2Table,
                "CARDDEMO.OTHER",
                AccessIntent::Read,
            )
            .unwrap();
            assert_eq!(
                EnterpriseAuthorizer::authorize(&*racf, &principal("WEBADM"), &unrelated),
                Err(HostProblem::Unauthorized)
            );
            assert!(server.graceful_shutdown().await);
        });
        let _ = fs::remove_dir_all(artifact_root);
    }
}
