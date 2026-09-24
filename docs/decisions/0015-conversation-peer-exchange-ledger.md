# ADR-0015: Persist explicit conversation peer frames with protocol state

Status: **Proposed for v0.9 development**
Owner: **CICS provider and execution maintainers**
Scope: **CIC-905 conversation-open CONVERSE and sibling exchange commands**
Applies from: **mainframe-env 0.9.0 development**

## Context

IBM CICS TS 6.x CONVERSE sends application data and receives a partner
response. Its length, truncation, EOC, FMH, SIGNAL, and STATE results depend on
the actual peer frame. Successful local delivery or an MQ acknowledgement
cannot establish those results. The source baseline, rows, topic paths and
SHA-256 identities are recorded in the [0.9 status ledger](../delivery/coverage-versions/status/0.9.0.md).

## Decision

1. The conversation ledger accepts canonical version 1 rows with no peer
   frames. The next mutation writes version 2. Optional exchange state is
   keyed by CONVID and is validated against a live mapped APPC or MRO record.
2. A trusted conversation adapter offers an explicit, bounded inbound frame
   with response bytes, next protocol state and EOC/FMH/SIGNAL indicators.
   The frame and an owner-fenced event replay receipt use one provider-state
   CAS transaction. There is no implicit or generic success frame.
3. CONVERSE consumes that frame and records its outbound application data and
   selected structured attach header in the same CAS transaction as the
   conversation state and exact command replay receipt. NOTRUNCATE retains
   the remainder for a later RECEIVE sibling. FREE removes exchange state.
4. The frame contract does not represent MQ delivery, an acknowledgement, or
   licensed APPC execution. Source condition policy, EIB flags, and output
   bounds remain command-level responsibilities. The ledger row is capped at
   4 MiB, each frame at 1 MiB, and each per-conversation queue at 32 frames.

## Consequences

- Existing version 1 ledger rows reopen without reinterpretation and upgrade
  on mutation. A version-1-only reader needs a pre-upgrade snapshot for rollback.
- Peer frames and outbound records are durable, task-owned, replay-fenced and
  transport neutral. They do not claim a remote system received or executed
  a process unless its adapter explicitly supplies the corresponding frame.
- The same authority can be consumed by later SEND, RECEIVE and GDS siblings
  without adding their command rows or tags to this slice.
