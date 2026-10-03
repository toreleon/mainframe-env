//! Sole original HostCall implementation, borrowed only by the actual drive.
use super::*;

pub(super) struct OriginalDispatch<'borrow, 'native, 'hooks, F> {
    pub(super) coordinator: &'borrow ExecutionCoordinator,
    pub(super) invocation: &'borrow Invocation,
    pub(super) journal: &'borrow mut Option<JournalCursor<'native, 'hooks>>,
    pub(super) control: &'borrow mut ExecutionControl,
    pub(super) observe: &'borrow mut F,
    pub(super) resumable: bool,
    pub(super) native_child: bool,
}

impl<F> OriginalDispatch<'_, '_, '_, F>
where
    F: FnMut() -> Result<ExecutionControl, ExecutionControlError>,
{
    pub(super) fn dispatch(self, effect: EffectRequest) -> Result<EffectResult, ExecutionOutcome> {
        let Self {
            coordinator,
            invocation,
            journal,
            control: control_slot,
            observe,
            resumable,
            native_child,
        } = self;
        let mut control = *control_slot;
        if native_child
            && (!root_terminal::local_effect(&effect) || effect.idempotency_key.is_none())
        {
            return Err(failed_outcome(problem(
                FailureCategory::UnknownOutcome,
                "native child effect outside local profile",
            )));
        }
        if let Some(cursor) = journal.as_mut()
            && let Some(progress) = cursor.native.as_deref_mut()
            && progress
                .require_effect(&cursor.store, cursor.tick, &effect)
                .is_err()
        {
            return Err(failed_outcome(problem(
                FailureCategory::UnknownOutcome,
                "native root effect outside the selected local profile",
            )));
        }
        let Some(host) = &coordinator.host else {
            let outcome = ExecutionOutcome::ProviderFailure(problem(
                FailureCategory::ProviderFailure,
                "selected execution profile has no host provider",
            ));
            let _ = record_step(
                journal,
                Some(ExecutionState::Failed),
                LifecycleEventKind::Failed,
                None,
                None,
            );
            return Err(outcome);
        };
        let request_digest = match canonical_request_digest(&effect.request) {
            Ok(digest) => digest,
            Err(_) => {
                return Err(failed_outcome(problem(
                    FailureCategory::ResourceExhausted,
                    "canonical host request exceeds the journal encoding budget",
                )));
            }
        };
        let audit_resource = canonical_audit_resource_digest(&effect.request);
        let mutating = effect.request.is_mutating();
        let capability = effect
            .request
            .required_capability(InvocationLimits::default());
        if resumable && let Some(key) = effect.idempotency_key.as_ref() {
            let Some(replay_journal) = journal.as_ref() else {
                return Err(infrastructure_failure(
                    "durable resume journal is unavailable",
                ));
            };
            let existing = match replay_journal.store.effect(key) {
                Ok(existing) => existing,
                Err(_) => {
                    return Err(infrastructure_failure(
                        "prior effect lookup failed during durable resume",
                    ));
                }
            };
            if let Some(existing) = existing {
                if existing.execution_id != invocation.execution_id
                    || existing.run_unit_id != invocation.run_unit_id
                    || existing.sequence != effect.sequence
                    || existing.digest_format != EffectDigestFormat::CanonicalHostV1
                    || existing.request_digest != request_digest
                {
                    return Err(failed_outcome(problem(
                        FailureCategory::UnknownOutcome,
                        "durable effect replay identity does not match",
                    )));
                }
                if existing.state != EffectState::Completed {
                    return Err(failed_outcome(problem(
                        FailureCategory::UnknownOutcome,
                        "durable effect requires reconciliation before resume",
                    )));
                }
                control = match observe_checked(
                    observe,
                    control,
                    invocation,
                    invocation.deadline_tick.min(effect.deadline_tick),
                    journal,
                ) {
                    Ok(control) => control,
                    Err(outcome) => return Err(outcome),
                };
                let effect_sequence = effect.sequence;
                if coordinator.checked_inquiry_replay && super::checked_replay::eligible(&effect) {
                    let cursor = journal
                        .as_mut()
                        .ok_or_else(|| infrastructure_failure("checked replay requires journal"))?;
                    let result = super::checked_replay::dispatch(
                        coordinator,
                        invocation,
                        cursor,
                        effect,
                        existing,
                        control,
                    );
                    if result.is_ok() {
                        *control_slot = control;
                    }
                    return result;
                }
                let audited = host.invoke(
                    invocation,
                    control.now_tick,
                    control.cancellation_requested,
                    effect,
                );
                let (result, audit) = audited.into_transaction_parts();
                let replay_digest = match canonical_result_digest(&result.outcome) {
                    Ok(digest) => digest,
                    Err(_) => {
                        return Err(failed_outcome(problem(
                            FailureCategory::UnknownOutcome,
                            "replayed host result cannot be encoded",
                        )));
                    }
                };
                if existing.result_digest != Some(replay_digest) {
                    return Err(failed_outcome(problem(
                        FailureCategory::UnknownOutcome,
                        "replayed host result differs from the reconciled result",
                    )));
                }
                if record_audited_step(
                    journal,
                    coordinator.audit_sink.as_deref(),
                    None,
                    LifecycleEventKind::EffectResult {
                        sequence: effect_sequence,
                    },
                    None,
                    None,
                    audit,
                )
                .is_err()
                {
                    return Err(infrastructure_failure(
                        "replayed host audit persistence failed",
                    ));
                }
                *control_slot = control;
                return Ok(result);
            }
        }
        let intent_epoch = journal
            .as_ref()
            .and_then(|journal| journal.sequence.checked_add(1))
            .unwrap_or(effect.sequence);
        let intent = effect.idempotency_key.as_ref().map(|key| EffectRecord {
            execution_id: invocation.execution_id.clone(),
            run_unit_id: invocation.run_unit_id.clone(),
            sequence: effect.sequence,
            key: key.clone(),
            digest_format: EffectDigestFormat::CanonicalHostV1,
            request_digest,
            intent: EffectIntentMetadata {
                owner: invocation.execution_id.clone(),
                attempt: invocation.attempt,
                capability: Some(capability),
                audit_resource: Some(audit_resource),
                audit_invocation_key: Some(invocation.idempotency_key.clone()),
                created_tick: control.now_tick,
                recovery_after_tick: invocation.deadline_tick.min(effect.deadline_tick),
                epoch: intent_epoch,
                recovery_lease: None,
            },
            state: EffectState::Intent,
            result_digest: None,
            resolved_tick: None,
        });
        if record_step(
            journal,
            None,
            LifecycleEventKind::EffectIntent {
                sequence: effect.sequence,
            },
            intent.clone(),
            None,
        )
        .is_err()
        {
            return Err(infrastructure_failure("effect intent persistence failed"));
        }
        control = match observe_checked(
            observe,
            control,
            invocation,
            invocation.deadline_tick.min(effect.deadline_tick),
            journal,
        ) {
            Ok(control) => control,
            Err(outcome) => return Err(outcome),
        };
        let dispatch_deadline = invocation.deadline_tick.min(effect.deadline_tick);
        let audited = host.invoke(
            invocation,
            control.now_tick,
            control.cancellation_requested,
            effect,
        );
        // Dispatch has already happened. Sample the clock again before recording
        // completion so a slow synchronous provider receives its full retention
        // lifetime. A failed or regressed observation must not discard the result or
        // manufacture an early age; the committed result remains conservatively
        // unaged and therefore ineligible for retention until reconciliation.
        let post_dispatch_control = observe()
            .ok()
            .filter(|observed| observed.now_tick >= control.now_tick);
        let (result, audit) = audited.into_transaction_parts();
        let result_digest = match canonical_result_digest(&result.outcome) {
            Ok(digest) => digest,
            // Dispatch already happened; leave the durable intent for reconciliation.
            Err(_) => {
                return Err(failed_outcome(problem(
                    FailureCategory::UnknownOutcome,
                    "host result cannot be encoded after dispatch; reconcile the intent",
                )));
            }
        };
        let result_record = intent.map(|mut record| {
            record.state = match &result.outcome {
                Ok(_) => EffectState::Completed,
                Err(HostProblem::UnknownOutcome) => EffectState::UnknownOutcome,
                Err(_) => EffectState::Failed,
            };
            record.result_digest = Some(result_digest);
            record.resolved_tick =
                matches!(record.state, EffectState::Completed | EffectState::Failed)
                    .then(|| post_dispatch_control.map(|observed| observed.now_tick))
                    .flatten()
                    .filter(|tick| *tick != 0);
            record
        });
        if record_audited_step(
            journal,
            coordinator.audit_sink.as_deref(),
            None,
            LifecycleEventKind::EffectResult {
                sequence: result.sequence,
            },
            result_record,
            None,
            audit,
        )
        .is_err()
        {
            if mutating || matches!(&result.outcome, Err(HostProblem::UnknownOutcome)) {
                return Err(failed_outcome(problem(
                    FailureCategory::UnknownOutcome,
                    "host dispatch completed but result persistence failed; reconcile the intent",
                )));
            }
            return Err(infrastructure_failure("effect result persistence failed"));
        }
        if matches!(&result.outcome, Err(HostProblem::UnknownOutcome)) {
            // Never poll again or resume an ordinary exception handler
            // before preserving this already-observed uncertainty.
            if !resumable {
                let _ = record_step(
                    journal,
                    Some(ExecutionState::Failed),
                    LifecycleEventKind::Failed,
                    None,
                    None,
                );
            }
            return Err(failed_outcome(problem(
                FailureCategory::UnknownOutcome,
                "host outcome unknown",
            )));
        }
        let Some(observed) = post_dispatch_control else {
            let _ = record_step(
                journal,
                Some(ExecutionState::Failed),
                LifecycleEventKind::Failed,
                None,
                None,
            );
            return Err(infrastructure_failure(
                "post-dispatch execution clock unavailable or regressed",
            ));
        };
        let previous_control = control;
        control = observed;
        if let Err(outcome) = check_control(
            control,
            Some(previous_control),
            invocation,
            dispatch_deadline,
            journal,
        ) {
            return Err(outcome);
        }
        *control_slot = control;
        Ok(result)
    }
}

#[cfg(test)]
mod tests;
