# Durable storage profile

- Status: **Implemented**
- Applies from: mainframe-env 0.8.3 hardening
- Owner: store adapters and core-server composition
- Scope: PostgreSQL quotas, shared immutable artifacts, and local publication durability

## PostgreSQL quotas

`PostgresStateStore` records the configured `provider-state` row bound in
`store_quota`. `PostgresArtifactStore` records an independent
`artifact-object` bound. A count-changing operation locks its quota row inside
the same transaction, verifies the immutable configured maximum, adjusts used
capacity, performs the data mutation, and commits both together. Conflicts,
capacity failures, and aborted transactions explicitly roll back before their
connection is returned.

On first quota-aware open, the adapter locks and initializes the row from the
actual legacy table count. Later opens require the same maximum and exact
recorded/actual count. A missing row, incompatible maximum, negative count,
count drift, or legacy count above the maximum fails closed. Deployments must
drain pre-quota writers before upgrading.

## Shared immutable artifacts

The PostgreSQL product profile uses the dedicated `artifact_object` table,
never `artifact_root`. Objects are keyed by their lowercase `sha256:` identity
and store raw payload bytes, media type, digest, and schema version. Publication
is one `INSERT ... ON CONFLICT DO NOTHING` under artifact quota reservation.
Concurrent identical publication is idempotent; a different envelope for the
same content address conflicts. Every read recomputes the payload digest and
rejects malformed schema, media, digest, key, or oversized payload. Separate
adapter and product instances observe the same committed object.

`ServerConfig` accepts `artifact_profile = "local" | "shared"` and the
`MAINFRAME_ENV_ARTIFACT_STORE` override. Memory and SQLite require `local`;
PostgreSQL requires `shared`. Invalid pairings fail configuration validation,
and PostgreSQL startup never silently creates a node-local artifact tree.

## Local publication

`LocalArtifactStore` writes a unique temporary file beside the destination,
syncs its bytes, and atomically hard-links it to the final digest path. A hard
link cannot replace an existing name. The adapter syncs the containing
directory after publication and temporary removal, and syncs the object-root
directory when a new digest-prefix directory is created. A crash can leave only
an unreferenced temporary file; it cannot expose a partial final object or
replace an already-published object.
