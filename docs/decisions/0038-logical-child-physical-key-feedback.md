# ADR-0038: Real logical child physical-path key feedback

Status: **Proposed**
Owner: **IMS provider maintainers**
Scope: **IMS-1403.logical-child-physical-key-feedback**
Applies from: **mainframe-env current subsystem contracts**

The selected PCB feedback projection currently rejects every database that
declares a logical relationship. Its existing physical traversal can already
retrieve a real child and concatenate data from its exact retained logical
parent, including a non-root parent. The selected source occurrence still has
an engine-owned physical hierarchy and metadata sequence fields.

Admit successful DbBatch typed Get/Get Hold through a primary HIDAM G/AP PCB when
every source ancestor and selected record has a fixed length and unique
sequence field. A real child endpoint must have exactly one deduplicated
forward declaration, one validated occurrence link and an actual physical
parent. Root-link metadata does not establish the admitted physical-child
ancestry and retains Unsupported; no root logical-key rule is inferred.
Resolve that link's
destination by its retained database/record identity and confirm its segment
and fixed-length physical path. An ordinary physical path that does not cross
a logical child uses the same source recipe in a database with relationships.
The key consists only of the actual source ancestor and child sequence bytes.
Do not derive it from transferred concatenated data or destination keys.

IBM IMS 15.6 pinned authority, with prefix
`SSEPH2_15.6.0/com.ibm.ims156.doc.`:

| Baseline / topic / locator | SHA-256 |
|---|---|
| database-contracts-2026-09-11, apg/ims_processinglogicalrelationships.htm:39–60 | ae5f2c81859d6631c28380eaa7d6744735b16ba99c9337cdd701f9d54bc0bfee |
| metadata-contracts-2026-09-11, sur/ims_segmstmt.htm:683–742, especially 730–732 | 014bf18fd4c89941bd6bafe3dccd66e607daf0523e618648334ca251eb2824d9 |
| programming-contracts-2026-09-11, apr/ims_ccmdcode.htm:1–56 | 038fcdaa210493f4c423ae2b0bc7fbbbf8bd381146a8ad25cf7615d28174d65a |
| programming-contracts-2026-09-11, apg/ims_imsdbdbpcbmask.htm:1–187 | 699a551e0c2804db26725d0997be3b1f9fdc91379a69f490c8e509d76fcc61b3 |
| metadata-contracts-2026-09-11, sur/ims_psbgensensegstmt.htm:1–179 | baf8ed5e7ad87faf1ba02801da3479385ba5b4b5fc74474ca051d32d3014ebf4 |

Full baseline names start `ibm-ims-15.6-`. Selected retained topic_path files
were absent; archive bytes matched manifest hashes before repository
plain-text parsing and actual ibm_docs.py search/read. The unchanged bounded
audit supplies exact source receipts; no refresh or publication body is added.
Catalog `ibm-ims-15.6-dli-2026-08-31:dli-call-families` rows 0005/0006 use
`html-table:comparison;row:<n>;command`; inherited maintenance and recovery
rows retain their existing bindings and pending official dispositions.

SEGM SOURCE distinguishes physical-twin and logical-twin sequence fields by
direction. The owned metadata has no SOURCE alias, virtual layout, reverse
sequence or destination KEY/DATA/RULES recipe. Those classes remain
Unsupported, as do multiple links, unkeyed/nonunique or variable-length
logical paths, other organizations/contexts/PCB options, logical ISRT feedback and
failed-call witnesses. Unsupported is a source gap, not a guessed IBM status.

The only production change is the admission recipe in service/feedback.rs.
Existing SSA/C selection, sensitivity, source/destination SAF, integrity/foreign
undo, Q fences, correlated key maintenance and proposal/replay/CAS remain
owners. Feedback is computed from the same unpublished proposal and published
atomically with the existing result and position; capacity failure discards
that proposal. No second cursor, store, checkpoint variant or shared authority.

Historical Unsupported receipts replay verbatim rather than gaining new keys.
No canonical, host, SQL or envelope migration occurs. Checkpoints continue to
save the real source physical path through the existing recovery owner.
Downgrade to the base merely restores Unsupported for fresh calls; inherited
strict reader, selected-package, undo and coherent backup restrictions apply.
ADR Proposed is within this user-authorized leaf, not an external approval.
Local tests and the leaf seal never complete parent ims.programming or official, HUMAN,
participant or licensed acceptance.
