//! Projection of the existing unpublished proposal; never a second cursor or replay authority.
use super::*;
use crate::ImsDatabasePcbMetadata;
use mainframe_env_host_api::{
    ImsPcbFeedbackRequestV1, ImsPcbFeedbackResultV1, ImsPcbFeedbackUnsupportedV1 as Unproved,
    ImsPcbFeedbackV1, ImsPcbKeyFeedbackV1,
};

pub(in crate::service) struct ExecutionOutput {
    pub result: ImsResult,
    pub address: Option<mainframe_env_host_api::ImsGsamAddress>,
    pub feedback: Option<ImsPcbFeedbackV1>,
}

impl ExecutionOutput {
    pub(in crate::service) fn feedback_result(self) -> Result<ImsPcbFeedbackResultV1, HostProblem> {
        Ok(ImsPcbFeedbackResultV1 {
            result: self.result,
            feedback: self.feedback.ok_or(HostProblem::InfrastructureFailure)?,
        })
    }
    pub(in crate::service) fn gsam_result(self) -> mainframe_env_host_api::ImsGsamResult {
        mainframe_env_host_api::ImsGsamResult {
            result: self.result,
            address: self.address,
        }
    }
}

impl ImsService {
    pub fn execute_pcb_feedback_v1(
        &self,
        invocation: &Invocation,
        request: &ImsPcbFeedbackRequestV1,
    ) -> Result<ImsPcbFeedbackResultV1, HostProblem> {
        self.execute_feedback_at(invocation, request, invocation.deadline_tick)
    }
    pub(in crate::service) fn execute_feedback_at(
        &self,
        invocation: &Invocation,
        request: &ImsPcbFeedbackRequestV1,
        tick: u64,
    ) -> Result<ImsPcbFeedbackResultV1, HostProblem> {
        request.validate(mainframe_env_host_api::HostLimits::default())?;
        let navigation = request.navigation();
        self.execute_operands_at(
            invocation,
            &request.request,
            tick,
            navigation.as_ref(),
            None,
            Some(request),
        )?
        .feedback_result()
    }
}

pub(in crate::service) fn prepare(
    state: &State,
    run: &str,
    request: &ImsPcbFeedbackRequestV1,
) -> Result<(), HostProblem> {
    let session = state.sessions.get(run).ok_or(HostProblem::NotFound)?;
    if !session.generic {
        return Err(HostProblem::Unsupported);
    }
    let pcb = generic::session_pcb(state, run, request.request.pcb)?;
    let metadata = state.metadata.as_ref().ok_or(HostProblem::Unsupported)?;
    let db = metadata
        .databases
        .iter()
        .find(|db| normalize(&db.name) == normalize(&pcb.database))
        .ok_or(HostProblem::InfrastructureFailure)?;
    if !matches!(
        db.organization,
        crate::ImsDatabaseOrganization::Hdam
            | crate::ImsDatabaseOrganization::Hidam
            | crate::ImsDatabaseOrganization::Hisam
            | crate::ImsDatabaseOrganization::Shisam
    ) {
        return Err(HostProblem::Unsupported);
    }
    Ok(())
}

pub(in crate::service) fn project(
    state: &State,
    run: &str,
    request: &ImsPcbFeedbackRequestV1,
    result: &ImsResult,
    limits: ImsLimits,
) -> Result<ImsPcbFeedbackV1, HostProblem> {
    let pcb = generic::session_pcb(state, run, request.request.pcb)?;
    let key = if result.status != "  " {
        ImsPcbKeyFeedbackV1::Unsupported(Unproved::FailedCallWitness)
    } else if pcb.secondary_index.is_some() && request.request.operation == ImsOperation::Replace {
        ImsPcbKeyFeedbackV1::InvalidatedSecondaryReplace
    } else if pcb.secondary_index.is_some() {
        ImsPcbKeyFeedbackV1::Unsupported(Unproved::SecondarySequence)
    } else if matches!(
        request.request.operation,
        ImsOperation::Replace | ImsOperation::Delete
    ) {
        ImsPcbKeyFeedbackV1::Unsupported(Unproved::NonKeyOperation)
    } else {
        primary_key(state, run, request, pcb, limits)?
    };
    Ok(ImsPcbFeedbackV1 {
        pcb: request.request.pcb,
        database: normalize(&pcb.database),
        processing_options: pcb.processing_options.clone(),
        sensitive_segment_count: u32::try_from(pcb.sensitive_segments.len())
            .map_err(|_| HostProblem::ResourceExhausted)?,
        transferred_data_length: result
            .segments
            .iter()
            .try_fold(0u64, |n, s| n.checked_add(s.data.len() as u64))
            .ok_or(HostProblem::ResourceExhausted)?,
        key,
    })
}

fn primary_key(
    state: &State,
    run: &str,
    request: &ImsPcbFeedbackRequestV1,
    pcb: &ImsDatabasePcbMetadata,
    limits: ImsLimits,
) -> Result<ImsPcbKeyFeedbackV1, HostProblem> {
    let engine = generic::restored(state, &normalize(&pcb.database), limits)?;
    let session = state.sessions.get(run).ok_or(HostProblem::NotFound)?;
    let current = generic::pcb::position(session, request.request.pcb)
        .current()
        .ok_or(HostProblem::InfrastructureFailure)?;
    let path = engine
        .path_to(current)
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    let metadata = state
        .metadata
        .as_ref()
        .ok_or(HostProblem::InfrastructureFailure)?;
    let db = metadata
        .databases
        .iter()
        .find(|db| normalize(&db.name) == normalize(&pcb.database))
        .ok_or(HostProblem::InfrastructureFailure)?;
    if !db.logical_relationships.is_empty() {
        return Ok(ImsPcbKeyFeedbackV1::Unsupported(
            Unproved::LogicalRelationship,
        ));
    }
    let mut bytes = Vec::new();
    for view in &path {
        let segment = db
            .segments
            .iter()
            .find(|s| normalize(&s.name) == view.segment)
            .ok_or(HostProblem::InfrastructureFailure)?;
        let Some(field) = segment.fields.iter().find(|f| f.sequence) else {
            return Ok(ImsPcbKeyFeedbackV1::Unsupported(
                Unproved::MissingSequenceField,
            ));
        };
        let key = view
            .data
            .get(
                field.offset
                    ..field
                        .offset
                        .checked_add(field.length)
                        .ok_or(HostProblem::InfrastructureFailure)?,
            )
            .ok_or(HostProblem::InfrastructureFailure)?;
        if bytes
            .len()
            .checked_add(key.len())
            .is_none_or(|n| n > request.key_capacity as usize)
        {
            return Err(HostProblem::ResourceExhausted);
        }
        bytes.extend_from_slice(key);
    }
    let last = path.last().ok_or(HostProblem::InfrastructureFailure)?;
    Ok(ImsPcbKeyFeedbackV1::Valid {
        segment_name: last.segment.clone(),
        segment_level: u16::try_from(path.len()).map_err(|_| HostProblem::ResourceExhausted)?,
        bytes,
    })
}
