# ADR-0020: Persist explicit conversation peer frames with protocol state

Status: **Proposed for v0.9 development**
Owner: **CICS provider and execution maintainers**
Scope: **CIC-905 conversation-open CONVERSE and sibling exchange commands**
Applies from: **mainframe-env 0.9.0 development**

## Context

IBM CICS TS 6.x CONVERSE sends application data and receives a partner
response. Its length, truncation, EOC, FMH, SIGNAL, and STATE results depend on
the actual peer frame. Successful local delivery or an MQ acknowledgement
cannot establish those results. The source baseline, rows, topic paths and
SHA-256 identities are recorded in the [0.9 status ledger](../delivery/subsystems/cics/application-api-status.md).

## Decision

1. The shared `cics-conversation-v1` ledger accepts canonical version 1 rows
   used by EXTRACT, including their indicator and LU 6.1 fields. The next
   mutation writes version 2. Optional exchange state is keyed by CONVID and
   is validated against a live mapped APPC or MRO record. EXTRACT retains
   task presentation and positioning only; it does not own a second protocol
   record.
2. A trusted conversation adapter offers an explicit, bounded inbound frame
   with response bytes, next protocol state and EOC/FMH/SIGNAL indicators.
   The frame and an owner-fenced event replay receipt use one provider-state
   CAS transaction. There is no implicit or generic success frame.
3. CONVERSE first stages one outbound application frame and a snapshot of its
   selected structured attach header in this ledger. A CAS write makes the
   frame visible before the command suspends for a peer. Reissuing the same
   effect does not stage it again, and FREE cannot discard a pending frame.
4. The compiled continuation may have a new coordinator effect sequence. A
   bounded pending record admits only a later sequence from the same effect
   stream, principal, lease, and mutation-free request digest. Completion
   consumes one explicit peer frame, moves the pending send to outbound
   history, advances protocol state, and writes exact replay receipts for all
   accepted effect identities in one CAS transaction. NOTRUNCATE retains the
   remainder for a later RECEIVE sibling. FREE removes completed exchange
   state.
5. The frame contract does not represent MQ delivery, an acknowledgement, or
   licensed APPC execution. Source condition policy, EIB flags, and output
   bounds remain command-level responsibilities. The ledger row is capped at
   4 MiB, each frame at 1 MiB, and each per-conversation queue at 32 frames.
6. RECEIVE and SEND use that same exchange row. SEND stages numbered frames;
   WAIT and explicit carrier confirmation advance them in order. An attempted
   send is reconciled by its ID after an uncertain outcome and is never
   transmitted again under that ID. GDS data and principal signal events stay
   under the same owner and lease in this ledger. Mapped, basic, and terminal
   indicators retain their source-specific presentation and replay behavior.

## Consequences

- Existing version 1 ledger rows reopen without reinterpretation and upgrade
  on mutation. A version-1-only reader needs a pre-upgrade snapshot for rollback.
- Peer frames and outbound records are durable, task-owned, replay-fenced and
  transport neutral. They do not claim a remote system received or executed
  a process unless its adapter explicitly supplies the corresponding frame.
- A pending send survives suspension and SQLite reopen without a terminal
  reply. Older canonical version 2 rows omit its optional field; a reader
  predating this addition needs a pre-upgrade snapshot for rollback after a
  pending frame is written.
- The same authority can be consumed by later SEND, RECEIVE and GDS siblings
  through their own command rows and reserved tags. The selected data and wait
  slice adds exactly seven rows while preserving CONVERSE and EXTRACT authority.
