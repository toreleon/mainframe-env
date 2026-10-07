# ADR-0032: Selected secondary checkpoint positions

Status: **Proposed within the user-authorized bounded IMS leaf**
Owner: **IMS recovery and database maintainers**
Scope: **IMS-1405.secondary-index-checkpoint-restart, target ims.programming**
Applies from: **mainframe-env current subsystem contracts**

The IMS 15.6 XRST pin requires each positioned PCB to be re-established through
GU, with the resulting status replacing the saved status. A hierarchy key alone
does not identify which source pointer returned a target in secondary order.
Source and target can differ and multiple source pointers can reference one
target. The owned checkpoint bridge must not restore primary traversal instead.

SavedPcbPosition adds optional `secondary`, mutually exclusive with `gsam` and
the historical nonempty `segment_key`. It binds the existing PCB number and
database to a domain-separated digest of PSB, selected PCB and database metadata,
the index/search key, source/target/current occurrence witnesses, and uniquely
keyed source/current paths. Optional record witnesses live on the existing
engine image, materialized only inside the existing atomic checkpoint proposal.
Issuance includes canonical request, database CAS version, the application's
existing UOW incarnation/epoch and engine identity;
record ordinals alone are insufficient. No address registry or second authority
is created. Saved record versions are observations, not physical IMS timestamps.

The existing secondary traversal has one additional exact-source filter for the
resolver's GU. It uses the same pointer ordering and current/parentage/hold logic
as legacy and rich SSA retrieval; the SSA parser and selected_index_field resolver
are unchanged. XRST establishes GU parentage and no hold. A changed index key
invalidates the record witness even if a later REPL restores identical bytes;
ordinary data-only changes preserve it. This conservatively invalidates every
checkpoint witness on that source when any of its index keys changes.
Deleted/reinserted and utility-reloaded records cannot reuse prior witnesses.
Stale witnesses report GE. Real selected GN locates the predecessor of the saved
search/source/current boundary for continuation on keyed-root organizations.
Unkeyed/HDAM missing-boundary reposition remains Unsupported. Nonunique pointer
ties use the existing deterministic source order; licensed tie-order equivalence
is not claimed. Physical source/current paths with nonunique or missing sequence
keys, DEDB, nonroot inversion, aliases and optional index fields are excluded.

Shared recovery replay precedes fresh state guards; integrity reads use the
pristine proposal before any planned GSAM truncation. Existing database dependency
CAS fences publish database witnesses, per-PCB positions, recovery receipts and
UOW release atomically. Capacity, real CAS conflicts, lost acknowledgements and
read-only unknown-outcome observation retain their existing meanings. This leaf
uses the existing generic Batch undo and named backout-point owner. At its
witnessed backout publication boundary, restored secondary identities survive
only when the same occurrence witness is still live in the actual current image.
A changed key or deleted occurrence cannot be revived from an older undo image;
data-only replacement retains its live witness. The reconciled image is added
to that same UOW's owned-image digest set for subsequent backout. Generic
Rollback reuses this boundary. No new store, registry or recovery dispatcher is
introduced. Named points retain the root's epoch/incarnation checks and cannot
cross CHKP or rescheduling. A retained checkpoint can legitimately reposition
the same live database occurrence in a new scheduled application incarnation.
Coordinator leases and participant admission remain unchanged.

When a saved occurrence is missing, the existing PCB position retains the original
bounded `SavedSecondaryPosition` as continuation provenance, separate from its
ordinary index-order predecessor. Independent equality groups compare the original
search key and physical source/current boundary in qualification order; they never
interpret a byte-sorted predecessor as an independent-group cursor. A successful
real read consumes this provenance. Until then CHKP refuses to turn the guessed
predecessor into a new proven position. The field is omitted on historical rows,
validated with the existing recovery bounds, retained across reopen, and rejected
by incompatible strict readers. It adds no store, second cursor or dispatcher.
Reopen also checks root-to-child order, exact sequence-key widths, the index's
source endpoint and shared root target against the retained database definition;
these checks do not require a deleted occurrence to remain live.

Historical absent fields still deserialize and serialize without new fields;
their checkpoint digests and host canonical bytes stay fixed. GSAM identities,
format fields and namespaces are preserved. New secondary rows need a compatible
reader. Old writers can discard optional engine fields and strict old checkpoint
readers reject the additive variant. Stop admission, drain UOWs/holds/Q, reconcile
unknown effects and back up a coherent database/session/recovery/metadata/journal/
audit set before changing binaries. Concurrent old/new writers are unsupported.
Downgrade requires a compatible backup or removal of all new retained references
through the existing retention policy, not ad hoc checkpoint rewriting. Durable
reopen tests do not prove backup restoration or retention expiry.

Source baselines are the registered recovery, database and programming contracts
for IMS 15.6, particularly apr/ims_xrstcall.htm (`aff46320...`),
apr/ims_symbolicchkpcall.htm (`87ede582...`), apg/ims_ssassecondaryindex.htm
(`4b3a1ee3...`), dag/ims_howseindexmaint.htm (`910d3494...`) and
dag/ims_howhierrstruc_fullfunction.htm (`8535859c...`). Exact identities and catalog
rows are in the programming status and external source receipt. No official,
human, licensed, parent or release completion is asserted.

Related boundaries: [GSAM logical addresses](0034-gsam-logical-address.md),
[selected PCB feedback](0035-selected-pcb-feedback.md),
[TM recovery publication](0031-ims-tm-recovery-publication.md),
[IMS programming status](../delivery/subsystems/ims/programming-status.md), and
[transaction participant contract](../contracts/TRANSACTION-PARTICIPANT-V1.md).
Backout composition also consults the pinned IMS 15.6 SETS/SETU, ROLS, ROLB and
ROLL calls and apg/ims_backingoutintermediate.htm. These source-review pins grant
zero execution or licensed credit.
