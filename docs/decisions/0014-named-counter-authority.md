# ADR-0014: Keep named counters in one versioned pool authority

Status: **Proposed for v0.9 development**
Owner: **CICS provider and execution maintainers**
Scope: **twelve COUNTER and DCOUNTER application rows, typed plans, and durable replay**
Applies from: **mainframe-env 0.9.0 development**

## Context

IBM CICS TS 6.x application-command rows 0034/0035, 0044/0045, 0087/0088,
0153/0154, 0179/0180, and 0226/0227 share one named-counter pool. GET and
UPDATE must advance or compare a value atomically across workers. QUERY must
observe the same current value, including the one-past-maximum limit state.
The exact offline source baseline, catalog rows, topics, and SHA-256 values
are recorded in the [0.9 status ledger](../delivery/coverage-versions/status/0.9.0.md).

The integrated CICS service, product, and interpreter roots have frozen
production-line budgets. Growing any of those roots to hold counter state or
command logic would break the module boundary gate.

## Decision

1. The CICS provider owns a single versioned `cics-counter-control-v1` state
   record. A pool/name key selects one signed fullword or unsigned doubleword
   counter. DEFINE, DELETE, GET, REWIND, and UPDATE write the counter and their
   owner-fenced replay entry in one compare-and-swap transition. QUERY reads
   the same durable row without an effect mutation.
2. The record carries the inclusive minimum and maximum and an explicit
   at-limit bit. This represents the unsigned one-past-maximum value even
   when a doubleword maximum is `u64::MAX`. Source-defined DCOUNTER sentinel
   reductions, signed fullword low-bit returns, and LENGERR warnings are
   handled at the command boundary.
3. Counter name and pool validation, numeric widths, option legality, and
   output bindings are checked in focused IR, compiler, interpreter, and
   provider children. New MCEP v2 tags occupy operations 118–129, operands
   384–447, options 316–379, and outputs 440–503. MCEP v1 decoding remains
   unchanged; a counter plan requiring a wide tag cannot be encoded as v1.
4. The provider authorizes `COUNTER CICS.COUNTER.<pool>.<name>` before any
   mutation. Security calls are audited. A pool rebuild yields BUSY with
   NOSUSPEND or a bounded suspension that the existing online coordinator
   reissues under its deadline and live cancellation checks. A post-dispatch
   receipt failure reports UnknownOutcome and permits only matching owner and
   request reconciliation.
5. The frozen root modules delegate to bounded counter children. The typed
   CICS descriptor array and interpreter registry live in bounded child
   modules; the semantic boundary checker follows those reexports. The
   committed production-line budgets are unchanged.

## Consequences

- Memory and SQLite use the same versioned CAS path; focused concurrent
  workers and SQLite reopen are exercised. The PostgreSQL parity test needs
  an isolated PostgreSQL 18 URL and is not claimed as executed here.
- This authority models CICS named-counter behavior locally. A native
  coupling facility server, pool options table, or licensed differential
  would require separate evidence and routing decisions.
