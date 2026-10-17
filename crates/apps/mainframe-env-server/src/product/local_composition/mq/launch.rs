//! Explicit local committed PUT1 occurrence and genuine JES submission.
//! This neither supplies Complete/Applied publication nor IBM trigger eligibility.
use super::*;
use mainframe_env_batch::{JclBundle, JobPlan};
use mainframe_env_host_api::{MqRequest, canonical_mq_request_digest};
use mainframe_env_mq::MqCommittedPutOffer;

const PROFILE_PATH: &str = "resource/mq/controller-launch-profile";
const PROFILE: &[u8] = b"mainframe-env.local-mq-controller-launch@1";
const NAMESPACE: &str = "application-local-mq-occurrence";
const SCHEMA: &str = "mainframe-env.local-mq-occurrence@1";
const MAX_PLAN_BYTES: usize = 64 * 1024;

#[derive(Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "phase", deny_unknown_fields)]
enum Phase {
    LaunchUnknown,
    Submitted {
        job: String,
    },
    Completed {
        job: String,
        row_version: u64,
        row_sha256: [u8; 32],
    },
}
#[derive(Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct Occurrence {
    schema_version: String,
    application: String,
    generation: u64,
    identity: String,
    queue: String,
    process: String,
    controller: String,
    source_key: String,
    source_execution: String,
    source_run: String,
    source_principal: String,
    source_selector: String,
    source_artifact: String,
    source_attempt: u32,
    source_sequence: u64,
    request_digest: [u8; 32],
    result_digest: [u8; 32],
    source_resolved_tick: u64,
    plan_sha256: [u8; 32],
    phase: Phase,
}

/// Sign the raw, bounded JCL itself; no caller-provided typed plan can replace it.
pub(super) fn bind_plans(
    package: &ApplicationPackageV2,
    controllers: Option<&mainframe_env_batch::BatchControllerGeneration>,
    used: &mut BTreeSet<String>,
) -> Result<usize, HostProblem> {
    if !package
        .base
        .manifest
        .entries
        .iter()
        .any(|entry| entry.path == PROFILE_PATH)
    {
        return Ok(0);
    }
    bind_entry(package, EntryKind::Resource, PROFILE_PATH, PROFILE, used)?;
    let controllers = controllers.ok_or(HostProblem::NotFound)?;
    let mut bytes = PROFILE.len();
    let mut count = 0usize;
    for process in package
        .sections
        .mq_resources
        .iter()
        .filter(|item| item.kind == MqResourceKind::Process)
    {
        count = count.checked_add(1).ok_or(HostProblem::ResourceExhausted)?;
        let path = format!("resource/mq/launch/{}", process.name);
        let raw = signed_plan(package, &path)?;
        bytes = bytes
            .checked_add(raw.len())
            .ok_or(HostProblem::ResourceExhausted)?;
        if bytes > MAX_PROVIDER_COMPOSITION_BYTES / 8 {
            return Err(HostProblem::ResourceExhausted);
        }
        let controller = controllers
            .controllers
            .iter()
            .find(|controller| Some(controller.name.as_str()) == process.controller.as_deref())
            .ok_or(HostProblem::NotFound)?;
        // Same real parser and selected RUN PROGRAM used by Batch admission.
        let bundle = bundle(raw)?;
        let plan = mainframe_env_batch::parse_jcl(&bundle, Default::default())?;
        mainframe_env_batch::validate_local_controller_plan(&plan, controller)?;
        bind_entry(package, EntryKind::Resource, &path, raw, used)?;
    }
    if count == 0 || count > mainframe_env_mq::MqLimits::default().max_queues {
        return Err(HostProblem::Malformed);
    }
    Ok(bytes)
}

fn signed_plan<'a>(package: &'a ApplicationPackageV2, path: &str) -> Result<&'a [u8], HostProblem> {
    let entry = package
        .base
        .manifest
        .entries
        .iter()
        .find(|entry| entry.path == path && entry.kind == EntryKind::Resource)
        .ok_or(HostProblem::NotFound)?;
    if entry.bytes == 0 || entry.bytes > MAX_PLAN_BYTES {
        return Err(HostProblem::ResourceExhausted);
    }
    let raw = package
        .base
        .blobs
        .get(&entry.sha256)
        .ok_or(HostProblem::NotFound)?;
    if raw.len() != entry.bytes || entry.sha256 != format!("sha256:{:x}", Sha256::digest(raw)) {
        return Err(HostProblem::IdempotencyConflict);
    }
    Ok(raw)
}
fn bundle(raw: &[u8]) -> Result<JclBundle, HostProblem> {
    if raw.is_empty() || raw.len() > MAX_PLAN_BYTES {
        return Err(HostProblem::ResourceExhausted);
    }
    Ok(JclBundle {
        primary: std::str::from_utf8(raw)
            .map_err(|_| HostProblem::Malformed)?
            .into(),
        ..JclBundle::default()
    })
}
fn plan_digest(plan: &JobPlan) -> Result<[u8; 32], HostProblem> {
    Ok(
        Sha256::digest(serde_json::to_vec(plan).map_err(|_| HostProblem::InfrastructureFailure)?)
            .into(),
    )
}
fn occurrence_key(application: &str, source_key: &str) -> Result<IdempotencyKey, HostProblem> {
    let mut digest = Sha256::new();
    for field in [
        SCHEMA.as_bytes(),
        application.as_bytes(),
        source_key.as_bytes(),
    ] {
        digest_online_field(&mut digest, field);
    }
    // Generation is intentionally excluded: the same source occurrence cannot
    // be launched again after an application upgrade or rollback.
    IdempotencyKey::new(
        format!("local-mq:{:x}", digest.finalize()),
        InvocationLimits::default(),
    )
    .map_err(|_| HostProblem::Malformed)
}
fn occurrence_write(
    key: &IdempotencyKey,
    value: &Occurrence,
    previous: Option<u64>,
) -> Result<ProviderStateWrite, HostProblem> {
    let payload = serde_json::to_vec(value).map_err(|_| HostProblem::InfrastructureFailure)?;
    if payload.len() > MAX_PROVIDER_COMPOSITION_BYTES / 8 {
        return Err(HostProblem::ResourceExhausted);
    }
    Ok(ProviderStateWrite {
        expected_version: previous,
        record: ProviderStateRecord {
            namespace: NAMESPACE.into(),
            key: key.as_str().into(),
            version: previous
                .map_or(Some(1), |version| version.checked_add(1))
                .filter(|version| *version <= i64::MAX as u64)
                .ok_or(HostProblem::ResourceExhausted)?,
            payload,
        },
    })
}
fn decode(row: &ProviderStateRecord) -> Result<Occurrence, HostProblem> {
    row.validate_write(MAX_PROVIDER_COMPOSITION_BYTES / 8)
        .map_err(store_error)?;
    let value: Occurrence =
        serde_json::from_slice(&row.payload).map_err(|_| HostProblem::InfrastructureFailure)?;
    let expected_version = match value.phase {
        Phase::LaunchUnknown => 1,
        Phase::Submitted { .. } => 2,
        Phase::Completed { .. } => 3,
    };
    if row.namespace != NAMESPACE
        || row.version != expected_version
        || value.schema_version != SCHEMA
        || value.generation == 0
        || value.generation > i64::MAX as u64
        || value.source_attempt == 0
        || value.source_sequence == 0
        || value.source_resolved_tick == 0
        || occurrence_key(&value.application, &value.source_key)?.as_str() != row.key
        || serde_json::to_vec(&value).map_err(|_| HostProblem::InfrastructureFailure)?
            != row.payload
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(value)
}

impl ProductServer {
    /// Claim one explicitly signed local occurrence, then use the original
    /// Batch submit and durable JES work producer. Occupied occurrences never
    /// redispatch; a lost acknowledgement retains LaunchUnknown.
    pub fn start_committed_local_mq_controller(
        &self,
        application: &str,
        source: &Invocation,
        request: &MqRequest,
    ) -> Result<String, HostProblem> {
        let _publication = self
            .application_publication
            .read()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        self.require_local_owner_known()?;
        let (_, selected) = self
            .local_selection(&application.to_ascii_uppercase())?
            .ok_or(HostProblem::NotFound)?;
        self.verify_static_local_rows(&selected)?;
        let durable = self
            .applications_v2
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let actual = durable
            .installer
            .selected(&selected.package)
            .map_err(application_install_problem)?
            .ok_or(HostProblem::NotFound)?;
        if actual.generation != selected.generation
            || actual.identity != selected.identity
            || actual.state != InstallState::Ready
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        let retained = durable
            .installer
            .generation(&selected.package, selected.generation, &selected.identity)
            .map_err(application_install_problem)?
            .ok_or(HostProblem::NotFound)?;
        self.require_complete_application_publication(&retained)?;
        let package = retained.package();
        super::matches_graph(package, &selected)?;
        let controllers = selected.batch.as_ref().ok_or(HostProblem::NotFound)?;
        self.batch
            .selected_controller_composition_matches(controllers)?;
        // The source artifact must be one of the real signed compiler products,
        // rather than an uninstalled or virtual host callback.
        if !selected.rows.iter().any(|row| {
            row.namespace == "online-program" && row.payload == source.artifact.as_str().as_bytes()
        }) {
            return Err(HostProblem::Unauthorized);
        }
        let max_occurrences = mainframe_env_mq::MqLimits::default().max_replays;
        let current = self
            .store
            .list_provider_state_bounded(
                NAMESPACE,
                max_occurrences + 1,
                MAX_PROVIDER_COMPOSITION_BYTES / 8,
            )
            .map_err(store_error)?;
        if current.len() >= max_occurrences {
            return Err(HostProblem::ResourceExhausted);
        }
        // Charge a finite retained occurrence namespace within original bounds;
        // release its scan before cloning the provider's exact source CAS rows.
        drop(current);
        let catalog = selected.mq.as_ref().ok_or(HostProblem::NotFound)?;
        let prepared = self.mq.prepare_committed_local_put(
            &self.store,
            MqCommittedPutOffer {
                source,
                request,
                catalog,
            },
        )?;
        let process = package
            .sections
            .mq_resources
            .iter()
            .find(|item| item.kind == MqResourceKind::Process && item.name == prepared.process())
            .ok_or(HostProblem::NotFound)?;
        let controller = process.controller.as_deref().ok_or(HostProblem::NotFound)?;
        let mut used = BTreeSet::new();
        if bind_plans(package, Some(&controllers.generation), &mut used)? == 0 {
            return Err(HostProblem::Unsupported);
        }
        let raw = signed_plan(package, &format!("resource/mq/launch/{}", process.name))?;
        let bundle = bundle(raw)?;
        let plan = self.batch.plan(&bundle)?;
        let capabilities = job_capabilities(self.store.as_ref(), &plan)?;
        let invocation = self.invocation(
            source.principal.id().as_str(),
            "zosmf:job-submit",
            ServiceClass::Batch,
            &capabilities,
        )?;
        let mutation = request
            .mutation
            .as_ref()
            .ok_or(HostProblem::MissingIdempotency)?;
        let key = occurrence_key(&selected.package, mutation.idempotency_key.as_str())?;
        if self
            .store
            .get_provider_state(NAMESPACE, key.as_str())
            .map_err(store_error)?
            .is_some()
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        let batch = self.batch.prepare_local_controller_submission(
            &invocation,
            &controllers.generation,
            controller,
            &bundle,
            &key,
        )?;
        let effect = prepared.source_effect();
        let mut occurrence = Occurrence {
            schema_version: SCHEMA.into(),
            application: selected.package.clone(),
            generation: selected.generation,
            identity: selected.identity.clone(),
            queue: prepared.queue().into(),
            process: prepared.process().into(),
            controller: controller.into(),
            source_key: mutation.idempotency_key.as_str().into(),
            source_execution: source.execution_id.as_str().into(),
            source_run: source.run_unit_id.as_str().into(),
            source_principal: source.principal.id().as_str().into(),
            source_selector: source.selector.as_str().into(),
            source_artifact: source.artifact.as_str().into(),
            source_attempt: source.attempt,
            source_sequence: mutation.sequence,
            request_digest: canonical_mq_request_digest(request)?,
            result_digest: effect.result_digest.ok_or(HostProblem::UnknownOutcome)?,
            source_resolved_tick: effect.resolved_tick.ok_or(HostProblem::UnknownOutcome)?,
            plan_sha256: plan_digest(&plan)?,
            phase: Phase::LaunchUnknown,
        };
        let unknown = occurrence_write(&key, &occurrence, None)?;
        let mut writes = prepared.writes().to_vec();
        writes.extend_from_slice(batch.writes());
        writes.push(unknown);
        let provider_store: Arc<dyn ProviderStateStore> = self.store.clone();
        let commit = match ProviderCompositionCommit::publish(provider_store, writes) {
            Ok(commit) => commit,
            Err(problem) => {
                if matches!(
                    problem,
                    StoreError::Infrastructure(_) | StoreError::Poisoned
                ) {
                    prepared.fence_unknown();
                    return Err(HostProblem::UnknownOutcome);
                }
                return Err(store_error(problem));
            }
        };
        prepared.finish(&commit)?;
        // The original selected publication, package and controller owners stay
        // held until submission and work admission. No synchronous fake callback.
        let job = batch
            .submit(&commit)
            .map_err(|_| HostProblem::UnknownOutcome)?;
        self.enqueue_jes_work(&invocation, &job, &capabilities, self.jes_tick()?)
            .map_err(|_| HostProblem::UnknownOutcome)?;
        occurrence.phase = Phase::Submitted {
            job: job.id.clone(),
        };
        let submitted = occurrence_write(&key, &occurrence, Some(1))?;
        self.store
            .put_provider_states_atomic(vec![submitted])
            .map_err(|_| HostProblem::UnknownOutcome)?;
        Ok(job.id)
    }

    /// Observe real successful Batch/JES retirement while its actual owner is
    /// held, then settle only this occurrence. Never run or retry the controller.
    pub fn retire_committed_local_mq_controller(
        &self,
        application: &str,
        source_key: &IdempotencyKey,
    ) -> Result<String, HostProblem> {
        let _publication = self
            .application_publication
            .read()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        self.require_local_owner_known()?;
        let key = occurrence_key(&application.to_ascii_uppercase(), source_key.as_str())?;
        let row = self
            .store
            .get_provider_state(NAMESPACE, key.as_str())
            .map_err(store_error)?
            .ok_or(HostProblem::NotFound)?;
        let mut occurrence = decode(&row)?;
        let job = match &occurrence.phase {
            Phase::Submitted { job } => job.clone(),
            _ => return Err(HostProblem::UnknownOutcome),
        };
        let (_, selected) = self
            .local_selection(&occurrence.application)?
            .ok_or(HostProblem::NotFound)?;
        if selected.generation != occurrence.generation || selected.identity != occurrence.identity
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        self.verify_static_local_rows(&selected)?;
        let durable = self
            .applications_v2
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let actual = durable
            .installer
            .selected(&selected.package)
            .map_err(application_install_problem)?
            .ok_or(HostProblem::NotFound)?;
        if actual.generation != selected.generation
            || actual.identity != selected.identity
            || actual.state != InstallState::Ready
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        let retained = durable
            .installer
            .generation(&selected.package, selected.generation, &selected.identity)
            .map_err(application_install_problem)?
            .ok_or(HostProblem::NotFound)?;
        self.require_complete_application_publication(&retained)?;
        super::matches_graph(retained.package(), &selected)?;
        self.batch
            .with_local_controller_completion(&key, &job, |actual, snapshot| {
                let work = self
                    .store
                    .get_work(&format!("jes:{job}"))
                    .map_err(store_error)?
                    .ok_or(HostProblem::UnknownOutcome)?;
                let payload = JesWorkPayload::decode(&work.payload).map_err(store_error)?;
                if work.work_id != format!("jes:{job}")
                    || work.state != mainframe_env_store_api::WorkState::Completed
                    || work.attempt == 0
                    || work.terminal_tick.is_none_or(|tick| tick == 0)
                    || work.required_selector.as_str() != "zosmf:job-submit"
                    || work.required_generation != JES_WORK_GENERATION
                    || work.artifact.as_str() != "artifact:none"
                    || work.max_attempts != 3
                    || work.priority != snapshot.priority
                    || payload.job_id != job
                    || payload.owner != snapshot.owner
                    || !payload.capabilities.iter().all(|capability| {
                        JES_ALLOWED_WORK_CAPABILITIES.contains(&capability.as_str())
                    })
                {
                    return Err(HostProblem::UnknownOutcome);
                }
                occurrence.phase = Phase::Completed {
                    job: job.clone(),
                    row_version: actual.version,
                    row_sha256: Sha256::digest(&actual.payload).into(),
                };
                let completed = occurrence_write(&key, &occurrence, Some(row.version))?;
                self.store
                    .put_provider_states_atomic(vec![completed])
                    .map_err(store_error)?;
                Ok(job.clone())
            })
    }
}
