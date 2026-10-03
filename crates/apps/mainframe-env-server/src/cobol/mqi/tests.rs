use super::*;
use mainframe_env_host_api::mq_mqi::{MqMqiContext, MqMqiLimits};
use mainframe_env_host_api::{MqHandleOwner, MqHostEnvironment, MqSyncpointOwner};
use mainframe_env_interpreter::MqMqiProgramProfile;
use mainframe_env_store::MemoryStore;
use std::sync::Mutex;
mod installed;
mod sessions;
mod setup;

struct Admission {
    observed: Mutex<Vec<Invocation>>,
    events: Arc<Mutex<Vec<ExecutionOutcome>>>,
    aborts: Arc<Mutex<Vec<HostProblem>>>,
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
struct Session {
    frame: Arc<dyn MqMqiProgramFrame>,
    events: Arc<Mutex<Vec<ExecutionOutcome>>>,
    aborts: Arc<Mutex<Vec<HostProblem>>>,
    store: Arc<dyn PlatformStore>,
    control: Arc<dyn ProgramExecutionControl>,
}
impl InstalledMqFrameSession for Session {
    fn store(&self) -> &Arc<dyn PlatformStore> {
        &self.store
    }
    fn execution_control(&self) -> &Arc<dyn ProgramExecutionControl> {
        &self.control
    }
    fn program_frame(&self) -> Result<Arc<dyn MqMqiProgramFrame>, HostProblem> {
        Ok(self.frame.clone())
    }
    fn abort_preparation(&mut self, problem: &HostProblem) -> Result<(), HostProblem> {
        self.aborts.lock().unwrap().push(problem.clone());
        Ok(())
    }
    fn finish(&mut self, outcome: &ExecutionOutcome) -> Result<(), HostProblem> {
        self.events.lock().unwrap().push(outcome.clone());
        Ok(())
    }
}
impl ProgramMqHostAdmission for Admission {
    fn admit_installed_batch(
        &self,
        proof: &InstalledBatchAdmission<'_>,
    ) -> Result<Box<dyn InstalledMqFrameSession>, HostProblem> {
        let invocation = proof.child();
        assert_eq!(
            proof.core_intent().execution_id,
            proof.parent().execution_id
        );
        assert_eq!(
            proof.catalog_record().unwrap().payload,
            proof.artifact().as_str().as_bytes()
        );
        assert!(
            proof
                .artifact_metadata()
                .validates_payload(&proof.content_digest())
        );
        self.observed.lock().unwrap().push(invocation.clone());
        if self.refuse {
            Err(HostProblem::Unauthorized)
        } else {
            Ok(Box::new(Session {
                frame: Arc::new(Frame(invocation.clone())),
                events: self.events.clone(),
                aborts: self.aborts.clone(),
                store: proof.store().clone(),
                control: proof.execution_control().clone(),
            }))
        }
    }
}
fn admission(refuse: bool) -> Arc<Admission> {
    Arc::new(Admission {
        observed: Mutex::new(vec![]),
        events: Arc::new(Mutex::new(vec![])),
        aborts: Arc::new(Mutex::new(vec![])),
        refuse,
    })
}

#[test]
fn unset_factory_preserves_existing_batch_profile_without_state_access() {
    let program = CobolProgram::new();
    let mut invocation = super::super::hardening::parent();
    let before = invocation.clone();
    assert!(
        program
            .admit_batch_mqi(&mut invocation, None, None)
            .unwrap()
            .is_none()
    );
    assert_eq!(invocation, before);
}

#[test]
fn configured_direct_helper_cannot_fabricate_an_original_call() {
    let program = CobolProgram::new();
    let factory = admission(false);
    assert!(program.mqi_host.set(factory.clone()).is_ok());
    assert!(
        program
            .store
            .set(Arc::new(MemoryStore::new(Default::default())))
            .is_ok()
    );
    assert!(program.control.set(setup::control()).is_ok());
    let mut invocation = super::super::hardening::parent();
    let before = invocation.clone();
    assert!(matches!(
        program.admit_batch_mqi(&mut invocation, None, None),
        Err(HostProblem::Unsupported)
    ));
    assert_eq!(invocation, before);
    assert!(factory.observed.lock().unwrap().is_empty());
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
