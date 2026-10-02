# ADR-0029: Versioned selected database PCB feedback

Status: **Proposed**
Owner: **host-contract and IMS provider maintainers**
Scope: **IMS-1401.selected-pcb-feedback, owned host projection**
Applies from: **mainframe-env 0.14.0 development**

`ImsResult` has no owned PCB/key-feedback output. Adding fields to its frozen
canonical object would change historical effect and replay identities. Reading
the current cursor after replay would combine an old status/data response with
new keys, and cannot recover failed-call feedback from retained position.

Introduce `HostRequest::ImsPcbFeedbackV1` and
`HostResult::ImsPcbFeedbackV1`, with separately named V1 records and canonical
encoders. The request embeds the existing typed request, execution context,
optional display-code SSAs, and an explicit maximum key-byte capacity. Existing
SSA validation, metadata, selection and navigation remain authoritative.
This is an owned host response, not EBCDIC COBOL/C storage, an AIB, or an IBM
binary mask. No dependency or alternate runtime is introduced.

The existing execution pipeline authorizes the selected PCB's resources and
checks exact replay before fresh navigation. For a fresh call, the feedback
helper projects from that pipeline's unpublished proposal. Primary successful
retrieval and ISRT concatenate metadata sequence fields from the engine-owned
path to the selected occurrence. Capacity failure discards the proposal; no
key truncation or position/data/replay publication occurs. Path retrieval and
key-only suppression retain their existing owners. Status and transferred data
length come from the same result, and database/PROCOPT/sensitive-segment count
come from the selected PCB's validated metadata.

The new output is optional `pcb_feedback_v1` inside the existing strict receipt,
published with image, cursor, UOW and observer changes in the same atomic CAS.
Retention validates and hashes the new result variant. Replay returns that
stored output verbatim, including after later mutation or process restart.
No cursor/feedback state map, store, replay namespace, lock, migration runner,
recovery engine or second coordinator is introduced. Contradictory GSAM and
feedback outputs in one receipt fail closed.

Availability is explicit. `Valid` contains segment name, level and only valid
key bytes; its length is those bytes' length. Secondary REPL reports
`InvalidatedSecondaryReplace`. `Unsupported` names a missing failed-call
witness, secondary-sequence authority, non-key operation rule, sequence field,
or logical-relationship feedback authority. Unsupported does not assert empty
keys, zero levels or source-defined invalidity. See the exact
[class review](../delivery/subsystems/ims/selected-pcb-feedback.md).

Historical request/result encodings and receipts without the optional field
remain byte-identical. No SQL/envelope version migration, eager rewrite or
automatic pruning occurs. New receipts participate in the existing replay
capacity, age, owner and journal dependency rules. Prior strict receipt readers
reject the new field; old binaries cannot dispatch the new variants. Before
downgrade, stop admission, reconcile/drain new effects and UOWs, and retain a
compatible reader or restore a coherent pre-feature backup containing database,
session, undo, checkpoint, replay, package, journal and audit references. Never
strip feedback from live receipts to simulate rollback.

The existing `HostResult` declaration moves to its existing validation module
behind a stable re-export to lower the frozen request-module budget. This is a
mechanical boundary move; no old variant or canonical identifier changes.
The local source and backend checks give no official, maintainer, parent,
licensed, participant or release acceptance.
