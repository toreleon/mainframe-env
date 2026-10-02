#[cfg(test)]
mod tests {
    use super::super::*;
    use mainframe_env_execution_api::CancellationProbe;

    fn run(factory: u8, cancel: bool) {
        let root = TestRoot::new();
        let fixture = Fixture::new(
            &root,
            Arc::new(MemoryStore::new(Default::default())),
            HostProblem::NotFound,
            false,
        );
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. CONTROL. PROCEDURE DIVISION. GOBACK.";
        fixture.install("CONTROL", source);
        let mut source_actor = parent();
        source_actor.audit_correlation = "actual-parent-audit".into();
        source_actor.priority = 87;
        source_actor.deadline_tick = 100;
        source_actor.limits.max_frames = 4;
        source_actor.limits.max_storage_bytes = 128 * 1024;
        source_actor.provider_generations.insert(
            CapabilityId::new("host.program.invoke", InvocationLimits::default()).unwrap(),
            "1".into(),
        );
        let probe = CancellationProbe::default();
        source_actor.cancellation_probe = Some(probe.clone());
        let observed = Arc::new(Mutex::new(Vec::<Invocation>::new()));
        let captured = observed.clone();
        let signal = probe.clone();
        let original_execution = source_actor.execution_id.clone();
        fixture
            .router
            .bind_execution_control(Arc::new(move |actor: &Invocation| {
                if actor.execution_id != original_execution {
                    captured.lock().unwrap().push(actor.clone());
                }
                if cancel {
                    signal.request();
                }
                Ok(ExecutionControl {
                    now_tick: 1,
                    cancellation_requested: false,
                })
            }))
            .unwrap();
        let (program, payload) = if factory == 0 {
            ("CONTROL", call_payload(&[]))
        } else {
            (
                if factory == 1 { "CONTROL" } else { "COBOL" },
                BoundedPayload::new(
                    "mainframe-env.program.input@1",
                    serde_json::to_vec(&input(source)).unwrap(),
                    InvocationLimits::default(),
                )
                .unwrap(),
            )
        };
        let result = fixture.call(&source_actor, program, 1, payload);
        let seen = observed.lock().unwrap();
        assert!(
            !seen.is_empty(),
            "factory {factory} reached actual child control: {:?}",
            result.outcome
        );
        for actor in seen.iter() {
            assert_ne!(actor.execution_id, source_actor.execution_id);
            assert_eq!(
                actor.parent_execution_id.as_ref(),
                Some(&source_actor.execution_id)
            );
            assert_eq!(
                actor.audit_correlation, "actual-parent-audit",
                "factory {factory}"
            );
            assert_eq!(
                actor.cancellation_probe, source_actor.cancellation_probe,
                "factory {factory}"
            );
            assert_eq!(actor.cancellation, source_actor.cancellation);
            assert_eq!(actor.priority, 87);
            assert_eq!(actor.service_class, source_actor.service_class);
            assert_eq!(actor.deadline_tick, 100);
            assert_eq!(actor.limits, source_actor.limits);
            assert_eq!(actor.principal, source_actor.principal);
            assert_eq!(
                actor.provider_generations,
                source_actor.provider_generations
            );
            assert_ne!(actor.trace_id, source_actor.trace_id);
        }
        if cancel {
            assert!(probe.is_requested());
            assert_eq!(
                result.outcome,
                Err(HostProblem::Cancelled),
                "factory {factory}"
            );
        } else {
            assert!(
                matches!(result.outcome, Ok(HostResult::Program(_))),
                "factory {factory}: {:?}",
                result.outcome
            );
        }
    }

    #[test]
    fn compiled_native_and_both_batch_factories_preserve_actual_audit_and_control() {
        for factory in 0..3 {
            run(factory, false);
        }
    }

    #[test]
    fn cancellation_requested_during_actual_child_observation_wins_in_all_factories() {
        for factory in 0..3 {
            run(factory, true);
        }
    }
}
