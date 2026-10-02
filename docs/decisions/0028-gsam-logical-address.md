# ADR-0028: GSAM logical record addresses on the owned host route

Status: **Proposed within the human-authorized bounded IMS implementation**
Owner: **host-contract and IMS maintainers**
Scope: **IMS-1403.gsam-record-addressability; IMS-1405.gsam-checkpoint-restart**
Applies from: **mainframe-env 0.14.0 development**

IBM IMS 15.6 defines an RSA as an access-method-dependent physical record
address, previously returned by GN or ISRT. The existing GSAM engine stores
bounded logical records and does not own a BSAM/VSAM physical data-set layout.
An ordinal, full-function hierarchy key or invented RBA cannot represent that
IBM contract. Physical operational parity is outside the programming plan's
comparison boundary, but application-observable behavior still requires a
reviewed adapter and licensed differential; this implementation claims neither.

The additive `ImsGsamRequest` and `ImsGsamResult` use an opaque 32-byte host
token plus the normalized database identity. GU accepts only an issued record
address or a typed Beginning operand, the owned equivalent of the source's
reset sentinel. GN/ISRT optionally return a saved address. Exact bytes, append
order, independent selected PCB positions, direct lookup, GB and its next-call
restart at the beginning, AH for missing GU address, and AJ for invalid/stale/
wrong-database address are bounded local contracts. Invalid GU addresses leave
data and position unchanged; the precise IBM invalid-address position is not
established by these pins and is not claimed. Beginning clears position and
returns no record; raw sentinel I/O-area behavior is unproved. GSAM has no
hierarchical parentage, keys, SSAs or Get Hold on this route.

Only `DbBatch` (standalone DL/I batch), G/GS retrieval, L/LS append and fixed
length metadata are admitted. The broad existing context enum cannot prove
BMP/JBP region identity; other contexts fail Unsupported. RECFM V/U, LL/RDW,
undefined PCB length, raw 8/12-byte RSA layouts, INIT RSA12, BSAM tape/DASD
selection, volumes/concatenations, OPEN/CLOSE/PURGE, physical I/O errors and
raw PCB key feedback remain unsupported. The engine's single segment name is
host metadata for a record shape, not an IBM GSAM segment-name observation.

Identity lives on the existing engine record as optional `gsam_address`.
Issuance hashes a dedicated domain, canonical request identity, current shared
database row CAS version and bounded engine identity. It is never derived
from Debug text or a guessed ordinal. Append preserves existing identities;
rollback/removal/image replacement invalidates identities of removed records,
including identical data reinserted at a reused internal occurrence. First
saved-address GN on a historical record materializes identity through the same
UOW witness and atomic row publication as other image changes. This conservative
host metadata mutation may acquire the existing database UOW fence; it is not
an IBM record-lock claim. There is no address registry, second store or recovery
engine. No-save GN needs no identity materialization; ISRT assigns an identity
even if its optional output is absent.

Historical absent record fields read as None and serialize without that field.
Existing request/result canonical encodings and no-GSAM replay payloads remain
unchanged. A GSAM replay adds optional `gsam` output to the existing receipt,
and retention validation hashes the correct additive HostResult variant.
Addresses are bounded by the existing record limit, persist with their live
record, and do not expire when replay retention prunes an unrelated receipt.
Removed addresses can remain in completed replies but lookup returns AJ.
No SQL migration or retention authority changes. Before downgrade, stop new
GSAM dispatch, settle/drain active UOWs, and preserve compatible image/replay/
checkpoint backups. Older writers can drop record identity fields and older
strict replay readers reject GSAM output; concurrent/downgrade writes are not
supported. Never rewrite historical receipts or silently retry unknown outcomes.

Recovery stays with the existing `CheckpointRequest`/`SavedPcbPosition`,
`RecoverySession::xrst` resolver/transition and selected per-PCB helpers.
Basic CHKP cannot checkpoint GSAM according to the pinned source. The typed
CHKP/XRST adapter extends the existing saved-position interface with an optional
discriminated GSAM logical address, beginning, EOF or integrity-checked output
prefix; the full-function `segment_key` is empty for this case. Symbolic CHKP
materializes any required live identities before saving the position and commits
actual pending writes while releasing PCB position. XRST resolves input through
the existing direct address lookup and retains the next GN position. A witnessed
unsettled output suffix is removed after validating its live prefix. Surviving
identities and monotonic occurrence allocation remain intact. Missing/replaced
prefixes fail; later committed output without an ownership witness returns
UnknownOutcome without erasure. Mere settlement or row-version advancement does
not invalidate a retained live address. Session/status/database/UOW/recovery and
selection fences publish in the same atomic transition. Replay after later work
returns the original receipt without reapplying CHKP or XRST.

The logical output-prefix operation models repositioning within this owned
record image. It is not proof of physical BSAM file truncation or permission
to restart unsupported VSAM loading, temporary datasets or raw format paths.
The specific GSAM checkpoint pin requires symbolic CHKP/XRST and describes
BSAM/output dataset qualifications; those physical applicability obligations
remain open. DbBatch fixed-length G/GS and L/LS are the admitted host projection.
Root timestamp/day/region authority remains absent; logical ticks cannot supply
IBM's timestamp identity. The tests cover Memory/file SQLite, independent PCBs,
multiple files, real CAS races, lost acknowledgments and process exit/reopen;
they grant no official, maintainer or licensed checkpoint credit.

Absent `gsam` saved-position fields preserve historical checkpoint digest bytes.
New GSAM checkpoint rows require a compatible reader; older readers/writers are
not admitted concurrently. No SQL migration is required. Drain admission and
active UOWs, reconcile ambiguous effects and take a coherent backup of images,
sessions/checkpoints/recovery, selected metadata, journal and audit before
rollback or upgrade. A coherent backup restore/retention expiry exercise remains
a parent obligation; successful reopen is not a backup certification.

Sources: IMS 15.6 database baseline
`ibm-ims-15.6-database-contracts-2026-09-11`, topics
`ims_retrieveinsertgsamdb.htm`, `ims_processinggsamdb.htm`,
`ims_gsamstatuscodes.htm`, `ims_gsamrecordformats.htm`; programming baseline
`ibm-ims-15.6-programming-contracts-2026-09-11`, `ims_pcbmaskgsamdb.htm`;
recovery baseline `ibm-ims-15.6-recovery-utilities-2026-09-11`,
`ims_symbolicchkpcall.htm`, `ims_xrstcall.htm`, `ims_basicchkpcall.htm`.
Separate zero-credit GSAM recovery baseline
`ibm-ims-15.6-gsam-recovery-2026-09-11`,
`SSEPH2_15.6.0/com.ibm.ims156.doc.apg/ims_gsamsymbolicchkpandxrst.htm`,
SHA-256 `ebc695770a487c4fd3b9b06fa0d6948353a1f6543c75e29ec45e94048066616c`.
Exact committed hashes and topic-set identities are recorded in the IMS status.
Catalog context is `ibm-ims-15.6-dli-2026-08-31:dli-call-families:0005/:0008`
and checkpoint/restart `:0002/:0023/:0016/:0025`.
Source review and local tests grant no official row, licensed, participant,
parent-work-package or release credit.
