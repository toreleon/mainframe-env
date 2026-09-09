# Core-server operations

Status: **Development runbook for the current `main` binary**

The `core-server` profile is a single-node development composition. It is not a
turnkey production service. Read the current limitations below before using the
sample configuration.

## Current operational limitations

- The service invocation accepts one optional positional configuration path.
  Retention subcommands add operation-specific flags, but there are still no
  general CLI configuration overrides.
- A fresh store has no operator-facing bootstrap command. The library exposes
  bootstrap helpers for tests and embedding, but the shipped binary cannot
  create the first authenticated administrator.
- `postgres_url_reference` and `tls.private_key_reference` are validated as
  non-empty markers but are not dereferenced by the binary. The current process
  reads the actual PostgreSQL URL from `MAINFRAME_ENV_POSTGRES_URL` and the TLS
  key path from `MAINFRAME_ENV_TLS_KEY_PATH`.
- `ProductServer::metrics()` is an in-process API. No `/metrics` endpoint or
  exporter is shipped.
- Retention forecast/archive controls are privileged in-process APIs for an
  embedding control plane and offline subcommands of the standalone binary.
  There is no retention HTTP endpoint or background retention scheduler.
- `/zosmf/info` is a liveness-oriented development probe. On a fresh store it
  can report `ready=true` even though no principal can authenticate.

These limitations are tracked as pre-0.9 blockers in the
[deep review](../reviews/PRE-0.9.0-DEEP-REVIEW.md). Do not compensate for them
with undocumented production procedures.

## Configuration sources

Configuration precedence in the shipped binary is:

1. the TOML file (`config/mainframe-env.toml` by default, or the first
   positional argument);
2. the supported `MAINFRAME_ENV_*` environment overrides below; and
3. `ConfigOverrides` only for library embedders, not the standalone binary.

Unknown TOML fields and unsupported schema versions fail closed.

| Environment variable | Current effect | Secret value? |
|---|---|---|
| `MAINFRAME_ENV_LISTEN` | Override `listen` | No |
| `MAINFRAME_ENV_STORE` | `memory`, `sqlite`, or `postgres` | No |
| `MAINFRAME_ENV_SQLITE_URL` | Override the SQLite URL | Potentially |
| `MAINFRAME_ENV_POSTGRES_URL_REF` | Override the validated reference marker | No |
| `MAINFRAME_ENV_POSTGRES_URL` | Actual URL consumed by the current binary | **Yes** |
| `MAINFRAME_ENV_ARTIFACT_STORE` | `local` for memory/SQLite or `shared` for PostgreSQL | No |
| `MAINFRAME_ENV_ARTIFACT_ROOT` | Override the local artifact directory | No |
| `MAINFRAME_ENV_TLS` | Enable or disable TLS | No |
| `MAINFRAME_ENV_TLS_KEY_PATH` | Actual private-key path consumed by the current binary | Sensitive path |
| `MAINFRAME_ENV_PACKAGE_HMAC_KEY_REFS` | JSON map of package key IDs to `env-base64:` references | References only |
| `MAINFRAME_ENV_SECRET_*` | Base64-encoded package verification secret resolved on use | **Yes** |

Package verification references must have the form
`env-base64:MAINFRAME_ENV_SECRET_<NAME>`. Raw secret values must not be written
to TOML, logs, evidence, shell history, or checked-in examples.

## Local SQLite smoke start

This starts only the unauthenticated information route because the standalone
binary has no first-user bootstrap workflow:

```bash
MAINFRAME_ENV_STORE=sqlite \
MAINFRAME_ENV_SQLITE_URL='sqlite://mainframe-env-dev.db?mode=rwc' \
MAINFRAME_ENV_ARTIFACT_ROOT='mainframe-env-artifacts-dev' \
MAINFRAME_ENV_TLS=false \
MAINFRAME_ENV_LISTEN='127.0.0.1:10443' \
cargo run --locked -p mainframe-env-server --bin mainframe-env-server
```

In another shell:

```bash
curl --fail --silent --show-error http://127.0.0.1:10443/zosmf/info
```

Treat this as a process/startup smoke test, not an authentication, worker, or
production-readiness test.

## PostgreSQL and TLS inputs

For the current implementation, provide both the reference markers in TOML and
the actual process values:

```bash
MAINFRAME_ENV_STORE=postgres \
MAINFRAME_ENV_ARTIFACT_STORE=shared \
MAINFRAME_ENV_POSTGRES_URL='postgres://USER:PASSWORD@HOST/DATABASE' \
MAINFRAME_ENV_TLS=true \
MAINFRAME_ENV_TLS_KEY_PATH='/absolute/path/to/server.key' \
cargo run --locked -p mainframe-env-server --bin mainframe-env-server -- \
  /absolute/path/to/mainframe-env.toml
```

The certificate path still comes from `tls.certificate_path` in TOML. Restrict
process-environment and file permissions appropriately. This temporary split
between references and concrete values must be removed before production use.

## Readiness and health

`GET /zosmf/info` reports product and route-contract metadata plus the current
development readiness boolean. A production readiness contract must also check
at least:

- schema/migration compatibility and a writable durable-store probe;
- first-principal/bootstrap completion and authentication health;
- background worker health and queue progress;
- artifact-store durability and visibility for the selected store profile; and
- retention/capacity headroom for events, outbox records, sessions, and replay
  journals.

The current readiness bit now requires the JES worker pool to be started and
not stopping. Until the remaining checks exist, external orchestration must not
use `ready=true` as a production traffic gate.

## Offline retention procedure

The retention subcommands are filesystem/database operator controls, not
network endpoints. They validate only the durable-store/retention configuration
and open the SQLite or PostgreSQL durable state authority plus the shared
store-only planner. They
do not open Product, providers, artifact/package authorities, authentication or
publication recovery, caches, `listen`, or workers. Memory and in-memory SQLite
profiles are rejected because they cannot recover state across a restart.
Commands emit one compact JSON object on stdout; failures emit JSON on stderr
and return non-zero.

For guaranteed progress, first drain **all** servers, workers, and tools that
can mutate the selected state store. For PostgreSQL, drain every node sharing
the database. Take and verify a backup before the first destructive pass. The
bounded conflict retries tolerate a small race; they do not make retention
live under continuous traffic.

1. Forecast every target in the contract's frozen dependency-safe order. Supply
   a measured row-growth rate when capacity timing matters:

   ```bash
   mainframe-env-server /etc/mainframe-env.toml retention forecast \
     --observed-growth-per-tick 10 --conflict-retries 3
   ```

2. Run one bounded pass. The command deletes only whole archive batches older
   than `archive_ticks`, cumulatively bounded by their source-row count, then
   processes each live provider/core target once in provider-first order.
   `--max-records` cannot exceed `retention.max_batch` or the contract maximum:

   ```bash
   mainframe-env-server /etc/mainframe-env.toml retention maintain \
     --max-records 1024 --conflict-retries 3
   ```

   If the command reports `oversized_archive_authorization_required`, inspect
   the exact archive ID and source-row count, then authorize only that reviewed
   indivisible archive:

   ```bash
   mainframe-env-server /etc/mainframe-env.toml retention maintain \
     --max-records 1024 \
     --authorize-oversized-archive sha256:EXACT_REVIEWED_ID
   ```

   Keep every JSON receipt with the change record. Target receipts include
   `observations_created`, `observations_reused`, and
   `stale_observations_removed`. A pass is not globally atomic: a later target
   can fail after earlier target transactions committed. The
   `mainframe-env.retention-error@2` status is then `partial` and preserves the
   failing phase, expired-archive receipt, and every completed target receipt
   in order; the process exits nonzero. Those commits are replay-safe; fix the
   reported problem and run another pass. Repeat while `eligible_records` or
   receipt counts exceed one batch.

3. Inspect protected legacy rows. This listing is cumulatively bounded across
   every target and supplies exact namespace/key/version CAS tokens:

   ```bash
   mainframe-env-server /etc/mainframe-env.toml retention legacy \
     --max-records 1024
   ```

4. Reconcile only a row whose provenance and age were independently verified.
   The `terminal-executions`, `terminal-work`, `delivered-outbox`,
   `resolved-effects`, `audit`, and `lifecycle-events` targets must omit
   `--owner-execution`; legacy Db2, IMS, MQ, and CICS replay rows require their
   exact verified owner:

   ```bash
   mainframe-env-server /etc/mainframe-env.toml retention reconcile \
     --target db2-replay --namespace db2-v1-replay --key EXACT_KEY \
     --expected-version 7 --owner-execution EXACT_EXECUTION
   ```

   Never infer an owner from a key. Provider-specific rows that cannot be
   safely reconciled through this generic contract fail closed. Re-list after
   every reconciliation; a stale version means the row changed and must be
   inspected again.

5. Run a final forecast, verify expected headroom and saturation, then restart
   the normal service invocation. Retention does not make pending outbox rows,
   unresolved effects, active work, checkpoints, corrupt provider rows, or
   unverified legacy rows eligible; repair their owning workflow instead.

## Shutdown

Send `SIGTERM` or `SIGINT`. Admission and new JES claims stop, idle workers are
woken, and the product waits up to `shutdown_millis` for its tracked active
request set and bounded worker pool. A worker that cannot finish before the
deadline is detached without a lease completion; its durable item becomes
reclaimable at lease expiry with a higher fencing epoch. The current non-TLS
Axum path does not independently enforce a hard transport shutdown deadline,
so operators should verify process exit and investigate blocked synchronous
backend work rather than immediately issuing `SIGKILL`.

## Observability

The library tracks request, failure, active-request, JES worker/active-work,
authentication-session, console-message, and outbox counters. They are
currently visible only to an
embedding application or test through `ProductServer::metrics()`. The
standalone binary does not install a metrics exporter or a tracing subscriber.

Before production use, expose authenticated health/metrics endpoints, install a
structured redacting tracing subscriber, define alert thresholds, and add
retention/saturation metrics. Never log authorization headers, bearer tokens,
database URLs, private-key material, package keys, or protected application
fields.

## Troubleshooting

| Symptom | Check |
|---|---|
| Configuration is reported malformed | Schema version, unknown TOML fields, non-zero limits, TLS certificate/reference fields |
| PostgreSQL reference appears valid but startup fails | `MAINFRAME_ENV_POSTGRES_URL` is still required by the current binary |
| TLS reference appears valid but startup fails | `MAINFRAME_ENV_TLS_KEY_PATH` is still required and must point to readable PEM key material |
| Information route works but authentication cannot succeed | A fresh standalone deployment has no supported first-user bootstrap path |
| Requests exceed `timeout_millis` | Backend dispatch is currently synchronous; see the pre-0.9 timeout finding |
| New work fails after extended uptime | Inspect retention forecasts and protected rows; archive eligible lifecycle/outbox/effect/replay batches, but resolve checkpoints or stale intents through their owning recovery workflows |

For SQLite recovery procedures see [Backup and restore](BACKUP-RESTORE.md). For
capacity assumptions and failure modes see
[Capacity and recovery](CAPACITY-AND-RECOVERY.md).
