//! Selected application receipts remain in the existing recovery replay map.
use super::*;

pub(super) fn replay_data_bound(data: &[u8], limits: RecoveryLimits) -> usize {
    if data.starts_with(APPLICATION_RESULT_DOMAIN) {
        limits.max_state_bytes.min(2 * 1024 * 1024)
    } else {
        limits.max_user_area_bytes
    }
}

impl RecoverySession {
    pub(crate) fn application_replay(
        &self,
        effect_id: &str,
        canonical_digest: [u8; 32],
    ) -> Result<Option<(RecoveryTransition, Vec<u8>)>, RecoveryProblem> {
        let Some(transition) = self.replay(effect_id, canonical_digest)? else {
            return Ok(None);
        };
        let data = self.state.replays[effect_id].returned_data.clone();
        if !data.starts_with(APPLICATION_RESULT_DOMAIN) {
            return Err(RecoveryProblem::CorruptImage);
        }
        Ok(Some((transition, data)))
    }

    /// Bind a staged owned operation to canonical host identity and its result.
    /// This accepts neither caller row identities nor resource mutations.
    pub(crate) fn bind_application_result(
        &self,
        mut transition: RecoveryTransition,
        effect_id: &str,
        canonical_digest: [u8; 32],
        data: Vec<u8>,
    ) -> Result<RecoveryTransition, RecoveryProblem> {
        if !data.starts_with(APPLICATION_RESULT_DOMAIN)
            || data.len() > replay_data_bound(&data, self.limits)
            || transition.replayed
        {
            return Err(RecoveryProblem::InvalidRequest);
        }
        let Some(ProviderStateMutation::Put(write)) = &mut transition.mutation else {
            return Err(RecoveryProblem::InvalidRequest);
        };
        if write.record.namespace != ROW_NAMESPACE
            || write.record.key != self.state.run
            || write.expected_version != self.version
        {
            return Err(RecoveryProblem::Conflict);
        }
        let mut state: StoredState = serde_json::from_slice(&write.record.payload)
            .map_err(|_| RecoveryProblem::CorruptImage)?;
        let record = state
            .replays
            .get_mut(effect_id)
            .ok_or(RecoveryProblem::CorruptImage)?;
        record.request_digest = canonical_digest;
        record.returned_data = data;
        state.seal();
        state.verify(&self.state.run, self.limits)?;
        write.record.payload =
            serde_json::to_vec(&state).map_err(|_| RecoveryProblem::InfrastructureFailure)?;
        if write.record.payload.len() > self.limits.max_state_bytes {
            return Err(RecoveryProblem::LimitExceeded);
        }
        Ok(transition)
    }
}
