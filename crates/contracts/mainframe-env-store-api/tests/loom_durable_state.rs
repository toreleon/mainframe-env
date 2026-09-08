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
