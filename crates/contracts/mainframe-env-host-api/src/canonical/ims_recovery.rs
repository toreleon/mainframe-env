//! Additive canonical wire identities; no existing request or result changes.
use super::*;

impl Canonical for ImsRecoveryCall {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        match self {
            Self::Sets { token, user_data } | Self::Setu { token, user_data } => {
                let variant = if matches!(self, Self::Sets { .. }) {
                    "Sets"
                } else {
                    "Setu"
                };
                out.variant("ImsRecoveryCall", variant, 2)?;
                out.text("token")?;
                token.encode(out)?;
                out.text("user_data")?;
                user_data.encode(out)
            }
            Self::Rols { token, area_length } => {
                out.variant("ImsRecoveryCall", "Rols", 2)?;
                out.text("area_length")?;
                area_length.encode(out)?;
                out.text("token")?;
                token.encode(out)
            }
            Self::Rolb => out.variant("ImsRecoveryCall", "Rolb", 0),
            Self::Roll => out.variant("ImsRecoveryCall", "Roll", 0),
            Self::Log { code, data } => {
                out.variant("ImsRecoveryCall", "Log", 2)?;
                out.text("code")?;
                code.encode(out)?;
                out.text("data")?;
                data.encode(out)
            }
            Self::BasicCheckpoint { id } => {
                out.variant("ImsRecoveryCall", "BasicCheckpoint", 1)?;
                out.text("id")?;
                id.encode(out)
            }
            Self::SymbolicCheckpoint { id, user_areas } => {
                out.variant("ImsRecoveryCall", "SymbolicCheckpoint", 2)?;
                out.text("id")?;
                id.encode(out)?;
                out.text("user_areas")?;
                user_areas.encode(out)
            }
            Self::Restart {
                selection,
                area_lengths,
            } => {
                out.variant("ImsRecoveryCall", "Restart", 2)?;
                out.text("area_lengths")?;
                area_lengths.encode(out)?;
                out.text("selection")?;
                selection.encode(out)
            }
        }
    }
}

impl Canonical for ImsRestartSelection {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        match self {
            Self::Normal => out.variant("ImsRestartSelection", "Normal", 0),
            Self::Last => out.variant("ImsRestartSelection", "Last", 0),
            Self::Checkpoint(id) => {
                out.variant("ImsRestartSelection", "Checkpoint", 1)?;
                out.text("0")?;
                id.encode(out)
            }
            Self::Timestamp(timestamp) => {
                out.variant("ImsRestartSelection", "Timestamp", 1)?;
                out.text("0")?;
                timestamp.encode(out)
            }
        }
    }
}

impl Canonical for ImsRecoveryRequest {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let Self {
            application,
            call,
            context,
            database,
            mutation,
            package_identity,
            psb,
            syntax,
        } = self;
        out.object("ImsRecoveryRequest", 8)?;
        out.text("application")?;
        application.encode(out)?;
        out.text("call")?;
        call.encode(out)?;
        out.text("context")?;
        context.encode(out)?;
        out.text("database")?;
        database.encode(out)?;
        out.text("mutation")?;
        mutation.encode(out)?;
        out.text("package_identity")?;
        package_identity.encode(out)?;
        out.text("psb")?;
        psb.encode(out)?;
        out.text("syntax")?;
        syntax.encode(out)
    }
}

impl Canonical for ImsRecoveryResult {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        match self {
            Self::Savepoint { status } => {
                out.variant("ImsRecoveryResult", "Savepoint", 1)?;
                out.text("status")?;
                status.encode(out)
            }
            Self::BackedOut { status, user_data } => {
                out.variant("ImsRecoveryResult", "BackedOut", 2)?;
                out.text("status")?;
                status.encode(out)?;
                out.text("user_data")?;
                user_data.encode(out)
            }
            Self::Abended { code } => {
                out.variant("ImsRecoveryResult", "Abended", 1)?;
                out.text("code")?;
                code.encode(out)
            }
            Self::Logged { sequence, status } => {
                out.variant("ImsRecoveryResult", "Logged", 2)?;
                out.text("sequence")?;
                sequence.encode(out)?;
                out.text("status")?;
                status.encode(out)
            }
            Self::Checkpointed {
                id,
                sequence,
                status,
            } => {
                out.variant("ImsRecoveryResult", "Checkpointed", 3)?;
                out.text("id")?;
                id.encode(out)?;
                out.text("sequence")?;
                sequence.encode(out)?;
                out.text("status")?;
                status.encode(out)
            }
            Self::Restarted {
                checkpoint_id,
                pcb_statuses,
                status,
                user_areas,
            } => {
                out.variant("ImsRecoveryResult", "Restarted", 4)?;
                out.text("checkpoint_id")?;
                checkpoint_id.encode(out)?;
                out.text("pcb_statuses")?;
                pcb_statuses.encode(out)?;
                out.text("status")?;
                status.encode(out)?;
                out.text("user_areas")?;
                user_areas.encode(out)
            }
        }
    }
}
