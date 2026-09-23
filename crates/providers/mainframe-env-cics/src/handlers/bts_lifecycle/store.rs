use super::*;

/// Store facade used by all BTS process/activity commands.
pub struct BtsLifecycleStore<'a> {
    store: &'a dyn ProviderStateStore,
}

impl<'a> BtsLifecycleStore<'a> {
    pub fn new(store: &'a dyn ProviderStateStore) -> Self {
        Self { store }
    }

    /// The key is an unambiguous encoding of the full process-type/name pair.
    pub fn process_key(process_type: &str, name: &str) -> Result<String, HostProblem> {
        validate_name(process_type, 8, true)?;
        validate_name(name, 36, true)?;
        let mut key = String::with_capacity((process_type.len() + name.len()) * 2 + 1);
        for byte in process_type.bytes() {
            key.push_str(&format!("{byte:02x}"));
        }
        key.push('/');
        for byte in name.bytes() {
            key.push_str(&format!("{byte:02x}"));
        }
        Ok(key)
    }

    /// Stable, opaque 52-character root identity for one process incarnation.
    pub fn root_id(process_type: &str, name: &str) -> Result<String, HostProblem> {
        let key = Self::process_key(process_type, name)?;
        Ok(activity_id(&key, 0))
    }

    /// Stable, opaque child identity within one process row's monotonic sequence.
    pub fn child_id(process_type: &str, name: &str, sequence: u64) -> Result<String, HostProblem> {
        if sequence == 0 {
            return Err(HostProblem::Malformed);
        }
        let key = Self::process_key(process_type, name)?;
        Ok(activity_id(&key, sequence))
    }

    pub fn load_process(
        &self,
        process_type: &str,
        name: &str,
    ) -> Result<Option<BtsProcess>, HostProblem> {
        let key = Self::process_key(process_type, name)?;
        let Some(row) = self
            .store
            .get_provider_state(PROCESS_NAMESPACE, &key)
            .map_err(store_error)?
        else {
            return Ok(None);
        };
        if row.version == 0 || row.payload.len() > MAX_ROW_BYTES {
            return Err(HostProblem::InfrastructureFailure);
        }
        let mut process: BtsProcess =
            serde_json::from_slice(&row.payload).map_err(|_| HostProblem::InfrastructureFailure)?;
        process.row_version = row.version;
        process.validate()?;
        if process.process_type != process_type || process.name != name {
            return Err(HostProblem::InfrastructureFailure);
        }
        Ok(Some(process))
    }

    pub fn load_acquisition(&self, run_unit: &str) -> Result<Option<BtsAcquisition>, HostProblem> {
        validate_identifier(run_unit, 256)?;
        let Some(row) = self
            .store
            .get_provider_state(ACQUISITION_NAMESPACE, run_unit)
            .map_err(store_error)?
        else {
            return Ok(None);
        };
        if row.version == 0 || row.payload.len() > 2048 {
            return Err(HostProblem::InfrastructureFailure);
        }
        let mut acquisition: BtsAcquisition =
            serde_json::from_slice(&row.payload).map_err(|_| HostProblem::InfrastructureFailure)?;
        acquisition.row_version = row.version;
        acquisition.validate()?;
        Ok(Some(acquisition))
    }

    /// Resolve a child or root identity through its exact persisted index row.
    pub fn load_activity_index(
        &self,
        activity_id: &str,
    ) -> Result<Option<BtsActivityIndex>, HostProblem> {
        validate_activity_id(activity_id)?;
        let Some(row) = self
            .store
            .get_provider_state(ACTIVITY_INDEX_NAMESPACE, activity_id)
            .map_err(store_error)?
        else {
            return Ok(None);
        };
        if row.version == 0 || row.payload.len() > 1024 {
            return Err(HostProblem::InfrastructureFailure);
        }
        let mut index: BtsActivityIndex =
            serde_json::from_slice(&row.payload).map_err(|_| HostProblem::InfrastructureFailure)?;
        index.row_version = row.version;
        index.validate()?;
        if index.activity_id != activity_id {
            return Err(HostProblem::InfrastructureFailure);
        }
        Ok(Some(index))
    }

    /// Persist a new process and its defining UOW acquisition together.
    pub fn define_process(
        &self,
        process: BtsProcess,
        run_unit: &str,
        owner_execution: &str,
        owner_principal: &str,
    ) -> Result<(), HostProblem> {
        validate_identifier(run_unit, 256)?;
        if process.pending_uow.as_deref() != Some(run_unit) || process.row_version != 0 {
            return Err(HostProblem::Malformed);
        }
        if process.root_id != Self::root_id(&process.process_type, &process.name)? {
            return Err(HostProblem::Malformed);
        }
        let mut acquisition = self
            .load_acquisition(run_unit)?
            .unwrap_or(BtsAcquisition::empty(owner_execution, owner_principal)?);
        if acquisition.owner_execution != owner_execution
            || acquisition.owner_principal != owner_principal
            || acquisition.is_held()
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        acquisition.process_type = Some(process.process_type.clone());
        acquisition.process_name = Some(process.name.clone());
        acquisition.activity_id = Some(process.root_id.clone());
        let key = Self::process_key(&process.process_type, &process.name)?;
        let root_index = BtsActivityIndex {
            schema_version: ACTIVITY_INDEX_SCHEMA.into(),
            activity_id: process.root_id.clone(),
            process_type: process.process_type.clone(),
            process_name: process.name.clone(),
            parent_id: None,
            pending_uow: Some(run_unit.into()),
            row_version: 0,
        };
        self.store
            .mutate_provider_states_atomic(vec![
                put_process(&key, &process, None)?,
                put_acquisition(run_unit, &acquisition)?,
                put_activity_index(&root_index, None)?,
            ])
            .map_err(store_error)
    }

    /// Acquire one published process or descendant in the current UOW.
    /// An activity may be held by only one UOW at a time.
    pub fn acquire(
        &self,
        run_unit: &str,
        owner_execution: &str,
        owner_principal: &str,
        process_type: &str,
        process_name: &str,
        activity_id: &str,
    ) -> Result<(), HostProblem> {
        validate_identifier(run_unit, 256)?;
        validate_activity_id(activity_id)?;
        let key = Self::process_key(process_type, process_name)?;
        for _ in 0..MAX_CAS_ATTEMPTS {
            let index = self
                .load_activity_index(activity_id)?
                .ok_or(HostProblem::NotFound)?;
            if index.process_type != process_type
                || index.process_name != process_name
                || index
                    .pending_uow
                    .as_deref()
                    .is_some_and(|owner| owner != run_unit)
            {
                return Err(HostProblem::NotFound);
            }
            let mut process = self
                .load_process(process_type, process_name)?
                .ok_or(HostProblem::NotFound)?;
            if !process.visible_to(run_unit) {
                return Err(HostProblem::NotFound);
            }
            let root = activity_id == process.root_id;
            let activity = process
                .activities
                .get_mut(activity_id)
                .ok_or(HostProblem::InfrastructureFailure)?;
            if activity.acquired_by.is_some() {
                return Err(HostProblem::Condition {
                    name: if root { "PROCESSBUSY" } else { "ACTIVITYBUSY" }.into(),
                    response: if root { 106 } else { 107 },
                    response2: if root { 13 } else { 19 },
                });
            }
            let mut acquisition = self
                .load_acquisition(run_unit)?
                .unwrap_or(BtsAcquisition::empty(owner_execution, owner_principal)?);
            if acquisition.owner_execution != owner_execution
                || acquisition.owner_principal != owner_principal
                || acquisition.is_held()
            {
                return Err(HostProblem::IdempotencyConflict);
            }
            activity.acquired_by = Some(run_unit.into());
            acquisition.process_type = Some(process_type.into());
            acquisition.process_name = Some(process_name.into());
            acquisition.activity_id = Some(activity_id.into());
            let expected = process.row_version;
            process.epoch = process
                .epoch
                .checked_add(1)
                .ok_or(HostProblem::ResourceExhausted)?;
            match self.store.mutate_provider_states_atomic(vec![
                put_process(&key, &process, Some(expected))?,
                put_acquisition(run_unit, &acquisition)?,
            ]) {
                Ok(()) => return Ok(()),
                Err(StoreError::Conflict | StoreError::AlreadyExists) => continue,
                Err(error) => return Err(store_error(error)),
            }
        }
        Err(HostProblem::UnknownOutcome)
    }

    /// Atomically mutate one stable process tree and save its exact reply.
    /// The closure may run more than once after CAS conflicts and must perform
    /// no I/O. Child insertion/deletion uses dedicated index-aware methods.
    pub fn mutate_process(
        &self,
        process_type: &str,
        process_name: &str,
        run_unit: &str,
        owner_execution: &str,
        owner_principal: &str,
        replay_key: &str,
        request_digest: [u8; 32],
        transition: impl Fn(&mut BtsProcess) -> Result<BtsReply, HostProblem>,
    ) -> Result<BtsReply, HostProblem> {
        validate_identifier(run_unit, 256)?;
        validate_identifier(owner_execution, 256)?;
        validate_identifier(owner_principal, 256)?;
        validate_identifier(replay_key, 256)?;
        let key = Self::process_key(process_type, process_name)?;
        for _ in 0..MAX_CAS_ATTEMPTS {
            let mut process = self
                .load_process(process_type, process_name)?
                .ok_or(HostProblem::NotFound)?;
            if !process.visible_to(run_unit) {
                return Err(HostProblem::NotFound);
            }
            if let Some(saved) = process.replays.get(replay_key) {
                if saved.owner_execution != owner_execution
                    || saved.owner_run_unit != run_unit
                    || saved.owner_principal != owner_principal
                    || saved.request_digest != request_digest
                {
                    return Err(HostProblem::IdempotencyConflict);
                }
                return Ok(BtsReply {
                    condition: saved.condition.clone(),
                    response: saved.response,
                    response2: saved.response2,
                    outputs: saved.outputs.clone(),
                });
            }
            if process.replays.len() >= MAX_REPLAYS {
                return Err(HostProblem::ResourceExhausted);
            }
            let old = process.clone();
            let reply = transition(&mut process)?;
            if process.schema_version != old.schema_version
                || process.process_type != old.process_type
                || process.name != old.name
                || process.root_id != old.root_id
                || process.pending_uow != old.pending_uow
                || process.activities.keys().ne(old.activities.keys())
                || reply.condition.is_empty()
                || reply.condition.len() > 32
                || reply.outputs.len() > 16
                || reply.outputs.values().any(|bytes| bytes.len() > 1024)
            {
                return Err(HostProblem::Malformed);
            }
            process.epoch = old
                .epoch
                .checked_add(1)
                .ok_or(HostProblem::ResourceExhausted)?;
            process.replays.insert(
                replay_key.into(),
                BtsReplay {
                    owner_execution: owner_execution.into(),
                    owner_run_unit: run_unit.into(),
                    owner_principal: owner_principal.into(),
                    request_digest,
                    condition: reply.condition.clone(),
                    response: reply.response,
                    response2: reply.response2,
                    outputs: reply.outputs.clone(),
                },
            );
            match self.store.mutate_provider_states_atomic(vec![put_process(
                &key,
                &process,
                Some(old.row_version),
            )?]) {
                Ok(()) => return Ok(reply),
                Err(StoreError::Conflict | StoreError::AlreadyExists) => continue,
                Err(error) => return Err(store_error(error)),
            }
        }
        Err(HostProblem::UnknownOutcome)
    }

    /// End a UOW, publishing or rolling back a pending DEFINE and releasing
    /// its acquisition in one store transaction. The tombstone fences ABA.
    pub fn finish_uow(
        &self,
        run_unit: &str,
        owner_execution: &str,
        owner_principal: &str,
        commit: bool,
    ) -> Result<(), HostProblem> {
        for _ in 0..MAX_CAS_ATTEMPTS {
            let Some(mut current) = self.load_acquisition(run_unit)? else {
                return Ok(());
            };
            if current.owner_execution != owner_execution
                || current.owner_principal != owner_principal
            {
                return Err(HostProblem::IdempotencyConflict);
            }
            if !current.is_held() {
                return Ok(());
            }
            let process_type = current
                .process_type
                .clone()
                .ok_or(HostProblem::InfrastructureFailure)?;
            let process_name = current
                .process_name
                .clone()
                .ok_or(HostProblem::InfrastructureFailure)?;
            let activity_id = current
                .activity_id
                .clone()
                .ok_or(HostProblem::InfrastructureFailure)?;
            let mut process = self
                .load_process(&process_type, &process_name)?
                .ok_or(HostProblem::InfrastructureFailure)?;
            let index = self
                .load_activity_index(&activity_id)?
                .ok_or(HostProblem::InfrastructureFailure)?;
            if index.process_type != process_type || index.process_name != process_name {
                return Err(HostProblem::InfrastructureFailure);
            }
            let activity = process
                .activities
                .get_mut(&activity_id)
                .ok_or(HostProblem::InfrastructureFailure)?;
            if activity.acquired_by.as_deref() != Some(run_unit) {
                return Err(HostProblem::InfrastructureFailure);
            }
            activity.acquired_by = None;
            current.epoch = current
                .epoch
                .checked_add(1)
                .ok_or(HostProblem::ResourceExhausted)?;
            current.process_type = None;
            current.process_name = None;
            current.activity_id = None;
            let key = Self::process_key(&process_type, &process_name)?;
            let mut writes = vec![put_acquisition(run_unit, &current)?];
            if process.pending_uow.as_deref() == Some(run_unit) && !commit {
                writes.push(ProviderStateMutation::Delete {
                    namespace: PROCESS_NAMESPACE.into(),
                    key,
                    expected_version: process.row_version,
                });
                writes.push(ProviderStateMutation::Delete {
                    namespace: ACTIVITY_INDEX_NAMESPACE.into(),
                    key: activity_id,
                    expected_version: index.row_version,
                });
            } else {
                if process.pending_uow.as_deref() == Some(run_unit) {
                    process.pending_uow = None;
                    let mut published = index;
                    published.pending_uow = None;
                    writes.push(put_activity_index(&published, Some(published.row_version))?);
                }
                let expected = process.row_version;
                process.epoch = process
                    .epoch
                    .checked_add(1)
                    .ok_or(HostProblem::ResourceExhausted)?;
                writes.push(put_process(&key, &process, Some(expected))?);
            }
            match self.store.mutate_provider_states_atomic(writes) {
                Ok(()) => return Ok(()),
                Err(StoreError::Conflict | StoreError::AlreadyExists) => continue,
                Err(error) => return Err(store_error(error)),
            }
        }
        Err(HostProblem::UnknownOutcome)
    }
}
