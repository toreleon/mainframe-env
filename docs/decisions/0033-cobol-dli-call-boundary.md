# ADR-0033: Raw COBOL DL/I needs an owned CALL and PCB binding contract

Status: **Proposed; raw execution blocked pending contract-owner decisions**
Owner: **compiler, interpreter, host-contract and IMS maintainers**
Scope: **IMS-1401.cobol-dli-call-boundary**
Applies from: **mainframe-env current subsystem contracts**

The bounded investigation cannot correctly admit raw `CALL 'CBLTDLI'` even
for primary full-function DB-batch GU/GHU/GN/REPL. Typed IMS operations already
use selected metadata, authorization, canonical replay, per-PCB positions,
hold/update, witnessed undo and atomic provider-row publication. Their existence
does not establish the language ABI that connects a guest PCB address to those
authorities. This proposal adds no runtime adapter or authority.

The compiler recognizes CALL through the shared HIR/operation lowering. The
interpreter's `program_effect` currently collects identifier tokens after USING,
drops BY REFERENCE/CONTENT/VALUE, and `call_codec::encode_call_values` emits
copied name/value pairs under `mainframe-env.cobol.call@1`. Every item has tag
one. The transport has lengths but no resolved allocation/offset identity,
passing mode, null/omitted distinction, alias relationships or character encoding.
`PendingKind::ProgramCall` retains target names and copies successful result
values back sequentially. It is not a transaction over all receiving extents;
it must not be assumed to provide raw overlapping-argument atomic copyout.

`DefaultProgramRouter` has four explicitly bound LE runtime services and the
ordinary installed-program path. It has no CBLTDLI ABI registration. Raw CALL
therefore produces a Program request, not an IMS request. EXEC DLI instead
uses `ims_effect`, emitting the existing typed IMS request. EXEC DLI tests
prove that distinct producer only.

The real signed CardDemo metadata projection retains DBD/SENSEG/PROCOPT and
PCB order, but drops source KEYLEN. `ImsDatabasePcbMetadata` has no declared
raw key capacity or guest entry binding. `ImsPcbLayout` supplies reviewed field
offsets once a variable area length is supplied; it cannot authenticate that
length or a selected PSB/PCB from guest bytes. `ImsResult` has status and segment
data, without all validity-tagged raw feedback. Equal masks, DBD names, variable
names, allocation order, current keys or the scheduled PCB number cannot repair
that missing identity. The now-integrated versioned typed feedback route still
marks unsuccessful key observations Unsupported and supplies neither raw key
capacity nor a guest PCB entry binding; it does not close these ABI gaps.

The proposed next owner boundary has three parts:

1. Compiler/IR/interpreter owners retain typed CALL passing modes and resolve
   operands once to existing bounded guest storage references. A versioned
   owned call frame must describe mode, direction, class, extent, encoding,
   null/omitted and alias identity. Physical pointers remain entirely within
   the checked guest adapter; providers receive no native address. Resolve
   and validate every input/output before dispatch. Plan one atomic guest
   write set, with source-proven overlap rules, before accepting any result.
   Preserve historical artifact/checkpoint/CALL payload readers explicitly.
2. IMS metadata and product-entry owners bind an admitted signed generation,
   PSB, DB-batch context and ordered PCB list to guest linkage areas. Retain
   source KEYLEN/capacity and ABI version as authoritative metadata, with
   source-specific validation. A raw mask must be recognized by its admitted
   storage binding, never its contents or program/database/segment names.
   The raw PCB's reserved field is owned by IMS, not a caller token. Batch
   PARM transport is not an IMS PCB list. CMPAT, I/O-PCB insertion, BMP and
   other contexts require their own source-backed entry contracts later.
3. Host/IMS owners supply complete validity-tagged feedback for each admitted
   successful and unsuccessful call class, using the existing selected-PCB
   cursor/engine authority. Derive current length, level, segment name and
   valid concatenated key from that authority. Preserve invalid/tail bytes
   as required; never synthesize zeros or a successful unknown key. A single
   generic ABI adapter may be selected by public function identity CBLTDLI.
   It must emit existing typed IMS/navigation requests through the shared
   capability/SAF/canonical/coordinator route, not call a private engine.

Raw execution stays unavailable until all three owners admit a coherent class.
No new public DTO, private API, PCB registry, memory engine, store, coordinator,
queue or dispatch branch is introduced by this leaf. Other workers' SSA,
secondary checkpoint and feedback algorithms are unchanged.

## Source packet and obligations

The separate five-topic zero-credit source supplement is
`ibm-ims-15.6-cobol-dli-boundary-2026-09-11`, registered as
`ims-cobol-dli-boundary` in the existing ims.programming manifest registry. It adds no
official catalog row or rule acceptance and repins no historical baseline.
All selected bodies were absent at retained topic paths and matched exact
SHA-archive bytes; they were parsed locally using `ibm_docs.py`. No network
refresh or publication body enters Git.

| Baseline / topic | SHA-256 | Boundary rule |
|---|---|---|
| COBOL 6.5 `ibm-enterprise-cobol-6.5-2026-05-31`, `lr/ref/rlpscall.html` | `fdf73c18a03049cd540efcfa652bfc7f842a16a1000c68a09a18938e2689cc4a` | Positional correspondence; default reference shares storage; modes persist until changed; linkage requires addressability |
| Invocation supplement, APG `ims_imsdbcobolapp.htm` | `49543439b7d78448832f226646b1393de857cc5290567b3d7a5f3984a0be8994` | Optional binary fullword parmcount; four DISPLAY function bytes, left justified/blank padded; entry PCB identity; bounded I/O and up to 15 SSAs |
| Invocation supplement, APG `ims_codingbatchcobol.htm` | `b128e9b82ea93b39b92e75feafaaa620cdff6552713fe6cebe7a55ecfca35f6c` | IMS passes PSB PCBs in definition order; linkage masks; GHU before REPL; optional parmcount; CMPAT controls I/O PCB |
| Programming 2026-09-11, APG `ims_imsdbdbpcbmask.htm` | `699a551e0c2804db26725d0997be3b1f9fdc91379a69f490c8e509d76fcc61b3` | 36-byte fixed prefix; status offset 10; character level; fullword feedback length; only valid key prefix, uncleared tail |
| Metadata 2026-09-11, SUR `ims_psbgendlipcbstmt.htm` | `0dad54edd1a9940ca9a6988e06412836ff35a706cc2e1f7fbd36eb14fa02cfba` | KEYLEN is the longest concatenated sensitive path key, not a guest-provided current key length |
| Programming 2026-09-11, APR `ims_gughucall.htm` | `0a9b433d9e38e58c125232a94147acd309e5bbbd7baf3c8cb900a3e8fdf6c8b9` | GU/GHU parameters and selected position/hold obligations |
| Programming 2026-09-11, APR `ims_gnghncall.htm` | `063ff108614ee13694ea7df2f7b39647447059590162614da56de2aa2eb49cb4` | GN/GHN continuation and status obligations |
| Programming 2026-09-11, APG `ims_ssacodingrules.htm` | `cfb772b7ae68ea657006d135792441a4da20185c2c8833389171c76432c0fba5` | Comparative bytes match DBD field width; binary SSA values cannot be decoded as text |

Full IMS topic paths start `SSEPH2_15.6.0/com.ibm.ims156.doc.` followed by
the APG/APR/SUR component in lower case. COBOL starts `SS6SG3_6.5/`.
Catalog scope is IMS baseline `ibm-ims-15.6-dli-2026-08-31`, rows
`dli-call-families:0005/:0006/:0015`, and COBOL
`procedure-statements:0005`. Additional selected pins, byte counts and parser
receipts stay in the external worker source packet.

The first future admitted class must prove actual compiled signed CALL GU,
Get Hold, GN and REPL with independent literal raw PCB/I/O bytes, exact encoding,
selected cursor/hold/undo and status/feedback validity. Cover optional count,
two PCBs including equal-content masks, binary SSAs, area aliasing and overlapping
copyout; short/oversized/null/malformed operands; absent capability, SAF denial,
wrong context/selection and stale storage. All errors preserve guest/provider
state except source-defined valid status updates, and no invalid output may
partially write. Prove canonical replay after later calls and Memory/file SQLite
reopen; persistence changes require substantive subprocess recovery. Fixed DB
segments do not imply message LLZZ, variable-segment LL, GSAM RDW/RSA, AIB or
all-family framing equivalence. These remain explicit unproved classes.

The committed tests are actual compiled raw CALL gap witnesses and rejection
regressions on the signed selected CardDemo package with a real held occurrence.
They do not pass required raw execution, raw PCB binding or official gate credit.
The copied-value mode-collision witness documents current behavior, not a desired
ABI: replace it with differing owned-mode assertions when the frame is admitted.
The malformed/short/alias cases currently encounter the absent program adapter;
they do not prove raw operand validation, IMS SAF denial or valid PCB writeback.
The capability-negative case proves the shared Program capability fence only.

## Compatibility and handoff

This leaf changes no runtime artifact, host effect, canonical byte representation,
provider row, storage schema or package metadata schema. Existing CALL and typed
IMS behavior stays in its owners. No migration or writer-drain change is needed
for this test/source/proposal-only delta. Existing manager-base restrictions
still require stopping admission, draining/reconciling UOWs/effects and preserving
a coherent compatible selected-package/image/session/undo/checkpoint/replay/
journal/audit backup for binary rollback. This is not backup/restore certification.

The next action belongs jointly to shared CALL-frame and IMS metadata/entry/
feedback owners. The parent must integrate those contracts, then replace these
gap expectations with real raw execution acceptance. Typed-only/manual harnesses
and EXEC DLI cannot close it. Human rule acceptance, participant admission,
official IR bindings, full parent/release acceptance and licensed certification
remain pending and are not self-approved by a local feature seal.
