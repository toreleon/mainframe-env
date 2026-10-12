# Batch baseline strict lint repair

This repair removes redundant reborrows in job retirement and collapses nested
error-fencing conditions in retirement, step execution, and live Running step
observations. It passes the batch package's strict all-target/all-feature Clippy
gate on the pinned toolchain without new lint suppressions or module ceilings.

The underlying `Job` borrow remains the same mutable reference. Event admission,
spool publication, output completion or cancellation, version checks, and
checkpoint writes remain in their existing order. Conditional chains preserve
short-circuit evaluation, active-owner checks, error poisoning, and the exact
set of problems that prevent cleanup or later host actions.

No public Rust signatures, private authority exposure, schema tags, durable
rows, wire payloads, subsystem semantics, or test assertions change. There is no
caller or durable-data migration. This is baseline lint maintenance, not new
Foundation functionality or a promotion of source-only contracts.

Focused run-stop and Running step regressions check ordinary and known-abend
retirement, stopped runs, revocation, changed physical rows, retained views,
Memory/SQLite reopen, and control failures. External receipts retain their actual
candidate identity and test counts. No PostgreSQL or licensed execution credit
is asserted. Aggregate policy and integration-tree checks remain manager-owned;
this bounded repair does not close larger issues or establish phase acceptance.
