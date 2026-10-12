//! Existing snapshot access bodies, isolated with the finite composition quarantine guard.
use super::*;
impl SecurityDatabase {
    pub(super) fn initialize_retention_clock(&self) -> Result<(), HostProblem> {
        self.mutate_if_changed(|snapshot| {
            let observed = snapshot.max_record_retention_tick();
            if snapshot.retention_tick == 0 && observed != 0 {
                snapshot.retention_tick = observed;
                Ok(((), true))
            } else {
                Ok(((), false))
            }
        })
        .map(|_| ())
    }

    pub(crate) fn read(&self) -> Result<SecurityDatabaseSnapshot, HostProblem> {
        self.require_composition_known()?;
        let _writer = self
            .writer
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        self.require_composition_known()?;
        let record = self
            .store
            .get_provider_state(DATABASE_NAMESPACE, DATABASE_KEY)
            .map_err(store_problem)?
            .ok_or(HostProblem::InfrastructureFailure)?;
        decode_snapshot(&record, self.limits)
    }

    pub(crate) fn mutate<T>(
        &self,
        change: impl FnOnce(&mut SecurityDatabaseSnapshot) -> Result<T, HostProblem>,
    ) -> Result<(T, u64), HostProblem> {
        self.mutate_if_changed(|snapshot| change(snapshot).map(|result| (result, true)))
    }

    pub(crate) fn mutate_if_changed<T>(
        &self,
        change: impl FnOnce(&mut SecurityDatabaseSnapshot) -> Result<(T, bool), HostProblem>,
    ) -> Result<(T, u64), HostProblem> {
        self.require_composition_known()?;
        let _writer = self
            .writer
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        self.require_composition_known()?;
        let record = self
            .store
            .get_provider_state(DATABASE_NAMESPACE, DATABASE_KEY)
            .map_err(store_problem)?
            .ok_or(HostProblem::InfrastructureFailure)?;
        let mut snapshot = decode_snapshot(&record, self.limits)?;
        let (result, changed) = change(&mut snapshot)?;
        if !changed {
            return Ok((result, snapshot.generation));
        }
        snapshot.generation = snapshot
            .generation
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        let payload = encode_snapshot(&snapshot, self.limits)?;
        self.store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: DATABASE_NAMESPACE.into(),
                    key: DATABASE_KEY.into(),
                    version: snapshot.generation,
                    payload,
                },
                Some(record.version),
            )
            .map_err(store_problem)?;
        Ok((result, snapshot.generation))
    }

    pub(crate) fn mutate_retry<T>(
        &self,
        mut change: impl FnMut(&mut SecurityDatabaseSnapshot) -> Result<(T, bool), HostProblem>,
    ) -> Result<(T, u64), HostProblem> {
        const MAX_ATTEMPTS: usize = 4;
        self.require_composition_known()?;
        let _writer = self
            .writer
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        self.require_composition_known()?;
        for attempt in 0..MAX_ATTEMPTS {
            let record = self
                .store
                .get_provider_state(DATABASE_NAMESPACE, DATABASE_KEY)
                .map_err(store_problem)?
                .ok_or(HostProblem::InfrastructureFailure)?;
            let mut snapshot = decode_snapshot(&record, self.limits)?;
            let (result, changed) = change(&mut snapshot)?;
            if !changed {
                return Ok((result, snapshot.generation));
            }
            snapshot.generation = snapshot
                .generation
                .checked_add(1)
                .ok_or(HostProblem::ResourceExhausted)?;
            let payload = encode_snapshot(&snapshot, self.limits)?;
            match self.store.put_provider_state(
                ProviderStateRecord {
                    namespace: DATABASE_NAMESPACE.into(),
                    key: DATABASE_KEY.into(),
                    version: snapshot.generation,
                    payload,
                },
                Some(record.version),
            ) {
                Ok(()) => return Ok((result, snapshot.generation)),
                Err(StoreError::Conflict) if attempt + 1 < MAX_ATTEMPTS => {}
                Err(problem) => return Err(store_problem(problem)),
            }
        }
        Err(HostProblem::IdempotencyConflict)
    }

    /// Resolve a duration-based observation tick without advancing time per request.
    pub(crate) fn retention_observation_tick(
        &self,
        supplied_tick: u64,
    ) -> Result<u64, HostProblem> {
        self.require_composition_known()?;
        let floor = if supplied_tick == 0 {
            retention_wall_tick()?
        } else {
            supplied_tick
        };
        match self.store.advance_logical_clock(floor) {
            Ok(tick) => Ok(tick.max(floor)),
            // Provider-state test doubles and older embedding adapters may not expose the
            // dedicated clock yet. A caller-supplied logical tick remains authoritative; a
            // direct route falls back to the sampled duration clock rather than call count.
            Err(StoreError::InvalidTransition) => Ok(floor),
            Err(problem) => Err(store_problem(problem)),
        }
    }
}
