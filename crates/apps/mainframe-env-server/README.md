# mainframe-env-server

Single-node development product composition. It constructs the selected store and
provider generation, owns authentication sessions/readiness/lifecycle and the
bounded console authority, and supplies application services to the thin
z/OSMF gateway. Public advertisement follows the selected capability and route
catalogs; package presence does not imply complete subsystem support.

Start with the [operations runbook](../../../docs/runbooks/OPERATIONS.md) for
configuration precedence, secret references, first-administrator bootstrap,
local SQLite smoke startup, and readiness checks. The checked-in default uses
PostgreSQL with shared artifacts and TLS; it needs provisioned secret inputs
before it can start.

The selected JES COBOL program accepts its primary source through `SYSIN` and
ordered copybook members through `SYSLIB*` DDs. `FORMAT=FIXED` selects fixed
source; the old no-library free-form route remains compatible.

The composed IMS host provider accepts the additive typed DB-batch LOG
projection against published/installed IMS metadata. The canonical execution
coordinator owns its intent, audit and terminal result; RecoverySession owns
the log and the existing utility bridge publishes its selected-generation fence.
This bounded host route does not admit IMS into a shared transaction participant
or add COBOL recovery operand lowering. Remaining recovery families stay pending.

Selected signed application packages are also the composition boundary for
batch controllers. The server checks the selected package identity, decodes
the bounded property contract into typed batch plans, and atomically publishes
the complete generation; it does not embed workload identities.

Retained package generations are durably stored and signature-reverified on
open. Controller and Db2 publication use one generation/identity-bound recovery
state machine with explicit partial states; no generation is reported selected
until every applicable section is durable. Production package trust stores only
`SecretRef` values and resolves zeroizing HMAC verification material per use;
signing remains test/tooling-only.

Artifact placement is explicit. Memory and SQLite profiles require
`artifact_profile = "local"`; the PostgreSQL profile requires
`artifact_profile = "shared"` and stores content-addressed objects in the same
PostgreSQL authority through `PostgresArtifactStore`. PostgreSQL startup fails
closed rather than falling back to a node-local artifact directory.

The standalone binary applies TOML, environment, then named CLI overrides. Its
PostgreSQL URL, TLS private key, bootstrap credential, and package keys are
resolved through the same bounded `SecretRef` provider; raw secret values have
no parallel configuration fields. A fresh store stays unready until the
one-time first administrator is durable; committed restart never resolves that
one-time secret again. `/zosmf/info` reports the validated listener plus
liveness, rollback-only store writability, retention saturation and warning,
identity, capability, writable artifact/headroom, and queue-progress readiness
components separately. Each JES worker must have made durable queue progress
within three heartbeat intervals; a running but stalled worker fails readiness.

The validated `retention` configuration defines lifecycle, idempotency, and
archive lifetimes plus alert watermarks and a hard batch bound. Embedders can
forecast headroom, atomically archive/prune eligible rows, inspect archives,
and prune expired archives through `ProductServer`. The standalone binary also
exposes the same controls as an offline operator CLI; it does not expose an
unauthenticated retention route or start a scheduler.

## Offline retention maintenance

The legacy invocation remains unchanged: `mainframe-env-server [CONFIG]`
starts the service. Adding a `retention` action opens and migrates only the
durable state store, then uses the shared store-only retention planner. It does
not open Product, providers, artifact/package authorities, authentication or
application recovery, caches, a listener, or workers:

```text
mainframe-env-server [CONFIG] retention forecast --observed-growth-per-tick 10
mainframe-env-server [CONFIG] retention maintain --max-records 1024
mainframe-env-server [CONFIG] retention maintain --max-records 1024 \
  --authorize-oversized-archive sha256:REVIEWED_ARCHIVE_ID
mainframe-env-server [CONFIG] retention legacy --max-records 1024
```

These commands reject Memory and in-memory SQLite profiles. Output is one compact JSON document
on stdout; an operational failure is a JSON document on stderr with a non-zero
exit status. `maintain` first removes a bounded number of expired archive rows
in whole verified batches, then runs one bounded archive-before-prune operation
for every target in the contract's frozen provider-first order. It reports
exact per-target receipts, including created/reused/stale-removed observation
counts, and any remaining legacy rows. If the oldest indivisible historical
archive exceeds `--max-records`, the first run deletes nothing and reports its
exact ID; only a retry with that ID may delete it. A failure after prior work
uses the `mainframe-env.retention-error@2` partial receipt with the phase and
all completed receipts in order.

Run maintenance only after draining every process that can write the store.
The CLI retries a small, bounded set of optimistic conflicts with short jitter,
but this is collision tolerance, not an online-progress guarantee. Re-run a
pass when receipts show more eligible records than the configured batch bound.

Legacy rows without trustworthy age remain protected. Inspect their exact CAS
tokens with `retention legacy`, verify provenance outside the process, and then
record a conservative observation explicitly:

```text
mainframe-env-server CONFIG retention reconcile \
  --target db2-replay --namespace db2-v1-replay --key KEY \
  --expected-version VERSION --owner-execution EXECUTION
```

The `terminal-executions`, `terminal-work`, `delivered-outbox`,
`resolved-effects`, `audit`, and `lifecycle-events` targets require the owner to
be omitted. Legacy Db2, IMS, MQ, and CICS replay targets require the exact
verified owner; the CLI never guesses it. A stale version, wrong namespace,
missing dependency, or provider row that fails its full decoder is rejected
without changing the source row.

## Documentation

[Documentation portal](../../../docs/README.md) · [Current package map](../../../docs/architecture/PACKAGE-MAP.md) · [Embedding guide](../../../docs/guides/EMBEDDING.md) · [Contribution and verification](../../../CONTRIBUTING.md)
