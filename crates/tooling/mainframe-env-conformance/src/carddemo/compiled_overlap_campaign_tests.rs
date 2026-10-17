//! Controlled futures exercise scheduling and error ownership, with no corpus credit.
use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll, Waker};
use tokio::sync::oneshot;

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

fn pending_submissions(
    finished: &Arc<AtomicUsize>,
) -> (Vec<JoinHandle<Submission>>, Vec<oneshot::Sender<()>>) {
    let mut submissions = Vec::new();
    let mut releases = Vec::new();
    for _ in 0..16 {
        let (release, receiver) = oneshot::channel();
        let finished = finished.clone();
        submissions.push(tokio::spawn(async move {
            receiver.await.unwrap();
            finished.fetch_add(1, Ordering::SeqCst);
            Ok((StatusCode::CREATED, Vec::new()))
        }));
        releases.push(release);
    }
    (submissions, releases)
}

#[test]
fn prepared_resumes_are_polled_before_all_submission_joins_finish() {
    runtime().block_on(async {
        let finished = Arc::new(AtomicUsize::new(0));
        let resumes = Arc::new(AtomicUsize::new(0));
        let (submissions, releases) = pending_submissions(&finished);
        let resumed = resumes.clone();
        let exchanges = async move {
            let (account, cards) = tokio::join!(
                async {
                    resumed.fetch_add(1, Ordering::SeqCst);
                    serde_json::json!({"account": true})
                },
                async {
                    resumed.fetch_add(1, Ordering::SeqCst);
                    serde_json::json!({"cards": true})
                },
            );
            Ok((account, cards))
        };
        let mut campaign = Box::pin(join_campaign(submissions, exchanges));
        let mut cx = Context::from_waker(Waker::noop());
        assert!(matches!(campaign.as_mut().poll(&mut cx), Poll::Pending));
        // This fails immediately for the old join-all-before-online ordering;
        // no timer or scheduler speed is part of the regression oracle.
        assert_eq!(resumes.load(Ordering::SeqCst), 2);
        assert_eq!(finished.load(Ordering::SeqCst), 0);
        for release in releases {
            release.send(()).unwrap();
        }
        let (joined, exchanges) = campaign.await;
        assert_eq!(joined.len(), 16);
        assert!(joined.iter().all(|row| row.as_ref().unwrap().is_ok()));
        assert_eq!(finished.load(Ordering::SeqCst), 16);
        let (account, cards) = exchanges.unwrap();
        assert_eq!(account, serde_json::json!({"account": true}));
        assert_eq!(cards, serde_json::json!({"cards": true}));
    });
}

#[test]
fn online_failure_waits_for_every_offered_submission_before_return() {
    runtime().block_on(async {
        let finished = Arc::new(AtomicUsize::new(0));
        let (submissions, releases) = pending_submissions(&finished);
        let exchanges = async {
            Err(CorpusProblem::new(
                "controlled.online.failure",
                "preserve failure until all offered submissions join",
            ))
        };
        let mut campaign = Box::pin(join_campaign(submissions, exchanges));
        let mut cx = Context::from_waker(Waker::noop());
        assert!(matches!(campaign.as_mut().poll(&mut cx), Poll::Pending));
        assert_eq!(finished.load(Ordering::SeqCst), 0);
        for release in releases {
            release.send(()).unwrap();
        }
        let (joined, exchanges) = campaign.await;
        assert_eq!(joined.len(), 16);
        assert_eq!(finished.load(Ordering::SeqCst), 16);
        assert_eq!(exchanges.unwrap_err().code, "controlled.online.failure");
        // Caller-side drain/failed-namespace retention can only begin here,
        // after every offered task has joined despite the online failure.
    });
}

#[test]
fn submission_failure_and_panic_preserve_all_joins_and_online_outcome() {
    runtime().block_on(async {
        let finished = Arc::new(AtomicUsize::new(0));
        let mut submissions = Vec::new();
        let mut releases = Vec::new();
        for index in 0..16 {
            let (release, receiver) = oneshot::channel();
            let finished = finished.clone();
            submissions.push(tokio::spawn(async move {
                receiver.await.unwrap();
                finished.fetch_add(1, Ordering::SeqCst);
                match index {
                    0 => Err(CorpusProblem::new(
                        "controlled.submit.failure",
                        "first task failed",
                    )),
                    1 => panic!("controlled submission task panic"),
                    _ => Ok((StatusCode::CREATED, Vec::new())),
                }
            }));
            releases.push(release);
        }
        let exchanges = async { Ok((serde_json::json!(1), serde_json::json!(2))) };
        let mut campaign = Box::pin(join_campaign(submissions, exchanges));
        let mut cx = Context::from_waker(Waker::noop());
        assert!(matches!(campaign.as_mut().poll(&mut cx), Poll::Pending));
        for release in releases {
            release.send(()).unwrap();
        }
        let (joined, exchanges) = campaign.await;
        assert_eq!(joined.len(), 16);
        assert_eq!(finished.load(Ordering::SeqCst), 16);
        assert_eq!(
            joined[0].as_ref().unwrap().as_ref().unwrap_err().code,
            "controlled.submit.failure"
        );
        assert!(joined[1].as_ref().unwrap_err().is_panic());
        assert!(joined[2..].iter().all(|row| row.as_ref().unwrap().is_ok()));
        assert_eq!(
            exchanges.unwrap(),
            (serde_json::json!(1), serde_json::json!(2))
        );
    });
}
