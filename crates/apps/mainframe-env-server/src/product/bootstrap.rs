use super::{BOOTSTRAP_CAS_ATTEMPTS, ProductServer};
use mainframe_env_host_api::{AccessIntent, HostProblem};

impl ProductServer {
    pub(super) fn grant_bootstrap_profiles(&self, user: &str) -> Result<(), HostProblem> {
        for (class, pattern, access) in [
            (
                "DATASET",
                format!("{}.**", user.to_ascii_uppercase()),
                AccessIntent::Alter,
            ),
            ("JESJOBS", "JOB.**".into(), AccessIntent::Alter),
            ("FACILITY", "CONSOLE.**".into(), AccessIntent::Alter),
            ("FACILITY", "CICS.COMMAND.**".into(), AccessIntent::Alter),
            ("TCICSTRN", "CICS.**".into(), AccessIntent::Execute),
            ("DB2TABLE", "**".into(), AccessIntent::Alter),
            ("DB2PLAN", "**".into(), AccessIntent::Control),
            ("DB2UOW", "**".into(), AccessIntent::Control),
            ("IMSPSB", "**".into(), AccessIntent::Execute),
            ("IMSDB", "**".into(), AccessIntent::Alter),
            ("IMSUOW", "**".into(), AccessIntent::Control),
            ("MQQUEUE", "**".into(), AccessIntent::Alter),
            ("MQUOW", "**".into(), AccessIntent::Control),
        ] {
            let mut granted = false;
            for _ in 0..BOOTSTRAP_CAS_ATTEMPTS {
                match self.racf.define_profile(class, &pattern, user, None) {
                    Ok(()) | Err(HostProblem::IdempotencyConflict) => {}
                    Err(problem) => return Err(problem),
                }
                match self.racf.permit(class, &pattern, user, access) {
                    Ok(()) => {
                        granted = true;
                        break;
                    }
                    Err(HostProblem::IdempotencyConflict | HostProblem::NotFound) => continue,
                    Err(problem) => return Err(problem),
                }
            }
            if !granted {
                return Err(HostProblem::IdempotencyConflict);
            }
        }
        Ok(())
    }
}
