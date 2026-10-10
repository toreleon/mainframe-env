// Source-frozen public CICS API -> real ProductServer Dataset/RACF controls.
// No terminal map, compiled application, or IBM execution claim.
#[cfg(test)]
mod cics_first_reverse_tests {
    use super::*;
    use mainframe_env_execution_api::CancellationProbe;
    use mainframe_env_host_api::{CicsDisposition, CicsResponse, ClockRequest};
    use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};

    const FILE: &[u8] = b"REVFILE";
    const BASE: &str = "IBMUSER.REVERSE";
    const INDEX: &str = "IBMUSER.REVERSE.AIX";
    const PATH: &str = "IBMUSER.REVERSE.PATH";

    #[derive(Clone, Debug)]
    struct DatasetObservation {
        request: EffectRequest,
        result: EffectResult,
    }

    struct ObservingDataset {
        inner: Arc<dyn HostProvider>,
        observations: Arc<Mutex<Vec<DatasetObservation>>>,
    }

    impl HostProvider for ObservingDataset {
        fn descriptor(&self) -> &CapabilityDescriptor {
            self.inner.descriptor()
        }

        fn invoke(&self, invocation: &Invocation, request: EffectRequest) -> EffectResult {
            let observed = request.clone();
            let result = self.inner.invoke(invocation, request);
            self.observations.lock().unwrap().push(DatasetObservation {
                request: observed,
                result: result.clone(),
            });
            result
        }
    }

    struct CommandObservation {
        operation: CicsOperation,
        result: Result<CicsResponse, HostProblem>,
        delegates: Vec<DatasetObservation>,
    }

    struct Fixture {
        name: &'static str,
        server: Arc<ProductServer>,
        artifact_root: std::path::PathBuf,
        cics: Option<Arc<CicsService>>,
        command_host: Option<Arc<ScopedHostService>>,
        selected_dataset: Option<DatasetName>,
        invocation: Option<Invocation>,
        session: SessionId,
        now_tick: u64,
        sequence: u64,
        seed_sequence: u64,
        observations: Arc<Mutex<Vec<DatasetObservation>>>,
        clock_observations: Arc<Mutex<Vec<DatasetObservation>>>,
        deadline_millis: Option<u64>,
        commands: Vec<CommandObservation>,
        owned: Option<(DatasetName, String)>,
        launched: bool,
    }

    fn literal(bytes: &[u8]) -> BoundedPayload {
        BoundedPayload::new(
            "mainframe-env.cics.literal@1",
            bytes.to_vec(),
            InvocationLimits::default(),
        )
        .unwrap()
    }

    impl Fixture {
        fn new(name: &'static str) -> Self {
            let mut settings = config();
            settings.artifact_root = std::env::temp_dir().join(format!(
                "mainframe-cics-first-reverse-{}-{name}",
                std::process::id()
            ));
            assert!(
                !settings.artifact_root.exists(),
                "fixture root already exists"
            );
            let artifact_root = settings.artifact_root.clone();
            Self {
                name,
                server: ProductServer::memory(settings).unwrap(),
                artifact_root,
                cics: None,
                command_host: None,
                selected_dataset: None,
                invocation: None,
                session: SessionId::new(format!("first-reverse-{name}"), 128).unwrap(),
                now_tick: 0,
                sequence: 0,
                seed_sequence: 0,
                observations: Arc::new(Mutex::new(Vec::new())),
                clock_observations: Arc::new(Mutex::new(Vec::new())),
                deadline_millis: None,
                commands: Vec::new(),
                owned: None,
                launched: false,
            }
        }

        fn seed_mutation(&mut self) -> Mutation {
            self.seed_sequence += 1;
            Mutation {
                sequence: self.seed_sequence,
                idempotency_key: IdempotencyKey::new(
                    format!("first-reverse-{}-seed-{}", self.name, self.seed_sequence),
                    InvocationLimits::default(),
                )
                .unwrap(),
                transaction: None,
            }
        }

        fn prepare(&mut self, aix: bool) {
            self.server.bootstrap_user("IBMUSER", b"TESTPASS").unwrap();
            let base = DatasetName::new(BASE, 128).unwrap();
            let mutation = self.seed_mutation();
            self.server
                .dataset_call(
                    "IBMUSER",
                    DatasetRequest::Create {
                        dataset: base.clone(),
                        attributes: DatasetAttributes {
                            organization: DatasetOrganization::KeySequenced,
                            record_format: RecordFormat::Fixed,
                            logical_record_length: 4,
                            key_offset: Some(0),
                            key_length: Some(2),
                            ccsid: None,
                        },
                        mutation,
                    },
                )
                .unwrap();
            let records = if aix {
                vec![
                    b"AA01".to_vec(),
                    b"BB02".to_vec(),
                    b"CC02".to_vec(),
                    b"DD03".to_vec(),
                ]
            } else {
                vec![b"AA01".to_vec(), b"BB02".to_vec(), b"CC03".to_vec()]
            };
            let mutation = self.seed_mutation();
            self.server
                .dataset_call(
                    "IBMUSER",
                    DatasetRequest::Write {
                        dataset: base.clone(),
                        member: None,
                        records,
                        expected_version: Some(1),
                        mutation,
                    },
                )
                .unwrap();
            let selected = if aix {
                let index = DatasetName::new(INDEX, 128).unwrap();
                let mutation = self.seed_mutation();
                self.server
                    .dataset_call(
                        "IBMUSER",
                        DatasetRequest::DefineAlternateIndex {
                            base,
                            index: index.clone(),
                            key_offset: 2,
                            key_length: 2,
                            allow_duplicates: true,
                            upgrade: true,
                            mutation,
                        },
                    )
                    .unwrap();
                let path = DatasetName::new(PATH, 128).unwrap();
                let mutation = self.seed_mutation();
                self.server
                    .dataset_call(
                        "IBMUSER",
                        DatasetRequest::DefinePath {
                            path: path.clone(),
                            index,
                            mutation,
                        },
                    )
                    .unwrap();
                path
            } else {
                base
            };

            let limits = InvocationLimits::default();
            let mut providers: Vec<Arc<dyn HostProvider>> =
                dataset_providers(self.server.dataset.clone(), limits)
                    .into_iter()
                    .map(|inner| {
                        Arc::new(ObservingDataset {
                            inner,
                            observations: self.observations.clone(),
                        }) as Arc<dyn HostProvider>
                    })
                    .collect();
            providers.extend(racf_providers(self.server.racf.clone(), limits));
            providers.push(Arc::new(ObservingDataset {
                inner: Arc::new(SystemClockProvider::new(limits)),
                observations: self.clock_observations.clone(),
            }));
            let host = Arc::new(ScopedHostService::new(
                Arc::new(RegistrySnapshot::new(1, providers, limits).unwrap()),
                HostLimits::default(),
            ));
            let store: Arc<dyn ProviderStateStore> = self.server.store.clone();
            let work_store: Arc<dyn WorkStore> = self.server.store.clone();
            let cics = CicsService::open_with_runtime(
                host,
                store,
                work_store,
                Default::default(),
                Arc::new(EnterpriseReplayClock(self.server.jes_clock.clone())),
            )
            .unwrap();
            cics.register_file_aliases(&BTreeMap::from([("REVFILE".into(), selected.clone())]))
                .unwrap();
            self.command_host = Some(Arc::new(ScopedHostService::new(
                Arc::new(
                    RegistrySnapshot::new(1, vec![cics_provider(cics.clone(), limits)], limits)
                        .unwrap(),
                ),
                HostLimits::default(),
            )));
            self.selected_dataset = Some(selected);
            let mut invocation = self
                .server
                .cics_invocation("IBMUSER", "RF01", None)
                .unwrap();
            invocation.cancellation_probe = Some(CancellationProbe::new());
            if let Some(millis) = self.deadline_millis {
                invocation.deadline_tick =
                    self.server.jes_tick().unwrap().checked_add(millis).unwrap();
            }
            self.now_tick = self.server.jes_tick().unwrap();
            cics.launch_terminal(
                invocation.clone(),
                &self.session,
                "RF01",
                24,
                80,
                "first-reverse-csrf",
                self.now_tick,
                60_000,
            )
            .unwrap();
            self.launched = true;
            self.invocation = Some(invocation);
            self.cics = Some(cics);
        }

        fn command(&mut self, operation: CicsOperation, key: Option<&[u8]>, equal: bool) -> usize {
            self.sequence += 1;
            let mut arguments =
                if matches!(operation, CicsOperation::Return | CicsOperation::AsktimeEib) {
                    BTreeMap::new()
                } else {
                    BTreeMap::from([("FILE".into(), literal(FILE))])
                };
            if let Some(key) = key {
                arguments.insert("RIDFLD".into(), literal(key));
            }
            if equal {
                arguments.insert(
                    "OPTION.EQUAL".into(),
                    BoundedPayload::new(
                        "mainframe-env.cics.option@1",
                        Vec::new(),
                        InvocationLimits::default(),
                    )
                    .unwrap(),
                );
            }
            let mutation = operation.is_mutating().then(|| Mutation {
                sequence: self.sequence,
                idempotency_key: IdempotencyKey::new(
                    format!("first-reverse-{}-command-{}", self.name, self.sequence),
                    InvocationLimits::default(),
                )
                .unwrap(),
                transaction: Some("RF01".into()),
            });
            let request = CicsRequest {
                operation,
                arguments,
                condition_policy: CicsConditionPolicy::Default,
                mutation,
            };
            let invocation = self.invocation.as_ref().unwrap();
            let effect = EffectRequest {
                run_unit: invocation.run_unit_id.clone(),
                sequence: self.sequence,
                deadline_tick: invocation.deadline_tick,
                idempotency_key: request.mutation.as_ref().map(|m| m.idempotency_key.clone()),
                request: HostRequest::Cics(request.clone()),
            };
            let before = self.observations.lock().unwrap().len();
            let result = self
                .command_host
                .as_ref()
                .unwrap()
                .invoke(invocation, self.server.jes_tick().unwrap(), false, effect)
                .persist_with(|audit| self.server.store.record_audit(audit).map_err(store_error));
            let result = match result.outcome {
                Ok(HostResult::Cics(response)) => Ok(response),
                Ok(_) => panic!("actual CICS host returned another result domain"),
                Err(problem) => Err(problem),
            };
            let delegates = self.observations.lock().unwrap()[before..].to_vec();
            // Preserve actual cursor ownership before any source assertion can panic.
            for call in &delegates {
                match (&call.request.request, &call.result.outcome) {
                    (
                        HostRequest::Dataset(DatasetRequest::StartBrowse { dataset, .. }),
                        Ok(HostResult::Dataset(DatasetResult::Browse { cursor, .. })),
                    ) => {
                        assert!(self.owned.is_none(), "unretired earlier fixture cursor");
                        self.owned = Some((dataset.clone(), cursor.clone()));
                    }
                    (
                        HostRequest::Dataset(DatasetRequest::EndBrowse { dataset, cursor }),
                        Ok(HostResult::Dataset(DatasetResult::Browse { cursor: ended, .. })),
                    ) if cursor == ended => {
                        assert_eq!(
                            self.owned.as_ref(),
                            Some(&(dataset.clone(), cursor.clone()))
                        );
                        self.owned = None;
                    }
                    _ => {}
                }
            }
            let at = self.commands.len();
            self.commands.push(CommandObservation {
                operation,
                result,
                delegates,
            });
            at
        }

        fn normal(&self, at: usize) -> &CicsResponse {
            let response = self.commands[at]
                .result
                .as_ref()
                .expect("actual CICS command refused");
            assert_eq!(
                (&*response.condition, response.response, response.response2),
                ("NORMAL", 0, 0)
            );
            response
        }

        fn start(&mut self, key: &[u8], equal: bool) -> String {
            let at = self.command(CicsOperation::StartBrowse, Some(key), equal);
            self.normal(at);
            let calls = &self.commands[at].delegates;
            assert_eq!(
                calls.len(),
                1,
                "STARTBR must create exactly one real cursor"
            );
            assert!(
                matches!(&calls[0].request.request, HostRequest::Dataset(DatasetRequest::StartBrowse { key: actual, .. }) if actual == key)
            );
            let (dataset, cursor) = self.owned.as_ref().expect("actual STARTBR cursor");
            assert!(
                matches!(&calls[0].result.outcome, Ok(HostResult::Dataset(DatasetResult::Browse { record: None, identity: None, key: None, cursor: actual })) if actual == cursor)
            );
            assert_eq!(Some(dataset), self.selected_dataset.as_ref());
            cursor.clone()
        }

        fn reset(&mut self, key: &[u8], equal: bool) -> usize {
            let owned = self.owned.clone().expect("RESETBR owns a started cursor");
            let at = self.command(CicsOperation::ResetBrowse, Some(key), equal);
            assert_eq!(self.owned.as_ref(), Some(&owned));
            let calls = &self.commands[at].delegates;
            assert_eq!(
                calls.len(),
                1,
                "RESETBR must not allocate or read another cursor"
            );
            assert!(
                matches!(&calls[0].request.request, HostRequest::Dataset(DatasetRequest::ResetBrowse { cursor, key: actual, .. }) if cursor == &owned.1 && actual == key)
            );
            at
        }

        fn read(&mut self, operation: CicsOperation, key: &[u8]) -> usize {
            self.command(operation, Some(key), false)
        }

        fn one_read_delegate(&self, at: usize) -> &DatasetObservation {
            let command = &self.commands[at];
            assert!(matches!(
                command.operation,
                CicsOperation::ReadNext | CicsOperation::ReadPrev
            ));
            assert_eq!(
                command.delegates.len(),
                2,
                "one Attributes + one browse read; no hidden pair/keyed fallback"
            );
            assert!(matches!(
                &command.delegates[0].request.request,
                HostRequest::Dataset(DatasetRequest::Attributes { .. })
            ));
            let read = &command.delegates[1];
            assert!(matches!(&read.request.request, HostRequest::Dataset(_)));
            assert!(
                !matches!(
                    &read.request.request,
                    HostRequest::Dataset(
                        DatasetRequest::Attributes { .. }
                            | DatasetRequest::Read { .. }
                            | DatasetRequest::ReadGeneric { .. }
                            | DatasetRequest::StartBrowse { .. }
                            | DatasetRequest::ResetBrowse { .. }
                            | DatasetRequest::EndBrowse { .. }
                    )
                ),
                "ordinary keyed/read-position rebuilding is not the cursor read"
            );
            assert!(read.request.sequence > command.delegates[0].request.sequence);
            read
        }

        fn record(&self, at: usize, payload: &[u8], logical_key: &[u8], base_identity: &[u8]) {
            let response = self.normal(at);
            // This full literal anchor assertion precedes any subsequent traversal.
            assert_eq!(
                response.payload.bytes(),
                payload,
                "first/successive literal record"
            );
            assert_eq!(response.outputs["RIDFLD"].bytes(), logical_key);
            let read = self.one_read_delegate(at);
            let (_, cursor) = self.owned.as_ref().expect("read retains its cursor");
            assert!(
                matches!(&read.result.outcome, Ok(HostResult::Dataset(DatasetResult::Browse {
                cursor: actual_cursor, record: Some(actual), identity: Some(identity), key: Some(key)
            })) if actual_cursor == cursor && actual == payload && identity == base_identity && key == logical_key),
                "full real provider tuple: {:?}",
                read.result
            );
        }

        fn condition(&self, at: usize, name: &str, primary: i32) {
            match &self.commands[at].result {
                Err(HostProblem::Condition {
                    name: actual,
                    response,
                    ..
                }) => {
                    assert_eq!((actual.as_str(), *response), (name, primary));
                }
                Ok(response) => assert_eq!(
                    (response.condition.as_str(), response.response),
                    (name, primary)
                ),
                other => panic!("actual condition prerequisite: {other:?}"),
            }
            // Secondary codes remain qualified; this control does not invent an oracle.
        }

        fn end(&mut self) {
            let owned = self.owned.clone().expect("explicit ENDBR owns a cursor");
            let at = self.command(CicsOperation::EndBrowse, None, false);
            self.normal(at);
            assert!(
                self.owned.is_none(),
                "real ENDBR must retire the known owner"
            );
            let calls = &self.commands[at].delegates;
            assert_eq!(calls.len(), 1, "exactly one real retirement, no retry");
            assert!(
                matches!(&calls[0].request.request, HostRequest::Dataset(DatasetRequest::EndBrowse { dataset, cursor }) if dataset == &owned.0 && cursor == &owned.1)
            );
        }

        fn abort(&mut self) {
            let owned = self
                .owned
                .clone()
                .expect("public abort owns a real STARTBR cursor");
            let before = self.observations.lock().unwrap().len();
            self.cics
                .as_ref()
                .unwrap()
                .abort_terminal_run(
                    &self.session,
                    self.invocation.as_ref().unwrap().principal.id(),
                    self.now_tick,
                )
                .expect("known abnormal task completion");
            self.launched = false;
            let calls = self.observations.lock().unwrap()[before..].to_vec();
            println!(
                "CICS_TASK_BROWSE_RETIREMENT_ABORT {}",
                json!({
                    "captured_dataset": owned.0.as_str(),
                    "captured_cursor": owned.1,
                    "actual_dataset_delegates": calls.iter().map(|call| json!({
                        "request": format!("{:?}", call.request),
                        "result": format!("{:?}", call.result),
                    })).collect::<Vec<_>>(),
                })
            );
            assert_eq!(calls.len(), 1, "public abort issues exactly one real END");
            assert!(matches!(&calls[0].request.request,
                HostRequest::Dataset(DatasetRequest::EndBrowse { dataset, cursor })
                if dataset == &owned.0 && cursor == &owned.1));
            assert!(matches!(&calls[0].result.outcome,
                Ok(HostResult::Dataset(DatasetResult::Browse { cursor, record: None, identity: None, key: None }))
                if cursor == &owned.1));
            self.owned = None;
        }

        fn retired_probe(&self, cursor: &str) {
            let result = self.server.dataset_call(
                "IBMUSER",
                DatasetRequest::ReadNext {
                    dataset: self.selected_dataset.as_ref().unwrap().clone(),
                    cursor: cursor.into(),
                    reverse: false,
                    control: Default::default(),
                },
            );
            println!(
                "CICS_TASK_BROWSE_RETIREMENT_PROBE {}",
                json!({
                    "captured_cursor": cursor,
                    "actual_result": format!("{result:?}"),
                })
            );
            assert_eq!(
                result,
                Err(GatewayProblem::new(
                    StatusCode::CONFLICT,
                    "condition",
                    "host service failed: Condition { name: \"INVREQ\", response: 16, response2: 0 }",
                )),
                "real captured cursor must be absent: {result:?}"
            );
        }

        fn insert_duplicate(&mut self) {
            let mutation = self.seed_mutation();
            self.server
                .dataset_call(
                    "IBMUSER",
                    DatasetRequest::Write {
                        dataset: DatasetName::new(BASE, 128).unwrap(),
                        member: None,
                        records: vec![b"AB02".to_vec()],
                        expected_version: None,
                        mutation,
                    },
                )
                .unwrap();
            let actual = self
                .server
                .dataset_call(
                    "IBMUSER",
                    DatasetRequest::Read {
                        dataset: DatasetName::new(BASE, 128).unwrap(),
                        member: None,
                        key: None,
                        max_records: 8,
                        control: Default::default(),
                    },
                )
                .unwrap();
            assert!(
                matches!(actual, DatasetResult::Records { records, identities, .. }
                if records == [b"AA01".to_vec(), b"AB02".to_vec(), b"BB02".to_vec(), b"CC02".to_vec(), b"DD03".to_vec()]
                && identities == [b"AA".to_vec(), b"AB".to_vec(), b"BB".to_vec(), b"CC".to_vec(), b"DD".to_vec()])
            );
        }

        fn teardown(&mut self) {
            if self.owned.is_some() {
                self.end();
            }
            if self.launched {
                self.cics
                    .as_ref()
                    .unwrap()
                    .complete_terminal_run(
                        &self.session,
                        self.invocation.as_ref().unwrap().principal.id(),
                        self.now_tick,
                    )
                    .expect("task completion after explicit ENDBR");
                self.launched = false;
            }
            assert!(self.owned.is_none());
        }

        fn evidence(
            &self,
            comparison_failure: Option<&str>,
            teardown_failure: Option<&str>,
            shutdown: bool,
        ) -> Value {
            let commands = self
                .commands
                .iter()
                .map(|command| {
                    json!({
                        "operation": format!("{:?}", command.operation),
                        "actual_result": format!("{:?}", command.result),
                        "actual_dataset_delegates": command.delegates.iter().map(|call| json!({
                            "request": format!("{:?}", call.request),
                            "result": format!("{:?}", call.result),
                        })).collect::<Vec<_>>(),
                    })
                })
                .collect::<Vec<_>>();
            json!({
                "control": self.name,
                "scope": "actual ScopedHostService CICS admission with live ProductServer clock and real same-store Dataset/RACF/SystemClock; no compiled application",
                "comparison_passed": comparison_failure.is_none(),
                "comparison_failure": comparison_failure,
                "explicit_endbr_and_completion_passed": teardown_failure.is_none(),
                "explicit_teardown_failure": teardown_failure,
                "product_shutdown": shutdown,
                "remaining_owned_cursor": self.owned.as_ref().map(|(dataset, cursor)| json!({"dataset": dataset.as_str(), "cursor": cursor})),
                "commands": commands,
            })
        }
    }

    fn panic_text(problem: &(dyn std::any::Any + Send)) -> String {
        if let Some(text) = problem.downcast_ref::<String>() {
            text.clone()
        } else if let Some(text) = problem.downcast_ref::<&str>() {
            (*text).into()
        } else {
            "non-string assertion panic; original payload retained for resume_unwind".into()
        }
    }

    async fn run(name: &'static str, aix: bool, check: impl FnOnce(&mut Fixture)) {
        run_with_deadline(name, aix, None, check).await;
    }

    async fn run_with_deadline(
        name: &'static str,
        aix: bool,
        deadline_millis: Option<u64>,
        check: impl FnOnce(&mut Fixture),
    ) {
        let mut fixture = Fixture::new(name);
        fixture.deadline_millis = deadline_millis;
        let result = catch_unwind(AssertUnwindSafe(|| {
            fixture.prepare(aix);
            check(&mut fixture);
        }));
        // Catch solely to perform/check owned cleanup and resume the original failure.
        // No Drop implementation, hidden retry, substituted reply, or swallowed cleanup error.
        let teardown = catch_unwind(AssertUnwindSafe(|| fixture.teardown()));
        let shutdown = fixture.server.graceful_shutdown().await;
        let comparison_failure = result
            .as_ref()
            .err()
            .map(|problem| panic_text(problem.as_ref()));
        let teardown_failure = teardown
            .as_ref()
            .err()
            .map(|problem| panic_text(problem.as_ref()));
        println!(
            "CICS_FIRST_REVERSE_ACTUAL {}",
            fixture.evidence(
                comparison_failure.as_deref(),
                teardown_failure.as_deref(),
                shutdown
            )
        );
        let artifact_root = fixture.artifact_root.clone();
        drop(fixture);
        let files = if artifact_root.exists() {
            std::fs::remove_dir_all(&artifact_root)
        } else {
            Ok(())
        };
        eprintln!(
            "CICS_FIRST_REVERSE_CLEANUP {name} explicit_teardown={} shutdown={shutdown} artifact_cleanup={files:?}",
            teardown.is_ok()
        );
        if let Err(original) = result {
            // Teardown/shutdown/file outcomes above remain visible even on genuine RED.
            resume_unwind(original);
        }
        if let Err(problem) = teardown {
            resume_unwind(problem);
        }
        assert!(shutdown, "actual ProductServer shutdown refused");
        files.expect("owned fixture artifact cleanup");
    }

    fn assert_genuine_clock(fixture: &mut Fixture) {
        let before = fixture.clock_observations.lock().unwrap().len();
        let at = fixture.command(CicsOperation::AsktimeEib, None, false);
        let response = fixture.normal(at);
        for output in ["EIBDATE", "EIBTIME"] {
            assert!(!response.outputs[output].bytes().is_empty());
        }
        let clocks = fixture.clock_observations.lock().unwrap();
        assert_eq!(clocks.len(), before + 1);
        let clock = &clocks[before];
        assert!(matches!(
            clock.request.request,
            HostRequest::Clock(ClockRequest::UtcTimestamp)
        ));
        assert_eq!(
            clock.request.run_unit,
            fixture.invocation.as_ref().unwrap().run_unit_id
        );
        assert!(matches!(&clock.result.outcome, Ok(HostResult::Clock(value))
            if value.len() == 17 && value.bytes().all(|byte| byte.is_ascii_digit())));
        println!(
            "CICS_GENUINE_CLOCK_ACTUAL {:?} {:?}",
            clock.request, clock.result
        );
    }

    #[tokio::test]
    async fn cics_task_browse_retirement_genuine_nested_clock_and_root_return() {
        run("genuine-clock-return", false, |fixture| {
            assert_genuine_clock(fixture);
            let cursor = fixture.start(b"AA", false);
            let first = fixture.read(CicsOperation::ReadPrev, b"AA");
            fixture.record(first, b"AA01", b"AA", b"AA");
            assert_genuine_clock(fixture);
            let at = fixture.command(CicsOperation::Return, None, false);
            assert_eq!(fixture.normal(at).disposition, CicsDisposition::Returned);
            assert!(fixture.owned.is_none());
            assert_eq!(fixture.commands[at].delegates.len(), 1);
            fixture.retired_probe(&cursor);
        })
        .await;
    }

    #[tokio::test]
    async fn cics_genuine_nested_clock_live_cancellation_refuses_before_dispatch() {
        run("genuine-clock-cancel", false, |fixture| {
            assert_genuine_clock(fixture);
            fixture
                .invocation
                .as_ref()
                .unwrap()
                .cancellation_probe
                .as_ref()
                .unwrap()
                .request();
            let before = fixture.clock_observations.lock().unwrap().len();
            let at = fixture.command(CicsOperation::AsktimeEib, None, false);
            assert!(matches!(
                fixture.commands[at].result,
                Err(HostProblem::Cancelled)
            ));
            assert_eq!(fixture.clock_observations.lock().unwrap().len(), before);
            assert!(fixture.owned.is_none());
        })
        .await;
    }

    #[tokio::test]
    async fn cics_genuine_nested_clock_expired_deadline_refuses_before_dispatch() {
        run_with_deadline("genuine-clock-deadline", false, Some(500), |fixture| {
            assert_genuine_clock(fixture);
            let deadline = fixture.invocation.as_ref().unwrap().deadline_tick;
            let waited = std::time::Instant::now();
            while fixture.server.jes_tick().unwrap() < deadline
                && waited.elapsed() < std::time::Duration::from_secs(2)
            {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            assert!(fixture.server.jes_tick().unwrap() >= deadline);
            let before = fixture.clock_observations.lock().unwrap().len();
            let at = fixture.command(CicsOperation::AsktimeEib, None, false);
            assert!(matches!(
                fixture.commands[at].result,
                Err(HostProblem::TimedOut)
            ));
            assert_eq!(fixture.clock_observations.lock().unwrap().len(), before);
            assert!(fixture.owned.is_none());
        })
        .await;
    }

    #[tokio::test]
    async fn cics_task_browse_retirement_root_return_ends_real_dataset_cursor() {
        run("task-return", false, |fixture| {
            let cursor = fixture.start(b"AA", false);
            let first = fixture.read(CicsOperation::ReadPrev, b"AA");
            fixture.record(first, b"AA01", b"AA", b"AA");
            let at = fixture.command(CicsOperation::Return, None, false);
            let response = fixture.normal(at);
            assert_eq!(response.disposition, CicsDisposition::Returned);
            assert!(
                fixture.owned.is_none(),
                "root RETURN must retire the captured cursor"
            );
            assert_eq!(fixture.commands[at].delegates.len(), 1);
            fixture.retired_probe(&cursor);
        })
        .await;
    }

    #[tokio::test]
    async fn cics_task_browse_retirement_public_abort_ends_real_dataset_cursor() {
        run("task-abort", false, |fixture| {
            let cursor = fixture.start(b"AA", false);
            let first = fixture.read(CicsOperation::ReadPrev, b"AA");
            fixture.record(first, b"AA01", b"AA", b"AA");
            fixture.abort();
            fixture.retired_probe(&cursor);
        })
        .await;
    }

    #[tokio::test]
    async fn cics_first_reverse_start_gte_existing_anchor_and_predecessor() {
        run("start-gte", false, |fixture| {
            fixture.start(b"BB", false);
            let first = fixture.read(CicsOperation::ReadPrev, b"BB");
            fixture.record(first, b"BB02", b"BB", b"BB");
            let next = fixture.read(CicsOperation::ReadPrev, b"BB");
            fixture.record(next, b"AA01", b"AA", b"AA");
        })
        .await;
    }

    #[tokio::test]
    async fn cics_first_reverse_start_equal_first_key_then_endfile() {
        run("start-equal", false, |fixture| {
            fixture.start(b"AA", true);
            let first = fixture.read(CicsOperation::ReadPrev, b"AA");
            fixture.record(first, b"AA01", b"AA", b"AA");
            let eof = fixture.read(CicsOperation::ReadPrev, b"AA");
            fixture.condition(eof, "ENDFILE", 20);
            fixture.one_read_delegate(eof);
        })
        .await;
    }

    #[tokio::test]
    async fn cics_first_reverse_reset_reuses_cursor_and_selects_new_anchor() {
        run("reset-anchor", false, |fixture| {
            let cursor = fixture.start(b"AA", false);
            let forward = fixture.read(CicsOperation::ReadNext, b"AA");
            fixture.record(forward, b"AA01", b"AA", b"AA");
            let reset = fixture.reset(b"CC", false);
            fixture.normal(reset);
            assert_eq!(fixture.owned.as_ref().unwrap().1, cursor);
            let first = fixture.read(CicsOperation::ReadPrev, b"CC");
            fixture.record(first, b"CC03", b"CC", b"CC");
            let next = fixture.read(CicsOperation::ReadPrev, b"CC");
            fixture.record(next, b"BB02", b"BB", b"BB");
        })
        .await;
    }

    #[tokio::test]
    async fn cics_first_reverse_missing_initial_key_refuses_then_reset_recovers() {
        run("missing-anchor", false, |fixture| {
            fixture.start(b"AB", false);
            let missing = fixture.read(CicsOperation::ReadPrev, b"AB");
            fixture.condition(missing, "NOTFND", 13);
            fixture.one_read_delegate(missing);
            let reset = fixture.reset(b"BB", false);
            fixture.normal(reset);
            let first = fixture.read(CicsOperation::ReadPrev, b"BB");
            fixture.record(first, b"BB02", b"BB", b"BB");
        })
        .await;
    }

    #[tokio::test]
    async fn cics_first_reverse_failed_reset_preserves_pending_anchor() {
        run("failed-reset", false, |fixture| {
            fixture.start(b"CC", false);
            let refused = fixture.reset(b"AB", true);
            fixture.condition(refused, "NOTFND", 13);
            let first = fixture.read(CicsOperation::ReadPrev, b"CC");
            fixture.record(first, b"CC03", b"CC", b"CC");
        })
        .await;
    }

    #[tokio::test]
    async fn cics_first_reverse_aix_duplicate_anchor_keeps_base_identity() {
        run("aix-anchor", true, |fixture| {
            fixture.start(b"02", false);
            let first = fixture.read(CicsOperation::ReadPrev, b"02");
            fixture.record(first, b"BB02", b"02", b"BB");
            let next = fixture.read(CicsOperation::ReadPrev, b"02");
            fixture.record(next, b"AA01", b"01", b"AA");
        })
        .await;
    }

    #[tokio::test]
    async fn cics_first_reverse_aix_new_duplicate_does_not_replace_snapshot_anchor() {
        run("aix-snapshot", true, |fixture| {
            fixture.start(b"02", false);
            fixture.insert_duplicate();
            let first = fixture.read(CicsOperation::ReadPrev, b"02");
            fixture.record(first, b"BB02", b"02", b"BB");
            let next = fixture.read(CicsOperation::ReadPrev, b"02");
            fixture.record(next, b"AA01", b"01", b"AA");
        })
        .await;
    }

    #[tokio::test]
    async fn cics_first_reverse_end_then_restart_has_a_fresh_initial_anchor() {
        run("restart-anchor", false, |fixture| {
            let old = fixture.start(b"BB", false);
            let first = fixture.read(CicsOperation::ReadPrev, b"BB");
            fixture.record(first, b"BB02", b"BB", b"BB");
            fixture.end();
            let fresh = fixture.start(b"CC", false);
            assert_ne!(old, fresh, "fresh STARTBR must own a distinct cursor");
            let restarted = fixture.read(CicsOperation::ReadPrev, b"CC");
            fixture.record(restarted, b"CC03", b"CC", b"CC");
            let next = fixture.read(CicsOperation::ReadPrev, b"CC");
            fixture.record(next, b"BB02", b"BB", b"BB");
        })
        .await;
    }
}
