//! Complete original mixed jobs and retire their actual owners inside one bound.
use super::{CorpusProblem, require};
use std::time::Duration;

pub(super) fn retired(completed: usize, active: usize, jes_active: usize) -> bool {
    completed == 16 && active == 0 && jes_active == 0
}

pub(super) async fn wait(
    original_deadline: tokio::time::Instant,
    mut sample: impl FnMut() -> Result<bool, CorpusProblem>,
) -> Result<(), CorpusProblem> {
    loop {
        if sample()? {
            return Ok(());
        }
        require(
            tokio::time::Instant::now() < original_deadline,
            "sixteen compiled jobs did not complete inside original 120-second bound",
        )?;
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    #[test]
    fn terminal_snapshots_wait_for_queued_owner_retirement_on_current_thread() {
        runtime().block_on(async {
            let jes_active = Arc::new(AtomicUsize::new(1));
            let first_sample_seen = Arc::new(AtomicBool::new(false));
            let worker_active = jes_active.clone();
            let worker_seen = first_sample_seen.clone();
            let retirement = tokio::spawn(async move {
                assert!(worker_seen.load(Ordering::SeqCst));
                worker_active.store(0, Ordering::SeqCst);
            });
            let mut samples = 0;
            wait(
                tokio::time::Instant::now() + Duration::from_secs(120),
                || {
                    samples += 1;
                    first_sample_seen.store(true, Ordering::SeqCst);
                    Ok(retired(16, 0, jes_active.load(Ordering::SeqCst)))
                },
            )
            .await
            .unwrap();
            assert!(samples >= 2);
            retirement.await.unwrap();
            assert_eq!(jes_active.load(Ordering::SeqCst), 0);
        });
    }

    #[test]
    fn original_deadline_counts_and_sampler_errors_still_refuse() {
        assert!(retired(16, 0, 0));
        for values in [(0, 0, 0), (15, 0, 0), (17, 0, 0), (16, 1, 0), (16, 0, 1)] {
            assert!(!retired(values.0, values.1, values.2));
        }
        runtime().block_on(async {
            let expired = tokio::time::Instant::now() - Duration::from_secs(120);
            for values in [(15, 0, 0), (16, 1, 0), (16, 0, 1)] {
                let mut samples = 0;
                let problem = wait(expired, || {
                    samples += 1;
                    Ok(retired(values.0, values.1, values.2))
                })
                .await
                .unwrap_err();
                assert_eq!(samples, 1);
                assert_eq!(problem.code, "carddemo.full.compiled_overlap_drift");
                assert_eq!(
                    problem.detail,
                    "sixteen compiled jobs did not complete inside original 120-second bound"
                );
            }
            let problem = wait(expired, || {
                Err(CorpusProblem::new(
                    "fixed.sampler.failure",
                    "original error",
                ))
            })
            .await
            .unwrap_err();
            assert_eq!(problem.code, "fixed.sampler.failure");
            assert_eq!(problem.detail, "original error");
            // The owner's existing final deadline check is still mandatory
            // even if all jobs and owners become complete after the deadline.
            wait(expired, || Ok(retired(16, 0, 0))).await.unwrap();
            assert!(
                require(
                    tokio::time::Instant::now() <= expired,
                    "compiled mixed workload exceeded the unchanged 120-second completion bound",
                )
                .is_err()
            );
        });
    }
}
