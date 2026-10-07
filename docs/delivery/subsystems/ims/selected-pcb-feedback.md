# Selected database PCB feedback: bounded contract and class review

Slice: **IMS-1401.selected-pcb-feedback**, parent IMS-1401.
Base: `a01659799de8bf2291511714ac224279619a8571`.
Branch: `codex/v014-selected-pcb-feedback-20261002`.
The prior official-IR candidate remains preserved at
`6f1bd076a2d0fee70ed34c7946fc6917416ada79` on its original branch.

The additive V1 typed/provider and signed selected-package route publishes
owned feedback from the existing proposal. It preserves all historical
canonical bytes and replay authorities. Supported organizations are bounded
HDAM/HIDAM/HISAM/SHISAM metadata in database contexts (DB batch, DB/DC, DBCTL);
organization/context admission is not proof of every IMS execution environment.
The public entry points are `ImsService::execute_pcb_feedback_v1`,
`ims_providers`, and `ProductServer::ims_pcb_feedback_selected_v1`.
The host request uses `host.ims.write` because retrieval changes PCB state.
All inherited participant/context/security composition limits remain explicit.

## Owned response and independent expectations

`ImsPcbFeedbackResultV1.result` retains the existing status/data result.
`feedback` carries the selected PCB number, normalized database name, PCB
PROCOPT, sensitive-segment count, transferred data length and key availability.
Valid keys carry segment name, hierarchy level and exact current key bytes.
Their valid length is obtained with `valid_length()`; no stale tail bytes are
returned. Transferred length counts owned result data bytes, not PCB area
capacity, stored segment length or a physical ABI field.

The mask rule requires ancestor keys followed by the selected segment key.
Independent metadata fixtures therefore expect ROOT key `A1`, CHILD key `C1`
and data `C1Z` to yield level 2, key `A1C1` (length 4), data length 3.
A D path returns ROOT `A1X` plus CHILD `C1Z` (data length 6). With CHILD
SENSEG K, the same path's data length is 3; direct key-only CHILD retrieval
has data length 0 while retaining `A1C1`. Independent PCB 2 at ROOT `B2`
cannot change PCB 1's key, cursor or hold. These are literals derived from
metadata and verified rules, not observations generated from the handler.

## Exact missing classes

| Class | Current response / prerequisite | Owner and next bounded step |
|---|---|---|
| Failed GU/GN/GNP/Get Hold, including GE/GB/GP, specification and mutation failures | Status and transferred length remain exact; key/name/level are `Unsupported(FailedCallWitness)`. The engine returns only a view or error plus retained position, not the last path level satisfied by the current search. Old cursor position is not that witness. | Navigation owner must add a bounded last-satisfied proposal output and independently test partial-path failures. No repeated search or inferred cursor witness is admitted here. |
| Selected secondary retrieval/ISRT feedback | Existing navigation returns the selected target's data (source pointer and target may differ); key/level/name are `Unsupported(SecondarySequence)`. Reviewed sources do not establish exact secondary-sequence feedback layout for the admitted pointer/ancestor shape. | Secondary/source owner must supply an exact authority-backed feedback recipe in its proposal. Rich secondary SSAs remain Unsupported on this manager base; newer worker code is not consumed. |
| Successful secondary REPL | Key feedback is explicitly invalidated by the pinned source; no old bytes or valid length are exposed. Parentage and holds keep their existing owner. | No secondary algorithm change or parentage equivalence claim is made by this projection. |
| Primary REPL and DLET feedback | `Unsupported(NonKeyOperation)`; neither reused previous bytes nor guessed invalidity is published. Status/affected count retain existing semantics. | Obtain exact per-call validity rules before extending the projection. |
| Metadata without a sequence field; logical classes outside the physical recipe below | `Unsupported(MissingSequenceField)` or `Unsupported(LogicalRelationship)`; no zero-filled or guessed concatenated key. | Obtain the missing direction/layout/key authority before extending feedback. |
| GSAM, DEDB, MSDB and other organizations; raw EBCDIC COBOL/C masks, reserved linkage, scheduled database-type SEGNAME, KEYLEN capacity | Route/physical ABI remains Unsupported/unproved. The current PCB metadata has no declared KEYLEN area size or reserved/raw framing source. | Their existing owners must provide source/ABI prerequisites; do not synthesize zeros, a raw mask, physical RSA or execution credit. |

Unavailable invalidation semantics are distinguished from proved invalidity.
Only `InvalidatedSecondaryReplace` asserts invalidity. Every Unsupported
variant has no valid byte length. This packet does not close the full PCB
feedback matrix or any official comparison row.

## Real logical child through its physical source hierarchy

Leaf `IMS-1403.logical-child-physical-key-feedback` narrows the former
database-wide logical guard for DbBatch typed Get/Get Hold through a primary
HIDAM PCB with G/AP. Every record on the actual source path must be fixed-length
and uniquely keyed. The selected real child has one deduplicated forward declaration
and one validated occurrence link, with an actual physical source parent;
root-link key semantics remain unproved. Its exact destination record and fixed-length
path must agree. A logical child in an ancestor position, multiple links,
unkeyed/nonunique/variable paths, other logical organizations/contexts and
logical ISRT feedback and other PCB options retain Unsupported. Ordinary physical
retrieval in a database with relationships uses the same proved recipe when its path crosses
no logical child. Selected secondary keys and failed witnesses stay unproved.

ROOT(A1) -> CHILD(C1) linked to DROOT(P9) -> LPARENT(L2) returns source key
`A1C1`, CHILD, level 2. Another LPARENT(L2) under Q8 has different data; the
retained link determines which parent data is transferred. Neither `P9L2`,
`Q8L2` nor concatenated result data supplies source key bytes. A physical C SSA
and equivalent qualified source path select the same child. Child K sensitivity
returns its key with no child or destination data. Source/destination SAF
precedes observation for K and replay. Fresh reads retain the existing integrity,
foreign-undo and Q fences; exact replay validates the retained receipt and
returns it without refreshing or reprojecting the live occurrence.

The projection reads the same unpublished engine occurrence as navigation.
Capacity failure discards it; atomic publication, per-PCB hold/parentage, Q,
foreign pending undo and correlated maintenance remain existing owners. Exact
historical `Unsupported(LogicalRelationship)` receipts replay without new keys.
The existing symbolic checkpoint saves source physical-path position alongside
primary/secondary/GSAM PCBs; feedback adds no checkpoint variant or cursor.

Source identities and direction limits are in
[Proposed ADR-0038](../../../decisions/0038-logical-child-physical-key-feedback.md).
The processing-logical pin establishes non-root logical parents; SEGM SOURCE
683–742, especially 730–732, distinguishes physical-twin and logical-twin keys.
The physical C and PCB mask pins define this admitted source recipe. Metadata
has no virtual layout, SOURCE alias, reverse ordering, destination KEY/DATA or
SEGM RULES; this leaf does not supply those missing recipes or raw mask framing.
No host/canonical/SQL/receipt migration occurs. Inherited ADR-0035 downgrade and
coherent-backup requirements apply; base rollback restores Unsupported for fresh
logical calls. Parent ims.programming and official/HUMAN/licensed/participant acceptance
remain incomplete.

## Pinned offline authority

All topics below are IMS 15.6. Check the retained `topic_path` under
`/Users/tore/Library/Application Support/mainframe-env/ibm-docs-html` first;
all selected files were absent there. Exact archive bytes under
`/Users/tore/Library/Caches/mainframe-env/ibm-docs-archive/raw/html/sha256`
matched the committed hashes and byte counts and were parsed by the repository
plain-text parser and `python3 -B conformance/tools/ibm_docs.py search/read`.
The relevant TOC matched
`aaa12586b41e9994921bfddce588b186dc5bdda8ab253db054ae1e5014d6f618`.
No selected registered topic was unavailable or mismatched; no refresh,
publication body in Git or licensed execution occurred. The partial reader's
unselected missing entries are not a whole-cache finding.

Programming baseline: `ibm-ims-15.6-programming-contracts-2026-09-11`.
Topics below use prefix `SSEPH2_15.6.0/com.ibm.ims156.doc.`:

| Topic suffix | SHA-256 |
|---|---|
| `apg/ims_imsdbdbpcbmask.htm` | `699a551e0c2804db26725d0997be3b1f9fdc91379a69f490c8e509d76fcc61b3` |
| `apr/ims_gughucall.htm` | `0a9b433d9e38e58c125232a94147acd309e5bbbd7baf3c8cb900a3e8fdf6c8b9` |
| `apr/ims_gnghncall.htm` | `063ff108614ee13694ea7df2f7b39647447059590162614da56de2aa2eb49cb4` |
| `apr/ims_gnpghnpcall.htm` | `6daaf5929bf3640a6d4ab17ea97d81328e60b26eb38c32b2c991c77d41592762` |
| `apg/ims_currentpos.htm` | `07aafcddb9b30591eef1da5ad50bfe8bc3c1e9b9e9e51b56b27d39bf6adf5673` |
| `apg/ims_processingoptions.htm` | `bb549e17230c1990ac0b5b5f0386512493bcd5552b3b4762bd7fc4436a923dd4` |
| `apg/ims_comparingcmdcodesandopts.htm` | `eec570be49de991fe17b672c5e81dc4a83733d66267d3e0b561502e8820854ec` |
| `apr/ims_cmdcodref.htm` | `cb1d0cc0cf765f13836b888efaea5538bcbcbbd0453028388b07f69736946093` |
| `mc/compcodes/ims_dlistatuscodestables_databasecalls.htm` | `2b41e1415ac50690e8ace7761263ef304a0ffd512e6d0beee088356fa439aa7f` |

Database baseline: `ibm-ims-15.6-database-contracts-2026-09-11`.

| Topic suffix | SHA-256 |
|---|---|
| `apg/ims_secondaryindexlogicalrelationships.htm` | `e924f4e2c9c336d4ecdc8a6e2ccf4ff0369b0ab2332788ca015e215c14d5bd53` |
| `apg/ims_ssassecondaryindex.htm` | `4b3a1ee3cc0eabbbb4984d23a0eb60fe93be50887e1853643e0f382710139900` |
| `dag/ims_insertdelseg.htm` | `6a048f9b794ed466655100a69892afceed54805bbdc9e3329f5843a2f38295d5` |
| `dag/ims_replacecall.htm` | `55778b03e47f21e92fb967995ec54b7ff1903c7886b98bdb1394d6d85e8fd23a` |
| `dag/ims_issuedeletecall.htm` | `f40ecf698e4817f47ca8a4abd5214baa880476393c4f765a5c215051acdc6a23` |

Metadata baseline: `ibm-ims-15.6-metadata-contracts-2026-09-11`.

| Topic suffix | SHA-256 |
|---|---|
| `sur/ims_psbgendlipcbstmt.htm` | `0dad54edd1a9940ca9a6988e06412836ff35a706cc2e1f7fbd36eb14fa02cfba` |
| `sur/ims_psbgensensegstmt.htm` | `baf8ed5e7ad87faf1ba02801da3479385ba5b4b5fc74474ca051d32d3014ebf4` |

Catalog context remains `ibm-ims-15.6-dli-2026-08-31:dli-call-families`
`:0005` (GU/GN/GNP), `:0006` (Get Hold), `:0008` (ISRT), `:0015`
(REPL), `:0004` (DLET). Mandatory local classes are those declared in the
progress record, not new official obligation IDs or a reduced denominator.

## Integration and verification handoff

Shared integration seams are host enum/export and canonical dispatch arms;
borrowed existing SSA validation; shared execution output and request digest
selection; the optional existing receipt output and retention result-family
validation/hash; provider dispatch; visibility of the existing PCB metadata
selector; the signed product facade; test module declarations; the mechanical
HostResult declaration move and lowered request-module inventory; normal docs
and unique fragment. All cursor, SSA parser, index, GSAM, recovery, STAT, TM,
backout, participant, store, lock and coordinator algorithms are unchanged.
Newer secondary SSA, GSAM checkpoint and backout leaf owners remain separate.

Receipts are external under
`/Users/tore/Library/Caches/mainframe-env/worker-receipts/v014-completion-20261002/IMS-1401.selected-pcb-feedback`.
`fail-first.log` records absent public DTO/request/result variants.
`feedback_goldens.py` independently encodes the documented binary schema:
request 685 bytes, SHA-256
`47f158c3d7ab302b61a07f9032149a79896348ab41a65c1a68b153af46b7ee06`;
result 760 bytes, SHA-256
`d9c22d43f47036fc6da003ea6d9bda6d61c634caba09c6072c909d905e617cc6`.
Old IMS/SSA/GSAM/STAT vectors remain unchanged. Source receipts grant zero
execution credit; passing local tests grant zero official/licensed credit.

The affected host and IMS packages and signed package regressions passed,
including a separate SQLite process boundary and the real coordinator.
Strict scoped no-deps Clippy, fmt, deny, catalog/schema/assurance, spec and
affected shared architecture/module guards passed. Receipt names are
`host-acceptance.log`, `ims-acceptance.log`, `package-acceptance.log`,
`clippy.log`, `catalog.log`, `assurance.log`, `spec.log`, `deny.log`,
`fmt.log` and `shared-guards.log`. The last also retains the additional
public-API documentation ratchet failure: untouched execution API exceeds
176 and the existing host surface exceeds 1128. New DTO items are documented;
that policy is unchanged and full-phase documentation acceptance is pending.
Normal docs/changelog and generated completion seal receipts complete the
handoff separately. Test/source receipts retain their original input identity;
the seal is a content check and does not turn them into committed CI evidence.

Compatibility and downgrade procedures are in
[ADR-0035](../../../decisions/0035-selected-pcb-feedback.md). Parent IMS-1401,
complete-ims.programming, participant, full-phase integration, official/maintainer IR,
licensed differentials and release acceptance remain open. No push or PR.
