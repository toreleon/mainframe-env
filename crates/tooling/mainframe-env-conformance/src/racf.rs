use mainframe_env_coverage::{
    CompiledSpec, ConformanceDriver, ConformanceLimits, ConformanceObservation,
    ConformancePredicate, DriverOutput, DriverRef, FixtureRef, ObservationCheck, ObservationRef,
    PredicateRef, RuntimeRegistry, SpecProblem,
};
use mainframe_env_execution_api::{InvocationLimits, PrincipalId};
use mainframe_env_host_api::SecretRef;
use mainframe_env_racf::{
    CommandContext, CommandDiagnosticCode, CommandFamily, MemorySecretResolver, RacfService,
    command_descriptors, recognize_command, validate_command,
};
use mainframe_env_store::MemoryStore;
use mainframe_env_store_api::ProviderStateStore;
use std::sync::Arc;

struct RacfCommandDriver;
struct RacfReady;
struct RacfPassed;

static RACF_DRIVER: RacfCommandDriver = RacfCommandDriver;
static RACF_READY: RacfReady = RacfReady;
static RACF_PASSED: RacfPassed = RacfPassed;

pub fn racf_runtime(spec: &CompiledSpec) -> Result<RuntimeRegistry<'static>, SpecProblem> {
    let limits = ConformanceLimits::default();
    RuntimeRegistry::new(
        spec,
        vec![(DriverRef::new("racf.command.driver", limits)?, &RACF_DRIVER)],
        vec![(
            PredicateRef::new("racf.authority.ready", limits)?,
            &RACF_READY,
        )],
        vec![(
            ObservationRef::new("racf.command.passed", limits)?,
            &RACF_PASSED,
        )],
        limits,
    )
}

impl ConformanceDriver for RacfCommandDriver {
    fn execute(&self, fixture: &FixtureRef) -> Result<DriverOutput, String> {
        let parts = fixture.as_str().split('.').collect::<Vec<_>>();
        if parts.len() != 6 || parts[0] != "racf" || parts[1] != "command" || parts[5] != "fixture"
        {
            return Err("unknown RACF fixture identity".into());
        }
        let sequence = parts[2]
            .parse::<usize>()
            .map_err(|_| "invalid RACF fixture sequence")?;
        let descriptor = command_descriptors()
            .get(
                sequence
                    .checked_sub(1)
                    .ok_or("invalid RACF fixture sequence")?,
            )
            .ok_or("RACF fixture sequence exceeds catalog")?;
        let obligation = parts[3];
        let gate = parts[4];
        match (obligation, gate) {
            ("syntax", "recognized") => {
                let actual = recognize_command(descriptor.keyword(), Default::default())
                    .map_err(|problem| problem.to_string())?;
                if actual != descriptor.family() {
                    return Err("RACF recognition selected the wrong family".into());
                }
            }
            ("syntax", "validated") => {
                let actual = validate_command(valid_form(descriptor.family()), Default::default())
                    .map_err(|problem| problem.to_string())?;
                if actual.family != descriptor.family() {
                    return Err("RACF validation selected the wrong family".into());
                }
            }
            ("malformed", "conditioned") => {
                let malformed = format!("{} UNKNOWN(value)", valid_form(descriptor.family()));
                let problem = validate_command(&malformed, Default::default())
                    .expect_err("RACF malformed fixture unexpectedly validated");
                if problem.code != CommandDiagnosticCode::UnknownOperand {
                    return Err(format!("unexpected redacted RACF diagnostic: {problem}"));
                }
            }
            ("authorized", "executed") => execute_matrix(descriptor.family())?,
            ("unauthorized", "executed" | "conditioned") => execute_denied(descriptor.family())?,
            _ => return Err("RACF fixture obligation/gate is unsupported".into()),
        }
        DriverOutput::new(
            format!("racf:{}:{obligation}:{gate}:pass", descriptor.keyword()).into_bytes(),
            ConformanceLimits::default(),
        )
        .map_err(|problem| problem.to_string())
    }
}

impl ConformancePredicate for RacfReady {
    fn evaluate(&self, fixture: &FixtureRef) -> Result<bool, String> {
        Ok(command_descriptors().len() == 34 && fixture.as_str().starts_with("racf.command."))
    }
}

impl ConformanceObservation for RacfPassed {
    fn evaluate(&self, output: &DriverOutput) -> Result<ObservationCheck, String> {
        let actual = std::str::from_utf8(output.bytes())
            .map_err(|_| "RACF driver output is not UTF-8")?
            .to_string();
        ObservationCheck::new(
            actual.starts_with("racf:") && actual.ends_with(":pass"),
            "bounded RACF selected-route pass observation",
            actual,
            ConformanceLimits::default(),
        )
        .map_err(|problem| problem.to_string())
    }
}

fn setup() -> Result<(Arc<RacfService>, CommandContext), String> {
    let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
    let secrets = Arc::new(MemorySecretResolver::default());
    secrets.insert("secret:admin", b"ADMIN-PASSWORD".to_vec());
    let service = RacfService::open(store, secrets, Default::default())
        .map_err(|problem| format!("RACF setup failed: {problem:?}"))?;
    service
        .bootstrap_administrator(
            "RACFADM",
            &SecretRef::new("secret:admin", Default::default())
                .map_err(|problem| format!("RACF bootstrap reference failed: {problem:?}"))?,
        )
        .map_err(|problem| format!("RACF bootstrap failed: {problem:?}"))?;
    let context = CommandContext::new(
        PrincipalId::new("RACFADM", InvocationLimits::default())
            .map_err(|problem| problem.to_string())?,
        "MATRIX-BASE",
        "RACF-CONFORMANCE",
        1,
    )
    .map_err(|problem| problem.to_string())?;
    Ok((service, context))
}

fn context(base: &CommandContext, sequence: usize) -> Result<CommandContext, String> {
    CommandContext::new(
        base.actor().clone(),
        format!("MATRIX-{sequence:04}"),
        base.correlation(),
        base.tick() + u64::try_from(sequence).map_err(|_| "RACF sequence overflow")?,
    )
    .map_err(|problem| problem.to_string())
}

fn execute_matrix(target: CommandFamily) -> Result<(), String> {
    let (service, base) = setup()?;
    for (sequence, command) in sec_502_matrix().iter().enumerate() {
        let result = service
            .execute_command(&context(&base, sequence + 1)?, command)
            .map_err(|problem| format!("RACF selected route failed: {problem}"))?;
        if result.family == target {
            return Ok(());
        }
    }
    Err("RACF executable family has no selected-route fixture".into())
}

fn execute_denied(target: CommandFamily) -> Result<(), String> {
    let (service, _) = setup()?;
    let before = service
        .database()
        .summary()
        .map_err(|problem| format!("RACF summary failed: {problem:?}"))?;
    let denied = CommandContext::new(
        PrincipalId::new("MISSING", InvocationLimits::default())
            .map_err(|problem| problem.to_string())?,
        format!("DENY-{:?}", target).to_ascii_uppercase(),
        "RACF-DENY-CONFORMANCE",
        2,
    )
    .map_err(|problem| problem.to_string())?;
    let problem = service
        .execute_command(&denied, valid_form(target))
        .expect_err("unauthorized RACF selected route unexpectedly succeeded");
    if problem.code != CommandDiagnosticCode::Unauthorized {
        return Err(format!(
            "unexpected redacted RACF deny diagnostic: {problem}"
        ));
    }
    let after = service
        .database()
        .summary()
        .map_err(|problem| format!("RACF summary failed: {problem:?}"))?;
    if (before.principals, before.groups, before.profiles)
        != (after.principals, after.groups, after.profiles)
        || after.audits != before.audits + 1
    {
        return Err("RACF denial mutated protected state or omitted audit".into());
    }
    Ok(())
}

fn sec_502_matrix() -> &'static [&'static str] {
    &[
        "ADDGROUP OPER OWNER(RACFADM)",
        "ADDGROUP DEV SUPGROUP(OPER) OWNER(RACFADM)",
        "ALTGROUP DEV DATA('DEVELOPMENT GROUP')",
        "ADDUSER USER2 DFLTGRP(DEV)",
        "ALTUSER USER2 DATA('DEVELOPER')",
        "CONNECT USER2 GROUP(OPER) AUTHORITY(USE)",
        "REMOVE USER2 GROUP(OPER)",
        "ADDSD 'USER2.**' GENERIC OWNER(USER2) UACC(NONE)",
        "ALTDSD 'USER2.**' AUDIT(ALL) LEVEL(1)",
        "PERMIT 'USER2.**' CLASS(DATASET) ID(USER2) ACCESS(READ)",
        "LISTDSD 'USER2.**' ALL",
        "RDEFINE FACILITY DEV.RESOURCE OWNER(USER2) UACC(NONE)",
        "RALTER FACILITY DEV.RESOURCE AUDIT(FAILURES)",
        "RLIST FACILITY DEV.RESOURCE ALL",
        "SEARCH CLASS(FACILITY) MASK(DEV)",
        "LISTUSER USER2 ALL",
        "LISTGRP DEV ALL",
        "DISPLAY ALL",
        "RDELETE FACILITY DEV.RESOURCE",
        "DELDSD 'USER2.**' GENERIC",
        "DELUSER USER2",
        "DELGROUP DEV",
    ]
}

fn valid_form(family: CommandFamily) -> &'static str {
    match family {
        CommandFamily::AddGroup => "ADDGROUP GROUP1",
        CommandFamily::AddSd => "ADDSD 'USER1.**'",
        CommandFamily::AddUser => "ADDUSER USER1",
        CommandFamily::AltSd => "ALTDSD 'USER1.**'",
        CommandFamily::AltGroup => "ALTGROUP GROUP1",
        CommandFamily::AltUser => "ALTUSER USER1",
        CommandFamily::Connect => "CONNECT USER1 GROUP(GROUP1)",
        CommandFamily::DelSd => "DELDSD 'USER1.**'",
        CommandFamily::DelGroup => "DELGROUP GROUP1",
        CommandFamily::DelUser => "DELUSER USER1",
        CommandFamily::Display => "DISPLAY ALL",
        CommandFamily::ListDsd => "LISTDSD 'USER1.**'",
        CommandFamily::ListGrp => "LISTGRP GROUP1",
        CommandFamily::ListUser => "LISTUSER USER1",
        CommandFamily::Password => "PASSWORD PASSWORD(old new)",
        CommandFamily::Permit => "PERMIT 'USER1.**' CLASS(DATASET) ID(USER1) ACCESS(READ)",
        CommandFamily::Racdcert => "RACDCERT LIST",
        CommandFamily::Raclink => "RACLINK USER1 LIST",
        CommandFamily::Racmap => "RACMAP LIST",
        CommandFamily::Racpriv => "RACPRIV LIST",
        CommandFamily::Racprmck => "RACPRMCK MEMBER(IRROPT01)",
        CommandFamily::Ralter => "RALTER FACILITY APP.RESOURCE",
        CommandFamily::Rdefine => "RDEFINE FACILITY APP.RESOURCE",
        CommandFamily::Rdelete => "RDELETE FACILITY APP.RESOURCE",
        CommandFamily::Remove => "REMOVE USER1 GROUP(GROUP1)",
        CommandFamily::Restart => "RESTART",
        CommandFamily::Rlist => "RLIST FACILITY APP.RESOURCE",
        CommandFamily::Rvary => "RVARY LIST",
        CommandFamily::Search => "SEARCH CLASS(DATASET)",
        CommandFamily::Set => "SET LIST",
        CommandFamily::Setropts => "SETROPTS LIST",
        CommandFamily::Signoff => "SIGNOFF LIST",
        CommandFamily::Stop => "STOP",
        CommandFamily::Target => "TARGET LIST",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_generated_families_have_valid_independent_forms() {
        for descriptor in command_descriptors() {
            let validated = validate_command(valid_form(descriptor.family()), Default::default())
                .unwrap_or_else(|problem| panic!("redacted fixture failed: {problem}"));
            assert_eq!(validated.family, descriptor.family());
        }
    }

    #[test]
    fn all_sec_502_families_have_executable_and_deny_routes() {
        for descriptor in command_descriptors()
            .iter()
            .filter(|descriptor| descriptor.work_package() == "SEC-502")
        {
            execute_matrix(descriptor.family()).unwrap();
            execute_denied(descriptor.family()).unwrap();
        }
    }
}
