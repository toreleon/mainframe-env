//! Actual transaction comparisons and their private receipt observations.
use super::*;
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};

const TRANSACT: &str = "AWS.M2.CARDDEMO.TRANSACT.VSAM.KSDS";
const TRANSACT_AIX: &str = "AWS.M2.CARDDEMO.TRANSACT.VSAM.AIX.PATH";
const SNAPSHOT_DATASETS: [&str; 5] = [
    TRANSACT,
    TRANSACT_AIX,
    "AWS.M2.CARDDEMO.USRSEC.VSAM.KSDS",
    "AWS.M2.CARDDEMO.CARDXREF.VSAM.KSDS",
    "AWS.M2.CARDDEMO.CARDXREF.VSAM.AIX.PATH",
];
pub(super) type RawRows = (Vec<Vec<u8>>, Vec<Vec<u8>>, u64);

struct ArtifactDirectory {
    path: PathBuf,
    remove_on_drop: bool,
}

impl ArtifactDirectory {
    fn create() -> Result<Self, CorpusProblem> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let parent = env::var_os("CARDDEMO_TRANSACTIONS_ARTIFACT_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| env::temp_dir().join("mainframe-env-carddemo-transactions"));
        fs::create_dir_all(&parent).map_err(|error| {
            CorpusProblem::new("carddemo.transaction.artifact_directory", error.to_string())
        })?;
        let child = parent.join(format!(
            "fixture-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        // Never reuse or delete an existing directory belonging to another fixture.
        fs::create_dir(&child).map_err(|error| {
            CorpusProblem::new("carddemo.transaction.artifact_directory", error.to_string())
        })?;
        Ok(Self {
            path: child,
            remove_on_drop: true,
        })
    }
}

impl Drop for ArtifactDirectory {
    fn drop(&mut self) {
        if self.remove_on_drop {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

pub(super) struct TransactionFixture {
    pub(super) app: axum::Router,
    pub(super) server: Arc<ProductServer>,
    // Drop the router and server before removing this fixture's artifact directory.
    _artifacts: ArtifactDirectory,
    output: Option<PathBuf>,
}

impl TransactionFixture {
    /// Await the actual server stop before allowing its owned artifact directory to be removed.
    pub(super) async fn finish<T>(
        mut self,
        result: Result<T, CorpusProblem>,
    ) -> Result<T, CorpusProblem> {
        let stopped = self.server.graceful_shutdown().await;
        self._artifacts.remove_on_drop = stopped;
        let exported = self.export_json(
            "shutdown.json",
            &(
                stopped,
                result
                    .as_ref()
                    .err()
                    .map(|problem| (&problem.code, &problem.detail)),
            ),
        );
        // Preserve the original operation error even if teardown or export also fails.
        result.and_then(|value| {
            require(!stopped, "transaction fixture shutdown did not complete")?;
            exported?;
            Ok(value)
        })
    }

    #[cfg(test)]
    pub(super) fn artifact_path(&self) -> &Path {
        &self._artifacts.path
    }

    pub(super) fn export_case(
        &self,
        response: &serde_json::Value,
        before: &[RawRows],
        after: &[RawRows],
        trace: &[mainframe_env_cics::CicsTraceEntry],
    ) -> Result<(), CorpusProblem> {
        if self.output.is_none() {
            return Ok(());
        }
        let actual_trace = trace
            .iter()
            .map(|entry| {
                (
                    format!("{:?}", entry.operation),
                    &entry.outcome,
                    entry.response,
                    entry.response2,
                    entry.payload_bytes,
                )
            })
            .collect::<Vec<_>>();
        self.export_json("screen.json", response)?;
        self.export_json("before.json", before)?;
        self.export_json("after.json", after)?;
        self.export_json("trace.json", &actual_trace)
    }

    fn export_bytes(&self, name: &str, bytes: &[u8]) -> Result<(), CorpusProblem> {
        let Some(directory) = &self.output else {
            return Ok(());
        };
        require(
            bytes.len() > 4 * 1024 * 1024,
            "compiled debug payload exceeds bounded size",
        )?;
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(directory.join(name))
            .and_then(|mut file| file.write_all(bytes))
            .map_err(|error| {
                CorpusProblem::new("carddemo.transaction.debug_export", error.to_string())
            })
    }

    pub(super) fn export_json<T: Serialize + ?Sized>(
        &self,
        name: &str,
        actual: &T,
    ) -> Result<(), CorpusProblem> {
        let Some(directory) = &self.output else {
            return Ok(());
        };
        let bytes = serde_json::to_vec(actual).map_err(|error| {
            CorpusProblem::new("carddemo.transaction.debug_export", error.to_string())
        })?;
        require(
            bytes.len() > 4 * 1024 * 1024,
            "transaction debug export exceeds bounded file size",
        )?;
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(directory.join(name))
            .and_then(|mut file| file.write_all(&bytes))
            .map_err(|error| {
                CorpusProblem::new("carddemo.transaction.debug_export", error.to_string())
            })
    }
    pub(super) async fn open_with_transaction_rows(
        rows: Vec<Vec<u8>>,
    ) -> Result<Self, CorpusProblem> {
        require(
            rows.is_empty() || rows.len() > 2 || rows.iter().any(|row| row.len() != 350),
            "fixture requires one or two complete 350-byte rows",
        )?;
        let keys = rows.iter().map(|row| &row[..16]).collect::<BTreeSet<_>>();
        require(
            keys.len() != rows.len(),
            "fixture primary keys are duplicated",
        )?;
        Self::open_selected_rows(rows, false).await
    }

    pub(super) async fn open_navigation_rows(rows: Vec<Vec<u8>>) -> Result<Self, CorpusProblem> {
        require(
            rows.is_empty() || rows.len() > 12 || rows.iter().any(|row| row.len() != 350),
            "navigation fixture requires one to twelve complete 350-byte rows",
        )?;
        let keys = rows.iter().map(|row| &row[..16]).collect::<BTreeSet<_>>();
        require(
            keys.len() != rows.len(),
            "fixture primary keys are duplicated",
        )?;
        Self::open_selected_rows(rows, true).await
    }

    async fn open_selected_rows(
        rows: Vec<Vec<u8>>,
        navigation: bool,
    ) -> Result<Self, CorpusProblem> {
        let corpus = env::var_os(CORPUS_ENV).ok_or_else(|| {
            CorpusProblem::new(
                "carddemo.corpus.environment_missing",
                "CARDDEMO_CORPUS_DIR is required",
            )
        })?;
        let corpus = Path::new(&corpus);
        let definition = if navigation {
            online_definition::navigation_online_definition(corpus)?
        } else {
            online_definition::transaction_online_definition(corpus)?
        };
        let mut artifacts = ArtifactDirectory::create()?;
        let output = create_output_directory(&artifacts.path)?;
        let server = ProductServer::open(
            ServerConfig {
                store_profile: StoreProfile::Memory,
                artifact_root: artifacts.path.clone(),
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
        .map_err(terminal_problem)?;
        // Unexpected drop/cancellation retains artifacts rather than deleting under a live server.
        artifacts.remove_on_drop = false;
        let fixture = Self {
            app: server.router(),
            server,
            _artifacts: artifacts,
            output,
        };
        let initialized = (|| {
            if navigation {
                for program in &definition.programs {
                    fixture.export_bytes(
                        &format!("compiled-{}.bin", program.name),
                        &program.payload,
                    )?;
                    fixture.export_json(
                        &format!("compiled-{}.json", program.name),
                        &(
                            &program.name,
                            format!("{:?}", program.artifact),
                            format!("{:?}", program.manifest),
                            &program.semantic_identity,
                            format!("{:x}", Sha256::digest(&program.payload)),
                        ),
                    )?;
                }
            }
            fixture.initialize(corpus, definition, rows)
        })();
        match initialized {
            Ok(()) => Ok(fixture),
            Err(problem) => fixture.finish(Err(problem)).await,
        }
    }

    fn initialize(
        &self,
        corpus: &Path,
        definition: OnlineApplicationDefinition,
        rows: Vec<Vec<u8>>,
    ) -> Result<(), CorpusProblem> {
        online_authorities::install_transaction_authorities(&self.server, corpus, &definition)?;
        self.server
            .install_online_application(definition)
            .map_err(terminal_problem)?;
        let mutation = |sequence| Mutation {
            sequence,
            idempotency_key: IdempotencyKey::new(
                format!("carddemo-transaction-fixture-{sequence}"),
                InvocationLimits::default(),
            )
            .expect("static fixture mutation"),
            transaction: Some("CARDDEMO-TRANSACTION-FIXTURE".into()),
        };
        let dataset = DatasetName::new(TRANSACT, 128).expect("static transaction dataset");
        let before = raw_rows(&self.server, TRANSACT)?;
        self.server
            .dataset_service()
            .invoke(DatasetRequest::Truncate {
                dataset: dataset.clone(),
                expected_version: Some(before.2),
                mutation: mutation(4),
            })
            .map_err(terminal_problem)?;
        let truncated = raw_rows(&self.server, TRANSACT)?;
        self.server
            .dataset_service()
            .invoke(DatasetRequest::Write {
                dataset,
                member: None,
                records: rows.clone(),
                expected_version: Some(truncated.2),
                mutation: mutation(5),
            })
            .map_err(terminal_problem)?;
        let actual = raw_rows(&self.server, TRANSACT)?;
        let expected_identities = rows
            .iter()
            .map(|row| row[..16].to_vec())
            .collect::<Vec<_>>();
        let mut index_rows = rows.clone();
        index_rows.sort_by(|left, right| {
            left[304..330]
                .cmp(&right[304..330])
                .then_with(|| left[..16].cmp(&right[..16]))
        });
        let index_identities = index_rows
            .iter()
            .map(|row| row[..16].to_vec())
            .collect::<Vec<_>>();
        require(
            actual != (rows, expected_identities, 3),
            "fixture raw rows, identities or version differ",
        )?;
        require(
            raw_rows(&self.server, TRANSACT_AIX)? != (index_rows, index_identities, 3),
            "fixture raw index rows, identities or version differ",
        )?;
        Ok(())
    }
}

fn create_output_directory(artifact_path: &Path) -> Result<Option<PathBuf>, CorpusProblem> {
    let Some(root) = env::var_os("CARDDEMO_TRANSACTIONS_OUTPUT_ROOT") else {
        return Ok(None);
    };
    let root = PathBuf::from(root);
    fs::create_dir_all(&root).map_err(|error| {
        CorpusProblem::new("carddemo.transaction.debug_export", error.to_string())
    })?;
    let name = artifact_path.file_name().ok_or_else(|| {
        CorpusProblem::new(
            "carddemo.transaction.debug_export",
            "owned fixture name is missing",
        )
    })?;
    let child = root.join(name);
    fs::create_dir(&child).map_err(|error| {
        CorpusProblem::new("carddemo.transaction.debug_export", error.to_string())
    })?;
    Ok(Some(child))
}

pub(super) fn bind_comparison(
    observations: &mut RouteObservations,
    requirement: &str,
    result: Result<(), CorpusProblem>,
) -> Result<(), CorpusProblem> {
    observations.compare(
        journey_closure::AuthorityKind::Journey,
        "CD.J06",
        requirement,
        result.is_err(),
        || {
            result.err().ok_or_else(|| {
                CorpusProblem::new(
                    "carddemo.transaction.comparison_missing",
                    "refusal has no comparison error",
                )
            })
        },
    )
}

fn require(refused: bool, detail: &str) -> Result<(), CorpusProblem> {
    if refused {
        return Err(CorpusProblem::new(
            "carddemo.transaction.observation_mismatch",
            detail,
        ));
    }
    Ok(())
}

fn raw_rows(server: &ProductServer, name: &str) -> Result<RawRows, CorpusProblem> {
    match server
        .dataset_service()
        .invoke(DatasetRequest::Read {
            dataset: DatasetName::new(name, 128).expect("static dataset"),
            member: None,
            key: None,
            max_records: 4096,
            control: Default::default(),
        })
        .map_err(terminal_problem)?
    {
        DatasetResult::Records {
            records,
            identities,
            version,
        } => Ok((records, identities, version)),
        _ => Err(CorpusProblem::new(
            "carddemo.transaction.rows_missing",
            name,
        )),
    }
}

pub(super) fn snapshot(server: &ProductServer) -> Result<Vec<RawRows>, CorpusProblem> {
    SNAPSHOT_DATASETS
        .into_iter()
        .map(|name| raw_rows(server, name))
        .collect()
}

// Literal CVTRA05Y fields; no date, decimal, program or provider result supplies expectations.
fn expected_record(
    key: &str,
    description: &str,
    origin: &str,
    processing: &str,
) -> Result<Vec<u8>, CorpusProblem> {
    let mut text = String::new();
    for (literal, width) in [
        (key, 16),
        ("01", 2),
        ("0001", 4),
        ("ONLINE", 10),
        (description, 100),
        ("0000000010{", 11),
        ("123456789", 9),
        ("BOUNDARY SHOP", 50),
        ("BOSTON", 50),
        ("02110", 10),
        ("0500024453765740", 16),
        (origin, 26),
        (processing, 26),
        ("", 20),
    ] {
        require(
            literal.len() > width,
            "literal record field exceeds its independent width",
        )?;
        text.push_str(literal);
        text.extend(std::iter::repeat_n(' ', width - literal.len()));
    }
    CodePage::Cp037.encode(&text, 350).map_err(|error| {
        CorpusProblem::new(
            "carddemo.transaction.expectation_encoding",
            error.to_string(),
        )
    })
}

fn fields(origin: &str, processing: &str) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("ACTIDIN".into(), "00000000050".into()),
        ("TTYPCD".into(), "01".into()),
        ("TCATCD".into(), "0001".into()),
        ("TRNSRC".into(), "ONLINE".into()),
        ("TDESC".into(), "DATE BOUNDARY PURCHASE".into()),
        ("TRNAMT".into(), "+00000001.00".into()),
        ("TORIGDT".into(), origin.into()),
        ("TPROCDT".into(), processing.into()),
        ("MID".into(), "123456789".into()),
        ("MNAME".into(), "BOUNDARY SHOP".into()),
        ("MCITY".into(), "BOSTON".into()),
        ("MZIP".into(), "02110".into()),
        ("CONFIRM".into(), "Y".into()),
    ])
}

fn compare_message(response: &serde_json::Value, expected: &str) -> Result<(), CorpusProblem> {
    require_online_mapset(response, "COTRN02", "transaction observation")?;
    let screen = online_screen_fields(response)?;
    let mut ascii = expected.as_bytes().to_vec();
    ascii.resize(78, b' ');
    let ebcdic = CodePage::Cp037
        .encode(std::str::from_utf8(&ascii).expect("literal ASCII"), 78)
        .map_err(|error| {
            CorpusProblem::new(
                "carddemo.transaction.expectation_encoding",
                error.to_string(),
            )
        })?;
    require(
        screen
            .get("ERRMSG")
            .is_none_or(|actual| actual != &ascii && actual != &ebcdic),
        expected,
    )
}

pub(super) async fn compare_date(
    origin: &str,
    processing: &str,
    refusal: Option<&str>,
) -> Result<(), CorpusProblem> {
    let baseline = expected_record(
        "0000000000000041",
        "BASELINE TRANSACTION",
        "2026-02-28",
        "2026-02-28",
    )?;
    let fixture = TransactionFixture::open_with_transaction_rows(vec![baseline.clone()]).await?;
    let result = async {
        let route = select_regular_option(&fixture.server, &fixture.app, 8, "COTRN02").await?;
        let before = snapshot(&fixture.server)?;
        let trace_before = fixture
            .server
            .online_trace(&route.session)
            .map_err(terminal_problem)?
            .len();
        let response = carddemo_terminal_exchange(
            &fixture.app,
            &route.session,
            &route.headers,
            0x7d,
            fields(origin, processing),
        )
        .await?;
        let after = snapshot(&fixture.server)?;
        let trace = fixture
            .server
            .online_trace(&route.session)
            .map_err(terminal_problem)?;
        fixture.export_case(&response, &before, &after, &trace)?;
        let observed = &trace[trace_before..];
        if let Some(message) = refusal {
            compare_message(&response, message)?;
            require(
                after != before,
                "invalid date changed raw dataset/index rows, identities or versions",
            )?;
            require(
                !observed.iter().any(|entry| {
                    entry.operation == CicsOperation::Read && entry.outcome == "NORMAL"
                }),
                "invalid date did not exercise actual account lookup",
            )?;
            require(
                observed
                    .iter()
                    .any(|entry| entry.operation == CicsOperation::Write),
                "invalid date reached WRITE",
            )?;
        } else {
            compare_message(
                &response,
                "Transaction added successfully.  Your Tran ID is 0000000000000042.",
            )?;
            let expected = expected_record(
                "0000000000000042",
                "DATE BOUNDARY PURCHASE",
                origin,
                processing,
            )?;
            let baseline_id = CodePage::Cp037
                .encode("0000000000000041", 16)
                .expect("literal key");
            let expected_id = CodePage::Cp037
                .encode("0000000000000042", 16)
                .expect("literal key");
            require(
                after[0]
                    != (
                        vec![baseline.clone(), expected.clone()],
                        vec![baseline_id.clone(), expected_id.clone()],
                        4,
                    ),
                "valid date raw primary rows, identities or version differ",
            )?;
            let index_expected = if processing < "2026-02-28" {
                (vec![expected, baseline], vec![expected_id, baseline_id], 4)
            } else {
                (vec![baseline, expected], vec![baseline_id, expected_id], 4)
            };
            require(
                after[1] != index_expected,
                "valid date raw index rows, identities or version differ",
            )?;
            require(
                after[2..] != before[2..],
                "valid date changed unrelated selected datasets",
            )?;
            let writes = observed
                .iter()
                .filter(|entry| entry.operation == CicsOperation::Write)
                .collect::<Vec<_>>();
            require(
                writes.len() != 1
                    || writes[0].outcome != "NORMAL"
                    || writes[0].response != 0
                    || writes[0].response2 != 0,
                "valid date did not produce exactly one actual NORMAL WRITE",
            )?;
        }
        Ok(())
    }
    .await;
    fixture.finish(result).await
}

pub(super) async fn exercise_transaction_dates(
    observations: &mut RouteObservations,
) -> Result<(), CorpusProblem> {
    let result = async {
        // Enumerate actual input cases, never expected observation tokens.
        for (origin, processing, message) in [
            (
                "2026/02/28",
                "2026-02-28",
                "Orig Date should be in format YYYY-MM-DD",
            ),
            (
                "2026-02-28",
                "2026/02/28",
                "Proc Date should be in format YYYY-MM-DD",
            ),
            (
                "2026-13-01",
                "2026-02-28",
                "Orig Date - Not a valid date...",
            ),
            (
                "2026-02-28",
                "2026-13-01",
                "Proc Date - Not a valid date...",
            ),
            (
                "2026-02-29",
                "2026-02-28",
                "Orig Date - Not a valid date...",
            ),
            (
                "2026-02-28",
                "2026-02-29",
                "Proc Date - Not a valid date...",
            ),
            (
                "1900-02-29",
                "2026-02-28",
                "Orig Date - Not a valid date...",
            ),
            (
                "2026-02-28",
                "1900-02-29",
                "Proc Date - Not a valid date...",
            ),
        ] {
            compare_date(origin, processing, Some(message)).await?;
        }
        for (origin, processing) in [
            ("2026-02-28", "2026-03-01"),
            ("2024-02-29", "2024-02-29"),
            ("2000-02-29", "2000-02-29"),
        ] {
            compare_date(origin, processing, None).await?;
        }
        Ok(())
    }
    .await;
    bind_comparison(observations, "date validation", result)
}

pub(super) async fn exercise_transaction_duplicate(
    observations: &mut RouteObservations,
) -> Result<(), CorpusProblem> {
    let result = compare_duplicate().await;
    bind_comparison(observations, "duplicate condition", result)
}

async fn compare_duplicate() -> Result<(), CorpusProblem> {
    let zero = expected_record(
        "0000000000000000",
        "RETAINED ZERO KEY",
        "2026-02-28",
        "2026-02-28",
    )?;
    let maximum = expected_record(
        "9999999999999999",
        "RETAINED MAXIMUM KEY",
        "2026-02-28",
        "2026-02-28",
    )?;
    let fixture = TransactionFixture::open_with_transaction_rows(vec![zero, maximum]).await?;
    let result = async {
        let route = select_regular_option(&fixture.server, &fixture.app, 8, "COTRN02").await?;
        let before = snapshot(&fixture.server)?;
        let trace_before = fixture
            .server
            .online_trace(&route.session)
            .map_err(terminal_problem)?
            .len();
        let response = carddemo_terminal_exchange(
            &fixture.app,
            &route.session,
            &route.headers,
            0x7d,
            fields("2026-02-28", "2026-02-28"),
        )
        .await?;
        let after = snapshot(&fixture.server)?;
        let trace = fixture
            .server
            .online_trace(&route.session)
            .map_err(terminal_problem)?;
        fixture.export_case(&response, &before, &after, &trace)?;
        compare_message(&response, "Tran ID already exist...")?;
        require(
            after != before,
            "duplicate refusal changed raw rows, identities or versions",
        )?;
        let trace = fixture
            .server
            .online_trace(&route.session)
            .map_err(terminal_problem)?;
        let observed = &trace[trace_before..];
        let actual = observed
            .iter()
            .filter(|entry| {
                matches!(
                    entry.operation,
                    CicsOperation::StartBrowse
                        | CicsOperation::ReadPrev
                        | CicsOperation::EndBrowse
                        | CicsOperation::Write
                )
            })
            .map(|entry| {
                (
                    entry.operation,
                    entry.outcome.as_str(),
                    entry.response,
                    entry.response2,
                )
            })
            .collect::<Vec<_>>();
        require(
            actual
                != vec![
                    (CicsOperation::StartBrowse, "NORMAL", 0, 0),
                    (CicsOperation::ReadPrev, "NORMAL", 0, 0),
                    (CicsOperation::EndBrowse, "NORMAL", 0, 0),
                    (CicsOperation::Write, "DUPREC", 14, 0),
                ],
            "duplicate route must browse maximum then attempt exactly one real DUPREC WRITE",
        )?;
        Ok(())
    }
    .await;
    fixture.finish(result).await
}

pub(super) async fn select_regular_option(
    server: &ProductServer,
    app: &axum::Router,
    option: u8,
    expected_mapset: &str,
) -> Result<CardDemoOnlineSession, CorpusProblem> {
    let route = open_carddemo_menu(
        server,
        app,
        "WEBUSER",
        "transport-password",
        "USER0001",
        "PASSWORD",
        "COMEN01",
    )
    .await?;
    let selected = carddemo_terminal_exchange(
        app,
        &route.session,
        &route.headers,
        0x7d,
        BTreeMap::from([("OPTION".into(), option.to_string())]),
    )
    .await?;
    require_online_mapset(
        &selected,
        expected_mapset,
        &format!("regular option {option}"),
    )?;
    Ok(route)
}

// Frozen navigation fixture literals and exact initial tuple admission.
pub(super) const NAVIGATION_PROCESSING: [&str; 12] = [
    "2026-03-03",
    "2026-03-01",
    "2026-03-01",
    "2026-03-02",
    "2026-03-04",
    "2026-03-04",
    "2026-03-02",
    "2026-03-05",
    "2026-03-03",
    "2026-03-01",
    "2026-03-05",
    "2026-03-02",
];

pub(super) fn navigation_refuse(
    failed: bool,
    detail: impl Into<String>,
) -> Result<(), CorpusProblem> {
    if failed {
        Err(CorpusProblem::new(
            "carddemo.navigation.comparison_mismatch",
            detail,
        ))
    } else {
        Ok(())
    }
}
pub(super) fn navigation_padded(value: &str, width: usize) -> String {
    format!("{value:<width$}")
}
pub(super) fn navigation_encoded(value: &str) -> Result<Vec<u8>, CorpusProblem> {
    CodePage::Cp037.encode(value, value.len()).map_err(|error| {
        CorpusProblem::new("carddemo.navigation.literal_encoding", error.to_string())
    })
}
pub(super) fn navigation_key(key: u8) -> String {
    format!("{key:016}")
}
pub(super) fn navigation_record(key: u8) -> Result<Vec<u8>, CorpusProblem> {
    let mut text = String::new();
    for (literal, width) in [
        (navigation_key(key), 16),
        ("01".into(), 2),
        ("0001".into(), 4),
        ("ONLINE".into(), 10),
        (format!("NAV ROW {key}"), 100),
        ("0000000010{".into(), 11),
        ("123456789".into(), 9),
        ("NAVIGATION SHOP".into(), 50),
        ("BOSTON".into(), 50),
        ("02110".into(), 10),
        ("0500024453765740".into(), 16),
        ("2026-02-28".into(), 26),
        (NAVIGATION_PROCESSING[usize::from(key - 41)].into(), 26),
        ("".into(), 20),
    ] {
        text.push_str(&navigation_padded(&literal, width));
    }
    navigation_refuse(
        text.len() != 350,
        "independent navigation navigation_record width differs",
    )?;
    navigation_encoded(&text)
}
pub(super) fn navigation_tuple(order: &[u8]) -> Result<RawRows, CorpusProblem> {
    Ok((
        order
            .iter()
            .copied()
            .map(navigation_record)
            .collect::<Result<_, _>>()?,
        order
            .iter()
            .map(|key| navigation_encoded(&navigation_key(*key)))
            .collect::<Result<_, _>>()?,
        3,
    ))
}
pub(super) fn navigation_rows() -> Result<Vec<Vec<u8>>, CorpusProblem> {
    (41..=52).map(navigation_record).collect()
}
pub(super) fn validate_navigation_initial(state: &[RawRows]) -> Result<(), CorpusProblem> {
    navigation_refuse(state.len() != 5, "five actual dataset tuples required")?;
    navigation_refuse(
        state[0] != navigation_tuple(&[41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52])?,
        "literal primary rows/identities/version differ",
    )?;
    navigation_refuse(
        state[1] != navigation_tuple(&[42, 43, 50, 44, 47, 52, 41, 49, 45, 46, 48, 51])?,
        "literal AIX rows/identities/version differ",
    )?;
    navigation_refuse(
        state[2..].iter().any(|tuple| tuple.2 != 1),
        "source seed version differs",
    )
}

// Exact opaque map comparisons belong beside the selected terminal fixture.
pub(super) fn navigation_fields(
    response: &serde_json::Value,
    expected: BTreeMap<String, String>,
) -> Result<(), CorpusProblem> {
    let actual = online_screen_fields(response)?;
    for (name, literal) in expected {
        let ebcdic = navigation_encoded(&literal)?;
        navigation_refuse(
            actual
                .get(&name)
                .is_none_or(|value| value.as_slice() != literal.as_bytes() && value != &ebcdic),
            format!(
                "field {name}: expected {literal:?}; actual {:?}",
                actual.get(&name)
            ),
        )?;
    }
    Ok(())
}
pub(super) fn navigation_screen_identity(
    response: &serde_json::Value,
    mapset: &str,
    map: &str,
    program: &str,
    transaction: &str,
) -> Result<(), CorpusProblem> {
    navigation_refuse(
        response["mapset"] != mapset || response["map"] != map,
        format!(
            "actual map identity {:?}/{:?}, expected {mapset}/{map}",
            response["mapset"], response["map"]
        ),
    )?;
    navigation_fields(
        response,
        BTreeMap::from([
            ("PGMNAME".into(), program.into()),
            ("TRNNAME".into(), transaction.into()),
        ]),
    )
}
