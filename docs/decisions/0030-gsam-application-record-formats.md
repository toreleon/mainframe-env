# ADR-0030: GSAM application record formats and owned undefined length

Status: **Proposed within the authorized bounded IMS leaf**
Owner: **host-contract and IMS maintainers**
Scope: **IMS-1403.gsam-record-formats**
Applies from: **mainframe-env current subsystem contracts**

This adds resolved record characteristics to the logical GSAM route of
[ADR-0034](0034-gsam-logical-address.md). It supersedes that ADR's fixed-only
restriction for explicit compatible metadata. Record identity, one dispatcher,
selected PCB positions, engine image validation, integrity digests, row CAS,
UOW ownership and the symbolic CHKP/XRST resolver retain their existing owners.
It does not introduce a physical data-set or raw PCB adapter.

`ImsDatabaseMetadata.gsam_format` is optional, closed and explicitly versioned.
Version 1 declares F/V/U, BSAM/VSAM applicability, an owned block-size bound and
None/ASA/machine control. The existing segment's min/max describe complete
application I/O-area bounds. They do not infer RECFM, access method, JCL/DCB,
labels or physical RSA. Explicit format requires one nonkeyed record shape and
GSAM organization. Unsupported future versions fail admission. Absent format
continues to admit only the historical fixed-size public GSAM route; ambiguous
nonfixed bounds stay Unsupported. Ordinary hierarchy requests never gain a
GSAM format by inference.

For V, the first two bytes are an unsigned big-endian LL equal to the complete
owned application area. Data starts at byte 2, or at byte 3 with the declared
control byte at byte 2. There is no application ZZ field. The pinned DATASET
topic states that GSAM/BSAM adds two bytes for the physical RDW when writing.
This adapter retains LL and application bytes verbatim; it never inserts,
strips or claims to decode a physical RDW. As a conservative owned admission
bound, BSAM V requires area+2 <= declared block size and area <= 32754; VSAM V
requires area <= 32756. These project the pinned 32756 logical-record ceiling
and BSAM's added ZZ bytes. They do not emulate block allocation or DBDGEN.
Trailing storage, mismatched LL, truncation and out-of-range areas are malformed
owned calls, with no data or position mutation. A source physical AF I/O status
is not invented for a malformed host envelope.

For U, application bytes contain only data and an optional first control byte.
The source requires a separate four-byte binary PCB length: input on ISRT,
output on successful GN/GU, greater than 11 and no larger than BLKSIZE. The
owned counterpart is `ImsGsamRequest.undefined_length: Option<u32>` and the
corresponding result field. ISRT requires it exactly equal to the supplied
area length; successful reads return the exact retained vector length. U is
BSAM-only, bounded to 12..32760 and the declared block size. F/V forbid this
operand. EOF and other status replies have no length. This is an owned adapter,
not support for raw PCB layout, endianness, key-feedback or application memory.
The control byte is retained, not validated as a printer control instruction.
Historical `ImsRequest` and navigation envelopes cannot supply or return the
separate U length and reject new U data calls as Unsupported after authorization
and exact retained replay lookup. Historical canonical receipts remain replayable
without redispatch or a newly required operand.
Administrative owned image load/export retains its existing explicit byte-vector
contract and validates record areas through the same engine.

Absence is serialized by omission. Old metadata, engine definitions, records,
non-GSAM replays and saved checkpoint positions preserve their prior JSON
bytes. Historical fixed GSAM canonical goldens stay unchanged. Present U length
adds the sorted `undefined_length` field as canonical u32 to the existing GSAM
request/result object; it participates in effect identity, conflict detection,
unknown-acknowledgment replay and journal/audit integrity. GSAM replay stores
the optional output on the existing receipt. There is no length side store.

The engine validates areas both during insert and image restoration, so invalid
durable records cannot bypass the adapter. Symbolic CHKP validates format and
image, then stores a domain-separated SHA-256 identity of the explicit format
and application bounds on each GSAM saved PCB. XRST compares it with current
metadata before any output suffix removal or position restoration. Wrong or
absent identities for an explicit format fail without publication; historical
absent-format fixed checkpoints retain an absent identity. Identity includes
control, access method, block size and bounds, not only RECFM. The existing
checkpoint integrity digest covers the optional identity.

Integration retains the common `service/execution.rs` proposal and its read
authorization, integrity refresh/fence, system reservations and image/Q fencing.
Owned U length travels through the selected execution output and retained GSAM
receipt. GSAM and PCB-feedback outputs are mutually exclusive. Application
backout preparation/settlement and feedback projection run before the same atomic
publication. Generic Batch inserts retain their undo and epoch until an explicit
commit/checkpoint/backout boundary; format admission does not restore autocommit.
XRST keeps the pristine read-source snapshot for every saved PCB while validating
format identity and the existing output-suffix witness in the unpublished proposal.

New explicit-format metadata/replay/checkpoints require compatible readers.
Older strict readers can reject them; older permissive writers can drop fields.
Do not run older writers concurrently or downgrade by discarding fields. Drain
admission and UOWs, resolve unknown acknowledgments and retain a coherent backup
of selected packages, image/session/UOW/checkpoint/recovery rows and journal/audit
before upgrade or rollback. No SQL migration is required. Reopen/process tests
are local durability evidence, not backup, physical recovery or certification.

The unresolved ABI classes remain explicit followups: raw PCB/AIB U length and
key-feedback memory ownership; physical RDW/BDW and blocked FB/VB layouts;
BSAM DASD/tape and VSAM file adapters; DBDGEN/JCL/label characteristic precedence,
BASIC/LARGE and concatenations; physical RSA and device errors. A future adapter
must declare those operands and authorities, pin their exact sources, preserve
the current owned route and add literal input/output, bad/truncated descriptor,
PCB memory bounds, device error, independent-PCB, checkpoint/restart and durable
replay tests through its public entry point. Until then these classes remain
Unsupported rather than being represented by guessed min/max or raw tokens.

Source pins: IBM IMS 15.6, product `SSEPH2_15.6.0`, database baseline
`ibm-ims-15.6-database-contracts-2026-09-11` (`ims_gsamrecordformats.htm`,
`ims_retrieveinsertgsamdb.htm`, `ims_processinggsamdb.htm`,
`ims_gsamstatuscodes.htm`); programming baseline
`ibm-ims-15.6-programming-contracts-2026-09-11` (`ims_pcbmaskgsamdb.htm`);
metadata baseline `ibm-ims-15.6-metadata-contracts-2026-09-11`
(`ims_dbdstmt.htm`); new zero-credit baseline
`ibm-ims-15.6-gsam-formats-2026-09-11` (`ims_gsamioareas.htm`,
`ims_origingsamdataset.htm`, `ims_datastmt.htm`). Exact hashes are committed in
their topic manifests. CHKP/XRST retain the recovery-utilities and separate
GSAM-recovery baselines cited by ADR-0034. Catalog applicability is
`ibm-ims-15.6-dli-2026-08-31:dli-call-families:0005/:0008`, with checkpoint
consumers `:0002/:0023/:0016/:0025`. Local source review and tests grant no
official, maintainer, licensed, parent-work-package or release completion.
