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
    SafVerifyAction, SecurityDatabaseSummary, TokenKind, command_descriptors, racroute_descriptors,
    recognize_command, validate_command,
};
use mainframe_env_store::MemoryStore;
use mainframe_env_store_api::ProviderStateStore;
use sha2::Digest;
use std::collections::{BTreeMap, BTreeSet};
use std::env;
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
    racf_runtime_with(
        spec,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        ConformanceLimits::default(),
    )
}

pub fn racf_runtime_with<'a>(
    spec: &CompiledSpec,
    mut drivers: Vec<(DriverRef, &'a dyn ConformanceDriver)>,
    mut predicates: Vec<(PredicateRef, &'a dyn ConformancePredicate)>,
    mut observations: Vec<(ObservationRef, &'a dyn ConformanceObservation)>,
    limits: ConformanceLimits,
) -> Result<RuntimeRegistry<'a>, SpecProblem> {
    drivers.extend([
        (
            DriverRef::new("racf.command.driver", limits)?,
            &RACF_DRIVER as &dyn ConformanceDriver,
        ),
        (
            DriverRef::new("racf.racroute.driver", limits)?,
            &RACROUTE_DRIVER as &dyn ConformanceDriver,
        ),
    ]);
    predicates.push((
        PredicateRef::new("racf.authority.ready", limits)?,
        &RACF_READY as &dyn ConformancePredicate,
    ));
    observations.push((
        ObservationRef::new("racf.command.passed", limits)?,
        &RACF_PASSED as &dyn ConformanceObservation,
    ));
    RuntimeRegistry::new(spec, drivers, predicates, observations, limits)
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
        if gate != "differential" {
            crate::racf_reference::verify_binding(
                "command",
                descriptor.row_id(),
                descriptor.keyword(),
                obligation,
                gate,
            )?;
        }
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
            ("bounded-limit", "conditioned") => execute_command_limit(descriptor.family())?,
            ("audit-redaction", "conditioned") => execute_command_audit(descriptor.family())?,
            ("atomic-retry", "recovered") => execute_command_atomic_retry(descriptor.family())?,
            ("restart-recovery", "recovered") => execute_command_recovery(descriptor.family())?,
            ("licensed-equivalence", "differential") => {
                return compare_licensed_oracle(
                    descriptor.row_id(),
                    fixture,
                    command_oracle_observation(descriptor.family())?,
                );
            }
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
        if gate != "differential" {
            crate::racf_reference::verify_binding(
                "racroute",
                descriptor.row_id(),
                descriptor.keyword(),
                obligation,
                gate,
            )?;
        }
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
                execute_racroute_case(descriptor.request_type(), RacrouteCase::Allowed)?;
            }
            ("unauthorized", "executed" | "conditioned") => {
                execute_racroute_case(descriptor.request_type(), RacrouteCase::Denied)?;
            }
            ("malformed", "conditioned") => {
                execute_racroute_case(descriptor.request_type(), RacrouteCase::Malformed)?;
            }
            ("bounded-limit", "conditioned") => {
                execute_racroute_case(descriptor.request_type(), RacrouteCase::Limit)?;
            }
            ("audit-redaction", "conditioned") => {
                execute_racroute_case(descriptor.request_type(), RacrouteCase::Audit)?;
            }
            ("atomic-retry", "recovered") => {
                execute_racroute_case(descriptor.request_type(), RacrouteCase::Atomic)?;
            }
            ("restart-recovery", "recovered") => {
                execute_racroute_case(descriptor.request_type(), RacrouteCase::Recovery)?;
            }
            ("licensed-equivalence", "differential") => {
                let observation =
                    execute_racroute_case(descriptor.request_type(), RacrouteCase::Differential)?
                        .ok_or("RACROUTE differential observation is missing")?;
                return compare_licensed_oracle(descriptor.row_id(), fixture, observation);
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
    let setup = setup_recoverable()?;
    Ok((setup.service, setup.context))
}

struct RecoverableSetup {
    service: Arc<RacfService>,
    context: CommandContext,
    store: Arc<dyn ProviderStateStore>,
    secrets: Arc<MemorySecretResolver>,
}

fn setup_recoverable() -> Result<RecoverableSetup, String> {
    let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
    let secrets = Arc::new(MemorySecretResolver::default());
    secrets.insert("secret:admin", b"ADMIN-PASSWORD".to_vec());
    secrets.insert("secret:user1", b"USER-PASSWORD".to_vec());
    secrets.insert("secret:bad", b"WRONG-PASSWORD".to_vec());
    let service = RacfService::open(store.clone(), secrets.clone(), Default::default())
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
    Ok(RecoverableSetup {
        service,
        context,
        store,
        secrets,
    })
}

#[derive(Clone, Copy)]
enum RacrouteCase {
    Allowed,
    Denied,
    Malformed,
    Limit,
    Audit,
    Atomic,
    Recovery,
    Differential,
}

fn execute_racroute_case(
    request_type: RacrouteRequestType,
    mode: RacrouteCase,
) -> Result<Option<serde_json::Value>, String> {
    let RecoverableSetup {
        service,
        context: base,
        store,
        secrets,
    } = setup_recoverable()?;
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
    if matches!(mode, RacrouteCase::Limit) {
        secrets.insert("secret:oversized", vec![b'x'; 4097]);
    }
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
    let acee = if matches!(
        mode,
        RacrouteCase::Allowed
            | RacrouteCase::Malformed
            | RacrouteCase::Limit
            | RacrouteCase::Audit
            | RacrouteCase::Atomic
            | RacrouteCase::Recovery
            | RacrouteCase::Differential
    ) && descriptor.requires_acee()
    {
        Some(admin_acee.as_str())
    } else {
        None
    };
    let request = match mode {
        RacrouteCase::Malformed => malformed_request(request_type, &admin, &admin_acee),
        RacrouteCase::Limit => limit_request(request_type, &admin, &admin_acee),
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
    if matches!(mode, RacrouteCase::Atomic) {
        let second = RacfService::open(store, secrets, Default::default())
            .map_err(|problem| format!("RACROUTE concurrent authority open: {problem:?}"))?;
        let selected_context = saf_context(&caller, acee, "CASE-REQUEST", 20)?;
        let race_context = CommandContext::new(
            base.actor().clone(),
            "SAF-ATOMIC-RACE",
            "RACF-ATOMIC-CONFORMANCE",
            10_000,
        )
        .map_err(|problem| problem.to_string())?;
        let barrier = Arc::new(std::sync::Barrier::new(3));
        let target_barrier = barrier.clone();
        let target_service = service.clone();
        let target_worker = std::thread::spawn(move || {
            target_barrier.wait();
            target_service.racroute(&selected_context, request)
        });
        let race_barrier = barrier.clone();
        let race_worker = std::thread::spawn(move || {
            race_barrier.wait();
            second.execute_command(&race_context, "ADDGROUP RACEGRP")
        });
        barrier.wait();
        let outcome = target_worker
            .join()
            .map_err(|_| "RACROUTE atomic target worker panicked")?
            .map_err(|problem| format!("RACROUTE atomic target failed: {problem:?}"))?;
        race_worker
            .join()
            .map_err(|_| "RACROUTE atomic race worker panicked")?
            .map_err(|problem| format!("RACROUTE atomic race failed: {problem}"))?;
        if outcome.request_type != request_type || outcome.status.reason != DecisionReason::Granted
        {
            return Err("RACROUTE atomic retry lost or denied the selected route".into());
        }
        let after = service
            .database()
            .summary()
            .map_err(|problem| format!("RACROUTE atomic summary: {problem:?}"))?;
        if after.groups != before.groups + 1 {
            return Err("RACROUTE atomic retry lost the concurrent mutation".into());
        }
        return Ok(None);
    }
    let outcome = service.racroute(&saf_context(&caller, acee, "CASE-REQUEST", 20)?, request);
    if matches!(mode, RacrouteCase::Limit) {
        return match outcome {
            Ok(outcome) if outcome.status.reason != DecisionReason::Granted => Ok(None),
            Err(
                mainframe_env_host_api::HostProblem::Malformed
                | mainframe_env_host_api::HostProblem::ResourceExhausted,
            ) => Ok(None),
            Ok(_) => Err("RACROUTE bounded request unexpectedly succeeded".into()),
            Err(problem) => Err(format!(
                "RACROUTE bounded request returned the wrong failure: {problem:?}"
            )),
        };
    }
    let outcome =
        outcome.map_err(|problem| format!("RACROUTE selected route failed: {problem:?}"))?;
    match mode {
        RacrouteCase::Allowed
        | RacrouteCase::Audit
        | RacrouteCase::Recovery
        | RacrouteCase::Differential
            if outcome.status.reason != DecisionReason::Granted =>
        {
            Err(format!(
                "RACROUTE allowed route denied with {:?}",
                outcome.status.reason
            ))
        }
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
            if !same_protected_state(&before, &after) || after.audits != before.audits + 1 {
                return Err("RACROUTE denial mutated protected state or omitted audit".into());
            }
            Ok(None)
        }
        RacrouteCase::Recovery => {
            let before_restart = service
                .database()
                .summary()
                .map_err(|problem| format!("RACROUTE pre-restart summary: {problem:?}"))?;
            drop(service);
            let reopened = RacfService::open(store, secrets, Default::default())
                .map_err(|problem| format!("RACROUTE restart failed: {problem:?}"))?;
            let after_restart = reopened
                .database()
                .summary()
                .map_err(|problem| format!("RACROUTE post-restart summary: {problem:?}"))?;
            if after_restart != before_restart {
                return Err("RACROUTE restart changed durable authority state".into());
            }
            Ok(None)
        }
        RacrouteCase::Audit => {
            let audits = service
                .smf_type80_records(0, 65_536)
                .map_err(|problem| format!("RACROUTE audit projection failed: {problem:?}"))?;
            let expected_action = if request_type == RacrouteRequestType::Audit {
                "CONFORMANCE.AUDIT"
            } else {
                descriptor.keyword()
            };
            let audit = audits
                .iter()
                .rev()
                .find(|audit| audit.action == expected_action)
                .ok_or("RACROUTE selected route omitted its audit")?;
            if audit.record_type != 80
                || audit.decision != DecisionOutcome::Allow
                || contains_secret_json(&serde_json::to_value(audit).map_err(|e| e.to_string())?)
            {
                return Err("RACROUTE selected-route audit is incomplete or unsafe".into());
            }
            Ok(None)
        }
        RacrouteCase::Differential => Ok(Some(serde_json::json!({
            "surface": "racroute",
            "keyword": descriptor.keyword(),
            "status": outcome.status,
            "states": outcome.states,
            "result": outcome.result,
        }))),
        _ => Ok(None),
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

fn limit_request(
    request_type: RacrouteRequestType,
    principal: &PrincipalId,
    parent_acee: &str,
) -> RacrouteRequest {
    let oversized = "X".repeat(4097);
    let oversized_class = "X".repeat(33);
    let oversized_name = "X".repeat(247);
    match request_type {
        RacrouteRequestType::Audit => RacrouteRequest::Audit {
            action: oversized_name,
            resource_digest: digest('1'),
            decision: DecisionOutcome::Deny,
            fields: BTreeMap::new(),
        },
        RacrouteRequestType::Auth => RacrouteRequest::Auth {
            class: oversized_class,
            resource: "APP.ONE".into(),
            access: AccessLevel::Read,
            environment: Default::default(),
        },
        RacrouteRequestType::Define => RacrouteRequest::Define {
            action: SafDefineAction::Add,
            class: oversized_class,
            resource: "APP.ONE".into(),
            owner: principal.as_str().into(),
            uacc: AccessLevel::None,
            generic: false,
        },
        RacrouteRequestType::Dirauth => RacrouteRequest::Dirauth {
            node: oversized_name,
        },
        RacrouteRequestType::Extract => RacrouteRequest::Extract {
            kind: SafExtractKind::User,
            class: None,
            name: oversized_name,
        },
        RacrouteRequestType::Fastauth => RacrouteRequest::Fastauth {
            class: oversized_class,
            resource: "APP.ONE".into(),
            access: AccessLevel::Read,
            environment: Default::default(),
        },
        RacrouteRequestType::List => RacrouteRequest::List {
            class: oversized_class,
            global: true,
            refresh: true,
        },
        RacrouteRequestType::Signon => RacrouteRequest::Signon {
            user: PrincipalId::new("USER1", InvocationLimits::default()).expect("static user"),
            credential_reference: SecretRef::new("secret:oversized", Default::default())
                .expect("bounded secret reference"),
        },
        RacrouteRequestType::Stat => RacrouteRequest::Stat {
            class: Some(oversized_class),
        },
        RacrouteRequestType::Tokenbld => RacrouteRequest::Tokenbld {
            owner: principal.clone(),
            kind: TokenKind::SafIdentity,
            token_reference: oversized,
            token_digest: digest('2'),
            scopes: BTreeSet::from(["FACILITY".into()]),
            expires_tick: Some(100),
        },
        RacrouteRequestType::Tokenmap => RacrouteRequest::Tokenmap {
            token_digest: oversized,
        },
        RacrouteRequestType::Tokenxtr => RacrouteRequest::Tokenxtr {
            token_id: oversized_name,
        },
        RacrouteRequestType::Verify => RacrouteRequest::Verify {
            user: principal.clone(),
            credential_reference: SecretRef::new("secret:oversized", Default::default())
                .expect("bounded secret reference"),
            action: SafVerifyAction::AuthenticateOnly,
            acee_id: None,
        },
        RacrouteRequestType::Verifyx => RacrouteRequest::Verifyx {
            user: principal.clone(),
            credential_reference: SecretRef::new("secret:oversized", Default::default())
                .expect("bounded secret reference"),
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

fn execute_command_limit(target: CommandFamily) -> Result<(), String> {
    let input = valid_form(target);
    let limits = mainframe_env_racf::CommandLanguageLimits {
        max_input_bytes: input.len().saturating_sub(1),
        ..Default::default()
    };
    let problem = validate_command(input, limits)
        .expect_err("RACF family-specific bounded input unexpectedly validated");
    if problem.code == CommandDiagnosticCode::InputLimit {
        Ok(())
    } else {
        Err(format!(
            "RACF bounded input returned the wrong diagnostic: {problem}"
        ))
    }
}

fn execute_command_audit(target: CommandFamily) -> Result<(), String> {
    let (service, base) = setup()?;
    for (sequence, command) in command_matrix().iter().enumerate() {
        let result = service
            .execute_command(&context(&base, sequence + 1)?, command)
            .map_err(|problem| format!("RACF audit route failed: {problem}"))?;
        if result.family == target {
            let descriptor = command_descriptors()
                .iter()
                .find(|descriptor| descriptor.family() == target)
                .ok_or("RACF audit descriptor is missing")?;
            let audits = service
                .smf_type80_records(0, 65_536)
                .map_err(|problem| format!("RACF audit projection failed: {problem:?}"))?;
            let audit = audits
                .last()
                .ok_or("RACF selected route omitted its audit")?;
            if audit.record_type != 80
                || audit.action != descriptor.keyword()
                || audit.decision != DecisionOutcome::Allow
                || contains_secret_json(&serde_json::to_value(audit).map_err(|e| e.to_string())?)
            {
                return Err("RACF selected-route audit is incomplete or unsafe".into());
            }
            return Ok(());
        }
    }
    Err("RACF audit family has no selected route".into())
}

fn execute_command_atomic_retry(target: CommandFamily) -> Result<(), String> {
    let RecoverableSetup {
        service,
        context: base,
        store,
        secrets,
    } = setup_recoverable()?;
    let (target_index, command) = command_matrix()
        .iter()
        .enumerate()
        .find(|(_, command)| recognize_command(command, Default::default()).ok() == Some(target))
        .ok_or("RACF atomic family has no selected route")?;
    for (sequence, prerequisite) in command_matrix().iter().take(target_index).enumerate() {
        service
            .execute_command(&context(&base, sequence + 1)?, prerequisite)
            .map_err(|problem| format!("RACF atomic setup failed: {problem}"))?;
    }
    let second = RacfService::open(store, secrets, Default::default())
        .map_err(|problem| format!("RACF concurrent authority open failed: {problem:?}"))?;
    let target_context = context(&base, target_index + 1)?;
    let race_context = CommandContext::new(
        base.actor().clone(),
        "ATOMIC-RACE",
        "RACF-ATOMIC-CONFORMANCE",
        10_000,
    )
    .map_err(|problem| problem.to_string())?;
    let barrier = Arc::new(std::sync::Barrier::new(3));
    let target_barrier = barrier.clone();
    let target_service = service.clone();
    let command = (*command).to_string();
    let target_worker = std::thread::spawn(move || {
        target_barrier.wait();
        target_service.execute_command(&target_context, &command)
    });
    let race_barrier = barrier.clone();
    let race_worker = std::thread::spawn(move || {
        race_barrier.wait();
        second.execute_command(&race_context, "ADDGROUP RACEGRP")
    });
    barrier.wait();
    let target_result = target_worker
        .join()
        .map_err(|_| "RACF atomic target worker panicked")?
        .map_err(|problem| format!("RACF atomic target failed: {problem}"))?;
    race_worker
        .join()
        .map_err(|_| "RACF atomic race worker panicked")?
        .map_err(|problem| format!("RACF atomic race failed: {problem}"))?;
    if target_result.family != target {
        return Err("RACF atomic retry selected the wrong family".into());
    }
    let listed = service
        .execute_command(
            &CommandContext::new(
                base.actor().clone(),
                "ATOMIC-LIST",
                "RACF-ATOMIC-CONFORMANCE",
                10_001,
            )
            .map_err(|problem| problem.to_string())?,
            "LISTGRP RACEGRP",
        )
        .map_err(|problem| format!("RACF atomic verification failed: {problem}"))?;
    if listed.records.len() != 1 {
        return Err("RACF atomic retry lost the concurrent mutation".into());
    }
    Ok(())
}

fn command_oracle_observation(target: CommandFamily) -> Result<serde_json::Value, String> {
    let (service, base) = setup()?;
    for (sequence, command) in command_matrix().iter().enumerate() {
        let result = service
            .execute_command(&context(&base, sequence + 1)?, command)
            .map_err(|problem| format!("RACF differential route failed: {problem}"))?;
        if result.family == target {
            let descriptor = command_descriptors()
                .iter()
                .find(|descriptor| descriptor.family() == target)
                .ok_or("RACF differential descriptor is missing")?;
            return Ok(serde_json::json!({
                "surface": "command",
                "keyword": descriptor.keyword(),
                "status": result.status,
                "records": result.records,
            }));
        }
    }
    Err("RACF differential family has no selected route".into())
}

fn compare_licensed_oracle(
    row_id: &str,
    fixture: &FixtureRef,
    observation: serde_json::Value,
) -> Result<DriverOutput, String> {
    let root = env::current_dir().map_err(|error| format!("RACF oracle root: {error}"))?;
    let campaign = crate::RacfOracleCampaign::load_optional(&root)?
        .ok_or("licensed RACF oracle campaign is not installed")?;
    let oracle = campaign
        .case(row_id)
        .ok_or_else(|| format!("licensed RACF oracle row is missing: {row_id}"))?;
    let fixture_digest = format!(
        "sha256:{:x}",
        sha2::Sha256::digest(fixture.as_str().as_bytes())
    );
    if oracle.fixture_digest != fixture_digest {
        return Err(format!("licensed RACF oracle fixture drifted for {row_id}"));
    }
    if oracle.observation != observation {
        return Err(format!(
            "licensed RACF oracle observation differs for {row_id} via {}/{}",
            campaign.environment_identity(),
            campaign.adapter_identity(),
        ));
    }
    let output = format!(
        "racf:{}:licensed-equivalence:differential:pass",
        oracle.keyword
    );
    DriverOutput::with_oracle_receipt(
        output.into_bytes(),
        campaign.digest(),
        ConformanceLimits::default(),
    )
    .map_err(|problem| problem.to_string())
}

fn contains_secret_json(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Object(values) => values.values().any(contains_secret_json),
        serde_json::Value::Array(values) => values.iter().any(contains_secret_json),
        serde_json::Value::String(value) => {
            let upper = value.to_ascii_uppercase();
            value.starts_with("secret:")
                || value.starts_with("vault:")
                || value.starts_with("keyring:")
                || value.starts_with("$argon2")
                || upper.contains("BEGIN PRIVATE KEY")
                || upper.contains("BEGIN CERTIFICATE")
                || upper.contains("ADMIN-PASSWORD")
                || upper.contains("USER-PASSWORD")
        }
        _ => false,
    }
}

fn execute_command_recovery(target: CommandFamily) -> Result<(), String> {
    let RecoverableSetup {
        service,
        context: base,
        store,
        secrets,
    } = setup_recoverable()?;
    let mut selected = None;
    for (sequence, command) in command_matrix().iter().enumerate() {
        let invocation = context(&base, sequence + 1)?;
        let result = service
            .execute_command(&invocation, command)
            .map_err(|problem| format!("RACF recovery setup failed: {problem}"))?;
        if result.family == target {
            selected = Some((invocation, *command));
            break;
        }
    }
    let (invocation, command) = selected.ok_or("RACF recovery family has no selected route")?;
    let before_restart = service
        .database()
        .summary()
        .map_err(|problem| format!("RACF pre-restart summary failed: {problem:?}"))?;
    drop(service);
    let reopened = RacfService::open(store, secrets, Default::default())
        .map_err(|problem| format!("RACF restart failed: {problem:?}"))?;
    let after_restart = reopened
        .database()
        .summary()
        .map_err(|problem| format!("RACF post-restart summary failed: {problem:?}"))?;
    if after_restart != before_restart {
        return Err("RACF restart changed durable authority state".into());
    }
    let descriptor = command_descriptors()
        .iter()
        .find(|descriptor| descriptor.family() == target)
        .ok_or("RACF recovery descriptor is missing")?;
    if descriptor.mutating() {
        let replay = reopened
            .execute_command(&invocation, command)
            .map_err(|problem| format!("RACF recovery replay failed: {problem}"))?;
        if !replay.replayed {
            return Err("RACF mutating recovery route did not replay idempotently".into());
        }
    }
    Ok(())
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
    if !same_protected_state(&before, &after) || after.audits != before.audits + 1 {
        return Err("RACF denial mutated protected state or omitted audit".into());
    }
    Ok(())
}

fn same_protected_state(left: &SecurityDatabaseSummary, right: &SecurityDatabaseSummary) -> bool {
    (
        left.principals,
        left.groups,
        left.connections,
        left.classes,
        left.templates,
        left.profiles,
        left.raclist_caches,
        left.acees,
        left.tokens,
        left.certificates,
        left.keys,
    ) == (
        right.principals,
        right.groups,
        right.connections,
        right.classes,
        right.templates,
        right.profiles,
        right.raclist_caches,
        right.acees,
        right.tokens,
        right.certificates,
        right.keys,
    ) && (
        left.keyrings,
        left.mfa_factors,
        left.identity_mappings,
        left.user_associations,
        left.rrsf_nodes,
        left.signon_sessions,
        left.recovery_records,
        left.migrations,
        left.subsystem_running,
    ) == (
        right.keyrings,
        right.mfa_factors,
        right.identity_mappings,
        right.user_associations,
        right.rrsf_nodes,
        right.signon_sessions,
        right.recovery_records,
        right.migrations,
        right.subsystem_running,
    )
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

    #[test]
    fn all_oracle_observations_are_canonical_bounded_and_secret_free() {
        let mut observations = Vec::new();
        for descriptor in command_descriptors() {
            observations.push(command_oracle_observation(descriptor.family()).unwrap());
        }
        for descriptor in racroute_descriptors() {
            observations.push(
                execute_racroute_case(descriptor.request_type(), RacrouteCase::Differential)
                    .unwrap()
                    .unwrap(),
            );
        }
        assert_eq!(observations.len(), 48);
        for observation in observations {
            let bytes = serde_json::to_vec(&observation).unwrap();
            assert!(bytes.len() <= 65_536);
            let shown = String::from_utf8(bytes).unwrap();
            for forbidden in [
                "secret:",
                "$argon2",
                "ADMIN-PASSWORD",
                "USER-PASSWORD",
                "conformance-token",
            ] {
                assert!(!shown.contains(forbidden));
            }
        }
    }
}
