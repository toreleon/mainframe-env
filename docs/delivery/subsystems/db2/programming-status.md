# Db2 — Complete programming surface progress

Subsystem: **db2**
Phase: **programming**

Status: **Blocked on db2.core acceptance; complete programming surface not implemented**

## Current scope

The 174-row catalog preserves 158 SQL headings and 16 SQL PL rows. The bounded
syntax, type/value and column-default APIs and existing signed static-SQL service
belong to [db2.core](core-status.md). They do not complete DB2-1301 through
DB2-1306 or establish advanced-programming acceptance.

| Work package | State | Missing acceptance |
|---|---|---|
| DB2-1301 | Pending | Complete SQL/SQL PL syntax, binding, diagnostics and conditions |
| DB2-1302 | Pending | Advanced query, temporal, XML/LOB/array and analytic semantics |
| DB2-1303 | Pending | Routines, triggers, variables and dependency lifecycle |
| DB2-1304 | Pending | Package/plan bind lifecycle, privileges and invalidation |
| DB2-1305 | Pending | Isolation, locks, deadlocks, savepoints, logging and recovery |
| DB2-1306 | Pending | Distributed behavior, scale, compatibility and licensed differentials |

## Dependencies and next step

Consume the accepted core parser/binder/catalog/relational and transaction
contracts before advancing the [programming plan](programming-plan.md). Preserve
the same [official catalog](../../../../conformance/subsystems/coverage/catalogs/db2.json),
source identities and shared Conformance IR. Declare each advanced bounded
slice and its independent acceptance cases; do not count core parser preparation
as a completed programming row.

No complete-programming acceptance or licensed differential credit is claimed.
Authentic Db2 13 oracle observations and supported backend/failure/recovery
matrices remain required and unavailable for a phase-completion claim.
