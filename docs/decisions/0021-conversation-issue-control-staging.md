# ADR-0021: Stage ISSUE controls in the shared conversation ledger

Status: **Proposed for v0.9 development**
Owner: **CICS provider and execution maintainers**
Scope: **CIC-905 mapped and GDS ISSUE controls**
Applies from: **mainframe-env 0.9.0 development**

## Context

ISSUE ABEND, CONFIRMATION, ERROR, PREPARE, and SIGNAL change an APPC
conversation through partner control flows. A local effect receipt or carrier
queue acceptance cannot establish the partner's outcome. The conversation-open
lane already owns the versioned APPC/MRO ledger described in [ADR-0020](0020-conversation-peer-exchange-ledger.md).
The pinned CICS TS 6.x sources-b row and SHA identities are recorded in the
[0.9 status ledger](../delivery/coverage-versions/status/0.9.0.md).

## Decision

1. The existing `ConversationRecord` retains at most one bounded ISSUE
   control intent. Its effect key, stable control ID, flow, and pre-dispatch
   marker are stored in the same `cics-conversation-v1` provider row. Older
   canonical records omit the optional field and reopen without conversion;
   staging upgrades a v1 record and retains its effective profile.
2. Owner lease, DPL principal, mapped versus basic form, state, and sync
   level are checked before staging. Staging leaves source-visible state
   unchanged. Competing protocol changes and FREE fail while a control is
   pending.
3. The pre-dispatch marker is persisted before a transport attempt. An
   attempted control cannot be silently discarded by task cleanup. A carrier
   retry must reconcile the stable ID. Only an explicit confirmed partner
   result may apply the state transition and clear the pending record.
4. Command handlers retain exact request identity in the staged record and
   write the final replay receipt atomically with the confirmed transition.
   They must use the data/wait lane's confirmed transport boundary for partner
   delivery and consumption. This ADR grants no catalog readiness until
   those routes and recovery gates pass.

## Consequences

- The optional intent is additive to the versioned shared ledger. It does not
  create a second APPC/MRO state authority or use MQ acknowledgement as a
  conversation result.
- Unknown outcomes retain enough identity to reconcile without a second
  control transmission. Task cleanup refuses to discard an attempted intent.
- Record-level tests prove staging, SQLite reopen, and confirmed transitions.
  An isolated PostgreSQL race proves one staged CAS winner and restart of the
  attempted control. Provider routing, partner consumption, and selected
  compiled execution remain separate acceptance gates.
- The internal mapped provider path can stage and replay an ISSUE intent.
  Its separate control-ID carrier call saves the attempt marker first, uses a
  read-only reconciliation after uncertain dispatch, and commits the final
  receipt only on a confirmed result. Partner ingress and public command
  routing remain acceptance gates.
