use super::*;
use mainframe_env_host_api::mq_mqi::{MqMqiContext, MqMqiLimits};
use mainframe_env_host_api::{MqHandleOwner, MqHostEnvironment, MqSyncpointOwner};
use mainframe_env_interpreter::MqMqiProgramProfile;
use mainframe_env_store::MemoryStore;
use std::sync::Mutex;
mod installed;

struct Admission {
    observed: Mutex<Vec<Invocation>>,
    refuse: bool,
}
struct Frame(Invocation);
impl MqMqiProgramFrame for Frame {
    fn profile(&self, invocation: &Invocation) -> Result<MqMqiProgramProfile, HostProblem> {
        if invocation != &self.0 {
            return Err(HostProblem::Unauthorized);
        }
        Ok(MqMqiProgramProfile {
            context: MqMqiContext {
                owner: MqHandleOwner {
                    environment: MqHostEnvironment::ZosBatch,
                    host_id: 1,
                    process_id: 2,
                    thread_id: 3,
                    task_id: 4,
                    syncpoint_epoch: 1,
                },
                syncpoint_owner: MqSyncpointOwner::QueueManager,
            },
            limits: MqMqiLimits::default(),
        })
    }
}
impl ProgramMqHostAdmission for Admission {
    fn admit_installed_batch(
        &self,
        invocation: &Invocation,
        _: &dyn PlatformStore,
    ) -> Result<Arc<dyn MqMqiProgramFrame>, HostProblem> {
        self.observed.lock().unwrap().push(invocation.clone());
        if self.refuse {
            Err(HostProblem::Unauthorized)
        } else {
            Ok(Arc::new(Frame(invocation.clone())))
        }
    }
}
fn admission(refuse: bool) -> Arc<Admission> {
    Arc::new(Admission {
        observed: Mutex::new(vec![]),
        refuse,
    })
}

#[test]
fn unset_factory_preserves_existing_batch_profile_without_state_access() {
    let program = CobolProgram::new();
    let mut invocation = super::super::hardening::parent();
    let before = invocation.clone();
    assert!(program.admit_batch_mqi(&mut invocation).unwrap().is_none());
    assert_eq!(invocation, before);
}

#[test]
fn configured_factory_observes_actual_identity_and_host_selected_context() {
    let program = CobolProgram::new();
    let factory = admission(false);
    assert!(program.mqi_host.set(factory.clone()).is_ok());
    assert!(
        program
            .store
            .set(Arc::new(MemoryStore::new(Default::default())))
            .is_ok()
    );
    let mut invocation = super::super::hardening::parent();
    invocation.bindings.insert(
        "mq.host-context".into(),
        BoundedPayload::new(
            "mainframe-env.mq.host-context@1",
            b"other-bindings|queue-manager".to_vec(),
            InvocationLimits::default(),
        )
        .unwrap(),
    );
    let mut expected = invocation.clone();
    let frame = program.admit_batch_mqi(&mut invocation).unwrap().unwrap();
    expected.bindings = invocation.bindings.clone();
    assert_eq!(invocation, expected);
    let binding = &invocation.bindings["mq.host-context"];
    assert_eq!(binding.bytes(), b"zos-batch|queue-manager");
    assert_eq!(binding.schema(), "mainframe-env.mq.host-context@1");
    assert_eq!(
        factory.observed.lock().unwrap().as_slice(),
        &[invocation.clone()]
    );
    assert!(frame.profile(&invocation).is_ok());
}

#[test]
fn unsupported_nested_provenance_is_not_erased_and_denial_stops_admission() {
    let program = CobolProgram::new();
    let factory = admission(true);
    assert!(program.mqi_host.set(factory.clone()).is_ok());
    assert!(
        program
            .store
            .set(Arc::new(MemoryStore::new(Default::default())))
            .is_ok()
    );
    for key in [
        "cics.execution-context",
        "cics.nested-effect-origin",
        "cics.outer-effect-origin",
    ] {
        let mut invocation = super::super::hardening::parent();
        invocation.bindings.insert(
            key.into(),
            BoundedPayload::new("schema", vec![1], InvocationLimits::default()).unwrap(),
        );
        let before = invocation.clone();
        assert!(matches!(
            program.admit_batch_mqi(&mut invocation),
            Err(HostProblem::Unsupported)
        ));
        assert_eq!(invocation, before);
    }
    assert!(factory.observed.lock().unwrap().is_empty());
    let mut invocation = super::super::hardening::parent();
    assert!(matches!(
        program.admit_batch_mqi(&mut invocation),
        Err(HostProblem::Unauthorized)
    ));
}

#[test]
fn host_factory_is_single_setup_and_cannot_be_selected_by_invocation_bindings() {
    let router = default_program_router();
    router.bind_mqi_program_host(admission(false)).unwrap();
    assert_eq!(
        router.bind_mqi_program_host(admission(false)),
        Err(HostProblem::IdempotencyConflict)
    );
    let registry =
        mainframe_env_host_api::RegistrySnapshot::new(1, vec![], InvocationLimits::default())
            .unwrap();
    assert!(
        router
            .cobol
            .host
            .set(Arc::new(ScopedHostService::new(
                Arc::new(registry),
                Default::default()
            )))
            .is_ok()
    );
    assert_eq!(
        router.bind_mqi_program_host(admission(false)),
        Err(HostProblem::IdempotencyConflict)
    );
}
