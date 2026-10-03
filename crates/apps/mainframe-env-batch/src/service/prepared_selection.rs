//! Private structural plan; never admission, a run owner or an atomic namespace fence.
use super::*;

const MAX_SELECTION_RECORDS: usize = 4_096;
const MAX_SELECTION_BYTES: usize = 64 * 1024 * 1024;

pub(super) fn next_job_version(version: u64) -> Result<u64, HostProblem> {
    version.checked_add(1).ok_or(HostProblem::ResourceExhausted)
}

/// Sole pure builders for the existing two sequential selection edges. The
/// ordinary path still constructs Running AFTER its known Selected write.
pub(super) struct SelectionEdges {
    selected_version: u64,
    running_version: u64,
    running_attempt: u32,
}
impl SelectionEdges {
    pub(super) fn prepare(current: &Job, limits: BatchLimits) -> Result<Self, HostProblem> {
        ensure_job_event_capacity(current, limits.max_events, 4)?;
        let selected_version = next_job_version(current.version)?;
        let running_version = next_job_version(selected_version)?;
        let running_attempt = current
            .attempt
            .checked_add(1)
            .filter(|attempt| *attempt <= limits.max_attempts)
            .ok_or(HostProblem::ResourceExhausted)?;
        Ok(Self {
            selected_version,
            running_version,
            running_attempt,
        })
    }
    pub(super) fn selected(
        &self,
        current: &Job,
        member: &str,
        initiator: &str,
        limits: BatchLimits,
    ) -> Result<Job, HostProblem> {
        let mut selected = current.clone();
        selected.version = self.selected_version;
        selected.state = JobState::Selected;
        selected.initiator = Some(initiator.into());
        selected.route.owner_member = Some(member.into());
        push_job_event(
            &mut selected,
            limits.max_events,
            format!("selected:{initiator}"),
        )?;
        Ok(selected)
    }
    pub(super) fn running(&self, selected: &Job, limits: BatchLimits) -> Result<Job, HostProblem> {
        let mut running = selected.clone();
        running.version = self.running_version;
        running.attempt = self.running_attempt;
        running.state = JobState::Running;
        push_job_event(&mut running, limits.max_events, "running")?;
        Ok(running)
    }
}

// Configuration rows and semantic observations are kept even when a configuration
// has no physical row. Such a version-zero observation is NOT a CAS dependency.
struct Configuration {
    scheduler: JesSchedulerConfiguration,
    topology: JesTopology,
    scheduler_row: Option<ProviderStateRecord>,
    topology_row: Option<ProviderStateRecord>,
    scheduler_bytes: Vec<u8>,
    topology_bytes: Vec<u8>,
}

/// Constructed only by the fresh contained runner from its actual service/cache.
/// No Clone/Serde, public factory, drive method or acknowledgement/adoption token.
pub(super) struct PreparedSelection<'service> {
    service: &'service BatchService,
    store: Arc<dyn ProviderStateStore>,
    original: Invocation,
    dependencies: Vec<ProviderStateRecord>,
    configuration: Configuration,
    current: ProviderStateRecord,
    selected: Job,
    running: Job,
    selected_row: ProviderStateRecord,
    running_row: ProviderStateRecord,
    member: String,
    initiator: String,
}

impl Configuration {
    fn capture(service: &BatchService) -> Result<Self, HostProblem> {
        let (scheduler, scheduler_version) = {
            let durable = service
                .scheduler
                .lock()
                .map_err(|_| HostProblem::InfrastructureFailure)?;
            durable.configuration.validate()?;
            (durable.configuration.clone(), durable.store_version)
        };
        let (topology, topology_version) = {
            let durable = service
                .topology
                .lock()
                .map_err(|_| HostProblem::InfrastructureFailure)?;
            durable
                .configuration
                .validate(service.limits.max_nje_nodes, service.limits.max_mas_members)?;
            (durable.configuration.clone(), durable.store_version)
        };
        let scheduler_bytes =
            serde_json::to_vec(&scheduler).map_err(|_| HostProblem::InfrastructureFailure)?;
        let topology_bytes =
            serde_json::to_vec(&topology).map_err(|_| HostProblem::InfrastructureFailure)?;
        let scheduler_row = configuration_row(
            service,
            SCHEDULER_STATE_NAMESPACE,
            scheduler_version,
            &scheduler_bytes,
        )?;
        let topology_row = configuration_row(
            service,
            TOPOLOGY_STATE_NAMESPACE,
            topology_version,
            &topology_bytes,
        )?;
        Ok(Self {
            scheduler,
            topology,
            scheduler_row,
            topology_row,
            scheduler_bytes,
            topology_bytes,
        })
    }

    fn same(&self, other: &Self) -> bool {
        self.scheduler_bytes == other.scheduler_bytes && self.topology_bytes == other.topology_bytes
    }
}

fn configuration_row(
    service: &BatchService,
    namespace: &str,
    version: u64,
    bytes: &[u8],
) -> Result<Option<ProviderStateRecord>, HostProblem> {
    let row = service
        .store
        .get_provider_state(namespace, CONFIGURATION_STATE_KEY)
        .map_err(store_error)?;
    match &row {
        None if version == 0 => {}
        Some(row)
            if version > 0
                && row.namespace == namespace
                && row.key == CONFIGURATION_STATE_KEY
                && row.version == version
                && row.payload == bytes => {}
        _ => return Err(HostProblem::IdempotencyConflict),
    }
    Ok(row)
}

fn bounded_records(
    rows: &[ProviderStateRecord],
    max_jobs: usize,
    bytes: usize,
) -> Result<usize, HostProblem> {
    if rows.len() > max_jobs.min(MAX_SELECTION_RECORDS - 2) {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut total = bytes;
    let mut previous = None;
    for row in rows {
        if row.namespace != "jes-job"
            || row.version == 0
            || row.version > i64::MAX as u64
            || durable_job_number(&row.key).is_err()
            || previous.is_some_and(|key: &str| key >= row.key.as_str())
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        previous = Some(row.key.as_str());
        total = total
            .checked_add(row.payload.len())
            .filter(|total| *total <= MAX_SELECTION_BYTES)
            .ok_or(HostProblem::ResourceExhausted)?;
    }
    if total > MAX_SELECTION_BYTES {
        return Err(HostProblem::ResourceExhausted);
    }
    Ok(total)
}

impl BatchService {
    // Snapshot reads are bounded observations, not one graph-wide atomic capture.
    fn selection_capture(
        &self,
        invocation: &Invocation,
        id: &str,
        member: &str,
        initiator: &str,
    ) -> Result<(Vec<ProviderStateRecord>, Configuration, Job, usize), HostProblem> {
        let max_jobs = self.limits.max_jobs.min(MAX_SELECTION_RECORDS - 2);
        let rows = self
            .store
            .list_provider_state("jes-job", max_jobs + 1)
            .map_err(store_error)?;
        // Check physical cardinality and payload budget BEFORE copying any Job.
        let namespace_bytes = bounded_records(&rows, self.limits.max_jobs, 0)?;
        let configuration = Configuration::capture(self)?;
        let bytes = namespace_bytes
            .checked_add(configuration.scheduler_bytes.len())
            .and_then(|bytes| bytes.checked_add(configuration.topology_bytes.len()))
            .filter(|bytes| *bytes <= MAX_SELECTION_BYTES)
            .ok_or(HostProblem::ResourceExhausted)?;
        let definition = configuration
            .topology
            .members
            .get(member)
            .filter(|m| m.enabled && configuration.topology.node_available(&m.node))
            .ok_or(HostProblem::IdempotencyConflict)?;
        let state = self.lock()?;
        if state.jobs.len() != rows.len() {
            return Err(HostProblem::IdempotencyConflict);
        }
        if state
            .jobs
            .values()
            .filter(|job| job.state == JobState::Queued)
            .count()
            > self.limits.max_queued
        {
            return Err(HostProblem::ResourceExhausted);
        }
        let mut class_active = BTreeMap::<char, usize>::new();
        let mut initiator_active = 0;
        let mut member_active = 0;
        for ((key, job), row) in state.jobs.iter().zip(&rows) {
            if key != &row.key || job_record(job)? != *row {
                return Err(HostProblem::IdempotencyConflict);
            }
            validate_durable_job_shape(job, self.jcl_limits, self.limits)?;
            ensure_job_event_capacity(job, self.limits.max_events, 0)?;
            if matches!(job.state, JobState::Selected | JobState::Running) {
                *class_active.entry(job.class).or_default() += 1;
                initiator_active += usize::from(job.initiator.as_deref() == Some(initiator));
                member_active += usize::from(job.route.owner_member.as_deref() == Some(member));
            }
        }
        if self.limits.max_active == 0 || member_active >= definition.max_active {
            return Err(HostProblem::IdempotencyConflict);
        }
        let current = state.jobs.get(id).ok_or(HostProblem::NotFound)?;
        if current.owner != invocation.principal.id().as_str() {
            return Err(HostProblem::Unauthorized);
        }
        if current.state != JobState::Queued
            || current.attempt != 0
            || current.steps.iter().any(|step| step.attempt != 0)
            || !current.spool.is_empty()
            || current.program_registrations.is_empty()
            || current
                .program_registrations
                .values()
                .any(|r| r.handler != RegisteredProgramHandler::ProgramService)
        {
            return Err(HostProblem::Unsupported);
        }
        validate_plan_programs(&current.plan, &current.program_registrations)?;
        configuration.topology.validate_route(&current.route)?;
        if current.route.execution_node != definition.node
            || current
                .route
                .owner_member
                .as_deref()
                .is_some_and(|m| m != member)
            || select_job(
                &configuration.scheduler,
                initiator,
                initiator_active,
                &class_active,
                [JobSelectionCandidate {
                    id,
                    class: current.class,
                    priority: current.priority,
                    state: current.state,
                }],
            )? != Some(id)
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        Ok((rows, configuration, current.clone(), bytes))
    }

    // Private: preflight remains its actual existing builder and audit owner.
    pub(super) fn prepare_selection(
        &self,
        scope: &(impl RunInput + ?Sized),
        id: &str,
        member: &str,
        initiator: &str,
    ) -> Result<PreparedSelection<'_>, HostProblem> {
        if !scope.contained() {
            return Err(HostProblem::Unsupported);
        }
        scope.check()?;
        let (dependencies, configuration, current, bytes) =
            self.selection_capture(scope.original(), id, member, initiator)?;
        self.authorize(
            scope,
            "JESJOBS",
            &format!("JOB.{}", current.name),
            AccessIntent::Execute,
            2,
        )?;
        scope.check()?;
        let edges = SelectionEdges::prepare(&current, self.limits)?;
        if edges.running_version > i64::MAX as u64 {
            return Err(HostProblem::ResourceExhausted);
        }
        let selected = edges.selected(&current, member, initiator, self.limits)?;
        let running = edges.running(&selected, self.limits)?;
        let current_row = job_record(&current)?;
        let selected_row = job_record(&selected)?;
        let running_row = job_record(&running)?;
        let plan = PreparedSelection {
            service: self,
            store: self.store.clone(),
            original: scope.original().clone(),
            dependencies,
            configuration,
            current: current_row,
            selected,
            running,
            selected_row,
            running_row,
            member: member.into(),
            initiator: initiator.into(),
        };
        plan.expected_writes()
            .iter()
            .try_fold(bytes, |total, row| total.checked_add(row.payload.len()))
            .filter(|b| *b <= MAX_SELECTION_BYTES)
            .ok_or(HostProblem::ResourceExhausted)?;
        plan.revalidate(self, scope.original())
            .map_err(|p| scope.poison(p))?;
        Ok(plan)
    }
}

impl PreparedSelection<'_> {
    pub(super) fn current_row(&self) -> &ProviderStateRecord {
        &self.current
    }
    pub(super) fn expected_writes(&self) -> [&ProviderStateRecord; 2] {
        [&self.selected_row, &self.running_row]
    }
    pub(super) fn dependencies(&self) -> &[ProviderStateRecord] {
        &self.dependencies
    }
    pub(super) fn configuration_rows(&self) -> [Option<&ProviderStateRecord>; 2] {
        [
            self.configuration.scheduler_row.as_ref(),
            self.configuration.topology_row.as_ref(),
        ]
    }
    pub(super) fn revalidate(
        &self,
        service: &BatchService,
        invocation: &Invocation,
    ) -> Result<(), HostProblem> {
        if !std::ptr::eq(self.service, service)
            || !Arc::ptr_eq(&self.store, &service.store)
            || self.original != *invocation
        {
            return Err(HostProblem::Unauthorized);
        }
        let (rows, configuration, current, _) = service.selection_capture(
            invocation,
            &self.current.key,
            &self.member,
            &self.initiator,
        )?;
        if rows != self.dependencies()
            || !self.configuration.same(&configuration)
            || self.configuration_rows()
                != [
                    configuration.scheduler_row.as_ref(),
                    configuration.topology_row.as_ref(),
                ]
            || job_record(&current)? != self.current
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        Ok(())
    }
    // Consumed only by the existing owning sequential publication, never a public drive.
    pub(super) fn into_edges(self) -> (Job, Job) {
        (self.selected, self.running)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_preflight_rejects_count_key_order_identity_and_overflow_before_decode() {
        let row = ProviderStateRecord {
            namespace: "jes-job".into(),
            key: "JOB00001".into(),
            version: 1,
            payload: vec![0],
        };
        assert_eq!(
            bounded_records(std::slice::from_ref(&row), 1, MAX_SELECTION_BYTES - 1),
            Ok(MAX_SELECTION_BYTES)
        );
        assert_eq!(
            bounded_records(std::slice::from_ref(&row), 0, 0),
            Err(HostProblem::ResourceExhausted)
        );
        assert_eq!(
            bounded_records(std::slice::from_ref(&row), 1, usize::MAX),
            Err(HostProblem::ResourceExhausted)
        );
        assert_eq!(
            bounded_records(std::slice::from_ref(&row), 1, MAX_SELECTION_BYTES),
            Err(HostProblem::ResourceExhausted)
        );
        assert_eq!(
            bounded_records(&[row.clone(), row.clone()], 2, 0),
            Err(HostProblem::IdempotencyConflict)
        );
        for corrupt in [
            ProviderStateRecord {
                namespace: "other".into(),
                ..row.clone()
            },
            ProviderStateRecord {
                key: "foreign".into(),
                ..row.clone()
            },
            ProviderStateRecord {
                version: 0,
                ..row.clone()
            },
            ProviderStateRecord {
                version: i64::MAX as u64 + 1,
                ..row.clone()
            },
        ] {
            assert_eq!(
                bounded_records(&[corrupt], 1, 0),
                Err(HostProblem::IdempotencyConflict)
            );
        }
        let rows = (1..=MAX_SELECTION_RECORDS - 1)
            .map(|n| ProviderStateRecord {
                key: format!("JOB{n:05}"),
                ..row.clone()
            })
            .collect::<Vec<_>>();
        assert_eq!(
            bounded_records(&rows, MAX_SELECTION_RECORDS, 0),
            Err(HostProblem::ResourceExhausted)
        );
    }
}
