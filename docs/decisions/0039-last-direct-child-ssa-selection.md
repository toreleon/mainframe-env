# ADR-0039: Last remaining direct child SSA selection

Status: **Proposed**
Owner: **IMS provider maintainers**
Scope: **IMS-1403.ssa-last-direct-child, bounded local runtime leaf**
Applies from: **mainframe-env 0.14.0 development**

The existing SSA provider rejects L. Admit only DbBatch GNP/GHNP with one
unqualified terminal direct-child SSA and exactly one active L command. The
selected PCB must use primary HIDAM with exactly two physical hierarchy levels,
one child type, unique named sequence keys and fixed-length root/child records.
Existing root parentage and a live current root or its direct child are required.
Both metadata and engine validate the actual path before proposal publication.
Exclude selected database participation in any logical declaration anywhere in
the existing catalog, plus any retained engine logical links. Declaration hosting
in another database does not establish an ordinary physical access path.

Use a private call-local First/Last policy inside the sole existing navigation
loop. Last examines the remaining forward parent-bounded interval and selects
its final matching occurrence. It does not restart at the parent or traverse
backward. Ordinary Unique/Next/NextInParent first-selection failure behavior is
unchanged. From A1/C1, L selects C3. Already at C3 or under an empty A0, a fresh
attempt returns GE with no segment, retains the existing current anchor and root
parentage, and cancels that PCB's old hold. Repeated fresh attempts remain GE;
ordinary GNP remains GE and ordinary GN continues in its existing physical order.
In the independently bounded A0/B2 fixture, GN reaches B2Y without L crossing it.

Root parentage is owned by the existing position, without a new GU-history
marker. GU/GHU, GN/GHN or a preceding P call can establish it. The new L request
contains no P. Only successful GHNP creates a real versioned hold; an attempted
same-PCB get cancels the old hold through the existing navigation owner. Another
PCB stays untouched. AC/AM remain source-defined condition receipts with normal
observer/canonical publication and unchanged cursor, hold and database contents.
Unsupported shape, pre-validation rejection, SAF denial and failed atomic
publication retain their existing stronger no-mutation boundary.

Reuse the existing proposal, CAS, authorization, undo, canonical replay and error
mapping owners. Exact retained replay returns historical data without evaluating
the cursor or recreating a hold; changed raw bytes, PCB or execution context
conflict under existing identity. CHKP commits undo and clears positions/holds.
XRST restores the saved child through real GU, establishing child parentage with
no hold. Fresh L stays excluded until a real root GU reestablishes root parentage.
Deletion invalidation remains with the existing engine/PCB owners; unresolved
or deleted anchors cannot manufacture root authority.

The source-review baseline is
`ibm-ims-15.6-ssa-last-position-2026-09-11` (`ims-ssa-last-position`):
`ims_lcmdcode.htm` SHA-256
`fe03a71d0cfddca1a822c6560aa92be8b3522e54a20ac4026a33b22382949610`,
plain-text lines 8–24;
`ims_currentpossuccess.htm`
`6efede425fe9eedf95238368fce603298dd2287f24e9b61313df91cae570d64f`,
lines 45–52, 63;
`ims_currentpostafterfail.htm`
`992855fdf264472dfc4e40d28dd81cfce9429feb1b9712e0f4a23bc0b8898726`,
lines 39–78, 108–140. Forward no-match anchoring is a manager-reviewed source
composition, not a verbatim IBM L failure example or accepted conformance rule.
The unchanged programming-contracts baseline supplies `ims_gnpghnpcall.htm`
`6daaf5929bf3640a6d4ab17ea97d81328e60b26eb38c32b2c991c77d41592762`,
lines 74–114 (including GU/GN/P at 94–111), and unchanged GU/GHU/GHN hold
excerpts. Catalog context remains `ibm-ims-15.6-dli-2026-08-31` rows
`dli-call-families:0005/:0006/:0015/:0004/:0002/:0023/:0025`.

Root prefixes (even matching), qualified child, GU/GN L, root L, deeper/multiple
child types, nonunique/unkeyed levels, secondary/logical access, other database
organizations, other active commands, path/update/ISRT L and unresolved retained
positions remain excluded. Root-level qualification ambiguity stays pending.
Null slots belong to the separate manager parser leaf. The worker base retains
their old rejection; root composition admits null slots around the sole active
L while preserving exact raw request identity and all finite shape exclusions.
No public request field, persisted position marker, schema, namespace, alternate
traversal, dispatcher, coordinator, permission or shared raw CALL/TM contract is
introduced. Historical request/result bytes and cursor fields remain unchanged.

Older binaries reject fresh L; they can read existing historical cursor/receipt
shapes. Before downgrade, stop L admission, reconcile/drain effects and UOWs, and
retain an appropriate reader or restore a coherent pre-feature backup of database,
session, undo, checkpoint, replay, package, journal and audit references. Do not
rewrite live receipts or fabricate position/history to resume L. Local Memory and
SQLite proofs, including cold processes and actual CHKP/XRST, grant no PostgreSQL,
HUMAN, official, licensed, accepted-IR, full IMS-1403/IMS-1401 or v0.14 completion.
