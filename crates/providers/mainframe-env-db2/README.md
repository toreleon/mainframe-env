# mainframe-env-db2

Ownership: a bounded durable generic static-SQL catalog, typed table/column/key
and foreign-key metadata, rows, cursors, SQLCA-shaped results, extraction
layouts, replay, and unit-of-work commit/rollback.

Application identities and table shapes are not production code. A selected
application package supplies a content-addressed `Db2CatalogGeneration` with
schemas, optional seed rows, constraints, result encodings, and extract
layouts. Installation validates the entire generation and persists schema,
rows, ownership, identity, and selection atomically. Same-identity retry is
idempotent; same-generation conflict, cross-application table collision, stale
generation, malformed schema, missing reference, and non-quiescent install fail
closed.

Signed catalog JSON is decoded through streaming cardinality, text, nesting,
duplicate, and aggregate bounds before the complete owned graph is constructed.
Compatible in-place upgrades require full normalized semantic equality.
Provenance distinguishes legacy tables from application-created tables, so
rollback restores adopted legacy state while removing only newer
application-owned tables and dependents. Raw predicates and cursors compare
exact bytes; only declared VARCHAR columns use strict length-prefix decoding.

The SQL route parses table, column, key, assignment, cursor, and DDL identities
against the installed catalog. It contains no application table or host-variable
dispatch. Non-goals are the complete Db2 13 SQL language, optimizer, utility,
package/plan, and distributed portfolio assigned to later versions. Verify with
`cargo test -p mainframe-env-db2 --locked`, `cargo xtask db2-catalog --check`,
and the pinned CardDemo Db2 and authorization gates.
