//! Logical GSAM addresses for the owned host interface, never IBM physical RSA bytes.
use crate::{
    HostLimits, HostProblem, HostRequest, HostResult, ImsExecutionContext, ImsOperation,
    ImsRequest, ImsResult,
};
use serde::{Deserialize, Serialize};

/// Opaque identity issued for a record by GN/ISRT. Not an ordinal, key, RBA or TTR.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ImsGsamAddress {
    /// Normalized metadata database identity.
    pub database: String,
    /// Fixed-size opaque host token; raw physical RSA bytes are not accepted.
    pub token: [u8; 32],
}
impl ImsGsamAddress {
    /// Validate shape and finite name bounds before address lookup.
    pub fn validate(&self, limits: HostLimits) -> Result<(), HostProblem> {
        if self.database.is_empty()
            || self.database.len() > limits.max_name_bytes
            || self.database.len() > 64
            || !self
                .database
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b"@#$".contains(&b))
            || self.token == [0; 32]
        {
            return Err(HostProblem::Malformed);
        }
        Ok(())
    }
}
/// Source-applicable GSAM direct positioning operand in the owned host interface.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ImsGsamSearchArgument {
    /// Owned equivalent of the source-defined reset-to-start RSA sentinel.
    Beginning,
    /// Address previously issued by a successful GN or ISRT.
    Record(ImsGsamAddress),
}
/// Bounded GU/GN/ISRT operands with explicit context and saved-address selection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImsGsamRequest {
    /// Owned input counterpart of U's four-byte PCB length, ISRT only.
    pub undefined_length: Option<u32>,
    /// Existing effect identity/PCB/data envelope; SSA and hierarchy operands are forbidden.
    pub request: ImsRequest,
    /// Standalone DL/I batch; BMP/JBP need an explicit region identity.
    pub context: ImsExecutionContext,
    /// GU address or start sentinel; absent GU returns AH without changing position.
    pub search: Option<ImsGsamSearchArgument>,
    /// Optional fourth GN/ISRT operand. GU does not generate a new saved address.
    pub save_address: bool,
}
impl ImsGsamRequest {
    /// Reject ambiguous shapes and oversized operands before dispatch.
    pub fn validate(&self, limits: HostLimits) -> Result<(), HostProblem> {
        if !matches!(
            self.request.operation,
            ImsOperation::GetUnique | ImsOperation::GetNext | ImsOperation::Insert
        ) || !self.request.segments.is_empty()
            || !self.request.qualifiers.is_empty()
            || self.request.psb.is_some()
            || self.request.checkpoint_id.is_some()
            || self.request.system.is_some()
            || self.request.q_class.is_some()
            || self.request.operation != ImsOperation::Insert && !self.request.data.is_empty()
            || self.request.operation != ImsOperation::GetUnique && self.search.is_some()
            || self.request.operation == ImsOperation::GetUnique && self.save_address
        {
            return Err(HostProblem::Malformed);
        }
        if self.undefined_length.is_some_and(|length| {
            self.request.operation != ImsOperation::Insert
                || length < 12
                || length as usize != self.request.data.len()
                || length > 32760
        }) {
            return Err(HostProblem::Malformed);
        }
        if let Some(ImsGsamSearchArgument::Record(address)) = &self.search {
            address.validate(limits)?;
        }
        HostRequest::Ims(self.request.clone()).validate(limits)
    }
}
/// Additive result shape; historical `ImsResult` canonical bytes stay frozen.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImsGsamResult {
    /// Owned output counterpart of U's PCB length on successful GN/GU.
    pub undefined_length: Option<u32>,
    /// Existing bounded status/data result; synthetic metadata record names are host-only.
    pub result: ImsResult,
    /// Saved fourth-operand output for successful GN/ISRT, when requested.
    pub address: Option<ImsGsamAddress>,
}
impl ImsGsamResult {
    /// Validate bounded output and no data/address on a condition status.
    pub fn validate(&self, limits: HostLimits) -> Result<(), HostProblem> {
        HostResult::Ims(self.result.clone()).validate(limits)?;
        crate::resolve_ims_status(
            self.result.status.as_bytes(),
            crate::ImsStatusContext::Database,
            crate::ImsPcbKind::Gsam,
        )
        .map_err(|_| HostProblem::Malformed)?;
        if self.result.segments.len() > 1
            || self.result.system.is_some()
            || self.result.checkpoint_id.is_some()
            || self.result.affected_segments > 1
            || self.result.segments.iter().any(|s| s.parent_key.is_some())
            || self.result.status != "  "
                && (!self.result.segments.is_empty()
                    || self.address.is_some()
                    || self.result.affected_segments != 0)
        {
            return Err(HostProblem::Malformed);
        }
        if let Some(address) = &self.address {
            address.validate(limits)?;
        }
        if self.undefined_length.is_some_and(|length| {
            self.result.status != "  "
                || self.result.segments.len() != 1
                || !(12..=32760).contains(&length)
                || self.result.segments[0].data.len() != length as usize
        }) {
            return Err(HostProblem::Malformed);
        }
        Ok(())
    }
}
