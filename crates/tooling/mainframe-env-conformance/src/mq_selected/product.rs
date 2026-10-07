//! Real selected authority and durable coordinator; fixture machine is not COBOL.
mod machine;
mod security;
mod setup;
use super::*;
use mainframe_env_execution_api::*;
use mainframe_env_host_api::mq_mqi::*;
use mainframe_env_host_api::*;
use mainframe_env_interpreter::{CoordinatorLimits, ExecutionControl, ExecutionCoordinator};
use mainframe_env_mq::{MqTrustedBatchFrame, MqTrustedBatchRuntime};
use mainframe_env_store_api::*;
use std::sync::{Arc, Mutex};

pub(super) struct Port {
    descriptor: CapabilityDescriptor,
    runtime: MqTrustedBatchRuntime,
    invocation: Invocation,
    frame: Mutex<Option<MqTrustedBatchFrame>>,
    store: Arc<dyn PlatformStore>,
    steps: Mutex<Vec<Step>>,
    replay_ok: Mutex<bool>,
    deny_put: bool,
    saf: Arc<setup::Saf>,
    refused: Mutex<Option<(HostProblem, bool)>>,
    originals: Mutex<Vec<(EffectRequest, EffectResult)>>,
}
impl HostProvider for Port {
    fn descriptor(&self) -> &CapabilityDescriptor {
        &self.descriptor
    }
    fn invoke(&self, invocation: &Invocation, effect: EffectRequest) -> EffectResult {
        let failed = |problem| EffectResult {
            sequence: effect.sequence,
            outcome: Err(problem),
        };
        if invocation != &self.invocation {
            return failed(HostProblem::Unauthorized);
        }
        let mut guard = self.frame.lock().unwrap();
        let Some(frame) = guard.as_mut() else {
            return failed(HostProblem::Unauthorized);
        };
        let occurrence = match effect.mq_mqi_occurrence(Default::default()) {
            Ok(Some(v)) => v,
            _ => return failed(HostProblem::Malformed),
        };
        let before = self.store.list_provider_state_prefix("mq-", 4096).unwrap();
        if self.deny_put && occurrence.envelope().request.call() == MqMqiCall::Put {
            if let Err(e) = self.saf.revoke() {
                return failed(e);
            }
        }
        let observed = match self.saf.scope(invocation, &effect, "original") {
            Ok(scope) => {
                let outcome = frame.dispatch(occurrence);
                drop(scope);
                outcome
            }
            Err(e) => return failed(e),
        };
        let result = match observed {
            Ok(v) => v,
            Err(e) => {
                let unchanged =
                    self.store.list_provider_state_prefix("mq-", 4096).unwrap() == before;
                *self.refused.lock().unwrap() = Some((e.clone(), unchanged));
                return failed(e);
            }
        };
        let rows = self.store.list_provider_state_prefix("mq-", 4096).unwrap();
        let again = match self.saf.scope(invocation, &effect, "replay") {
            Ok(scope) => {
                let outcome = frame.dispatch(
                    effect
                        .mq_mqi_occurrence(Default::default())
                        .unwrap()
                        .unwrap(),
                );
                drop(scope);
                outcome
            }
            Err(e) => return failed(e),
        };
        *self.replay_ok.lock().unwrap() &= again.as_ref() == Ok(&result)
            && self.store.list_provider_state_prefix("mq-", 4096).unwrap() == rows;
        match observe(&result) {
            Ok(step) => self.steps.lock().unwrap().push(step),
            Err(e) => self.steps.lock().unwrap().push(Step {
                call: "invalid-observation".into(),
                status: None,
                kind: e,
                body_hex: None,
                data_length: None,
                backout_count: None,
                resolved_queue: None,
                descriptor_fields: None,
            }),
        }
        self.originals
            .lock()
            .unwrap()
            .push((effect, result.clone()));
        result
    }
}

pub(super) fn run(fixture: &str) -> Result<Report, String> {
    let mut report = execute(fixture, false)?;
    let denial = execute(fixture, true)?;
    report.denial_error = denial.denial_error;
    report.forbidden_mutation = denial.forbidden_mutation;
    report.denied_saf = denial.saf;
    Ok(report)
}
fn execute(fixture: &str, deny_put: bool) -> Result<Report, String> {
    let sqlite = match fixture {
        "mq.memory.local-v1" => false,
        "mq.sqlite.local-v1" => true,
        _ => return Err("unknown MQ fixture".into()),
    };
    let backend = setup::Backend::new(sqlite)?;
    let store = backend.open()?;
    setup::initialize(&*store)?;
    let saf = Arc::new(setup::Saf::new(store.clone())?);
    let descriptor = setup::descriptor();
    let clock = Arc::new(setup::Clock(store.clone()));
    let invocation = setup::invocation(
        mainframe_env_mq::MqReplayClock::now_tick(&*clock).map_err(|e| e.to_string())?,
    );
    let mut runtime = MqTrustedBatchRuntime::open(
        store.clone(),
        Default::default(),
        3,
        5,
        saf.clone(),
        clock.clone(),
        descriptor.clone(),
        Default::default(),
        Default::default(),
    )
    .map_err(|e| e.to_string())?;
    runtime
        .configure_producer_source(&store, Arc::new(setup::Source(invocation.clone())))
        .map_err(|e| e.to_string())?;
    let port = Arc::new(Port {
        descriptor,
        runtime,
        invocation: invocation.clone(),
        frame: Mutex::new(None),
        store: store.clone(),
        steps: Mutex::new(Vec::new()),
        replay_ok: Mutex::new(true),
        deny_put,
        saf: saf.clone(),
        refused: Mutex::new(None),
        originals: Mutex::new(Vec::new()),
    });
    let host = Arc::new(ScopedHostService::new(
        Arc::new(
            RegistrySnapshot::new(1, vec![port.clone()], Default::default())
                .map_err(|e| e.to_string())?,
        ),
        Default::default(),
    ));
    let coordinator =
        ExecutionCoordinator::durable(host, store.clone(), CoordinatorLimits::default());
    let mut machine = machine::Consumer::new(port.clone());
    let outcome = coordinator.execute_with_control(&mut machine, &invocation, || {
        Ok(ExecutionControl {
            now_tick: mainframe_env_mq::MqReplayClock::now_tick(&*clock)
                .map_err(|_| mainframe_env_interpreter::ExecutionControlError::Unavailable)?,
            cancellation_requested: false,
        })
    });
    if !deny_put && !matches!(outcome, ExecutionOutcome::Completed(_)) {
        return Err(format!("MQ fixture coordinator: {outcome:?}"));
    }
    if deny_put && !matches!(outcome, ExecutionOutcome::ProviderFailure(_)) {
        return Err(format!("denied MQ fixture coordinator: {outcome:?}"));
    }
    let refusal = port.refused.lock().unwrap().clone();
    let (denial_error, forbidden_mutation) = if deny_put {
        match refusal {
            Some((e, unchanged)) => (format!("{e:?}"), !unchanged),
            None => return Err("missing physical denial evidence".into()),
        }
    } else {
        (String::new(), false)
    };
    let effects = (1..=15)
        .map(|s| store.effect(&setup::key(s)))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let originals = port.originals.lock().unwrap();
    let execution = store
        .get_execution(&invocation.execution_id)
        .map_err(|e| e.to_string())?;
    let core_completed_effects = effects
        .iter()
        .enumerate()
        .filter(|(index, r)| {
            let Some((original, result)) = originals.get(*index) else {
                return false;
            };
            r.as_ref().is_some_and(|r| {
                r.state == EffectState::Completed
                    && r.execution_id == invocation.execution_id
                    && r.run_unit_id == invocation.run_unit_id
                    && original.run_unit == invocation.run_unit_id
                    && r.sequence == original.sequence
                    && r.sequence == (*index + 1) as u64
                    && r.key == setup::key((*index + 1) as u64)
                    && original.idempotency_key.as_ref() == Some(&r.key)
                    && r.digest_format == EffectDigestFormat::CanonicalHostV1
                    && r.intent.owner == invocation.execution_id
                    && r.intent.attempt == invocation.attempt
                    && r.intent.capability
                        == Some(original.request.required_capability(Default::default()))
                    && r.intent.audit_resource
                        == Some(canonical_audit_resource_digest(&original.request))
                    && r.intent.audit_invocation_key.as_ref() == Some(&invocation.idempotency_key)
                    && execution.as_ref().is_some_and(|e| {
                        e.execution_id == invocation.execution_id
                            && e.run_unit_id == invocation.run_unit_id
                            && &e.principal == invocation.principal.id()
                            && e.attempt == invocation.attempt
                    })
                    && r.request_digest == canonical_request_digest(&original.request).unwrap()
                    && r.result_digest == canonical_result_digest(&result.outcome).ok()
            })
        })
        .count();
    let rows = store
        .list_provider_state_prefix("mq-", 4096)
        .map_err(|e| e.to_string())?;
    let provider_receipts = rows
        .iter()
        .filter(|r| r.namespace == "mq-selected-v1-occurrence")
        .count();
    let audits = store
        .audit_records(&invocation.execution_id, 0, 256)
        .map_err(|e| e.to_string())?;
    let audited_effects = originals
        .iter()
        .filter(|(original, _)| {
            audits.iter().any(|a| {
                a.effect_sequence == original.sequence
                    && a.execution_id == invocation.execution_id
                    && a.run_unit_id == invocation.run_unit_id
                    && a.attempt == invocation.attempt
                    && a.invocation_key == invocation.idempotency_key
                    && a.resource == canonical_audit_resource_digest(&original.request)
                    && a.decision == AuditDecision::Success
                    && &a.principal == invocation.principal.id()
                    && a.capability.as_str() == "host.mq.write"
            })
        })
        .count();
    drop(originals);
    let steps = port.steps.lock().unwrap().clone();
    let replay_unchanged = *port.replay_ok.lock().unwrap();
    let saf_calls = saf.observations().map_err(|e| e.to_string())?;
    drop(machine);
    drop(coordinator);
    drop(port);
    drop(saf);
    drop(clock);
    drop(store);
    let reopened = backend.open()?;
    let sqlite_reopen_equal = !sqlite
        || reopened
            .list_provider_state_prefix("mq-", 4096)
            .map_err(|e| e.to_string())?
            == rows;
    Ok(Report {
        schema: SCHEMA.into(),
        fixture: fixture.into(),
        fixture_digest: digest(INPUT),
        expectation_digest: digest(EXPECTED.as_bytes()),
        setup_digest: digest(SETUP.as_bytes()),
        steps,
        saf: saf_calls,
        denied_saf: Default::default(),
        replay_unchanged,
        forbidden_mutation,
        denial_error,
        core_completed_effects,
        provider_receipts,
        audited_effects,
        sqlite_reopen_equal,
    })
}

fn observe(result: &EffectResult) -> Result<Step, String> {
    let Ok(HostResult::MqMqi(result)) = &result.outcome else {
        return Err("missing actual typed result".into());
    };
    let result = &result.result;
    let (output, status) = match &result.outcome {
        MqMqiOutcome::Completed {
            output,
            status: MqMqiStatus::OkNone,
        } => (output, Some((0, 0))),
        MqMqiOutcome::ReviewedOutput { output, status } => (output, Some(status.wire_pair())),
        MqMqiOutcome::StatusPending { output } => (output, None),
        _ => return Err("pending/unknown is not a positive observation".into()),
    };
    let mut step = Step {
        call: result.call.label().into(),
        status,
        kind: String::new(),
        body_hex: None,
        data_length: None,
        backout_count: None,
        resolved_queue: None,
        descriptor_fields: None,
    };
    step.kind = match output {
        MqMqiOutput::Connected(_) => "connected",
        MqMqiOutput::Opened { dynamic: None, .. } => "opened",
        MqMqiOutput::Produced(p) => {
            step.descriptor_fields = Some(descriptor_fields(&p.descriptor));
            step.backout_count = Some(p.descriptor.fields().backout_count);
            step.resolved_queue = Some(hex(&p.resolved_queue));
            match p.outcome {
                MqDeliveryOutcome::Accepted => "accepted",
                MqDeliveryOutcome::Pending => "pending",
                _ => return Err("producer disposition".into()),
            }
        }
        MqMqiOutput::QualifiedFullGot(g) => {
            if g.cursor.is_some() || g.message.as_ref().is_some_and(|m| !m.properties.is_empty()) {
                return Err("finite removal profile returned cursor/properties".into());
            }
            step.data_length = g.data_length;
            step.resolved_queue = g.resolved_queue.as_ref().map(|q| hex(q));
            if let Some(m) = &g.message {
                step.descriptor_fields = Some(descriptor_fields(&m.descriptor));
                step.body_hex = Some(hex(&m.body));
                step.backout_count = Some(m.descriptor.fields().backout_count);
            }
            match g.disposition {
                MqGetDisposition::NoMessage => "no-message",
                MqGetDisposition::Message(MqTruncationDisposition::Complete { .. }) => "complete",
                MqGetDisposition::Message(MqTruncationDisposition::RejectedRetained { .. }) => {
                    "rejected-retained"
                }
                MqGetDisposition::Message(MqTruncationDisposition::AcceptedRemoved { .. }) => {
                    "accepted-removed"
                }
                _ => return Err("GET disposition".into()),
            }
        }
        MqMqiOutput::NoOutput => "no-output",
        MqMqiOutput::UnitOfWork { .. } => "unit",
        _ => return Err("wrong output class".into()),
    }
    .into();
    Ok(step)
}

// Observation projection only, not an MQMD codec or permission validator.
// The independent fixture explicitly fixes every MD1 field in this order.
fn descriptor_fields(md: &mainframe_env_host_api::mq_md_value::MqMdValue) -> Vec<String> {
    let f = md.fields();
    let mut fields = vec![
        md.version().to_string(),
        "ascii-compatible".into(),
        hex(&f.struc_id),
        f.report.to_string(),
        f.msg_type.to_string(),
        f.expiry.to_string(),
        f.feedback.to_string(),
        f.encoding.to_string(),
        f.coded_char_set_id.to_string(),
        hex(&f.format),
        f.priority.to_string(),
        f.persistence.to_string(),
        hex(&f.msg_id),
        hex(&f.correl_id),
        f.backout_count.to_string(),
        hex(&f.reply_to_q),
        hex(&f.reply_to_q_mgr),
        hex(&f.user_identifier),
        hex(&f.accounting_token),
        hex(&f.appl_identity_data),
        f.put_appl_type.to_string(),
        hex(&f.put_appl_name),
        hex(&f.put_date),
        hex(&f.put_time),
        hex(&f.appl_origin_data),
    ];
    if md.characters()
        != mainframe_env_host_api::mq_md_value::MqMdCharacterEncoding::AsciiCompatible
    {
        fields[1] = "unsupported-structure-profile".into();
    }
    fields
}
