//! Opt-in measurement, not a timing assertion or capacity certification.
use super::*;
use mainframe_env_execution_api::{
    InvocationLimits, LifecycleEventKind, PrincipalId, RunUnitId, Selector,
};
use std::sync::{Arc, Barrier};
use std::time::Instant;

fn execution(key: &str) -> ExecutionRecord {
    let l = InvocationLimits::default();
    ExecutionRecord {
        execution_id: ExecutionId::new(key, l).unwrap(),
        run_unit_id: RunUnitId::new(format!("run-{key}"), l).unwrap(),
        selector: Selector::new("program:BENCH", l).unwrap(),
        artifact: ArtifactRef::new("sha256:bench", l).unwrap(),
        principal: PrincipalId::new("BENCH", l).unwrap(),
        state: ExecutionState::Admitted,
        attempt: 1,
        version: 1,
        owner_lease: None,
        lease_expiry_tick: None,
        terminal_tick: None,
    }
}
fn event(record: &ExecutionRecord, sequence: u64) -> LifecycleEvent {
    LifecycleEvent {
        execution_id: record.execution_id.clone(),
        run_unit_id: record.run_unit_id.clone(),
        sequence,
        attempt: 1,
        tick: sequence,
        kind: LifecycleEventKind::Admitted,
    }
}
fn notification(record: &ExecutionRecord, sequence: u64) -> OutboxRecord {
    OutboxRecord {
        notification_id: format!("{}-{sequence}", record.execution_id),
        execution_id: record.execution_id.clone(),
        sequence,
        topic: "benchmark".into(),
        payload: vec![1],
        attempt: 0,
        delivered: false,
        delivered_tick: None,
        version: 1,
    }
}
fn seeded(dimension: &str, count: usize) -> MemoryStore {
    let store = MemoryStore::new(StoreLimits::default());
    // Setup is outside every measured interval. Direct construction avoids
    // measuring repeated admissions while growing the unrelated state.
    let mut state = store.lock().unwrap();
    for i in 0..count {
        let key = format!("retained-{i}");
        match dimension {
            "executions" => {
                let r = execution(&key);
                state.executions.insert(r.execution_id.clone(), r);
            }
            "events" => {
                let r = execution(&key);
                state
                    .events
                    .insert(r.execution_id.clone(), vec![event(&r, 1)]);
            }
            "artifacts" => {
                let id =
                    ArtifactRef::new(format!("sha256:retained-{i}"), InvocationLimits::default())
                        .unwrap();
                state.artifacts.insert(
                    id.clone(),
                    ArtifactRecord {
                        artifact: id,
                        media_type: "application/octet-stream".into(),
                        payload_digest: [0; 32],
                        payload: vec![7; 16 * 1024],
                    },
                );
                state.blob_bytes += 16 * 1024;
            }
            "provider_payload" => {
                state.provider_state.insert(
                    ("benchmark".into(), key.clone()),
                    ProviderStateRecord {
                        namespace: "benchmark".into(),
                        key,
                        version: 1,
                        payload: vec![7; 16 * 1024],
                    },
                );
                state.blob_bytes += 16 * 1024;
            }
            _ => panic!("unknown dimension"),
        }
    }
    drop(state);
    store
}
fn stats(mut values: Vec<u64>) -> serde_json::Value {
    values.sort_unstable();
    // Nearest-rank percentiles keep high percentiles honest for the deliberately
    // small bounded sample: with 12 observations, p95 and p99 are the maximum.
    let at = |p: usize| values[(values.len() * p).div_ceil(100).saturating_sub(1)];
    serde_json::json!({"samples":values.len(),"min_ns":values[0],"p50_ns":at(50),"p95_ns":at(95),"p99_ns":at(99),"max_ns":values[values.len()-1]})
}
fn elapsed_ns(start: Instant) -> u64 {
    u64::try_from(start.elapsed().as_nanos()).unwrap()
}

#[test]
#[ignore = "bounded opt-in performance investigation; timing is not a correctness gate"]
fn memory_store_scaling() {
    const OPERATIONS: usize = 12;
    let mut results = Vec::new();
    for dimension in ["executions", "events", "artifacts", "provider_payload"] {
        for count in [0, 64, 512] {
            for workers in [1, 4] {
                for operation in ["admit", "commit", "rollback"] {
                    for repeat in 0..3 {
                        let store = Arc::new(seeded(dimension, count));
                        let prepared: Vec<Vec<ExecutionRecord>> = (0..workers)
                            .map(|worker| {
                                (0..OPERATIONS)
                                    .map(|i| execution(&format!("target-{worker}-{i}")))
                                    .collect()
                            })
                            .collect();
                        if operation != "admit" {
                            for records in &prepared {
                                for r in records {
                                    store
                                        .admit_execution(r.clone(), event(r, 1), notification(r, 1))
                                        .unwrap();
                                }
                            }
                        }
                        let barrier = Arc::new(Barrier::new(workers));
                        let phase = std::thread::scope(|scope| {
                            let handles: Vec<_> = (0..workers)
                                .map(|_| {
                                    let store = Arc::clone(&store);
                                    let barrier = Arc::clone(&barrier);
                                    scope.spawn(move || {
                                        barrier.wait();
                                        let mut waits = Vec::new();
                                        let mut clones = Vec::new();
                                        for _ in 0..OPERATIONS {
                                            let start = Instant::now();
                                            let lock = store.lock().unwrap();
                                            waits.push(elapsed_ns(start));
                                            let start = Instant::now();
                                            let copy = std::hint::black_box(lock.clone());
                                            clones.push(elapsed_ns(start));
                                            drop(lock);
                                            drop(copy);
                                        }
                                        (waits, clones)
                                    })
                                })
                                .collect();
                            handles
                                .into_iter()
                                .map(|h| h.join().unwrap())
                                .collect::<Vec<_>>()
                        });
                        // The isolated lock/clone probe is excluded from journal
                        // throughput and is not mislabeled as in-method instrumentation.
                        let barrier = Arc::new(Barrier::new(workers));
                        let started = Instant::now();
                        let latencies = std::thread::scope(|scope| {
                            let handles: Vec<_> = prepared
                                .into_iter()
                                .map(|records| {
                                    let store = Arc::clone(&store);
                                    let barrier = Arc::clone(&barrier);
                                    scope.spawn(move || {
                                        barrier.wait();
                                        let mut values = Vec::new();
                                        for r in records {
                                            let e =
                                                event(&r, if operation == "admit" { 1 } else { 2 });
                                            let mut n = notification(&r, e.sequence);
                                            if operation == "rollback" {
                                                n.topic.clear();
                                            }
                                            let start = Instant::now();
                                            let result = if operation == "admit" {
                                                store.admit_execution(r.clone(), e, n)
                                            } else {
                                                store
                                                    .commit_execution_step(
                                                        &r.execution_id,
                                                        1,
                                                        None,
                                                        e,
                                                        None,
                                                        None,
                                                        None,
                                                        n,
                                                    )
                                                    .map(|_| ())
                                            };
                                            values.push(elapsed_ns(start));
                                            if operation == "rollback" {
                                                assert_eq!(
                                                    result,
                                                    Err(StoreError::InvalidTransition)
                                                );
                                                assert_eq!(
                                                    store.get_execution(&r.execution_id).unwrap(),
                                                    Some(r.clone())
                                                );
                                                assert_eq!(
                                                    store
                                                        .events(&r.execution_id, 1, 10)
                                                        .unwrap()
                                                        .len(),
                                                    1
                                                );
                                                assert!(
                                                    !store.lock().unwrap().outbox.contains_key(
                                                        &format!("{}-2", r.execution_id)
                                                    )
                                                );
                                            } else {
                                                result.unwrap();
                                            }
                                        }
                                        values
                                    })
                                })
                                .collect();
                            handles
                                .into_iter()
                                .flat_map(|h| h.join().unwrap())
                                .collect::<Vec<_>>()
                        });
                        let elapsed = started.elapsed().as_secs_f64();
                        let waits = phase.iter().flat_map(|(v, _)| v.iter().copied()).collect();
                        let clones = phase.iter().flat_map(|(_, v)| v.iter().copied()).collect();
                        let lock = store.lock().unwrap();
                        let retained_bytes = lock
                            .artifacts
                            .values()
                            .map(|v| v.payload.len())
                            .sum::<usize>()
                            + lock
                                .provider_state
                                .values()
                                .map(|v| v.payload.len())
                                .sum::<usize>();
                        results.push(serde_json::json!({"dimension":dimension,"retained_items":count,"retained_payload_bytes":retained_bytes,"workers":workers,"operation":operation,"repeat":repeat,"latency":stats(latencies),"isolated_clone_probe":stats(clones),"isolated_lock_wait_probe":stats(waits),"throughput_ops_s":(workers*OPERATIONS) as f64/elapsed,"throughput_includes_thread_start_and_postcondition_checks":true}));
                    }
                }
            }
        }
    }
    assert_eq!(results.len(), 216);
    let git = |args: &[&str]| {
        String::from_utf8(
            std::process::Command::new("git")
                .args(args)
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap()
        .trim()
        .to_owned()
    };
    let rustc = String::from_utf8(
        std::process::Command::new("rustc")
            .arg("-Vv")
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    let report = serde_json::json!({"schema":"mainframe-env.memory-scaling@1","candidate":git(&["rev-parse","HEAD"]),"tree":git(&["rev-parse","HEAD^{tree}"]),"working_tree_dirty":!git(&["status","--porcelain"]).is_empty(),"profile":if cfg!(debug_assertions){"debug"}else{"release"},"toolchain":rustc,"os":std::env::consts::OS,"architecture":std::env::consts::ARCH,"available_parallelism":std::thread::available_parallelism().map(|v|v.get()).unwrap_or(1),"operations_per_worker":OPERATIONS,"percentile_method":"nearest-rank","cpuinfo":std::fs::read_to_string("/proc/cpuinfo").ok().and_then(|s|s.lines().find(|line|line.starts_with("model name")).map(str::to_owned)),"process_status_after_campaign":std::fs::read_to_string("/proc/self/status").ok(),"allocation_count":null,"allocation_count_note":"not instrumented; process VmHWM is cumulative, not a per-case allocation or peak measurement","results":results});
    let output = std::env::var("MAINFRAME_ENV_MEMORY_BENCH_OUTPUT")
        .unwrap_or_else(|_| "/tmp/mainframe-memory-scaling.json".into());
    if let Some(parent) = std::path::Path::new(&output).parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(&output, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    for row in report["results"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["repeat"] == 0 && r["workers"] == 1 && r["operation"] == "commit")
    {
        println!("BENCH_SAMPLE {row}");
    }
    println!(
        "BENCH_REPORT {output}; 216 cases, 3 repeats, correctness postconditions passed; allocation count not measured"
    );
}

#[test]
fn latency_stats_use_nearest_rank_percentiles() {
    let summary = stats((1..=12).collect());
    assert_eq!(summary["p50_ns"], 6);
    assert_eq!(summary["p95_ns"], 12);
    assert_eq!(summary["p99_ns"], 12);
}
