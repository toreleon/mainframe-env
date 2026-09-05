# Memory-store transaction scaling investigation (#56)

This PR adds a reproducible, opt-in experiment. It does not substitute an unsafe
transaction rewrite for measurement, nor assert a capacity regression before
results exist. The complete JSON receipt is an external CI artifact pinned to
the tested commit/tree/toolchain; it is not generated into the candidate tree.

## Reproduce

```sh
MAINFRAME_ENV_MEMORY_BENCH_OUTPUT=/tmp/memory-scaling.json cargo test --release --locked -p mainframe-env-store memory::hardening_bench::memory_store_scaling -- --ignored --exact --nocapture
```

The 216 cases independently vary unrelated retained execution records, event
records, artifacts, and provider payloads (0/64/512 entries; payloads 16 KiB),
with 1/4 concurrent workers and three repetitions. Setup is outside timing.
Actual admission, step commit, and late rollback run through JournalStore;
rollback asserts unchanged execution/event/outbox state.

Latency distributions measure the journal call itself. Throughput includes
thread startup and postcondition checks and must not be treated as a service
capacity ceiling. The separate mutex-wait and clone-hold probes clone the same
State under its actual mutex, but are not instrumentation of the journal method.
The report records hardware/toolchain, workload dimensions, p50/p95/p99/range,
throughput, and process status. VmHWM is process-cumulative, **not per-case peak**;
allocation count is explicitly unmeasured rather than invented. A dedicated
allocator/RSS-per-process campaign is needed for allocation attribution.

## Disposition and optimization gate

Keep atomic clone-and-replace semantics until an optimization proves the same
rollback/isolation contracts. Use these results to judge the intended retained
state and concurrency envelope, not to certify arbitrary workloads. A scaling
trend warrants a separately reviewed change (for example structurally shared
immutable payloads or a targeted transactional overlay); it does not justify
weakening atomicity. Run the common backend Move contract from #51 with any such
change. No cross-backend equivalence or licensed IBM credit is implied.

The implementation remains bounded by StoreLimits. Those limits are memory and
correctness bounds, not throughput/latency guarantees. Re-run on deployment
hardware and the actual retention policy before making capacity commitments.
