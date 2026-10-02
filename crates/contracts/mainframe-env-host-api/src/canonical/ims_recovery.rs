//! Additive canonical wire identities; no existing request or result changes.
use super::*;

impl Canonical for ImsRecoveryCall {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        match self {
            Self::Log { code, data } => {
                out.variant("ImsRecoveryCall", "Log", 2)?;
                out.text("code")?;
                code.encode(out)?;
                out.text("data")?;
                data.encode(out)
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
            Self::Logged { sequence, status } => {
                out.variant("ImsRecoveryResult", "Logged", 2)?;
                out.text("sequence")?;
                sequence.encode(out)?;
                out.text("status")?;
                status.encode(out)
            }
        }
    }
}
