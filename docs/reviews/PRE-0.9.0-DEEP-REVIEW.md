# Pre-0.9.0 deep review

Status: **Complete review; broad 0.9.0 implementation is blocked**

- Review date: 2026-09-08
- Reviewed commit: `1bd294c170cae35c470f0d183635f758f80e2c98`
- Reviewed tree: `dd0c3b38c9a6d44eda73f29a520bd7039b7fd893`
- Published baseline: `mainframe-env-v0.8.2`
- GitHub tracking: [#101](https://github.com/toreleon/mainframe-env/issues/101)
  through [#128](https://github.com/toreleon/mainframe-env/issues/128), under
  roadmap epic [#15](https://github.com/toreleon/mainframe-env/issues/15)
- Decision owner: repository owner

## Executive decision

The repository has a strong deterministic core, unusually thorough semantic
tests, and useful fail-closed conformance machinery. The ordinary workspace
quality gates are green. That is not sufficient to begin broad 0.9.0 work.

This review found no P0 issue that proves unconditional data loss on the normal
happy path. It found multiple P1 issues on supported security, durability,
queueing, CICS, release, and licensing paths. Several of those issues can turn
an already-committed mutation into a retryable-looking failure, discard audit
evidence, strand durable work, or produce a release receipt for a binary that
cannot start.

**Decision: no-go.** Close every P1 item below, add the named regressions, and
rerun the entry gate before expanding the CICS implementation beyond the
accepted pilot.

Priority meanings in this report are:

- **P0:** active incident or unconditional corruption/security failure; stop all
  work.
- **P1:** correctness, security, durability, legal, or release blocker; must
  close before broad 0.9.0 implementation.
- **P2:** significant architecture, assurance, scalability, or operability risk;
  schedule before the affected 0.9 work package merges.
- **P3:** maintainability or documentation debt with a bounded near-term risk.

## What was reviewed

- all 26 Cargo workspace packages and their dependency graph;
- compiler stages, artifact identity, interpreter coordination, host effects,
  stores, providers, batch/JES, server composition, and z/OSMF routing;
- 841 Rust tests, 316 Python tests, ignored/environment-dependent suites, and
  conformance/evidence tooling;
- Jenkins selection, PostgreSQL parity, MSRV, dependency policy, release
  generation, archive reproducibility, SBOM, and provenance;
- root documentation, architecture decisions, release truth, runbooks,
  implementation prompts, cross-links, and command examples; and
- the proposed 0.9.0 dossier plus the accepted bounded CICS file/UOW pilot.

The review combined source tracing, dependency and size inventories, targeted
failure-path analysis, normal gates, manually discovered tooling tests, and
focused release/licensing probes. Historical receipts were treated as records,
not as proof that current `main` still behaves the same way.

## Finding index

| ID | Priority | Area | Disposition |
|---|---|---|---|
| [R-01](https://github.com/toreleon/mainframe-env/issues/101) | P1 | Effect journaling | Open; entry blocker |
| [R-02](https://github.com/toreleon/mainframe-env/issues/102) | P1 | Security audit | Open; entry blocker |
| [R-03](https://github.com/toreleon/mainframe-env/issues/103) | P1 | Online CICS durability | Open; entry blocker |
| [R-04](https://github.com/toreleon/mainframe-env/issues/104) | P1 | JES work lifecycle | Open; entry blocker |
| [R-05](https://github.com/toreleon/mainframe-env/issues/105) | P1 | HTTP timeout/backpressure | Open; entry blocker |
| [R-06](https://github.com/toreleon/mainframe-env/issues/106) | P1 | Store atomic invariants | Open; entry blocker |
| [R-07](https://github.com/toreleon/mainframe-env/issues/107) | P1 | Canonical replay identity | Open; entry blocker |
| [R-08](https://github.com/toreleon/mainframe-env/issues/108) | P1 | Authentication/session security | Open; entry blocker |
| [R-09](https://github.com/toreleon/mainframe-env/issues/109) | P1 | Configuration/readiness | Open; entry blocker |
| [R-10](https://github.com/toreleon/mainframe-env/issues/110) | P1 | Enterprise authorization | Open; entry blocker |
| [R-11](https://github.com/toreleon/mainframe-env/issues/111) | P1 | Retention/capacity | Open; entry blocker |
| [R-12](https://github.com/toreleon/mainframe-env/issues/112) | P1 | macOS release artifact | Open; release blocker |
| [R-13](https://github.com/toreleon/mainframe-env/issues/113) | P1 | License compliance | Open; release blocker |
| [R-14](https://github.com/toreleon/mainframe-env/issues/114) | P1 | Archive reproducibility | Open; release blocker |
| [R-15](https://github.com/toreleon/mainframe-env/issues/115) | P1 | Release identity/truth | Documentation partially corrected; automation/version open |
| [R-16](https://github.com/toreleon/mainframe-env/issues/116) | P2 | Compiler type-state | Open |
| [R-17](https://github.com/toreleon/mainframe-env/issues/117) | P2 | Provider persistence scale | Open |
| [R-18](https://github.com/toreleon/mainframe-env/issues/118) | P2 | Work lease semantics | Open |
| [R-19](https://github.com/toreleon/mainframe-env/issues/119) | P2 | PostgreSQL/artifact profile | Open |
| [R-20](https://github.com/toreleon/mainframe-env/issues/120) | P2 | Dataset list authorization | Open |
| [R-21](https://github.com/toreleon/mainframe-env/issues/121) | P2 | CI test selection | Open |
| [R-22](https://github.com/toreleon/mainframe-env/issues/122) | P2 | Schema/parser parity | Open |
| [R-23](https://github.com/toreleon/mainframe-env/issues/123) | P2 | SBOM/provenance | Open |
| [R-24](https://github.com/toreleon/mainframe-env/issues/124) | P2 | Fuzz/model/coverage | Documentation corrected; harnesses open |
| [R-25](https://github.com/toreleon/mainframe-env/issues/125) | P2 | MSRV/supply chain | Open |
| [R-26](https://github.com/toreleon/mainframe-env/issues/126) | P3 | Module boundaries | Open |
| [R-27](https://github.com/toreleon/mainframe-env/issues/127) | P3 | Public API documentation | Open |
| [R-28](https://github.com/toreleon/mainframe-env/issues/128) | P3 | Documentation system | Immediate errors corrected; automated governance open |

## P1 findings — required before broad 0.9.0 work

### R-01 — A committed mutation can become an unrecoverable orphan intent

Evidence:

- `crates/kernel/mainframe-env-interpreter/src/coordinator.rs:187-205` persists
  an effect intent before dispatch.
- The provider is invoked at `coordinator.rs:220-227`.
- If the provider succeeds but result journaling fails,
  `coordinator.rs:247-264` returns `InfrastructureFailure`. It returns
  `UnknownOutcome` only when the provider itself already reported uncertainty.
- `crates/stores/mainframe-env-store/src/durable.rs:564-581` and
  `memory.rs:596-607` list only records already marked `UnknownOutcome`; they do
  not surface orphan `Intent` records for reconciliation.
- `crates/apps/mainframe-env-server/src/cobol/hardening.rs:464-505` explicitly
  observes a remaining intent when event capacity is exhausted, but the test
  starts from a provider-reported unknown outcome and does not cover a known
  success followed by journal failure.

Impact: the external mutation may be committed while the caller sees an
ordinary infrastructure failure and retries. The retained intent has no
timestamp, lease, or recovery enumeration path, so it can remain invisible
indefinitely.

Required remediation:

1. Classify every persistence failure after dispatch of a mutating effect as an
   unknown outcome.
2. Give intents an owner/attempt and durable time or logical-epoch boundary.
3. Add an API and recovery worker for stale intents, not only already-labeled
   unknown records.
4. Add memory, SQLite, and PostgreSQL tests for `provider success -> result
   journal failure -> restart -> reconcile`, proving no duplicate effect.

### R-02 — The host audit result is produced and then discarded

Evidence:

- `crates/contracts/mainframe-env-host-api/src/service.rs:108-139` constructs an
  `AuditedEffectResult` for every host invocation.
- `crates/kernel/mainframe-env-interpreter/src/coordinator.rs:220-227` and the
  production callers consume only `.effect`; no production path persists the
  attached `.audit`.
- `crates/providers/mainframe-env-racf/src/authority.rs:565-586` evaluates the
  ordinary authorization path without recording an audit event.
- The generated host audit uses the constant resource value
  `redacted-at-contract-boundary`, so even a future sink could not distinguish
  resources.

Impact: sensitive dataset, JES, CICS, Db2, IMS, MQ, and SAF decisions have no
complete durable audit trail. This contradicts the accepted security
architecture and prevents reliable incident investigation.

Required remediation: introduce a mandatory typed `AuditSink`; derive a
canonical resource digest and typed decision; commit audit, effect result, and
lifecycle event in one transaction where they share authority; prove deny,
success, cancellation, provider failure, restart, and saturation behavior.

### R-03 — Online CICS bypasses durable execution and collapses uncertainty

Evidence:

- `crates/apps/mainframe-env-server/src/product.rs:1501-1508` constructs
  `ExecutionCoordinator::with_host` even though the product already owns a
  `PlatformStore`.
- `product.rs:1581-1586` maps every `ExecutionOutcome::ProviderFailure`,
  including a problem carrying `UnknownOutcome`, to a generic CICS condition.

Impact: an online CICS mutation can be in doubt without the standard
execution/effect journal, while the gateway exposes a normal condition that is
likely to be retried. This is directly in the scope that 0.9.0 intends to
expand.

Required remediation: use a resume-aware durable coordinator for online
sessions, retain the unknown-outcome category end to end, and add crash-point
tests around file, queue, program-link, and syncpoint mutations.

### R-04 — JES work claiming is request-coupled and uses a constant clock

Evidence:

- `crates/apps/mainframe-env-server/src/product.rs:2084-2107` enqueues a work
  item during HTTP job submission.
- `product.rs:2108-2135` immediately calls
  `claim("jes-worker-0", 1, 10)` and heartbeats at tick `2`.
- If the global claim returns another request's item,
  `product.rs:2114-2120` releases that item and fails the current request.
- `product.rs:2146-2147` drains queued jobs inline. No product worker loop owns
  the durable work queue.

Impact: concurrent users can race, a crash can leave a lease whose expiry is
never reached because all later claims still use tick 1, and a long batch job
occupies an HTTP concurrency slot.

Required remediation: run a bounded background worker pool using one real
monotonic/logical clock; process any valid claimed item; use fencing tokens;
make HTTP submission return the accepted job identity without running the job
inline; add multi-user, crash, lease-expiry, cancellation, and fairness tests.

### R-05 — HTTP timeouts cannot preempt synchronous backend work

Evidence:

- `crates/gateways/mainframe-env-zosmf/src/gateway.rs:211-216` defines a
  synchronous backend trait.
- Async handlers call it directly at `gateway.rs:796-804`, without yielding.
- `TimeoutLayer` is installed at `gateway.rs:250-255`, but it cannot regain
  control until the synchronous call returns.
- Product invocations use `u64::MAX` as their deadline at
  `crates/apps/mainframe-env-server/src/product.rs:2891-2892`.

Impact: `timeout_millis` is not an effective bound for compiler, SQL, provider,
or batch work. A blocked call can retain a concurrency permit and delay
shutdown indefinitely.

Required remediation: make the backend async or dispatch synchronous work to a
separate bounded lane; propagate request deadlines and cancellation into the
domain invocation; test a backend that blocks beyond the configured timeout.

### R-06 — Atomic journal writes bypass the direct store invariants

Evidence:

- Direct durable `record_result` validates execution, run unit, sequence,
  digest format, and request digest against the intent at
  `crates/stores/mainframe-env-store/src/durable.rs:529-555`.
- `commit_execution_step` at `durable.rs:823-841` checks only execution ID and
  replaces version 1 by key/CAS, allowing a mismatched terminal record.
- The checkpoint branch at `durable.rs:843-861` omits schema, generation,
  classification, non-empty-payload, and digest checks enforced by the direct
  path at `durable.rs:361-385`.
- Memory, SQLite, and PostgreSQL paths apply different validation. For example,
  `memory.rs:422-445` does not verify checkpoint digests, and
  `memory.rs:492-507` does not verify artifact content identity.

Impact: the transaction can advance execution state while committing a final
effect for a different request or a checkpoint that cannot later be restored.
The development backend can accept records rejected by production backends.

Required remediation: centralize record validators and call the same functions
from direct and journal APIs; run one backend contract suite against memory,
SQLite, and PostgreSQL, including hostile identity/digest/schema mutations.

### R-07 — Durable replay identities still depend on Rust `Debug`

Evidence:

- MQ, IMS, and Db2 hash `format!("{request:?}")` at
  `crates/providers/mainframe-env-mq/src/service.rs:583-585`,
  `mainframe-env-ims/src/service.rs:1103-1105`, and
  `mainframe-env-db2/src/service.rs:2175-2177`.
- RACF hashes several enum fields through `digest_saf_debug` at
  `crates/providers/mainframe-env-racf/src/saf.rs:2022-2164`.
- Lifecycle outbox payloads use `Debug` at
  `crates/kernel/mainframe-env-interpreter/src/coordinator.rs:554-564`.
- `tools/check_effect_encoding.py:8-22` scans only the coordinator, host
  service, canonical encoder, and durable field names. It misses provider replay
  formats, so `architecture-fast` still passes.

Impact: a harmless enum rename, field addition, or derived-`Debug` change can
turn the same logical request into an idempotency conflict after upgrade.

Required remediation: use explicit versioned canonical encoders for every
persisted replay/outbox identity; migrate existing rows; expand the guard to the
entire production tree and reject persisted diagnostic formatting.

### R-08 — Authentication retains secrets and bearer tokens too broadly

Evidence:

- `crates/apps/mainframe-env-server/src/product.rs:2463-2472` inserts a password
  into `MemorySecretResolver` before validating the principal and secret
  reference. Either `?` can return before cleanup.
- The resolver is an unbounded `BTreeMap` at
  `crates/providers/mainframe-env-racf/src/authority.rs:52-68`.
- The Basic-auth decode buffer is not zeroized at
  `crates/gateways/mainframe-env-zosmf/src/gateway.rs:843-856`.
- `AuthSession` has only user/version (`product.rs:161-164`); raw bearer tokens
  are durable provider-state keys (`product.rs:2496-2523`) and are recovered
  indefinitely (`product.rs:393-406`). There is no TTL, idle expiry, per-user
  cap, rotation, or automatic cleanup.
- `crates/providers/mainframe-env-racf/src/authority.rs:646-654` derives an
  Argon2 salt deterministically from the user name. Direct add/bootstrap paths
  also bypass the minimum/history checks that exist only in the RACF command
  processor.
- Password hashing uses the deterministic public salt `mainframe-env:{user}` at
  `crates/providers/mainframe-env-racf/src/authority.rs:646-654`. Direct
  add/bootstrap paths do not apply the minimum/history policy enforced only by
  the RACF command processor.

Impact: unauthenticated malformed-user requests can retain password material
and grow memory; a database disclosure reveals live bearer credentials; valid
users can exhaust session capacity by repeated login.

Required remediation: validate before insertion, use an RAII zeroizing secret
scope, bound the resolver, hash stored bearer tokens, add absolute and idle TTL,
per-user limits, revocation epochs, cleanup, and principal-state revalidation.
Use a CSPRNG salt per credential and one credential-policy function for every
create/change/bootstrap route.
Use a CSPRNG salt per credential and one policy function for every credential
creation/change path.

### R-09 — Configuration references and readiness do not describe reality

Evidence:

- `crates/apps/mainframe-env-server/src/config.rs:19-30,94-95,135-146`
  validates PostgreSQL/TLS reference fields.
- The binary ignores their values and reads fixed environment names at
  `crates/apps/mainframe-env-server/src/main.rs:41-47,71-80`.
- The binary accepts only a positional file and passes
  `ConfigOverrides::default()` (`main.rs:23-33`); documented CLI overrides did
  not exist.
- `ProductServer::ready` at `product.rs:1651-1663` treats `Ok(None)` for the JES
  metadata probe as ready and checks neither bootstrap identity nor worker
  health.

Impact: a valid documented config can fail at startup, secret rotation through
the stated reference mechanism does not work, and a fresh unusable store can
advertise readiness.

Required remediation: resolve actual `SecretRef` values through one bounded
resolver, add a real CLI, provide a secure first-admin bootstrap, and split
liveness from readiness with migration, writable-store, authentication,
artifact, worker, and capacity checks.

### R-10 — Enterprise write capability is inferred from raw JCL without
resource authorization

Evidence:

- `crates/apps/mainframe-env-server/src/product.rs:3863-3889` searches raw JCL
  text and grants broad Db2/IMS/MQ read/write capabilities.
- The host boundary checks possession of the broad grant, not a table, PSB,
  queue, or operation policy.
- The batch Db2 route at
  `crates/apps/mainframe-env-batch/src/service.rs:2748-2794` calls the provider
  without a corresponding resource SAF decision; IMS has the same architectural
  shape.

Impact: comments or data can overgrant capability, and a principal allowed to
submit a job may reach enterprise mutations without a resource-specific policy
decision.

Required remediation: derive required capabilities from the parsed and
verified plan, then separately authorize each typed sensitive resource and
intent through SAF before dispatch. Add deny-before-mutation tests for every
enterprise family.

### R-11 — Append-only state has no retention lifecycle

Evidence:

- `EventStore` and `OutboxStore` in
  `crates/contracts/mainframe-env-store-api/src/traits.rs:19-27,57-65` expose no
  archive/prune operation.
- `crates/apps/mainframe-env-server/src/product.rs:1732-1742` marks outbox rows
  delivered but never removes them.
- Memory event/outbox limits are 262,144; the standalone SQL stores also use a
  global 262,144 row limit.
- Db2, IMS, and MQ replay maps cap at 65,536 entries and have no GC lifecycle.

Impact: a healthy system eventually reaches a permanent capacity failure that
survives restart. Adding 263 CICS commands increases the rate and number of
retained identities.

Required remediation: define retention watermarks and idempotency lifetimes,
transactional archive/prune, operator controls, and saturation forecasting.
Prove that pruning cannot break replay, audit, or checkpoint recovery.

### R-12 — The macOS release path can certify an unlaunchable binary

Evidence:

- `xtask/src/main.rs:12218-12220` adds `-Wl,-no_uuid` for the retained macOS
  target and records it in build inputs at `xtask/src/main.rs:12924`.
- `tools/package_offline_bundle.sh:62-67` already documents that macOS 26 dyld
  rejects such binaries and deliberately omits the flag.
- A build with the exact xtask target flags succeeded; `otool` showed no
  `LC_UUID`, and executing `mainframe-env --version` aborted with exit 134 and
  `dyld: missing LC_UUID load command`.
- The release smoke at `xtask/src/main.rs:11619-11634` builds through a different
  path, so it cannot catch this artifact defect.

Impact: receipt generation and byte comparison can pass for a binary that
cannot start.

Required remediation: remove `-no_uuid`, consolidate one release build path,
and run `--version`, `--help`, plus server startup/readiness against the exact
target binaries before any receipt is written.

### R-13 — Licensing policy and shipped notices are not release-ready

Evidence:

- The workspace declares Apache-2.0 in `Cargo.toml:32-37`, but no root
  `LICENSE`, `NOTICE`, or `COPYING` file is tracked.
- `cargo deny check` fails because `decnumber-sys 0.1.6` uses the `ICU` license,
  which is absent from `deny.toml:8-19`.
- Jenkins does not run `cargo deny` in its current PR, full, or release stages.
- `xtask/src/main.rs:12970-12982` generates a list of license expressions, not
  full license/notice texts; the runtime bundle copies those receipts.

Impact: the declared dependency policy is red and source/binary distribution
does not carry the complete project and third-party license material required
by its own release policy.

Required remediation: obtain owner/legal approval for the ICU dependency,
track the exact Apache-2.0 license and any required NOTICE, generate full
third-party notices from the target production closure, and make
`cargo deny check` a blocking PR/full/release gate.

### R-14 — macOS archives are not reproducible and publication can overwrite
tag assets

Evidence:

- The BSD tar branches in `tools/package_offline_cargo_bundle.sh:69-75` and
  `tools/package_offline_bundle.sh:122-131` do not normalize both order and
  mtimes.
- A focused BSD-tar probe produced different SHA-256 values for identical
  content with different mtimes.
- Nested archives in `tools/offline_dev_assets/assemble.sh:99-108` are not
  normalized even when the outer archive is.
- `Jenkinsfile:388-399` publishes with `gh release upload --clobber`.

Impact: the same tag and source can publish different bytes, and a retry can
replace the archive plus checksum that consumers previously trusted.

Required remediation: create every archive in a digest-pinned GNU-tar build
environment, normalize recursively, reproduce twice from clean directories,
and refuse remote overwrite unless the existing digest is identical.

### R-15 — Release identity and public project truth are split

Evidence at review start:

- `VERSION` and Cargo identify the workspace as `0.8.2`, while `HEAD` is 95
  commits beyond the `mainframe-env-v0.8.2` tag.
- `CHANGELOG.md` had an empty Unreleased section.
- The root README and coverage index still called 0.8.1 latest.
- `docs/delivery/hardening/46-final-acceptance.md` explicitly says the 0.8.2 tag
  predates later accepted hardening.

Impact: two materially different source trees present the same product version,
and readers cannot tell released claims from development claims.

Required remediation: maintain separate released and development identities;
populate Unreleased on every user-visible merge; make the version gate validate
the public README, coverage index, project mapping, and tag distance; choose a
new development version before producing distributable artifacts.

Documentation truth and navigation were corrected during this review, but the
versioning automation and development version still require implementation.

## P2 findings — required by the affected work package

### R-16 — Compiler type-state and artifact identity are not sealed

`ParsedProgram::validated` and `SemanticProgram::validated` are public and let a
caller supply completeness and semantic identity
(`compiler-api/src/stage.rs:17-70`). `LegalizedMir::legalize` accepts an
unrelated `SourceId + Module` instead of consuming `VerifiedHir`
(`stage.rs:147-157`). `PublishedArtifact::publish` accepts arbitrary payload
bytes, while its `ArtifactId` omits the payload digest
(`artifact.rs:81-112`); a test deliberately proves different payloads share an
ID. This contradicts the documented non-fabricable pipeline and makes the
`sha256:`-prefixed execution identity ambiguous.

Seal construction behind compiler-owned factories, make each executable stage
consume the previous verified stage, and distinguish semantic identity from a
content digest everywhere it is serialized or prefixed as SHA-256.

### R-17 — Db2, IMS, and MQ rewrite one global state blob per mutation

Each provider locks a global mutex, clones the complete state, serializes it as
JSON, and overwrites one provider-state record. The state contains all tables
and rows, hierarchies and sessions, or queues and messages. This is O(total
state) CPU, allocation, and write amplification per record; it serializes every
run unit and temporarily multiplies memory. The blobs also lack an explicit
schema-version envelope.

Move to versioned per-object rows, scoped transactions/UoWs, separate indexes
and cursors, and explicit migrations before 0.9 adds cross-resource pressure.

### R-18 — Work lease semantics permit expired work and stale completion

Memory and durable `claim` paths check deadlines only when reclaiming an
already-claimed lease, not when selecting queued work. Terminal lease methods
do not receive a clock, and `valid_lease` checks only state plus lease ID.
Expired queued work can run, and an expired owner can still complete until a
competitor happens to reclaim it. Add deadline predicates and DB-enforced
fencing epochs to every terminal transition.

### R-19 — PostgreSQL quotas and local artifacts do not match the profile

PostgreSQL capacity uses `COUNT(*)` followed by insert without a serializable
quota reservation (`postgres.rs:111-154`), so concurrent writers can exceed the
declared bound. The PostgreSQL product profile still uses node-local
`LocalArtifactStore`; its check-then-rename publication is not no-replace atomic
and does not fsync the directory. Use a transactional quota row/advisory lock
and a shared immutable object-store adapter for a durable profile.

### R-20 — Dataset listing can disclose names outside discrete profiles

The server authorizes the requested wildcard expression and returns matching
dataset names; it authorizes individual datasets only when attributes are
requested. Either define an explicit catalog-list permission or filter every
returned resource through SAF before emitting its name.

### R-21 — CI does not execute all environment and tooling tests it claims

- Normal workspace tests pass 835 tests and skip 6.
- PostgreSQL parity runs only provider move and canonical-effect tests; it omits
  the ignored migration/durable contract and CardDemo PostgreSQL restart suite.
- The repository has 316 Python tests (one skipped); current Jenkins directly
  invokes only a small subset, while `spec` discovers only the 0.9 extractor
  directory.
- Documentation-only changes intentionally select zero checks.

Create a discovered `python-tooling-tests` gate, add both omitted PostgreSQL
suites with isolated database reset, and add Markdown link/anchor, command,
metadata, and version-truth validation for prose-only changes.

### R-22 — A 0.9 schema can be valid to Rust while invalid to JSON Schema

The CICS oracle capture schema requires `origin.signature` and enumerates four
origin kinds, while the Rust parser uses `Option<String>` plus an unrestricted
kind string and can return `AdapterContractValid` for schema-invalid local
origins. The generic schema gate stops its enumerated discovery before 0.9.
Auto-discover all versioned schemas and add parser/schema parity tests.

### R-23 — SBOM and provenance overstate their standards meaning

The SBOM generator starts from all Cargo metadata and removes only two package
names, so the 0.8.1 SBOM contains 62 components outside the server/CLI normal
closure and no useful dependency graph. The claimed `SLSA-v1` provenance uses
non-URI internal build and builder identifiers, a repeatable invocation ID, and
no signed DSSE/control-plane identity. Treat both files as internal inventory
until target-specific closure generation, official schema validation, trusted
builder identity, signing, and verification exist.

### R-24 — Verification strategy describes gates that do not exist

At review start, `docs/delivery/VERIFICATION-STRATEGY.md` described persistent
fuzz targets, Kani, Loom, TLA+/TLC, and a fuzz smoke milestone as current. The
tree has no fuzz workspace, Kani harness, Loom dependency, or TLA model, and CI
has no code coverage visibility. This review marks those sections planned; the
implementation gap remains. Add a bounded parser and decoder fuzz smoke,
periodic fuzzing, concurrency/model checks for the durable state machines, and a
non-gating coverage baseline.

### R-25 — MSRV and supply-chain enforcement are narrower than their claims

The workspace declares Rust 1.95, but Jenkins tests only eight foundation and
contract crates. The full workspace happens to pass 1.95 at this review commit;
the gate should cover that declared scope or the claim should be narrowed.
Offline-bundle container/package inputs and Jenkins plugins also need immutable
versions/digests and a reviewed update process.

## P3 maintainability and documentation findings

### R-26 — Module size defeats the repository's own review budget

The repository contains 192,711 Rust lines. Major non-generated files include
`carddemo.rs` (14,505), dataset `service.rs` (14,254), interpreter `machine.rs`
(13,523), batch `service.rs` (11,938), CICS `service.rs` (6,156), and server
`product.rs` (5,209). Many include large private test modules, but production
sections alone still exceed the accepted 800–1,200 line review trigger.
`mainframe-env-application/src/lib.rs` and the conformance `lib.rs` also contain
implementation despite the façade-only `lib.rs` rule.

Split by stable reason to change: parsing, validation, transition, persistence,
recovery, and command family. For 0.9, do not add 263 commands to the existing
CICS service module; freeze a generated descriptor layer and separate semantic
handlers by family.

### R-27 — Public Rust API documentation is not enforced

The normal warnings-denied Rustdoc build passes because no crate enables
`missing_docs`. A focused `-D missing-docs` probe on
`mainframe-env-store-api` fails across its public records and traits. Ratchet
`missing_docs` on contract crates first, add runnable examples for lifecycle and
store semantics, then expand to supported public crates.

### R-28 — Documentation had stale state, incomplete navigation, and dead commands

At review start the docs index omitted ADR-0005 through ADR-0007 and most
runbooks/contracts; current-state research contradicted the merged 0.9 pilot;
two official examples used nonexistent `xtask` subcommands; package-count and
release truth were stale; and operations text claimed CLI/secret/metrics
behavior absent from the binary. The direct command, navigation, release, pilot,
and operations errors were corrected as part of this review. A generated docs
gate and a superseding current package-map ADR remain open.

## Strengths to preserve

- All library crates forbid unsafe Rust.
- The foundation/contracts dependency direction is clean and mechanically
  checked.
- Source identities, paths, payloads, and many state transitions are explicitly
  typed and bounded.
- The execution outcome taxonomy distinguishes conditions, ABEND, cancellation,
  timeout, resource exhaustion, provider failure, infrastructure failure, and
  unknown outcome.
- The dataset provider already uses a versioned owned digest codec and offers a
  useful pattern for the other providers.
- Tests contain meaningful restart, replay, corruption, saturation, negative,
  mutation, and conformance cases rather than only happy-path examples.
- The Conformance IR keeps catalog rows, obligations, executable bindings, and
  licensed-credit policy separate and fail-closed.
- Rust/Cargo and the lockfile are pinned; normal checks use `--locked`; advisory
  and unknown-source checks pass.
- Exact-candidate receipts, disposable PostgreSQL isolation, and offline build
  support are solid foundations once the enforcement gaps are closed.

## Verification results

| Check | Result |
|---|---|
| `cargo fmt --all -- --check` | Pass |
| `cargo xtask spec --check` | Pass: 1,506 catalog rows, 497 claimed rows, 2,317 obligations, 2,807 bindings |
| `cargo xtask architecture-fast --check` | Pass; findings R-07/R-26 show scope gaps |
| `cargo xtask evidence-fast --check` | Pass |
| `cargo test --workspace --all-features --locked --no-fail-fast` | 835 passed, 6 ignored, 0 failed |
| All seven Python test groups | 315 passed, 1 skipped, 0 failed |
| Strict workspace Clippy | Pass |
| Warnings-denied workspace Rustdoc | Pass |
| Full workspace Rust 1.95 check | Pass |
| `cargo xtask conformance --check` | Pass |
| `cargo xtask certification` | Pass; does not execute all ignored external suites |
| Local PostgreSQL parity as wired in Jenkins | 2 passed; see R-21 omissions |
| `cargo deny check` | **Fail:** ICU license not allowed |
| Markdown relative-link scan | Pass: 0 broken relative targets |
| macOS release binary execution probe | **Fail:** abort 134, missing `LC_UUID` |
| BSD-tar reproducibility probe | **Fail:** digest changes with input mtime |

## Review limitations

- No licensed IBM COBOL, CICS, RACF, dataset, or JES differential campaign was
  available; no licensed-equivalence credit is added.
- The live Zowe route campaign and full pinned CardDemo corpus were not rerun.
- The two PostgreSQL tests omitted from the Jenkins parity stage were not
  relabeled as passes.
- No long-duration load/soak, fuzz campaign, branch-coverage run, chaos cluster,
  or external penetration test was performed.
- Static findings against PostgreSQL concurrency and multi-process artifact
  publication require the prescribed regression tests during remediation.

## Required hardening sequence

### Gate A — release and repository truth

1. Assign a distinct development version and keep Unreleased current.
2. Fix macOS target execution and deterministic archive generation.
3. Close the Apache/ICU license and notice obligations.
4. Make release smoke use the exact artifact and refuse non-identical clobber.

### Gate B — mutation safety and security

1. Close R-01, R-02, R-03, R-06, R-07, R-08, and R-10.
2. Run one hostile backend contract suite on memory, SQLite, and PostgreSQL.
3. Prove post-dispatch failures, stale intents, audit saturation, auth cleanup,
   and enterprise SAF denial across restart.

### Gate C — service lifecycle

1. Replace request-coupled JES execution with a real worker lane and clock.
2. Make HTTP timeout/cancellation effective across blocking work.
3. Implement secret references, bootstrap, readiness, metrics, and retention.
4. Prove graceful shutdown, backlog recovery, expiry, and capacity reclamation.

### Gate D — assurance and modularity

1. Run every Python and PostgreSQL suite from CI.
2. Auto-discover schemas; validate SBOM/provenance honestly; enforce full-scope
   MSRV and dependency policy.
3. Split the CICS runtime by command family before adding the full catalog.
4. Establish fuzz/coverage/concurrency baselines or mark them explicitly
   planned.

## 0.9.0 entry criteria

Broad CIC-901 implementation may begin only when:

- all R-01 through R-15 findings are closed with focused regressions;
- Gates A through C pass on one unchanged candidate;
- the accepted CICS pilot remains green on memory and SQLite, with PostgreSQL
  added where the affected contract requires it;
- `cargo deny check`, exact-target release smoke, archive reproduction, full
  Python discovery, and the complete PostgreSQL stage are blocking and green;
- `docs/delivery/coverage-versions/status/0.9.0.md` names the exact candidate,
  remaining blockers, and next reviewable work package; and
- CIC-901 through CIC-906 are delivered as bounded, reviewable integration
  increments while public capability remains disabled until the minor exit
  gate.

Passing this entry gate authorizes implementation work only. It does not imply
release, deployment, publication, or IBM compatibility certification.
