use super::*;

pub fn verify_carddemo_mq_authorization_from_env(
    inventory_path: &Path,
) -> Result<CardDemoMqAuthorizationReceipt, CorpusProblem> {
    verify_carddemo_mq_authorization_observed(inventory_path).map(|(receipt, _)| receipt)
}

pub(super) fn verify_carddemo_mq_authorization_observed(
    inventory_path: &Path,
) -> Result<(CardDemoMqAuthorizationReceipt, RouteObservations), CorpusProblem> {
    let corpus_dir = PathBuf::from(env::var_os(CORPUS_ENV).ok_or_else(|| {
        CorpusProblem::new(
            "carddemo.corpus.environment_missing",
            "CARDDEMO_CORPUS_DIR is required",
        )
    })?);
    let corpus = verify_carddemo_corpus(&corpus_dir, inventory_path)?;
    let compiler = CobolCompiler::default();
    let mut programs_compiled = 0usize;
    let mut mq_calls = BTreeMap::new();
    for (relative, bundle) in
        explicit_carddemo_bundles(&corpus_dir)?
            .into_iter()
            .filter(|(relative, _)| {
                relative.starts_with("app/app-vsam-mq/cbl/")
                    || relative.starts_with("app/app-authorization-ims-db2-mq/cbl/")
            })
    {
        let analysis = compiler.analyze(&bundle);
        if analysis.completeness != Completeness::Complete {
            return Err(CorpusProblem::new(
                "carddemo.mq.compile_failed",
                format!(
                    "{relative}: {}",
                    analysis
                        .diagnostics
                        .first()
                        .map_or("incomplete MQ compilation", |problem| problem
                            .public_message())
                ),
            ));
        }
        let calls = analysis
            .hir
            .ok_or_else(|| CorpusProblem::new("carddemo.mq.compile_failed", "MQ HIR missing"))?
            .statements
            .into_iter()
            .filter(|statement| statement.kind == StatementKind::Call)
            .filter_map(|statement| statement.arguments.first().cloned())
            .map(|target| target.trim_matches(['\'', '"']).to_ascii_uppercase())
            .filter(|target| {
                matches!(
                    target.as_str(),
                    "MQOPEN" | "MQGET" | "MQPUT" | "MQPUT1" | "MQCLOSE"
                )
            })
            .collect::<Vec<_>>();
        if calls.is_empty() {
            continue;
        }
        for call in calls {
            *mq_calls.entry(call).or_default() += 1;
        }
        if !matches!(
            compiler
                .compile(CompilerRequest {
                    source: bundle,
                    mode: CompilationMode::Executable,
                    target: CompileTarget::new("reference").expect("static target"),
                    options: CompileOptions::new(BTreeMap::new()).expect("static options"),
                })
                .map_err(|problem| CorpusProblem::new(
                    "carddemo.mq.compile_failed",
                    format!("{relative}: {problem:?}")
                ))?,
            CompilerResult::Published { .. }
        ) {
            return Err(CorpusProblem::new(
                "carddemo.mq.compile_failed",
                format!("{relative} did not publish"),
            ));
        }
        programs_compiled += 1;
    }
    for required in ["MQOPEN", "MQGET", "MQPUT", "MQPUT1", "MQCLOSE"] {
        if !mq_calls.contains_key(required) {
            return Err(CorpusProblem::new(
                "carddemo.mq.call_drift",
                format!("pinned sources no longer contain {required}"),
            ));
        }
    }
    let (definition, _) = carddemo_ims_definition(&corpus_dir)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| CorpusProblem::new("carddemo.mq.runtime", error.to_string()))?;
    let exercise = runtime.block_on(exercise_mq_authorization_routes(&corpus_dir, definition))?;
    let mut shape = Sha256::new();
    digest_field(&mut shape, corpus.commit.as_bytes());
    for (operation, count) in &mq_calls {
        digest_field(&mut shape, operation.as_bytes());
        digest_field(&mut shape, &(*count as u64).to_be_bytes());
    }
    for (queue, digest) in &exercise.queue_sha256 {
        digest_field(&mut shape, queue.as_bytes());
        digest_field(&mut shape, digest.as_bytes());
    }
    digest_field(&mut shape, &(exercise.ims_roots as u64).to_be_bytes());
    digest_field(&mut shape, &(exercise.ims_children as u64).to_be_bytes());
    digest_field(&mut shape, &(exercise.fraud_rows as u64).to_be_bytes());
    let route_observations = exercise.route_observations;
    Ok((
        CardDemoMqAuthorizationReceipt {
            schema_version: "mainframe-env.carddemo-mq-authorization-receipt@1".into(),
            status: "pass".into(),
            corpus_commit: corpus.commit,
            programs_compiled,
            mq_calls,
            queues_installed: exercise.queues_installed,
            triggers_installed: exercise.triggers_installed,
            journeys_passed: 4,
            request_reply_routes: 2,
            approval_decline_routes: 2,
            summary_detail_fraud_routes: 3,
            purge_routes: 1,
            correlation_controls: 2,
            timeout_controls: 1,
            syncpoint_controls: 3,
            rollback_controls: 3,
            unknown_outcome_controls: 1,
            restart_controls: 1,
            idempotency_controls: 2,
            authorization_controls: 1,
            ims_roots: exercise.ims_roots,
            ims_children: exercise.ims_children,
            fraud_rows: exercise.fraud_rows,
            queue_sha256: exercise.queue_sha256,
            authorization_shape_sha256: format!("{:x}", shape.finalize()),
        },
        route_observations,
    ))
}
