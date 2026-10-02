# IMS mixed Boolean SSA acceptance gap

Historical prerequisite packet retained from sealed bdbca36. The original
missing-registration and verification dispositions below describe that
candidate. Before integration, the worker read the four exact supplemental
pins offline; evaluation is a separate feature and seal. Original receipts
remain bound to their original candidates.

Disposition: **mixed evaluation pending source authority; local fail-closed
guard only**. Leaf `IMS-1401.mixed-boolean-ssa`, parent IMS-1401, base
`b7068757a3af472579c2e493f2ff9eb1a4909a66`. No official, maintainer or licensed
credit; no parent or release completion. This independently authored packet
defines the missing claims and discriminating acceptance classes. It does not
derive expected mixed results from the current matcher.

## Verified rules and missing claims

IMS 15.6 programming baseline
`ibm-ims-15.6-programming-contracts-2026-09-11` establishes these bounded rules:

| Topic under `SSEPH2_15.6.0/com.ibm.ims156.doc.apg/` | SHA-256 | Established claim |
| --- | --- | --- |
| `ims_ssas.htm` | `7a0fb0faa50c4308924576ebbe6bfcca0dd78e217f33c59e78480b9c6dd8ae20` | Binary field comparison, independent of declared data type; fixed field/value lengths |
| `ims_ssacodingrules.htm` | `cfb772b7ae68ea657006d135792441a4da20185c2c8833389171c76432c0fba5` | Two-byte relations; dependent AND `*`/`&`, OR `+`/`\|`, independent AND `#`; one outer qualification boundary; values exclude parentheses |
| `ims_ssacodingformats.htm` | `dd8afe24c819750ec361d8359529ad328c4ef280235553da7bdd918be28504d9` | Contiguous fixed-width framing; syntax adapters remain outside this logical route |

The secondary baseline `ibm-ims-15.6-database-contracts-2026-09-11`, APG
`ims_ssassecondaryindex.htm`, pin
`4b3a1ee3cc0eabbbb4984d23a0eb60fe93be50887e1853643e0f382710139900`,
establishes selected XDFLD qualification and pointer-to-target retrieval. It
does not establish the mixed-qualification rules for that access sequence.
Catalog context remains
`ibm-ims-15.6-dli-2026-08-31:dli-call-families:0005/:0006`; inherited update
interactions are :0004/:0008/:0015. Source review grants zero execution credit.

The hash-verified SSA overview links to the first missing topic below. The
verified IMS TOC (`aaa12586b41e9994921bfddce588b186dc5bdda8ab253db054ae1e5014d6f618`)
locates the other three. None has a registered body pin in this checkout:

| Missing APG topic under the same product/version | Claim requiring review |
| --- | --- |
| `ims_multiqualificationstmts.htm` | Dependent versus independent AND semantics, precedence with OR, implicit group boundaries and evaluation order |
| `ims_examplmultiqualificationstmts.htm` | Worked independent outcomes for more than one clause/group and connector ordering |
| `ims_multqualificationstmtshdam_phdam_dedb.htm` | Organization-specific restrictions and permitted search shapes |
| `ims_multqualificationstatementssecondaryindex.htm` | Mixed qualification restrictions and evaluation with selected index fields and physical fields |

Expected body SHA-256 and byte counts are **unavailable**, not guessed from a
filename, TOC locator or an unpinned archive body. The retained topic-path root
was checked before this disposition. No missing body was fetched or repinned.
The sixteen selected existing body pins matched archive SHA-256 and byte counts,
after absent retained topic-path files, and were read through `ibm_docs.py` and
its `plain_text` parser in an explicit bounded external reader cache. Other
scope entries omitted from that cache are outside this review, not newly
reported source failures. No contradictory rule was found; composition claims
are absent from the reviewed body set.

## Independently specified acceptance classes

Use the single existing rich SSA parser/AST/matcher and both public routes:
`ims_providers` and signed `ProductServer::ims_navigation_selected`. Preserve
all existing per-PCB cursors and publication owners. The following future
evaluation cases have **pending expected outcomes** until exact body pins are
reviewed; none is an ignored passing test or accepted official obligation.

| Class | Discriminating fixture / required observation |
| --- | --- |
| OR before dependent AND | Three predicates with truth values true, false, false; distinguish OR-of-AND grouping from left-to-right evaluation |
| Dependent AND before OR | Three predicates with truth values false, true, true; establish the source-defined combination independently |
| Independent AND with dependent AND | Test both connector orders, field repetition and different fields; establish what the distinction means instead of treating both as uniform AND |
| Independent AND with OR | Test both orders and multiple alternating groups; derive group boundaries from source rather than adding general nested parentheses |
| Both connector encodings | `*`/`&` and `+`/`\|` must retain their documented identities; `#` must retain its distinct identity |
| Binary relations and lengths | EQ/NE/LT/LE/GT/GE over 00/80/ff and connector-valued bytes; values consume DBD length, never delimiter scanning, numeric conversion or collation |
| Selected composite sequence | Primary order A then B; selected order B then A from a distinct child source; XDFLD bytes concatenate nonadjacent child fields while physical predicates inspect the target |
| Primary hierarchy / path | Multiple segment SSAs, C/D/P/O, GNP qualification and all six Get/Get Hold calls; preserve exact output, current/parentage/hold and status |
| Conditions and publication | Malformed/forbidden contexts, bounds, sensitivity, SAF, no mutation on rejection; exact replay/conflict, atomic actual CAS and lost acknowledgement |
| Durable continuation | Memory and file SQLite fresh-reader and independent child-process observations; old canonical and retained bytes remain exact |

Current executable packet tests preserve distinct connector AST identities and
binary byte boundaries, and enforce **local Unsupported** before mutation for
mixed connector identities on primary and selected secondary sequences. This
is a conservative local admission rule, not an IBM-documented failure status.
The fail-first guard specifically catches dependent AND plus independent AND
silently succeeding as uniform AND. Uniform connector forms and documented
encoding aliases retain existing behavior. Explicit nested parentheses remain
malformed; their validity is not established by this packet.

## Compatibility and next step

No public DTO, AST shape, canonical encoding, retained row/schema, index
maintenance or publication algorithm changes. Existing replay bytes remain
untouched. Previously completed mixed-AND requests may now fail admission
before replay; do not interpret that rejection as proof of nonpublication or
automatically redispatch under another key. Drain/reconcile those effects before
upgrade or downgrade. Binary rollback restores the earlier admission defect;
it supplies no mixed-Boolean guarantee. Existing extended-index/undo/recovery
compatibility restrictions from the previous handoff still apply.

The leaf seal certifies completion of this gap packet and its bounded local
guard/tests only. Mixed evaluation, accepted official IR bindings and licensed
differential remain pending. The next smallest action is an explicitly
authorized registration of the four exact topic bodies and their reviewed
claims, using retained matching bytes where available. Any network/browser
refresh needs a separate user request. Then replace the guard through the same
parser/AST/matcher, author independent outcomes and execute this matrix; do not
create another parser, cursor or evaluation engine.
