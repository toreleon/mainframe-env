use mainframe_env_coverage::{
    CompiledSpec, ConformanceDriver, ConformanceLimits, ConformanceObservation,
    ConformancePredicate, DriverOutput, DriverRef, FixtureRef, ObservationCheck, ObservationRef,
    PredicateRef, RuntimeRegistry, SpecProblem,
};
use mainframe_env_execution_api::{InvocationLimits, PrincipalId};
use mainframe_env_host_api::SecretRef;
use mainframe_env_racf::{
    AccessEnvironment, AccessLevel, CommandContext, CommandDiagnosticCode, CommandFamily,
    DecisionOutcome, DecisionReason, MemorySecretResolver, RacfService, RacrouteRequest,
    RacrouteRequestType, RacrouteResult, SafDefineAction, SafExtractKind, SafRequestContext,
    SafVerifyAction, TokenKind, command_descriptors, racroute_descriptors, recognize_command,
    validate_command,
};
use mainframe_env_store::MemoryStore;
use mainframe_env_store_api::ProviderStateStore;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

struct RacfCommandDriver;
struct RacrouteDriver;
struct RacfReady;
struct RacfPassed;

static RACF_DRIVER: RacfCommandDriver = RacfCommandDriver;
static RACROUTE_DRIVER: RacrouteDriver = RacrouteDriver;
static RACF_READY: RacfReady = RacfReady;
static RACF_PASSED: RacfPassed = RacfPassed;

pub fn racf_runtime(spec: &CompiledSpec) -> Result<RuntimeRegistry<'static>, SpecProblem> {
    let limits = ConformanceLimits::default();
    RuntimeRegistry::new(
        spec,
        vec![
            (
                DriverRef::new("racf.command.driver", limits)?,
                &RACF_DRIVER as &dyn ConformanceDriver,
            ),
            (
                DriverRef::new("racf.racroute.driver", limits)?,
                &RACROUTE_DRIVER as &dyn ConformanceDriver,
            ),
        ],
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

impl ConformanceDriver for RacrouteDriver {
    fn execute(&self, fixture: &FixtureRef) -> Result<DriverOutput, String> {
        let parts = fixture.as_str().split('.').collect::<Vec<_>>();
        if parts.len() != 6 || parts[0] != "racf" || parts[1] != "racroute" || parts[5] != "fixture"
        {
            return Err("unknown RACROUTE fixture identity".into());
        }
        let sequence = parts[2]
            .parse::<usize>()
            .map_err(|_| "invalid RACROUTE fixture sequence")?;
        let descriptor = racroute_descriptors()
            .get(
                sequence
                    .checked_sub(1)
                    .ok_or("invalid RACROUTE fixture sequence")?,
            )
            .ok_or("RACROUTE fixture sequence exceeds catalog")?;
        let obligation = parts[3];
        let gate = parts[4];
        match (obligation, gate) {
            ("syntax", "recognized") => {
                if racroute_descriptors()
                    .iter()
                    .filter(|candidate| candidate.keyword() == descriptor.keyword())
                    .count()
                    != 1
                {
                    return Err("RACROUTE generated recognition is ambiguous".into());
                }
            }
            ("syntax", "validated") => {
                if shape_request(descriptor.request_type()).request_type()
                    != descriptor.request_type()
                {
                    return Err("RACROUTE typed request selected the wrong state machine".into());
                }
            }
            ("authorized", "executed") => {
                execute_racroute_case(descriptor.request_type(), RacrouteCase::Allowed)?
            }
            ("unauthorized", "executed" | "conditioned") => {
                execute_racroute_case(descriptor.request_type(), RacrouteCase::Denied)?
            }
            ("malformed", "conditioned") => {
                execute_racroute_case(descriptor.request_type(), RacrouteCase::Malformed)?
            }
            _ => return Err("RACROUTE fixture obligation/gate is unsupported".into()),
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
        Ok(command_descriptors().len() == 34
            && racroute_descriptors().len() == 14
            && (fixture.as_str().starts_with("racf.command.")
                || fixture.as_str().starts_with("racf.racroute.")))
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
    secrets.insert("secret:user1", b"USER-PASSWORD".to_vec());
    secrets.insert("secret:bad", b"WRONG-PASSWORD".to_vec());
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

#[derive(Clone, Copy)]
enum RacrouteCase {
    Allowed,
    Denied,
    Malformed,
}

fn execute_racroute_case(
    request_type: RacrouteRequestType,
    mode: RacrouteCase,
) -> Result<(), String> {
    let (service, base) = setup()?;
    for (sequence, command) in [
        "ADDUSER USER1 PASSWORD('USER-PASSWORD')",
        "RDEFINE FACILITY APP.** OWNER(RACFADM) UACC(ALTER)",
        "RDEFINE RRSFDATA DIRECT.NODE1 OWNER(RACFADM) UACC(READ)",
        "SETROPTS CLASSACT(RRSFDATA) RACLIST(FACILITY)",
    ]
    .into_iter()
    .enumerate()
    {
        service
            .execute_command(&context(&base, 100 + sequence)?, command)
            .map_err(|problem| problem.to_string())?;
    }
    let admin = base.actor().clone();
    let verify = service
        .racroute(
            &saf_context(&admin, None, "PREP-VERIFY", 10)?,
            RacrouteRequest::Verify {
                user: admin.clone(),
                credential_reference: SecretRef::new("secret:admin", Default::default())
                    .map_err(|problem| format!("RACROUTE secret reference: {problem:?}"))?,
                action: SafVerifyAction::CreateAcee,
                acee_id: None,
            },
        )
        .map_err(|problem| format!("RACROUTE ACEE preparation: {problem:?}"))?;
    let admin_acee = match verify.result {
        Some(RacrouteResult::Verified {
            acee: Some(acee), ..
        }) => acee.id,
        _ => return Err("RACROUTE ACEE preparation returned no ACEE".into()),
    };
    let token_digest = digest('2');
    let token_id = if matches!(
        request_type,
        RacrouteRequestType::Tokenmap | RacrouteRequestType::Tokenxtr
    ) {
        let built = service
            .racroute(
                &saf_context(&admin, Some(&admin_acee), "PREP-TOKEN", 11)?,
                RacrouteRequest::Tokenbld {
                    owner: admin.clone(),
                    kind: TokenKind::SafIdentity,
                    token_reference: "secret:conformance-token".into(),
                    token_digest: token_digest.clone(),
                    scopes: BTreeSet::from(["FACILITY".into()]),
                    expires_tick: Some(100),
                },
            )
            .map_err(|problem| format!("RACROUTE token preparation: {problem:?}"))?;
        match built.result {
            Some(RacrouteResult::TokenBuilt(token)) => Some(token.id),
            _ => return Err("RACROUTE token preparation returned no token".into()),
        }
    } else {
        None
    };
    let caller = if matches!(mode, RacrouteCase::Denied) {
        PrincipalId::new("MISSING", InvocationLimits::default())
            .map_err(|problem| problem.to_string())?
    } else {
        admin.clone()
    };
    let descriptor = racroute_descriptors()
        .iter()
        .find(|descriptor| descriptor.request_type() == request_type)
        .ok_or("missing RACROUTE descriptor")?;
    let acee = if matches!(mode, RacrouteCase::Allowed | RacrouteCase::Malformed)
        && descriptor.requires_acee()
    {
        Some(admin_acee.as_str())
    } else {
        None
    };
    let request = match mode {
        RacrouteCase::Malformed => malformed_request(request_type, &admin, &admin_acee),
        _ => allowed_request(
            request_type,
            &admin,
            &admin_acee,
            token_id.as_deref(),
            &token_digest,
        ),
    };
    let before = service
        .database()
        .summary()
        .map_err(|problem| format!("RACROUTE summary: {problem:?}"))?;
    let outcome = service
        .racroute(&saf_context(&caller, acee, "CASE-REQUEST", 20)?, request)
        .map_err(|problem| format!("RACROUTE selected route failed: {problem:?}"))?;
    match mode {
        RacrouteCase::Allowed if outcome.status.reason != DecisionReason::Granted => Err(format!(
            "RACROUTE allowed route denied with {:?}",
            outcome.status.reason
        )),
        RacrouteCase::Denied | RacrouteCase::Malformed
            if outcome.status.reason == DecisionReason::Granted =>
        {
            Err("RACROUTE negative route unexpectedly allowed".into())
        }
        RacrouteCase::Denied => {
            let after = service
                .database()
                .summary()
                .map_err(|problem| format!("RACROUTE summary: {problem:?}"))?;
            if (
                before.principals,
                before.groups,
                before.profiles,
                before.acees,
                before.tokens,
            ) != (
                after.principals,
                after.groups,
                after.profiles,
                after.acees,
                after.tokens,
            ) || after.audits != before.audits + 1
            {
                return Err("RACROUTE denial mutated protected state or omitted audit".into());
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn saf_context(
    caller: &PrincipalId,
    acee: Option<&str>,
    id: &str,
    tick: u64,
) -> Result<SafRequestContext, String> {
    SafRequestContext::new(
        caller.clone(),
        acee.map(str::to_string),
        None,
        id,
        "RACF-CONFORMANCE",
        tick,
    )
    .map_err(|problem| format!("RACROUTE context: {problem:?}"))
}

fn digest(value: char) -> String {
    format!("sha256:{}", value.to_string().repeat(64))
}

fn shape_request(request_type: RacrouteRequestType) -> RacrouteRequest {
    let principal =
        PrincipalId::new("RACFADM", InvocationLimits::default()).expect("static principal");
    allowed_request(
        request_type,
        &principal,
        "ACEE00000000000000000001000000",
        Some("TOKEN00000000000000000001000000"),
        &digest('2'),
    )
}

fn allowed_request(
    request_type: RacrouteRequestType,
    principal: &PrincipalId,
    parent_acee: &str,
    token_id: Option<&str>,
    token_digest: &str,
) -> RacrouteRequest {
    match request_type {
        RacrouteRequestType::Audit => RacrouteRequest::Audit {
            action: "CONFORMANCE.AUDIT".into(),
            resource_digest: digest('1'),
            decision: DecisionOutcome::Allow,
            fields: BTreeMap::new(),
        },
        RacrouteRequestType::Auth => RacrouteRequest::Auth {
            class: "FACILITY".into(),
            resource: "APP.ONE".into(),
            access: AccessLevel::Read,
            environment: AccessEnvironment::default(),
        },
        RacrouteRequestType::Define => RacrouteRequest::Define {
            action: SafDefineAction::Add,
            class: "FACILITY".into(),
            resource: "SAF.DEFINED".into(),
            owner: principal.as_str().into(),
            uacc: AccessLevel::Read,
            generic: false,
        },
        RacrouteRequestType::Dirauth => RacrouteRequest::Dirauth {
            node: "NODE1".into(),
        },
        RacrouteRequestType::Extract => RacrouteRequest::Extract {
            kind: SafExtractKind::User,
            class: None,
            name: principal.as_str().into(),
        },
        RacrouteRequestType::Fastauth => RacrouteRequest::Fastauth {
            class: "FACILITY".into(),
            resource: "APP.ONE".into(),
            access: AccessLevel::Read,
            environment: AccessEnvironment::default(),
        },
        RacrouteRequestType::List => RacrouteRequest::List {
            class: "FACILITY".into(),
            global: true,
            refresh: true,
        },
        RacrouteRequestType::Signon => RacrouteRequest::Signon {
            user: PrincipalId::new("USER1", InvocationLimits::default()).expect("static user"),
            credential_reference: SecretRef::new("secret:user1", Default::default())
                .expect("static secret reference"),
        },
        RacrouteRequestType::Stat => RacrouteRequest::Stat { class: None },
        RacrouteRequestType::Tokenbld => RacrouteRequest::Tokenbld {
            owner: principal.clone(),
            kind: TokenKind::SafIdentity,
            token_reference: "secret:conformance-token".into(),
            token_digest: token_digest.into(),
            scopes: BTreeSet::from(["FACILITY".into()]),
            expires_tick: Some(100),
        },
        RacrouteRequestType::Tokenmap => RacrouteRequest::Tokenmap {
            token_digest: token_digest.into(),
        },
        RacrouteRequestType::Tokenxtr => RacrouteRequest::Tokenxtr {
            token_id: token_id.unwrap_or("TOKEN00000000000000000001000000").into(),
        },
        RacrouteRequestType::Verify => RacrouteRequest::Verify {
            user: principal.clone(),
            credential_reference: SecretRef::new("secret:admin", Default::default())
                .expect("static secret reference"),
            action: SafVerifyAction::AuthenticateOnly,
            acee_id: None,
        },
        RacrouteRequestType::Verifyx => RacrouteRequest::Verifyx {
            user: principal.clone(),
            credential_reference: SecretRef::new("secret:admin", Default::default())
                .expect("static secret reference"),
            mfa_reference: None,
            action: SafVerifyAction::CreateAcee,
            acee_id: None,
            parent_acee: Some(parent_acee.into()),
        },
    }
}

fn malformed_request(
    request_type: RacrouteRequestType,
    principal: &PrincipalId,
    parent_acee: &str,
) -> RacrouteRequest {
    match request_type {
        RacrouteRequestType::Audit => RacrouteRequest::Audit {
            action: "BAD.AUDIT".into(),
            resource_digest: "not-a-digest".into(),
            decision: DecisionOutcome::Deny,
            fields: BTreeMap::new(),
        },
        RacrouteRequestType::Auth => RacrouteRequest::Auth {
            class: String::new(),
            resource: "APP.ONE".into(),
            access: AccessLevel::Read,
            environment: Default::default(),
        },
        RacrouteRequestType::Define => RacrouteRequest::Define {
            action: SafDefineAction::Add,
            class: String::new(),
            resource: "BAD".into(),
            owner: principal.as_str().into(),
            uacc: AccessLevel::None,
            generic: false,
        },
        RacrouteRequestType::Dirauth => RacrouteRequest::Dirauth {
            node: "BAD NODE".into(),
        },
        RacrouteRequestType::Extract => RacrouteRequest::Extract {
            kind: SafExtractKind::User,
            class: None,
            name: String::new(),
        },
        RacrouteRequestType::Fastauth => RacrouteRequest::Fastauth {
            class: "FACILITY".into(),
            resource: String::new(),
            access: AccessLevel::Read,
            environment: Default::default(),
        },
        RacrouteRequestType::List => RacrouteRequest::List {
            class: String::new(),
            global: true,
            refresh: true,
        },
        RacrouteRequestType::Signon => RacrouteRequest::Signon {
            user: PrincipalId::new("USER1", InvocationLimits::default()).expect("static user"),
            credential_reference: SecretRef::new("secret:bad", Default::default())
                .expect("static secret reference"),
        },
        RacrouteRequestType::Stat => RacrouteRequest::Stat {
            class: Some(String::new()),
        },
        RacrouteRequestType::Tokenbld => RacrouteRequest::Tokenbld {
            owner: principal.clone(),
            kind: TokenKind::SafIdentity,
            token_reference: "plaintext".into(),
            token_digest: digest('2'),
            scopes: BTreeSet::new(),
            expires_tick: Some(100),
        },
        RacrouteRequestType::Tokenmap => RacrouteRequest::Tokenmap {
            token_digest: "bad-digest".into(),
        },
        RacrouteRequestType::Tokenxtr => RacrouteRequest::Tokenxtr {
            token_id: "BAD VALUE".into(),
        },
        RacrouteRequestType::Verify => RacrouteRequest::Verify {
            user: principal.clone(),
            credential_reference: SecretRef::new("secret:bad", Default::default())
                .expect("static secret reference"),
            action: SafVerifyAction::AuthenticateOnly,
            acee_id: None,
        },
        RacrouteRequestType::Verifyx => RacrouteRequest::Verifyx {
            user: principal.clone(),
            credential_reference: SecretRef::new("secret:bad", Default::default())
                .expect("static secret reference"),
            mfa_reference: None,
            action: SafVerifyAction::CreateAcee,
            acee_id: None,
            parent_acee: Some(parent_acee.into()),
        },
    }
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
    for (sequence, command) in command_matrix().iter().enumerate() {
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

fn command_matrix() -> &'static [&'static str] {
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
        "RACPRIV ON",
        "RACPRMCK MEMBER(IRROPT01)",
        "SET TRACE AUTOAPPL",
        "SETROPTS PROGRAM RULES",
        "RVARY LIST",
        "STOP",
        "RESTART",
        "ADDUSER USER3 PASSWORD('USER-PASSWORD')",
        "PASSWORD USER(USER3) PASSWORD('NEW-USER-PASSWORD')",
        "TARGET NODE(NODE1) DESCRIPTION('REMOTE NODE') PROTOCOL(TCP)",
        "RACLINK USER3 DEFINE(NODE1 REMOTE3)",
        "RACMAP ID(USER3) MAP(MAP1 REGISTRY LDAP NAME user3@example.com)",
        "RACDCERT ID(USER3) ADD(CERT1 CERTREF secret:cert1 FINGERPRINT sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa)",
        "SIGNOFF LIST",
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
    fn all_enabled_command_families_have_executable_and_deny_routes() {
        for descriptor in command_descriptors().iter().filter(|descriptor| {
            matches!(descriptor.work_package(), "SEC-502" | "SEC-503" | "SEC-505")
        }) {
            execute_matrix(descriptor.family()).unwrap();
            execute_denied(descriptor.family()).unwrap();
        }
    }
}
