//! Owned logical application recovery operands, independent of provider types.
//! Raw language LL/ZZ framing and AIB masks belong to a future language adapter.

use crate::{HostLimits, HostProblem, ImsCallSyntax, ImsExecutionContext, Mutation};

/// Exact logical XRST selection. No timestamp or snapshot identity is minted by callers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ImsRestartSelection {
    /// Blank I/O area and absent JCL CKPTID: normal start.
    Normal,
    /// A specific one-to-eight byte checkpoint identifier.
    Checkpoint(String),
    /// DFS0540I IIIIDDDHHMMSST selector; needs an authentic context authority.
    Timestamp(String),
    /// Last completed symbolic checkpoint, applicable only to BMP.
    Last,
}

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
    /// Commit database work and release every database PCB position.
    BasicCheckpoint {
        /// Logical checkpoint identifier, one to eight bytes.
        id: String,
    },
    /// Commit work and save up to seven owned application areas.
    SymbolicCheckpoint {
        /// Logical checkpoint identifier, one to eight bytes.
        id: String,
        /// Exact area bytes; lengths are carried by each owned byte vector.
        user_areas: Vec<Vec<u8>>,
    },
    /// Start normally or attempt restart from a symbolic checkpoint.
    Restart {
        /// Resolved logical selector. JCL precedence belongs to the language adapter.
        selection: ImsRestartSelection,
        /// Exact requested area lengths in original order; no arbitrary row mutations.
        area_lengths: Vec<usize>,
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
            ImsRecoveryCall::BasicCheckpoint { id } => validate_checkpoint_id(id)?,
            ImsRecoveryCall::SymbolicCheckpoint { id, user_areas } => {
                validate_checkpoint_id(id)?;
                validate_areas(user_areas.iter().map(Vec::len), limits)?;
            }
            ImsRecoveryCall::Restart {
                selection,
                area_lengths,
            } => {
                match selection {
                    ImsRestartSelection::Checkpoint(id) => validate_checkpoint_id(id)?,
                    ImsRestartSelection::Timestamp(timestamp) => {
                        if timestamp.len() != 14
                            || !timestamp.bytes().take(4).all(|b| b.is_ascii_alphanumeric())
                            || !timestamp.bytes().skip(4).all(|b| b.is_ascii_digit())
                        {
                            return Err(HostProblem::Malformed);
                        }
                    }
                    ImsRestartSelection::Normal | ImsRestartSelection::Last => {}
                }
                validate_areas(area_lengths.iter().copied(), limits)?;
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
    /// Completed checkpoint, with the separate I/O PCB status.
    Checkpointed {
        /// Blank I/O PCB success.
        status: String,
        /// Exact checkpoint ID.
        id: String,
        /// Persistent recovery sequence.
        sequence: u64,
    },
    /// Normal start or verified restart with actual GU observations.
    Restarted {
        /// Blank I/O PCB success; individual GU conditions stay separate.
        status: String,
        /// None means normal start, distinct from a selected checkpoint.
        checkpoint_id: Option<String>,
        /// Restored areas in request order.
        user_areas: Vec<Vec<u8>>,
        /// Actual DB PCB statuses, keyed by one-based PSB PCB number.
        pcb_statuses: Vec<(u16, String)>,
    },
}

impl ImsRecoveryResult {
    /// Validate the exact successful LOG projection and its nonzero sequence.
    pub fn validate(&self) -> Result<(), HostProblem> {
        match self {
            Self::Logged { status, sequence } if status == "  " && *sequence != 0 => Ok(()),
            Self::Checkpointed {
                status,
                id,
                sequence,
            } if status == "  " && *sequence != 0 => validate_checkpoint_id(id),
            Self::Restarted {
                status,
                checkpoint_id,
                user_areas,
                pcb_statuses,
            } if status == "  " => {
                if let Some(id) = checkpoint_id {
                    validate_checkpoint_id(id)?;
                }
                validate_areas(user_areas.iter().map(Vec::len), HostLimits::default())?;
                if pcb_statuses.len() > 64
                    || pcb_statuses.windows(2).any(|pair| pair[0].0 >= pair[1].0)
                    || pcb_statuses
                        .iter()
                        .any(|(pcb, status)| *pcb == 0 || !matches!(status.as_str(), "  " | "GE"))
                    || (checkpoint_id.is_none()
                        && (!user_areas.is_empty() || !pcb_statuses.is_empty()))
                {
                    return Err(HostProblem::Malformed);
                }
                Ok(())
            }
            _ => Err(HostProblem::Malformed),
        }
    }
}

fn validate_checkpoint_id(id: &str) -> Result<(), HostProblem> {
    if id.is_empty() || id.len() > 8 || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        Err(HostProblem::Malformed)
    } else {
        Ok(())
    }
}

fn validate_areas(
    lengths: impl Iterator<Item = usize>,
    limits: HostLimits,
) -> Result<(), HostProblem> {
    let lengths = lengths.collect::<Vec<_>>();
    if lengths.len() > 7
        || lengths
            .iter()
            .any(|length| *length == 0 || *length > limits.max_record_bytes.min(32 * 1024))
    {
        Err(HostProblem::ResourceExhausted)
    } else {
        Ok(())
    }
}

fn valid_name(name: &str, bound: usize) -> bool {
    !name.is_empty()
        && name.len() <= bound.min(8)
        && name
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b"@$#".contains(&b))
}
