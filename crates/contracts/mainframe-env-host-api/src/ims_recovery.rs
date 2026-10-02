//! Owned logical application recovery operands, independent of provider types.
//! Raw language LL/ZZ framing and AIB masks belong to a future language adapter.

use crate::{HostLimits, HostProblem, ImsCallSyntax, ImsExecutionContext, Mutation};

/// A bounded recovery operation. Additional families require their own real adapter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ImsRecoveryCall {
    /// Append caller text under an application log code (A0 through FF).
    Log {
        /// Caller-assigned application log code in A0–FF.
        code: u8,
        /// Exact logical text bytes, without language framing.
        data: Vec<u8>,
    },
}

/// Selected application binding plus canonical mutation identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImsRecoveryRequest {
    /// Published application metadata selection.
    pub application: String,
    /// Exact content identity of the selected package.
    pub package_identity: String,
    /// Selected PSB whose database PCB binds the authorization resource.
    pub psb: String,
    /// Database in the selected PSB, used for authorization and publication fencing.
    /// LOG never mutates this database or its PCB position.
    pub database: String,
    /// Source execution environment, currently restricted to DB batch.
    pub context: ImsExecutionContext,
    /// Source call syntax, currently restricted to CALL.
    pub syntax: ImsCallSyntax,
    /// Logical operation and its owned operands.
    pub call: ImsRecoveryCall,
    /// Outer effect sequence and idempotency identity.
    pub mutation: Mutation,
}

impl ImsRecoveryRequest {
    /// Validate the supported logical form; unsupported contexts never become success.
    pub fn validate(&self, limits: HostLimits) -> Result<(), HostProblem> {
        if self.application.is_empty()
            || self.application.len() > limits.max_name_bytes.min(128)
            || !self
                .application
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
            || !valid_name(&self.psb, limits.max_name_bytes)
            || !valid_name(&self.database, limits.max_name_bytes)
            || !self
                .package_identity
                .strip_prefix("sha256:")
                .is_some_and(|digest| {
                    digest.len() == 64
                        && digest
                            .bytes()
                            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                })
        {
            return Err(HostProblem::Malformed);
        }
        self.mutation.validate(limits)?;
        if self.mutation.transaction.is_some() {
            return Err(HostProblem::Unsupported);
        }
        // This slice owns a DB-batch CALL projection only. Other source-applicable
        // contexts must acquire real route owners before they can be admitted.
        if self.context != ImsExecutionContext::DbBatch || self.syntax != ImsCallSyntax::Call {
            return Err(HostProblem::Unsupported);
        }
        match &self.call {
            ImsRecoveryCall::Log { code, data } => {
                if *code < 0xa0 {
                    return Err(HostProblem::Malformed);
                }
                if data.len()
                    > limits
                        .max_record_bytes
                        .min(32 * 1024)
                        .min(u16::MAX as usize - 5)
                {
                    return Err(HostProblem::ResourceExhausted);
                }
            }
        }
        Ok(())
    }
}

/// Recovery response with the I/O PCB status, separate from database PCB results.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ImsRecoveryResult {
    /// Durable log sequence; this is not a database position or physical log address.
    Logged {
        /// Exact blank success status from the I/O PCB projection.
        status: String,
        /// Nonzero sequence in the persistent recovery log.
        sequence: u64,
    },
}

impl ImsRecoveryResult {
    /// Validate the exact successful LOG projection and its nonzero sequence.
    pub fn validate(&self) -> Result<(), HostProblem> {
        match self {
            Self::Logged { status, sequence } if status == "  " && *sequence != 0 => Ok(()),
            _ => Err(HostProblem::Malformed),
        }
    }
}

fn valid_name(name: &str, bound: usize) -> bool {
    !name.is_empty()
        && name.len() <= bound.min(8)
        && name
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b"@$#".contains(&b))
}
