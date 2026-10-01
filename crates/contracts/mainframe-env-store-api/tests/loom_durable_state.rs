use loom::sync::{Arc, Mutex};
use mainframe_env_store_api::{EffectState, ExecutionState};

#[derive(Clone, Copy, Debug)]
struct Versioned<T> {
    state: T,
    version: u64,
}

fn transition_execution(
    shared: &Mutex<Versioned<ExecutionState>>,
    expected_version: u64,
    next: ExecutionState,
) -> bool {
    let mut current = shared.lock().expect("modeled execution mutex");
    if current.version != expected_version || !current.state.can_transition_to(next) {
        return false;
    }
    current.state = next;
    current.version += 1;
    true
}

fn finish_effect(
    shared: &Mutex<Versioned<EffectState>>,
    expected_version: u64,
    next: EffectState,
) -> bool {
    let mut current = shared.lock().expect("modeled effect mutex");
    if current.version != expected_version
        || current.state != EffectState::Intent
        || !matches!(
            next,
            EffectState::Completed | EffectState::Failed | EffectState::UnknownOutcome
        )
    {
        return false;
    }
    current.state = next;
    current.version += 1;
    true
}

#[test]
fn version_fence_allows_exactly_one_concurrent_execution_transition() {
    loom::model(|| {
        let shared = Arc::new(Mutex::new(Versioned {
            state: ExecutionState::Admitted,
            version: 1,
        }));
        let queued = {
            let shared = shared.clone();
            loom::thread::spawn(move || transition_execution(&shared, 1, ExecutionState::Queued))
        };
        let cancelled = {
            let shared = shared.clone();
            loom::thread::spawn(move || transition_execution(&shared, 1, ExecutionState::Cancelled))
        };
        let winners = usize::from(queued.join().expect("queued transition thread"))
            + usize::from(cancelled.join().expect("cancel transition thread"));
        let current = *shared.lock().expect("modeled execution mutex");
        assert_eq!(winners, 1);
        assert_eq!(current.version, 2);
        assert!(matches!(
            current.state,
            ExecutionState::Queued | ExecutionState::Cancelled
        ));
    });
}

#[test]
fn version_fence_allows_exactly_one_concurrent_effect_result() {
    loom::model(|| {
        let shared = Arc::new(Mutex::new(Versioned {
            state: EffectState::Intent,
            version: 1,
        }));
        let completed = {
            let shared = shared.clone();
            loom::thread::spawn(move || finish_effect(&shared, 1, EffectState::Completed))
        };
        let uncertain = {
            let shared = shared.clone();
            loom::thread::spawn(move || finish_effect(&shared, 1, EffectState::UnknownOutcome))
        };
        let winners = usize::from(completed.join().expect("completed effect thread"))
            + usize::from(uncertain.join().expect("uncertain effect thread"));
        let current = *shared.lock().expect("modeled effect mutex");
        assert_eq!(winners, 1);
        assert_eq!(current.version, 2);
        assert!(matches!(
            current.state,
            EffectState::Completed | EffectState::UnknownOutcome
        ));
    });
}

#[test]
fn loom_finds_the_deliberately_unfenced_lost_update() {
    let detected = std::panic::catch_unwind(|| {
        loom::model(|| {
            use loom::sync::atomic::{AtomicUsize, Ordering};

            let counter = Arc::new(AtomicUsize::new(0));
            let first = {
                let counter = counter.clone();
                loom::thread::spawn(move || {
                    let observed = counter.load(Ordering::Relaxed);
                    loom::thread::yield_now();
                    counter.store(observed + 1, Ordering::Relaxed);
                })
            };
            let second = {
                let counter = counter.clone();
                loom::thread::spawn(move || {
                    let observed = counter.load(Ordering::Relaxed);
                    loom::thread::yield_now();
                    counter.store(observed + 1, Ordering::Relaxed);
                })
            };
            first.join().expect("first mutant thread");
            second.join().expect("second mutant thread");
            assert_eq!(counter.load(Ordering::Relaxed), 2);
        });
    });
    assert!(detected.is_err(), "Loom did not reject the unfenced mutant");
}

#[derive(Clone, Copy, Debug)]
struct CicsSyncpointModel {
    owner_epoch: u64,
    version: u64,
    effect: EffectState,
    cancellation_requested: bool,
    committed_mutations: u8,
}

fn cics_commit(shared: &Mutex<CicsSyncpointModel>, owner_epoch: u64, version: u64) -> bool {
    let mut row = shared.lock().expect("modeled CICS syncpoint mutex");
    if row.owner_epoch != owner_epoch
        || row.version != version
        || row.effect != EffectState::Intent
        || row.cancellation_requested
    {
        return false;
    }
    row.effect = EffectState::Completed;
    row.committed_mutations += 1;
    row.version += 1;
    true
}

fn cics_restart_claim(shared: &Mutex<CicsSyncpointModel>, version: u64) -> bool {
    let mut row = shared.lock().expect("modeled CICS syncpoint mutex");
    if row.version != version || row.effect != EffectState::Intent {
        return false;
    }
    row.owner_epoch += 1;
    row.version += 1;
    true
}

#[test]
fn cics_restart_claim_fences_stale_syncpoint_owner_and_replays_once() {
    loom::model(|| {
        let shared = Arc::new(Mutex::new(CicsSyncpointModel {
            owner_epoch: 1,
            version: 1,
            effect: EffectState::Intent,
            cancellation_requested: false,
            committed_mutations: 0,
        }));
        let old_owner = {
            let shared = shared.clone();
            loom::thread::spawn(move || cics_commit(&shared, 1, 1))
        };
        let restart = {
            let shared = shared.clone();
            loom::thread::spawn(move || cics_restart_claim(&shared, 1))
        };
        let old_committed = old_owner.join().expect("old owner thread");
        let reclaimed = restart.join().expect("restart thread");
        assert_ne!(old_committed, reclaimed);
        if reclaimed {
            assert!(
                !cics_commit(&shared, 1, 1),
                "stale owner crossed the epoch fence"
            );
            assert!(cics_commit(&shared, 2, 2), "recovery owner completes once");
        }
        assert!(
            !cics_commit(&shared, 2, 2),
            "replayed syncpoint cannot commit twice"
        );
        let row = *shared.lock().expect("modeled CICS syncpoint mutex");
        assert_eq!(row.effect, EffectState::Completed);
        assert_eq!(row.committed_mutations, 1);
    });
}

#[test]
fn cics_cancel_racing_syncpoint_never_reopens_a_terminal_effect() {
    loom::model(|| {
        let shared = Arc::new(Mutex::new(CicsSyncpointModel {
            owner_epoch: 1,
            version: 1,
            effect: EffectState::Intent,
            cancellation_requested: false,
            committed_mutations: 0,
        }));
        let commit = {
            let shared = shared.clone();
            loom::thread::spawn(move || cics_commit(&shared, 1, 1))
        };
        let cancel = {
            let shared = shared.clone();
            loom::thread::spawn(move || {
                let mut row = shared.lock().expect("modeled CICS syncpoint mutex");
                row.cancellation_requested = true;
                if row.effect == EffectState::Intent {
                    row.effect = EffectState::Failed;
                    row.version += 1;
                }
            })
        };
        let committed = commit.join().expect("commit thread");
        cancel.join().expect("cancel thread");
        let row = *shared.lock().expect("modeled CICS syncpoint mutex");
        assert_eq!(row.committed_mutations, u8::from(committed));
        assert_eq!(
            row.effect,
            if committed {
                EffectState::Completed
            } else {
                EffectState::Failed
            }
        );
        assert!(!cics_restart_claim(&shared, row.version));
        assert!(!cics_commit(&shared, row.owner_epoch, row.version));
    });
}
