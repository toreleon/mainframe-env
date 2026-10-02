# ADR-0042: Private IMS TM output identity and local completion order

Status: **Proposed**
Owner: **IMS provider maintainers**
Scope: **IMS-1404.tm-output-identity-local-order, finite local runtime leaf**
Applies from: **mainframe-env 0.14.0 development**

Ordinary ISRT builds a group against one PCB. Express PURG makes that completed
group available before commit, and the next group on the same PCB must have a
distinct opaque identity. The old output domain binds run unit/PCB/pending ordinal.
Express output stays outside pending IDs, so repeated express PURG reuses ordinal
zero and conflicts with the first immutable output. A new session reusing the run
unit can also reuse an old identity. Neither conflict is successful publication.

The private `mainframe-env.ims-tm-output@2` domain binds the existing message ID,
message sequence, work ID, lease ID, lease epoch, run unit, PCB, observed session
CAS version and deterministic completion slot. These fields identify a started
input/work incarnation and its completion occurrence. They do not authorize a
stale lease or make publication and WorkStore settlement atomic. PURG and commit
use the same helper and the existing single provider mutation proposal.

Sequence is checked `observed session version + completion slot + 1`. PURG has
slot zero. Commit enumerates new buffers in their existing sorted PCB order with
slots starting zero, leaving earlier pending output identities, sequences and
message bytes unchanged. Every successful nonterminal call advances the session
row version by one under the existing CAS; replay and failed publication do not.
Thus a later completion starts strictly after an earlier single-slot PURG. Commit
deletes the session in its atomic proposal, so its multi-slot interval cannot be
followed by another completion in that same started incarnation. Competing
proposals based on the same version cannot both publish. All arithmetic, buffer,
row and receipt capacities fail before mutation publication. This is neither a
dense sequence nor global FIFO across sessions/PCBs, nor monotonicity after session
deletion, rollback/reclaim or a new input/work incarnation.

Express available output must remain outside `pending_output_ids`. Ordinary
commit publishes earlier pending output and completes remaining buffers; ordinary
cancel/rollback delete pending ordinary output only. Adding express IDs to that
list would give those later owners authority to rewrite or discard transmitted
groups. Existing typed TM application recovery remains Unsupported; ordinary
rollback preservation is not acceptance of ROLB, SETS/ROLS or shared recovery.

No existing outbound row, pending group, ID, canonical request/result domain or
receipt is migrated or rewritten. The same schema and opaque-key decoder read
both algorithms. Exact retained replay preserves its original result and IDs;
changed canonical bytes under the same key still conflict. The current bounded
receipt lifetime fails closed at capacity; there is no pruning or age rule.
Fresh calls require distinct existing call keys. Reusing the same key for a new
input can match an old retained canonical receipt; this mandatory lifetime seam
is not fixed by output identity. Terminal PURG replay/package routing, receipt
deletion/pruning and hidden caller lifetime protocols are outside this leaf.

Before admitting this algorithm, stop/drain old writers and reconcile unknown
outcomes while keeping all retained rows and receipts. A readable schema does not
fence an old writer algorithm. There is no rolling-writer guarantee or fabricated
algorithm-version flag. Retain the genuine old read-only reader and base-produced
fixture for compatibility checks; never synthesize historical rows with a new
writer. Downgrade writers need the same stop/drain/reconciliation boundary.
Coherent restore needs catalog/input/session/outbound/replay/work epochs/packages/
artifacts/clock together. Matching IDs or hashes cannot certify mixed images.
No backup decoder or shared restore authority is introduced.

Source baseline `ibm-ims-15.6-tm-contracts-2026-09-11`, scope `ims-tm-contracts`,
product `SSEPH2_15.6.0`, ordinary TM usage (not only Spool API):

- `com.ibm.ims156.doc.apr/ims_isrtcalltm.htm`, SHA-256
  `9b0bd68473b41b3614641637776047ba148e2f28d9d5133ec0d78cf931560a31`,
  lines 63–90 and 151–159 (grouping/transmission).
- `com.ibm.ims156.doc.apr/ims_purgcall.htm`, SHA-256
  `3b6414156c4c76da888278ff17a1a3d8ed9cedfee1068373f11c46588719e0a6`,
  lines 66–79 (one complete PCB group and next message).
- `com.ibm.ims156.doc.apg/ims_conversationrecovery.htm`, SHA-256
  `5afd0e6ec7fd527047ff0ecbb30be819cfb11fc9efa894adae232299c9fbc6c3`,
  lines 30–38 (complete express output before commit and despite termination).

Full offline selected reads are 167/132/49 lines. Catalog context remains
`ibm-ims-15.6-dli-2026-08-31:dli-call-families:0008` ISRT and `:0005` GU/GN/GNP;
PURG is supplemental, with no invented comparison row. Reference credit is zero.
The opaque identity/local order algorithm is a manager-authorized implementation
decision, not an IBM-prescribed hash format or accepted Conformance IR rule.

Local Memory/file SQLite and genuine child-process compatibility/publication
proofs grant no PostgreSQL, full backup, official/HUMAN, licensed execution or
full IMS/v0.14 completion. ADR0031/0033 remain Proposed/unanswered: rolling writers,
pruning/terminal replay, atomic lease publication, settlement, raw CALL/coordinator
TM admission, participant/shared UOW and real TM backout require their actual
owners and approvals. This private-owner ADR is not their acceptance.
