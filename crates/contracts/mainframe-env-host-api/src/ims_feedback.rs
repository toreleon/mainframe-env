//! Versioned owned database PCB feedback, not a COBOL/C mask or physical ABI.
use crate::{
    HostLimits, HostProblem, HostRequest, HostResult, ImsExecutionContext, ImsNavigationRequest,
    ImsOperation, ImsRequest, ImsResult,
};
use serde::{Deserialize, Serialize};

/// A distinct route preserves historical request/result canonical bytes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImsPcbFeedbackRequestV1 {
    /// Existing database operation and selected PCB identity.
    pub request: ImsRequest,
    /// Database execution-context applicability.
    pub context: ImsExecutionContext,
    /// None uses existing typed operands; Some uses existing display-code SSA authority.
    pub ssas: Option<Vec<Vec<u8>>>,
    /// Maximum valid key bytes accepted. No truncation or partial publication.
    pub key_capacity: u32,
}

impl ImsPcbFeedbackRequestV1 {
    /// Reject ambiguous operands, unsupported contexts and oversized inputs.
    pub fn validate(&self, limits: HostLimits) -> Result<(), HostProblem> {
        if !matches!(
            self.request.operation,
            ImsOperation::GetUnique
                | ImsOperation::GetNext
                | ImsOperation::GetNextParent
                | ImsOperation::GetHoldUnique
                | ImsOperation::GetHoldNext
                | ImsOperation::GetHoldNextParent
                | ImsOperation::Insert
                | ImsOperation::Replace
                | ImsOperation::Delete
        ) || self.request.psb.is_some()
            || self.request.checkpoint_id.is_some()
            || self.request.system.is_some()
        {
            return Err(HostProblem::Malformed);
        }
        if !matches!(
            self.context,
            ImsExecutionContext::DbBatch | ImsExecutionContext::DbDc | ImsExecutionContext::Dbctl
        ) {
            return Err(HostProblem::Unsupported);
        }
        if self.key_capacity as usize > limits.max_record_bytes {
            return Err(HostProblem::ResourceExhausted);
        }
        if let Some(ssas) = &self.ssas {
            ImsNavigationRequest::validate_operands(&self.request, ssas, limits)?;
        }
        if !matches!(
            self.request.operation,
            ImsOperation::Insert | ImsOperation::Replace
        ) && !self.request.data.is_empty()
            || matches!(
                self.request.operation,
                ImsOperation::Replace | ImsOperation::Delete
            ) && (!self.request.segments.is_empty() || !self.request.qualifiers.is_empty())
        {
            return Err(HostProblem::Malformed);
        }
        HostRequest::Ims(self.request.clone()).validate(limits)
    }

    #[must_use]
    /// Borrow the existing SSA route's semantics when display-code operands exist.
    pub fn navigation(&self) -> Option<ImsNavigationRequest> {
        self.ssas.as_ref().map(|ssas| ImsNavigationRequest {
            request: self.request.clone(),
            context: self.context,
            ssas: ssas.clone(),
        })
    }
}

/// Required authority that this projection does not have. No guessed empty key.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ImsPcbFeedbackUnsupportedV1 {
    /// The proposal lacks the current call's last-satisfied-path witness.
    FailedCallWitness,
    /// Exact selected secondary-sequence key layout is unproved.
    SecondarySequence,
    /// Primary REPL/DLET key validity is unproved.
    NonKeyOperation,
    /// An occurrence's metadata has no sequence-field recipe.
    MissingSequenceField,
    /// Logical-route concatenation requires its owner's feedback recipe.
    LogicalRelationship,
}

/// Validity and source availability of the owned key projection.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub enum ImsPcbKeyFeedbackV1 {
    /// Only valid bytes, from root to selected occurrence; never stale area tails.
    Valid {
        /// Lowest selected segment of the successful call.
        segment_name: String,
        /// One-based level in the selected primary path.
        segment_level: u16,
        /// Current concatenated root-to-selected sequence-field bytes.
        bytes: Vec<u8>,
    },
    /// Source-defined invalidity after successful REPL through secondary sequence.
    InvalidatedSecondaryReplace,
    /// Required source or proposal authority is unavailable.
    Unsupported(ImsPcbFeedbackUnsupportedV1),
}

impl ImsPcbKeyFeedbackV1 {
    #[must_use]
    /// Return a byte length only when key feedback is valid.
    pub fn valid_length(&self) -> Option<usize> {
        match self {
            Self::Valid { bytes, .. } => Some(bytes.len()),
            _ => None,
        }
    }
}

/// Available selected-PCB metadata and feedback from a single proposal.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ImsPcbFeedbackV1 {
    /// One-based selected database PCB number.
    pub pcb: u16,
    /// Normalized selected database name.
    pub database: String,
    /// Validated PCB-level PROCOPT metadata, distinct from SENSEG overrides.
    pub processing_options: String,
    /// Sensitive segments declared by the selected PCB's metadata.
    pub sensitive_segment_count: u32,
    /// Bytes transferred in the owned result, not a raw PCB field or area capacity.
    pub transferred_data_length: u64,
    /// Valid key bytes or the exact unsupported/invalidated class.
    pub key: ImsPcbKeyFeedbackV1,
}

/// Versioned owned response retained by the existing receipt authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImsPcbFeedbackResultV1 {
    /// Status and data remain owned by the existing proposal.
    pub result: ImsResult,
    /// Feedback derived from the same result and unpublished proposal.
    pub feedback: ImsPcbFeedbackV1,
}

impl ImsPcbFeedbackResultV1 {
    /// Validate bounds, status context, transferred length and validity shape.
    pub fn validate(&self, limits: HostLimits) -> Result<(), HostProblem> {
        HostResult::Ims(self.result.clone()).validate(limits)?;
        crate::resolve_ims_status(
            self.result.status.as_bytes(),
            crate::ImsStatusContext::Database,
            crate::ImsPcbKind::Database,
        )
        .map_err(|_| HostProblem::Malformed)?;
        let f = &self.feedback;
        if f.pcb == 0
            || f.database.is_empty()
            || f.database.len() > limits.max_name_bytes
            || f.processing_options.is_empty()
            || f.processing_options.len() > 4
            || !f.processing_options.bytes().all(|b| b.is_ascii_uppercase())
            || f.sensitive_segment_count == 0
            || f.sensitive_segment_count as usize > limits.max_fields
            || self.result.checkpoint_id.is_some()
            || self.result.system.is_some()
            || self
                .result
                .segments
                .iter()
                .try_fold(0u64, |n, s| n.checked_add(s.data.len() as u64))
                != Some(f.transferred_data_length)
        {
            return Err(HostProblem::Malformed);
        }
        match &f.key {
            ImsPcbKeyFeedbackV1::Valid {
                segment_name,
                segment_level,
                bytes,
            } => {
                if self.result.status != "  "
                    || segment_name.is_empty()
                    || segment_name.len() > limits.max_name_bytes
                    || *segment_level == 0
                    || *segment_level > 15
                    || bytes.len() > limits.max_record_bytes
                {
                    return Err(HostProblem::Malformed);
                }
            }
            ImsPcbKeyFeedbackV1::InvalidatedSecondaryReplace if self.result.status != "  " => {
                return Err(HostProblem::Malformed);
            }
            _ => {}
        }
        Ok(())
    }
}
