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
The report records hardware/toolchain, workload dimensions, nearest-rank
p50/p95/p99/range, throughput, process status, and whether the measured working
tree differed from the recorded candidate. VmHWM is process-cumulative, **not
per-case peak**; allocation count is explicitly unmeasured rather than invented.
A dedicated allocator/RSS-per-process campaign is needed for allocation
attribution.

## Measured results

Candidate `4f10afe088c2bbed751c06d3417f1838f48c0c15`, tree
`070a22db372d81147d834b2e173a3d1ca6c7f466`, clean working tree, release profile,
Rust 1.98.0, aarch64-apple-darwin, 14 available parallelism, 216 cases,
nearest-rank percentiles, median across three repetitions. This is one developer
host, not deployment hardware, and grants no capacity certification.

Commit p50 latency (µs) by unrelated retained state:

| Dimension | w=1 n=0 | w=1 n=64 | w=1 n=512 | w=4 n=0 | w=4 n=64 | w=4 n=512 |
|---|---|---|---|---|---|---|
| events | 4.6 | 9.8 | 44.1 | 20.3 | 27.3 | 60.4 |
| executions | 16.8 | 20.8 | 55.3 | 52.8 | 37.2 | 71.6 |
| artifacts | 4.7 | 22.2 | 166.3 | 20.5 | 40.6 | 178.7 |
| provider_payload | 4.5 | 24.0 | 175.2 | 20.6 | 42.2 | 185.2 |

**Cost does scale with unrelated retained state.** The answer to the question
this investigation posed is yes, and the effect is large: a single-worker commit
touching one record costs 4.5 µs against an empty store and 175.2 µs when 512
unrelated provider payloads totalling 8 MiB are retained — a 39x increase for
identical work. Single-worker commit throughput falls from 157,206 to 5,582
ops/s across that same range.

Payload bytes, not record count, dominate. At n=512 the payload-bearing
dimensions reach 166-185 µs while events, which retain no payload, reach 44 µs.
The isolated clone probe accounts for 46-85% of measured latency and rises with
both retained state and worker count, so the growth is attributable to the
whole-state clone rather than to unrelated work.

Rollback costs essentially the same as commit (160.3 µs against 175.2 µs at
provider_payload n=512): an aborted transaction pays the full clone. Admission
is slightly cheaper but scales identically.

Contention compounds the clone rather than replacing it. At four workers and
n=512 the mutex wait probe reaches 425.5 µs p50 — longer than the 185.2 µs
operation itself — while at n=0 it is 53.4 µs. Because clone hold time sets
queueing delay, concurrency amplifies the same underlying cost.

## Supported envelope

On hardware comparable to the measured host, retained state up to roughly 64
records per dimension with payloads at or below 1 MiB keeps commit p50 under
about 45 µs and is a reasonable operating envelope. Beyond that the trend is
clearly superlinear in retained payload bytes, so deployments retaining hundreds
of payload-bearing records should either bound retention or accept
hundreds-of-microseconds commits. This envelope is derived from one host and one
synthetic workload; re-measure before relying on it.

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
