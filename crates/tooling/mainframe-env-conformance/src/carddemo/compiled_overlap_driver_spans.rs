//! Bounded driver observations only; no backend admission or overlap proof.
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

const REQUESTS: usize = 18;

#[derive(Clone, Copy)]
struct Stamp {
    elapsed_ns: Option<u64>,
    unix_ms: Option<u64>,
}

impl Stamp {
    fn json(self) -> serde_json::Value {
        serde_json::json!({"elapsed_ns": self.elapsed_ns, "unix_ms": self.unix_ms})
    }
}

#[derive(Clone, Copy, Default)]
struct Row {
    offered: Option<Stamp>,
    first_poll: Option<Stamp>,
    future_ready: Option<Stamp>,
    dropped: Option<Stamp>,
    polls: u64,
}

enum Event {
    Offered,
    Poll,
    Ready,
    Dropped,
}

pub(super) struct DriverSpans {
    origin: Instant,
    rows: Mutex<[Row; REQUESTS]>,
    incomplete: AtomicBool,
}

impl DriverSpans {
    pub(super) fn new() -> Self {
        Self {
            origin: Instant::now(),
            rows: Mutex::new([Row::default(); REQUESTS]),
            incomplete: AtomicBool::new(false),
        }
    }

    fn stamp(&self) -> Stamp {
        // Independent observations, not one atomic cross-clock sample. These
        // reads never replace the request's original deadline/clock sources.
        let elapsed_ns = u64::try_from(self.origin.elapsed().as_nanos()).ok();
        let unix_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|value| u64::try_from(value.as_millis()).ok());
        if elapsed_ns.is_none() || unix_ms.is_none() {
            self.incomplete.store(true, Ordering::Relaxed);
        }
        Stamp {
            elapsed_ns,
            unix_ms,
        }
    }

    fn event(&self, index: usize, event: Event) {
        let stamp = self.stamp();
        // Diagnostics must not wait for another recorder or replace a request
        // result. Poison, contention and invalid slots stay explicit uncertainty.
        let Ok(mut rows) = self.rows.try_lock() else {
            self.incomplete.store(true, Ordering::Relaxed);
            return;
        };
        let Some(row) = rows.get_mut(index) else {
            self.incomplete.store(true, Ordering::Relaxed);
            return;
        };
        match event {
            Event::Offered if row.offered.is_none() => row.offered = Some(stamp),
            Event::Poll => {
                if row.offered.is_none() || row.future_ready.is_some() {
                    self.incomplete.store(true, Ordering::Relaxed);
                }
                row.first_poll.get_or_insert(stamp);
                if let Some(polls) = row.polls.checked_add(1) {
                    row.polls = polls;
                } else {
                    self.incomplete.store(true, Ordering::Relaxed);
                }
            }
            Event::Ready if row.first_poll.is_some() && row.future_ready.is_none() => {
                row.future_ready = Some(stamp);
            }
            Event::Dropped if row.dropped.is_none() => row.dropped = Some(stamp),
            _ => self.incomplete.store(true, Ordering::Relaxed),
        }
    }

    pub(super) fn observe<F: Future>(
        self: &Arc<Self>,
        index: usize,
        future: F,
    ) -> RequestFuture<F> {
        self.event(index, Event::Offered);
        RequestFuture {
            log: self.clone(),
            index,
            inner: Box::pin(future),
        }
    }

    pub(super) fn export(&self) -> serde_json::Value {
        let snapshot = self.rows.try_lock().ok().map(|rows| *rows);
        if snapshot.is_none() {
            self.incomplete.store(true, Ordering::Relaxed);
        }
        let rows = snapshot.map(|rows| {
            rows.iter()
                .enumerate()
                .map(|(index, row)| {
                    let request = match index {
                        16 => "account-resume".to_string(),
                        17 => "cards-resume".to_string(),
                        _ => format!("batch-submit-{:02}", index + 1),
                    };
                    serde_json::json!({
                        "slot": index, "request": request,
                        "offered": row.offered.map(Stamp::json),
                        "first_poll": row.first_poll.map(Stamp::json),
                        "future_ready": row.future_ready.map(Stamp::json),
                        "dropped": row.dropped.map(Stamp::json), "polls": row.polls,
                    })
                })
                .collect::<Vec<_>>()
        });
        serde_json::json!({
            "schema": "mainframe-env.compiled-overlap-driver-spans@1",
            "request_slots": REQUESTS,
            "observer_incomplete": self.incomplete.load(Ordering::Relaxed),
            "all_18_offered": snapshot.is_some_and(|rows| rows.iter().all(|row| row.offered.is_some())),
            "all_18_futures_ready": snapshot.is_some_and(|rows| rows.iter().all(|row| row.future_ready.is_some())),
            "rows": rows,
            "clock_qualification": "elapsed_ns is monotonic driver wall time, not CPU. unix_ms is a separate non-atomic wallclock sample and may regress. Journal ticks retain their original clock and actual commit ordinals; do not infer cross-clock ordering or exact backend phases.",
            "scope": "offered=wrapper construction; first_poll=entry before original future poll; future_ready=original future returned Ready, including Err; dropped=wrapper drop. No actual backend admission/authentication/preflight timestamps or HTTP-success inference.",
            "observer_overhead": "one boxed future per request, bounded clock reads and nonblocking diagnostic locks; timing is instrumented, not an unchanged baseline.",
            "overlap_acceptance_credit": 0,
        })
    }
}

pub(super) struct RequestFuture<F: Future> {
    log: Arc<DriverSpans>,
    index: usize,
    inner: Pin<Box<F>>,
}

impl<F: Future> Future for RequestFuture<F> {
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        this.log.event(this.index, Event::Poll);
        match this.inner.as_mut().poll(context) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(value) => {
                this.log.event(this.index, Event::Ready);
                Poll::Ready(value)
            }
        }
    }
}

impl<F: Future> Drop for RequestFuture<F> {
    fn drop(&mut self) {
        self.log.event(self.index, Event::Dropped);
        // Ordinary field drop retains the original future's cancellation/drop.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::task::{Wake, Waker};

    struct OpaquePayload;

    #[derive(Default)]
    struct WakeCount(AtomicUsize);

    impl Wake for WakeCount {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }

        fn wake_by_ref(self: &Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    struct Controlled {
        ready: Arc<AtomicBool>,
        polls: Arc<AtomicUsize>,
        drops: Arc<AtomicUsize>,
        expected_waker: Waker,
        payload: Option<Arc<OpaquePayload>>,
    }

    impl Future for Controlled {
        type Output = Arc<OpaquePayload>;

        fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
            let this = self.get_mut();
            assert!(context.waker().will_wake(&this.expected_waker));
            this.polls.fetch_add(1, Ordering::SeqCst);
            if this.ready.load(Ordering::SeqCst) {
                Poll::Ready(this.payload.take().unwrap())
            } else {
                Poll::Pending
            }
        }
    }

    impl Drop for Controlled {
        fn drop(&mut self) {
            self.drops.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn controlled(
        ready: &Arc<AtomicBool>,
        polls: &Arc<AtomicUsize>,
        drops: &Arc<AtomicUsize>,
        waker: &Waker,
        payload: &Arc<OpaquePayload>,
    ) -> Controlled {
        Controlled {
            ready: ready.clone(),
            polls: polls.clone(),
            drops: drops.clone(),
            expected_waker: waker.clone(),
            payload: Some(payload.clone()),
        }
    }

    #[test]
    fn pending_ready_preserves_context_output_and_exact_polls_without_wakes() {
        let log = Arc::new(DriverSpans::new());
        let ready = Arc::new(AtomicBool::new(false));
        let polls = Arc::new(AtomicUsize::new(0));
        let drops = Arc::new(AtomicUsize::new(0));
        let payload = Arc::new(OpaquePayload);
        let wakes = Arc::new(WakeCount::default());
        let waker = Waker::from(wakes.clone());
        let mut context = Context::from_waker(&waker);
        let mut request =
            Box::pin(log.observe(0, controlled(&ready, &polls, &drops, &waker, &payload)));
        assert_eq!(polls.load(Ordering::SeqCst), 0);
        assert!(log.export()["rows"][0]["first_poll"].is_null());
        for expected in 1..=2 {
            assert!(matches!(request.as_mut().poll(&mut context), Poll::Pending));
            assert_eq!(polls.load(Ordering::SeqCst), expected);
            let snapshot = log.export();
            assert_eq!(snapshot["rows"][0]["polls"], expected);
            assert!(snapshot["rows"][0]["future_ready"].is_null());
            assert_eq!(polls.load(Ordering::SeqCst), expected);
            assert_eq!(wakes.0.load(Ordering::SeqCst), 0);
        }
        ready.store(true, Ordering::SeqCst);
        let Poll::Ready(actual) = request.as_mut().poll(&mut context) else {
            panic!("controlled ready result lost");
        };
        assert!(Arc::ptr_eq(&actual, &payload));
        let row = &log.export()["rows"][0];
        assert_eq!(row["polls"], 3);
        assert!(
            row["offered"]["elapsed_ns"].as_u64().unwrap()
                <= row["first_poll"]["elapsed_ns"].as_u64().unwrap()
        );
        assert!(
            row["first_poll"]["elapsed_ns"].as_u64().unwrap()
                <= row["future_ready"]["elapsed_ns"].as_u64().unwrap()
        );
        drop(request);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        assert_eq!(polls.load(Ordering::SeqCst), 3);
        assert_eq!(wakes.0.load(Ordering::SeqCst), 0);
        assert!(log.export()["rows"][0]["dropped"].is_object());
        assert_eq!(log.export()["observer_incomplete"], false);
    }

    #[test]
    fn ready_error_identity_and_caller_selected_poll_order_are_unchanged() {
        let log = Arc::new(DriverSpans::new());
        let payload = Arc::new(OpaquePayload);
        let seen = Arc::new(Mutex::new(Vec::new()));
        let account_seen = seen.clone();
        let account_payload = payload.clone();
        let mut account = Box::pin(log.observe(16, async move {
            account_seen.lock().unwrap().push("account");
            Err::<(), _>(account_payload)
        }));
        let cards_seen = seen.clone();
        let mut cards = Box::pin(log.observe(17, async move {
            cards_seen.lock().unwrap().push("cards");
            Ok::<_, ()>(7)
        }));
        assert!(seen.lock().unwrap().is_empty());
        let wakes = Arc::new(WakeCount::default());
        let waker = Waker::from(wakes.clone());
        let mut context = Context::from_waker(&waker);
        assert!(matches!(
            cards.as_mut().poll(&mut context),
            Poll::Ready(Ok(7))
        ));
        let Poll::Ready(Err(actual)) = account.as_mut().poll(&mut context) else {
            panic!("controlled original error lost");
        };
        assert!(Arc::ptr_eq(&actual, &payload));
        assert_eq!(*seen.lock().unwrap(), ["cards", "account"]);
        assert_eq!(wakes.0.load(Ordering::SeqCst), 0);
        let snapshot = log.export();
        for index in [16, 17] {
            assert_eq!(snapshot["rows"][index]["polls"], 1);
            assert!(snapshot["rows"][index]["future_ready"].is_object());
        }
        assert_eq!(snapshot["all_18_futures_ready"], false);
        // OpaquePayload has no Debug or Serialize implementation. Returning an
        // error still records only Ready, not HTTP success or payload contents.
        assert!(
            !serde_json::to_string(&snapshot)
                .unwrap()
                .contains("OpaquePayload")
        );
    }

    #[test]
    fn drop_before_first_poll_and_while_pending_drop_inner_exactly_once() {
        let log = Arc::new(DriverSpans::new());
        let ready = Arc::new(AtomicBool::new(false));
        let wakes = Arc::new(WakeCount::default());
        let waker = Waker::from(wakes.clone());
        let mut context = Context::from_waker(&waker);
        for (index, should_poll) in [(0, false), (1, true)] {
            let polls = Arc::new(AtomicUsize::new(0));
            let drops = Arc::new(AtomicUsize::new(0));
            let payload = Arc::new(OpaquePayload);
            let mut request =
                Box::pin(log.observe(index, controlled(&ready, &polls, &drops, &waker, &payload)));
            if should_poll {
                assert!(matches!(request.as_mut().poll(&mut context), Poll::Pending));
            }
            drop(request);
            assert_eq!(polls.load(Ordering::SeqCst), usize::from(should_poll));
            assert_eq!(drops.load(Ordering::SeqCst), 1);
            let row = &log.export()["rows"][index];
            assert_eq!(row["first_poll"].is_object(), should_poll);
            assert!(row["future_ready"].is_null());
            assert!(row["dropped"].is_object());
        }
        assert_eq!(wakes.0.load(Ordering::SeqCst), 0);
        assert_eq!(log.export()["observer_incomplete"], false);
    }

    #[test]
    fn eighteen_slots_are_bounded_and_bad_offers_do_not_replace_outputs() {
        let log = Arc::new(DriverSpans::new());
        let mut requests = (0..REQUESTS)
            .map(|index| Box::pin(log.observe(index, std::future::ready(index))))
            .collect::<Vec<_>>();
        let original_offer = log.export()["rows"][0]["offered"].clone();
        let mut duplicate = Box::pin(log.observe(0, std::future::ready(99)));
        assert_eq!(log.export()["rows"][0]["offered"], original_offer);
        let mut unknown =
            Box::pin(log.observe(REQUESTS, std::future::ready("private-output-sentinel")));
        let wakes = Arc::new(WakeCount::default());
        let waker = Waker::from(wakes.clone());
        let mut context = Context::from_waker(&waker);
        assert!(matches!(
            duplicate.as_mut().poll(&mut context),
            Poll::Ready(99)
        ));
        assert!(matches!(
            unknown.as_mut().poll(&mut context),
            Poll::Ready("private-output-sentinel")
        ));
        for (index, request) in requests.iter_mut().enumerate() {
            let Poll::Ready(actual) = request.as_mut().poll(&mut context) else {
                panic!("bounded slot replaced original ready output");
            };
            assert_eq!(actual, index);
        }
        let snapshot = log.export();
        assert_eq!(snapshot["rows"].as_array().unwrap().len(), REQUESTS);
        assert_eq!(snapshot["all_18_offered"], true);
        assert_eq!(snapshot["observer_incomplete"], true);
        assert_eq!(snapshot["rows"][16]["request"], "account-resume");
        assert_eq!(snapshot["rows"][17]["request"], "cards-resume");
        for index in 0..16 {
            assert_eq!(
                snapshot["rows"][index]["request"],
                format!("batch-submit-{:02}", index + 1)
            );
        }
        let expected_keys = std::collections::BTreeSet::from([
            "slot",
            "request",
            "offered",
            "first_poll",
            "future_ready",
            "dropped",
            "polls",
        ]);
        for row in snapshot["rows"].as_array().unwrap() {
            let keys = row
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect::<std::collections::BTreeSet<_>>();
            assert_eq!(keys, expected_keys);
            let stamp_keys = row["offered"]
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect::<std::collections::BTreeSet<_>>();
            assert_eq!(
                stamp_keys,
                std::collections::BTreeSet::from(["elapsed_ns", "unix_ms"])
            );
        }
        assert!(
            !serde_json::to_string(&snapshot)
                .unwrap()
                .contains("private-output-sentinel")
        );
        assert_eq!(wakes.0.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn recorder_contention_and_poison_keep_original_ready_error_and_value() {
        let log = Arc::new(DriverSpans::new());
        let payload = Arc::new(OpaquePayload);
        let mut request =
            Box::pin(log.observe(0, std::future::ready(Err::<(), _>(payload.clone()))));
        let wakes = Arc::new(WakeCount::default());
        let waker = Waker::from(wakes.clone());
        let mut context = Context::from_waker(&waker);
        let held = log.rows.lock().unwrap();
        let Poll::Ready(Err(actual)) = request.as_mut().poll(&mut context) else {
            panic!("diagnostic lock contention replaced original error");
        };
        assert!(Arc::ptr_eq(&actual, &payload));
        drop(held);
        let snapshot = log.export();
        assert_eq!(snapshot["observer_incomplete"], true);
        assert!(snapshot["rows"][0]["first_poll"].is_null());
        assert!(snapshot["rows"][0]["future_ready"].is_null());
        let poisoned = Arc::new(DriverSpans::new());
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _held = poisoned.rows.lock().unwrap();
            panic!("controlled driver diagnostic mutex poison");
        }));
        assert!(panic.is_err());
        let mut request = Box::pin(poisoned.observe(1, std::future::ready(41)));
        assert!(matches!(
            request.as_mut().poll(&mut context),
            Poll::Ready(41)
        ));
        let snapshot = poisoned.export();
        assert_eq!(snapshot["observer_incomplete"], true);
        assert!(snapshot["rows"].is_null());
        assert_eq!(wakes.0.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn all_eighteen_futures_join_despite_online_error_and_submission_panic() {
        use super::super::campaign;
        use super::super::{CorpusProblem, StatusCode};
        use tokio::sync::oneshot;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        runtime.block_on(async {
            let log = Arc::new(DriverSpans::new());
            let finished = Arc::new(AtomicUsize::new(0));
            let mut submissions = Vec::new();
            let mut releases = Vec::new();
            for index in 0..16 {
                let (release, receiver) = oneshot::channel::<()>();
                releases.push(release);
                let finished = finished.clone();
                submissions.push(tokio::spawn(log.observe(index, async move {
                    receiver.await.unwrap();
                    finished.fetch_add(1, Ordering::SeqCst);
                    match index {
                        0 => Err(CorpusProblem::new("controlled.submit.failure", "fixed")),
                        1 => panic!("controlled driver submission panic"),
                        _ => Ok((StatusCode::CREATED, Vec::new())),
                    }
                })));
            }
            let (account_release, account_receiver) = oneshot::channel::<()>();
            let (cards_release, cards_receiver) = oneshot::channel::<()>();
            let exchanges = async {
                let (account, cards) = tokio::join!(
                    log.observe(16, async {
                        account_receiver.await.unwrap();
                        finished.fetch_add(1, Ordering::SeqCst);
                        Err::<serde_json::Value, _>(CorpusProblem::new(
                            "controlled.online.failure",
                            "fixed",
                        ))
                    }),
                    log.observe(17, async {
                        cards_receiver.await.unwrap();
                        finished.fetch_add(1, Ordering::SeqCst);
                        Ok::<_, CorpusProblem>(serde_json::json!({"cards": true}))
                    }),
                );
                Ok((account?, cards?))
            };
            let mut campaign = Box::pin(campaign::join_campaign(submissions, exchanges));
            let wakes = Arc::new(WakeCount::default());
            let waker = Waker::from(wakes);
            let mut context = Context::from_waker(&waker);
            assert!(matches!(
                campaign.as_mut().poll(&mut context),
                Poll::Pending
            ));
            assert_eq!(finished.load(Ordering::SeqCst), 0);
            assert_eq!(log.export()["all_18_offered"], true);
            account_release.send(()).unwrap();
            cards_release.send(()).unwrap();
            assert!(matches!(
                campaign.as_mut().poll(&mut context),
                Poll::Pending
            ));
            assert_eq!(finished.load(Ordering::SeqCst), 2);
            // Even a known online error cannot return before all16 offered
            // submission tasks have resolved, including their original panic.
            for release in releases {
                release.send(()).unwrap();
            }
            let (submitted, online) = campaign.await;
            assert_eq!(submitted.len(), 16);
            assert_eq!(finished.load(Ordering::SeqCst), 18);
            assert_eq!(
                submitted[0].as_ref().unwrap().as_ref().unwrap_err().code,
                "controlled.submit.failure"
            );
            assert!(submitted[1].as_ref().unwrap_err().is_panic());
            assert!(
                submitted[2..]
                    .iter()
                    .all(|row| row.as_ref().unwrap().is_ok())
            );
            let panic_payload = submitted
                .into_iter()
                .nth(1)
                .unwrap()
                .unwrap_err()
                .into_panic();
            assert_eq!(
                panic_payload.downcast_ref::<&str>(),
                Some(&"controlled driver submission panic")
            );
            assert_eq!(online.unwrap_err().code, "controlled.online.failure");
            let snapshot = log.export();
            assert_eq!(snapshot["observer_incomplete"], false);
            assert_eq!(snapshot["all_18_offered"], true);
            assert_eq!(snapshot["all_18_futures_ready"], false);
            assert!(snapshot["rows"][1]["future_ready"].is_null());
            assert!(snapshot["rows"][1]["dropped"].is_object());
            assert_eq!(
                snapshot["rows"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|row| row["future_ready"].is_object())
                    .count(),
                17
            );
        });
    }
}
