# ADR-0036: Literal null SSA command slots

Status: **Proposed within the user-authorized bounded IMS leaf**
Owner: **host-contract and IMS provider maintainers**
Scope: **IMS-1401.null-ssa-command-slots, target 0.14.0**
Applies from: **mainframe-env 0.14.0 development**

The pinned IMS 15.6 command reference defines `-` as a null command slot and
permits one or more slots to reserve space for later active commands. The
existing display-code parser accepts an empty command field but rejects this
literal form. Its active-command catalog deliberately contains seventeen
letters; a placeholder does not introduce an eighteenth active behavior.

Represent the placeholder in the existing normative SSA grammar and generate
one private syntax byte through the existing contract compiler. Parsing consumes
each null slot without creating an active `ImsSsaCommand`. A separate bounded
slot count includes both active commands and nulls, so nulls cannot bypass the
configured command bound. Digits following a null slot, missing terminators and
unknown commands remain malformed. Active commands retain their existing
descriptors, subset-pointer requirements, applicability and provider behavior.

The parsed form is selection data, not a lossless source codec. The existing
owned request retains the original raw SSA bytes, including each null slot;
canonical request and idempotency conflict checks never normalize those bytes.
There is no new request/result field, cursor, receipt namespace, store, migration,
permission or dispatcher. Previously admitted inputs keep identical parsed
values and canonical encodings. The seventeen active commands and twenty-five
official call families remain unchanged. Grammar metadata digest changes are
explicit, without promoting any pending conformance rule.

Old binaries reject the newly admitted literal syntax; a downgrade must stop
new null-slot requests and use a compatible reader for their existing replay
receipts, or restore a coherent pre-feature backup. Never strip raw slots from
a retained request or recompute its idempotency identity. This leaf does not
establish raw EBCDIC CALL framing, update/path-update commands, other positioning
commands, official/human acceptance, licensed equivalence or minor completion.

Source: programming baseline
`ibm-ims-15.6-programming-contracts-2026-09-11`,
`SSEPH2_15.6.0/com.ibm.ims156.doc.apr/ims_cmdcodref.htm`,
SHA-256 `cb1d0cc0cf765f13836b888efaea5538bcbcbbd0453028388b07f69736946093`,
null summary and related-call table plus NULL discussion. Offline source review
has zero execution credit. Scope and verification remain in the
[IMS status](../delivery/subsystems/ims/programming-status.md).
