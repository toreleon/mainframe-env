# ADR-0041: First direct child SSA selection

Status: **Proposed**
Owner: **IMS provider maintainers**
Scope: **IMS-1403.ssa-first-direct-child, bounded local runtime leaf**
Applies from: **mainframe-env current subsystem contracts**

Admit only existing typed CALL/public-provider and signed-selected DbBatch
GNP/GHNP with one unqualified direct-child SSA and exactly one active F.
Null slots remain bounded raw operands under the existing parser. Require
primary HIDAM, exactly two physical levels and one child type, unique named
sequence fields and fixed-length root/child, live root parentage and a current
root or its direct child. Reuse the existing L metadata/live-path fence;
declarations anywhere in the catalog and retained engine links exclude logical
participation. Selected secondary paths and participating secondary definitions
exclude F; L's existing primary-path applicability is unchanged.

Use a private call-local first-under-parent start choice in the sole navigation
matcher/traversal. From root A1, C1, C2 or C3, F selects literal C1Q/A1, retains
root parentage and sets current to C1. Repeated F at C1 again selects C1. Fresh
GHNP obtains a real versioned C1 hold; fresh GNP cancels any prior hold. Ordinary
GN/GNP after C1 continues to C2. L still selects the last match in the remaining
forward interval; ordinary Unique/Next/NextInParent First behavior is unchanged.
Same-occurrence F satisfaction resets dependent positions in IMS. This admitted
two-level child has no dependents, so no new witness is required; deeper F is
excluded rather than bypassing that obligation.

On an empty established A0, fresh F returns GE with no segment, retains the
existing root anchor/parentage and cancels only that PCB's old hold through the
ordinary attempted-get owner. F never crosses to B2. Ordinary GN in the separate
A0/B2 graph retrieves B2Y. This finite no-match disposition composes existing
GNP and failure-position/hold contracts, not a dedicated IBM F failure example.
AC/AM remain the existing sensitivity/PROCOPT condition receipts and preserve
position/hold/database data while normal observation/replay rows may publish.
Key-only sensitivity suppresses returned data. Pre-validation shape rejection,
SAF denial and atomic publication failure preserve their existing stronger
no-mutation boundary; no HostProblem substitutes for an AC/AM condition.

SAF, canonical raw request identity, proposal/CAS, undo, replay, unknown outcome,
coordinator/package selection and CHKP/XRST remain with their existing owners.
Exact retained replies never rerun selection or recreate holds. Changed null
slots, other raw bytes, PCB or execution identity conflict through the existing
identity contract. REPL/backout and actual checkpoint composition use the real
versioned held record. CHKP clears positions/holds; XRST's physical GU restores
child parentage with no hold. Fresh F then remains excluded until a real root
GU reestablishes root parentage. Deleted or unresolved anchors cannot provide
authority. Another PCB is untouched by this PCB's attempted get.

Authority: IMS 15.6 `ims-ssa-position-commands`, baseline
`ibm-ims-15.6-ssa-position-commands-2026-09-11`, topic set
`f99728026ed7f14fcc8e104678bc55939581af2defee68385bc3bf35b170e8b9`;
`SSEPH2_15.6.0/com.ibm.ims156.doc.apr/ims_fcmdcode.htm`, SHA-256
`425e453bd01c99b47f2a513fcf74678a9b5ff84cd16c719d83696053e9e47cd0`,
7545 bytes, lines 8–33. Existing `ims-ssa-last-position` pins supply
`ims_currentpossuccess.htm` (45–52, 63), `ims_currentpostafterfail.htm`
(39–78, 108–140) and the unaffected L distinction. Existing
`ims-programming-contracts` `ims_gnpghnpcall.htm` (74–114),
`ims_gughucall.htm` (146–167) and `ims_gnghncall.htm` hold section supply
parentage/attempted-get composition. Source identities and actual offline
search/read are external. Catalog context remains
`ibm-ims-15.6-dli-2026-08-31:dli-call-families:0005/:0006/:0015/:0004/:0002/:0023/:0025`.

GU/GHU, GN/GHN, root F, any root prefix (even matching), qualified child,
deeper/multiple types, other organizations/contexts, secondary/logical
participation, unkeyed/nonunique/variable levels, mixed active commands,
update/ISRT F and unpositioned/unresolved/deleted anchors remain unfinished
required classes. No virtual/alias/twin/RULES or raw status recipe is inferred.
U/V per-level witness design is separate; ADR-0040 is reserved for that review.
Raw CALL/TM shared ADR-0031/0033 approvals remain unanswered. No public request
field, persisted schema/marker, namespace, second search pass or cursor authority
is introduced. Source/catalog/accepted IR/denominators and prior Proposed ADRs
are preserved. Source cache presence grants zero execution credit.

Historical cursor fields and ordinary receipt encodings remain readable with
unchanged canonical identities. An independently authored base-shaped ordinary
receipt is a compatibility fixture, not a licensed or captured historical run.
Older binaries reject fresh F; before downgrade stop admission, reconcile/drain
effects and UOWs and retain a suitable reader or restore a coherent pre-feature
backup with database/session/undo/checkpoint/replay/package/journal/audit
references. Never rewrite live replies or invent history to resume F.
Memory/file SQLite reopen and separate processes prove only their local routes,
not PostgreSQL, full backup/backend/official/HUMAN/licensed/participant acceptance
or completion of all F, IMS-1401/1403 or ims.programming.
