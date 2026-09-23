# COBOL runtime semantic contracts

Status: **Frozen for mainframe-env 0.4.0 implementation**
Owner: **compiler and interpreter maintainers**
Scope: **COBOL runtime semantic contracts**
Applies from: **mainframe-env 0.4.0**

The accepted typed COBOL HIR lowers once into the existing Core-MIR operation
catalog. The deterministic reference machine is the only execution authority;
it never reparses source text or dispatches on conformance row identities.

## Explicit execution boundary

Every transition is a pure function of the verified artifact, machine state,
one `MachineResume`, and the bounded quantum. Terminal, dataset, program, LE,
and subsystem I/O leave the kernel as one ordered typed `EffectRequest`. Clock,
locale, code-page, and pseudo-random inputs are invocation bindings and are
captured in checkpoint identity where execution can resume. There is no ambient
clock, locale, random generator, provider, or asynchronous I/O in the machine.

The accepted `ResourceLimits` remain authoritative. The COBOL adapter derives
table and sort bounds from storage/event limits and checks storage, output,
frames, effects, steps, table elements, sort records, decoded characters, and
generated output before mutation. A rejected, cancelled, timed-out, exhausted,
malformed, provider-failed, or indeterminate operation returns its exact typed
condition and does not mutate receiving storage unless that condition's COBOL
contract explicitly permits it.

## Values and storage

- Fixed decimal uses a bounded 36-digit `dec::Decimal<12>` primitive behind an
  owned adapter that admits at most 34 digits. The pinned 0.4 execution/oracle
  profile is `ARITH(EXTEND)` and the main arithmetic path uses its 34-digit
  context. The primitive spike also freezes the 18-digit `ARITH(COMPAT)`
  context, but propagating that compiler option into executable artifacts is an
  open completion-audit item. The adapter owns COBOL PICTURE, scale,
  intermediate precision, `ROUNDED` modes, truncation, `SIZE ERROR`,
  packed/zoned/binary encoding, and forbidden mutation.
- Decimal floating uses canonical IEEE decimal128 bytes. Binary short/long
  floating uses explicit big-endian IEEE bit wrappers so negative zero, NaN
  payloads, and infinities survive storage and checkpoint round trips.
- DISPLAY/DBCS values retain an explicit CCSID; NATIONAL is UTF-16BE and UTF-8
  retains validated source bytes. Conversion always normalizes to an owned
  bounded value while preserving source bytes, CCSID, and conversion condition.
- A storage reference resolves to one allocation plus checked offset/length.
  REDEFINES, RENAMES, groups, tables, reference modification, and pointer-based
  aliases are views over that authority; writes stage bytes before committing so
  overlaps and exception paths cannot observe partial mutation.
- `ALLOCATE` appends a positive-sized backing allocation within invocation
  storage/frame limits. Virtual pointer values encode checked base/offset
  identities; `SET ADDRESS OF` retargets linkage views, and `FREE` invalidates
  the allocation and its aliases atomically. Heap, alias, and freed state is
  checkpointed rather than reconstructed from ambient addresses.
- CICS `ADDRESS SET` reuses that virtual-pointer authority. Typed plans carry
  exactly one pointer role and one `ADDRESS OF` data-area role; provider requests
  contain opaque storage identities or virtual-pointer bytes, never native
  addresses. The interpreter validates pointer category and linkage targets
  before dispatch and applies the alias only after a successful audited CICS
  response, so denial, cancellation, malformed input, and audit failure cannot
  change storage.
- Bare CICS ASKTIME refreshes packed EIBDATE/EIBTIME from one checked clock
  instant. ASKTIME ABSTIME refreshes the same implicit fields and separately
  returns its packed absolute-time destination. Historical retained ABSTIME
  responses without implicit outputs retain their prior EIB state.
- CICS FORMATTIME decodes an exact `PIC S9(15) COMP-3` absolute-time input and
  writes source-sized character date/time fields plus a fullword binary
  millisecond value. Typed provider output schemas are checked before any
  receiving field is accepted.
- Typed CICS BMS plans admit a bounded local subset of `RECEIVE MAP`, `SEND MAP`,
  and `SEND TEXT`. MAP is a required 1–7 character literal or alpha/alphanumeric
  field for the two map commands; MAPSET is optional and defaults to MAP.
  `SEND MAP` optionally captures FROM bytes and admits LENGTH only with that
  explicit area; `SEND TEXT` requires captured FROM bytes and admits the same
  length forms. A literal, halfword-binary value, or matching `LENGTH OF`
  selects the formatted or text prefix, and an out-of-range SEND TEXT value
  returns LENGERR 22/0 before mutation. `SEND MAP MAPONLY` excludes FROM and
  LENGTH and writes only initialized defaults from the selected map. `SEND MAP
  DATAONLY` requires explicit symbolic FROM data, ignores map defaults, and
  applies each supplied field attribute while preserving the current attribute
  for `X'00'`. `RECEIVE MAP` optionally binds a writable INTO area. The provider
  validates the exact SEND request shape, checks the requested durable map definition, makes
  a successfully sent map current, and normalizes received named fields against
  that exact definition. The selected typed route reports EIBFN `1802`, `1804`,
  or `1806` respectively. SET pointers, omitted-MAP/AID-only receive, implicit
  symbolic map storage, RECEIVE length, paging, device and remaining
  terminal-control options are rejected before executable publication.
- Typed `PURGE MESSAGE` has no operands beyond the common condition controls.
  The runtime exposes no full-BMS ACCUM/page-building path, so local execution
  purges the exact reachable empty logical-message state without clearing the
  displayed screen or current map. It still carries a mutation identity and
  audit effect; DPL execution returns `INVREQ` 16/200. Nonempty accumulated
  pages and temporary-storage `TSIOERR` remain unimplemented.
- The typed CICS `ASSIGN` route returns APPLID, SYSID, USERID, and the
  current TASKPRIORITY from owned invocation context. Because no CICS
  application, platform, operation, version, or channel context is currently
  bound, their source-defined absence values are returned as fixed blanks or
  fullword `-1`; absent CWA/TWA lengths are zero, compatibility OPERKEYS are
  eight null bytes, and unsupported emergency task restart remains `X'00'`.
  Product installation names are not reinterpreted as IBM application context.
  Output references resolve before dispatch, numeric and fixed binary fields
  require their exact shapes, and no command can exceed IBM's 16-option limit.
  One append-only plan operation and a bounded 78-name output authority retain
  those resolved storage identities through publication and defensive runtime
  admission; retained raw artifacts still use the compatibility decoder.
  OPSECURITY is three null bytes and absent TCTUALENG is halfword zero locally;
  in a DPL server either option raises `INVREQ` 16/200 while other requested
  outputs are still applied when the condition policy permits return.
  Without an INITPARM system definition, INITPARM deliberately emits no output
  so its receiver stays unchanged and INITPARMLEN returns halfword zero.
  PROGRAM is the eight-character current program name derived from the selected
  execution frame; it changes with durable HANDLE ABEND program transfer and is
  never accepted as caller-supplied CICS command context.
  NEXTTRANSID returns four blanks while no supported in-task command has set a
  successor; it is prohibited with `INVREQ` 16/200 in a DPL server program.
  BRIDGE returns four blanks because the runtime has no bridge-monitor start
  path; IBM defines the same blank result in local non-bridge and DPL contexts.
  ABCODE, ABDUMP, and ABPROGRAM return the latest explicit EXEC CICS ABEND
  record from durable task state, or their documented blank/null absence values.
  Without a recoverable ASRA-class machine-check handoff, ABOFFSET is fullword
  zero and ASRAINTRPT/ASRAPSW/ASRAPSW16/ASRAREGS/ASRAREGS64 are their exact
  zero-filled widths; an explicit EXEC CICS ABEND is not treated as a
  processor fault.
  ERRORMSG and ERRORMSGLEN return 500 binary nulls and halfword zero because
  the runtime has no transaction-abend-control-block message authority.
  ALTSCRNHT/ALTSCRNWD, DEFSCRNHT/DEFSCRNWD, and SCRNHT/SCRNWD come from the
  owned durable terminal geometry. Nonterminal tasks receive `INVREQ` 16/5,
  while DPL returns the qualified `INVREQ` 16/200 and leaves those receivers
  unchanged.
  FCI is `X'01'` for that terminal attachment and `X'00'` when none exists;
  DPL prohibits the option with the same 16/200 condition.
  LINKLEVEL is halfword one for a top-level local program and two for a DPL
  target behind its level-one mirror. Deeper local program stacks require an
  owned CICS link-depth context and otherwise fail closed.
  PARTNSET returns six blanks for an owned terminal with no application
  partition set; nonterminal and DPL contexts follow the screen-option INVREQ
  rules.
  MAPCOLUMN/MAPLINE and MAPHEIGHT/MAPWIDTH come from the durable definition of
  the map most recently positioned by SEND MAP. `MECM6` persists its numeric
  DFHMDI origin, while `MECM1`–`MECM5` retain their historical top-left origin.
  A terminal task with no positioned map receives `INVREQ` 16/2; DPL receives
  16/200. SEND MAP rejects a definition that exceeds the terminal with
  source-named `INVMPSZ` 38 before session mutation.
  The virtual terminal's data-stream contract is 3270, not basic SCS, so
  DS3270 and DSSCS return `X'FF'` and `X'00'` respectively when it is attached.
  Optional keyboard, display, print, partition, symbol, DBCS, reader, and
  validation capabilities that the minimal terminal does not implement return
  `X'00'`; they are not inferred from received byte values.
  UNATTEND also returns `X'00'` because every virtual terminal session is an
  interactive, authenticated attachment; nonterminal and DPL contexts fail
  with their documented INVREQ qualifications.
  CMDSEC and RESSEC return `X`: CICS command dispatch always requires the
  execution grant and transaction authorization, and resource-owning handlers
  add their typed SAF checks before access.
  QNAME is a negative-only form until an ATI trigger path exists: local calls
  return `INVREQ` 16/4 and DPL calls return `INVREQ` 16/200 without changing
  the output area.
  ACTIVITY, ACTIVITYID, PROCESS, and PROCESSTYPE are negative-only until a BTS
  activity context exists; local and DPL calls return `INVREQ` 16/6 without
  changing their fixed-width output areas.
  DESTID and DESTIDLENG are negative-only until a BDI path exists: local calls
  return `INVREQ` 16/3 and DPL calls return `INVREQ` 16/200 without changing
  their eight-byte and halfword output areas.
  PRINSYSID is negative-only until an MRO, LU6.1, or APPC principal facility
  exists; local and DPL calls return `INVREQ` 16/5 without changing its
  four-byte output area.
  Handler-supplied condition names are admitted only through the generated CICS
  condition catalog, preserving names such as INVMPSZ without accepting
  arbitrary provider text.
  LOCALCCSID returns fullword 37 from the runtime's fixed CP037 CICS-region
  encoding authority in both local and DPL contexts.
  Other source-valid ASSIGN options remain rejected until their terminal,
  program-level, or failure context has an owned runtime representation.
- Terminal execution outcomes carry an explicit transaction-dump disposition.
  New CICS ABEND compilations use a typed task plan with an optional
  pre-resolved 1–4 character ABCODE input and distinct CANCEL/NODUMP flags.
  Results translate the provider's typed `ABEND.DUMP` metadata to requested or
  suppressed while retained responses from before that metadata remain
  unspecified. The interpreter preserves the supplied ABCODE as the terminal
  code instead of replacing it with the generic CICS condition name.

## Control, conditions, calls, and effects

Normal fallthrough, branch, loop, paragraph perform/return, `GO TO`, `GOBACK`,
`STOP RUN`, child invocation, transfer, suspension, and completion remain data,
not Rust errors. Condition phrases select exact CFG edges after the operation
records a typed condition status. Unhandled conditions become `Condition`,
`Abend`, or `Failed` according to the accepted execution taxonomy.

CICS HANDLE ABEND state distinguishes active from canceled and LABEL from
PROGRAM exits. Selecting either exit moves it to canceled before control
transfers, preventing a recursive abend from immediately selecting it. A LABEL
branches inside the current program. A PROGRAM names a registered local online
program, requires its exact `FACILITY CICS.PROGRAM.<name>` execute decision,
and receives the COMMAREA of the program that installed the exit. Missing local
programs return PGMIDERR 27/1; denied programs return NOTAUTH 70. RESET moves
the typed single-level exit back to active, and an explicit or default CANCEL
deactivates it. Autoinstall, current-channel transfer, and outward search across
LINK-created logical levels remain pending.
New HANDLE ABEND compilations encode those alternatives in the typed task plan:
LABEL is canonical source-control identity, PROGRAM is a literal or pre-resolved
1–8 character field, and CANCEL/RESET remain distinct append-only option tags.
Retained raw artifacts still execute through the version-one compatibility
interpreter.

The typed local LINK subset captures a 1–8 character PROGRAM name and models
COMMAREA as the same pre-resolved input/output storage slot. The interpreter
sends its current bytes, requires the returned `mainframe-env.cics.payload@1`
schema, and writes the response to that exact slot before continuing after
LINK. The provider performs `FACILITY CICS.PROGRAM.<name>` execute authorization
before nested program dispatch.

The typed local XCTL subset captures the same bounded PROGRAM and optional
COMMAREA inputs without registering a return binding. After the same exact
program authorization, its provider result becomes an unconditional
frame-replacing transfer. The online handoff carries the captured bytes into
the target program's `DFHCOMMAREA`; execution never resumes after XCTL in the
calling frame.

Typed local RETURN either completes the current top-level task or records a
1–4 character next TRANSID with a copied COMMAREA. COMMAREA is input-only and
requires TRANSID in this bounded runtime subset. The durable online handoff
terminalizes the old execution before the next terminal task can claim the
continuation; malformed schemas, unsupported DPL context, and unowned options
fail before continuation mutation.

The typed default file-browse loop gives RIDFLD explicit storage identity.
STARTBR reads its initial key but has no record output; explicit EQUAL requires
an exact starting key while GTEQ retains equal-or-next positioning. READNEXT
and READPREV send the current key, write the returned payload to INTO, and require a
`mainframe-env.cics.payload@1` RIDFLD output before updating that same key
slot. ENDBR carries only the resolved FILE/DATASET identity. The sequence
updates EIBFN to `060C`, `060E`, `0610`, and `0612`; forms needing named cursor,
remote routing, alternate record identities, SET storage, or RLS update-token
state do not enter this typed route.

Typed keyed file mutation resolves all data-bearing operands before dispatch.
DELETE reads an explicit RIDFLD storage slot or, after `READ UPDATE`, consumes
the latest held key for the same file; WRITE FILE reads FROM and RIDFLD storage
slots, while REWRITE reads FROM and consumes the same-file update hold. Optional
WRITE or REWRITE LENGTH is a bounded literal, halfword-binary value, or matching
`LENGTH OF` and selects the exact FROM prefix before the durable mutation.
READ LENGTH supplies a writable halfword capacity and receives the actual
record length after transfer. Optional READ, WRITE, or explicit-key DELETE
KEYLENGTH accepts a positive literal, halfword-binary value, or `LENGTH OF` the
RIDFLD area and must match the durable key definition. These operations require
exactly one FILE/DATASET alias, carry a mutation identity, and update the
operation-specific EIBFN. A current-record DELETE or REWRITE without a hold
returns its source-defined `INVREQ`; DELETE cannot carry KEYLENGTH without
RIDFLD. READ GTEQ selects the equal or first greater keyed record through a
request-local cursor that is closed before the command completes. READ GENERIC
selects only a record sharing the positive KEYLENGTH prefix of RIDFLD; combining
GENERIC with GTEQ retains first-greater fallback. Explicit EQUAL preserves the
default exact complete- or generic-key relation and cannot be combined with
GTEQ. READ GTEQ with runtime KEYLENGTH zero selects the first keyed record,
whether or not GENERIC is also present; zero fails closed for default EQUAL and
GENERIC without GTEQ.
Forms whose key or record is a literal, or whose semantics depend on TOKEN,
remote routing, alternate identities, mass insert, or RLS suspension do not
publish the typed executable.

Typed local WRITEQ TD captures a 1–4 character QUEUE name and one FROM storage
area. An omitted LENGTH selects the complete area; a present integer LENGTH
selects that exact leading byte count and raises LENGERR before mutation when
it exceeds the captured area. The persisted prefix and mutation identity drive
idempotent replay, and normal completion updates EIBFN to `0802`.
Typed local DELETEQ TD captures the same bounded QUEUE name, requires update
authorization for that queue resource, and atomically removes its durable
records and retained-byte accounting. A missing queue returns QIDERR 44/0;
extrapartition DELETE returns INVREQ.
Typed local READQ TD accepts exactly one INTO or SET destination and an optional
writable halfword LENGTH. INTO uses either the supplied positive maximum or the
compiler-derived area extent. SET returns a checked POINTER/POINTER-32 address
to interpreter-owned storage containing the complete record and survives a
checkpoint restore. Zero length and INTO truncation consume the record and
return LENGERR 22/0; a negative length or insufficient SET allocation capacity
does not consume. Missing and empty queues remain distinct as QIDERR 44/0 and
QZERO 23/0. Durable installed TDQUEUE definitions select intrapartition or
local extrapartition behavior, enabled/open direction, record-size rules, and
per-queue record/byte bounds. Definition state survives provider reopen and
produces exact DISABLED, INVREQ, NOTOPEN, LENGERR, NOSPACE, IOERR, QIDERR, and
QZERO conditions. On first registration, all compatibility-profile queues must
be declared by the complete proposed set, and retained records must satisfy the
new direction, record-size, record-count, and byte limits before any definition
is persisted. All three typed TDQ commands accept an explicit local-system
SYSID; any other name returns SYSIDERR 53/0 before authorization or mutation.
Remote routing, NOSUSPEND/QBUSY, indoubt locking, and external data set
integration remain fail-closed.

`DOCUMENT CREATE` lowers to a typed document plan whose DOCTOKEN is a writable
16-byte output and whose optional DOCSIZE is writable fullword binary storage.
Exactly one content source may be selected; buffer sources require a fullword
LENGTH, and SYMBOLLIST requires LISTLENGTH. The interpreter captures all input
bytes before dispatch and writes only typed DOCTOKEN/DOCSIZE outputs. The CICS
provider owns the bounded durable document/template state, source conditions,
template SAF decision, transaction ownership, atomic replay, SQLite reload and
task-end reclamation.

`DOCUMENT DELETE` lowers a 16-byte DOCTOKEN data area as an input and accepts
only common condition options. The interpreter captures its token bytes before
dispatch; the provider checks document ownership, releases the durable record
and aggregate capacity, and records the replay result in the same mutation.

`DOCUMENT INSERT` requires a 16-byte token and one content source or a
bookmark, with fullword LENGTH for buffer sources. The interpreter snapshots
all operands and writes optional DOCSIZE. The provider performs bounded
bookmark positioning and AT/TO overlay, template READ authorization, symbol
substitution, and atomic versioned persistence with replay. Its internal
tagged buffer preserves conversion blocks and bookmarks when a document from
this runtime is supplied through FROM.

`DOCUMENT RETRIEVE` lowers a 16-byte input DOCTOKEN, writable INTO byte area,
and writable fullword LENGTH. Optional fullword MAXLENGTH is capped by the
actual INTO extent; DATAONLY uses reserved document option tag 85. The
interpreter writes the available prefix and required LENGTH even on LENGERR
22/2. The provider emits a bounded tagged or data-only copy, converts the
supported CP037 client character sets on request, and leaves the source
document unchanged.

`DOCUMENT SET` lowers either SYMBOL/VALUE or SYMBOLLIST with a fullword
LENGTH and the shared 16-byte DOCTOKEN. DELIMITER is confined to symbol-list
mode and UNESCAPED keeps value bytes literal. The provider applies
case-sensitive symbol updates to the transaction-owned document with a
versioned document-plus-replay write. Existing inserted segments are not
rewritten when a symbol definition changes.
Typed WAIT JOURNALNAME accepts one 1–8 character named journal and an optional
fullword-binary REQID. The explicit token is task-owned; without it, the command
synchronizes the journal's current buffer even when another task created the
latest record. Completed output returns immediately, pending output suspends
and reissues the same typed statement under the execution deadline and
cancellation fence, and the durable provider authority maps IOERR, JIDERR,
NOTOPEN, and SAF denial to exact EIB response codes. The authority survives a
SQLite reopen without turning this non-mutating wait into a replayed mutation.
Typed WAIT JOURNALNUM uses a distinct numeric 1–99 operand and resolves it to
the corresponding `DFHJnn` journal before applying the same token ownership,
current-buffer, authorization, and completion rules.
Typed WRITE JOURNALNAME persists FROM bytes with a two-byte JTYPEID and an
optional PREFIX. FLENGTH and PFXLENG truncate their respective areas after
checked fullword and halfword input; invalid lengths leave journal state
unchanged with LENGERR. Deferred output returns a fullword REQID and waits for
trusted local output acknowledgement; WAIT hardens synchronously and excludes
REQID. NOSUSPEND returns NOJBUFSP when both local output buffer slots are
pending. The version 2 durable record retains data, prefix, token ownership,
and replay identity while remaining able to read version 1 WAIT records.
Typed WRITE JOURNALNUM is a separate compatibility operation. It accepts a
numeric 1–99 selector, maps that selector to `DFHJnn`, and uses the same local
record, output token, WAIT, NOSUSPEND, and condition rules as the named form.
The pinned compatibility page has no full option syntax; this bounded option
mapping is inferred from the adjacent named WRITE contract.
`EXEC CICS SPOOLCLOSE` lowers through the typed spool-control route with one
eight-character TOKEN input and optional KEEP or DELETE. RESP or NOHANDLE is
mandatory. The provider applies owner and JESSPOOL checks before its bounded
durable CAS; explicit input close defaults to DELETE while explicit output
close defaults to KEEP. A request-digest replay entry is committed with the
report transition, so recovery after a lost result returns the original
response without applying a second disposition. Successful or handled
negative completion records EIBFN `5610`.

`EXEC CICS SPOOLOPEN INPUT` resolves USERID and optional CLASS before dispatch
and writes the provider's exact eight-byte TOKEN only after normal completion.
The provider checks the four-character APPLID prefix and JESSPOOL authority,
then selects one durable available report and records run/principal ownership.
The input interface is single-threaded across tasks and distinguishes current-
owner from other-owner SPOLBUSY. Replay returns the same token without a second
claim, including after SQLite reopen. The command records EIBFN `5602`.

Typed local GETMAIN requires SET plus exactly one length selector: a literal or
fullword-binary FLENGTH, or a literal or unsigned-halfword-binary compatibility
LENGTH capped at 65,520 bytes. It optionally accepts one character INITIMG and
NOSUSPEND. The interpreter reports its bounded frame/byte capacity, receives
initialized bytes through the typed host result, allocates one checkpointed
virtual base, and writes only the checked virtual address to POINTER or
POINTER-32 storage. Zero or over-limit values return LENGERR 22/1 and clear SET;
unavailable capacity returns NOSTG 42/2, which is ignored by default. LENGTH
selects the source-defined below-line compatibility policy, but the virtual
allocator exposes no native 24-bit address. Native addresses, storage keys,
SHARED/EXECUTABLE policy and GETMAIN64 remain outside this subset.
The separate GETMAIN64 typed IR route requires a non-LE AMODE(64) invocation
binding and an eight-byte pointer target. COBOL source continues to reject
GETMAIN64; a 64-bit COBOL pointer alone does not grant that caller ABI.
Typed local FREEMAIN accepts exactly one of DATAPOINTER or DATA. DATAPOINTER
requires a POINTER or POINTER-32 value that the current machine can prove names
a live, offset-zero GETMAIN allocation. DATA accepts a declared COBOL area only
when its current virtual-storage view begins at such an allocation. Normal
completion records the released base in the checkpoint, makes existing linkage
views inaccessible, and restores its frame/byte capacity. A null, static,
malformed, foreign, unassigned, or already-freed identity returns INVREQ 16/1.
Key/shared/load ownership and FREEMAIN64 remain outside this subset.

CICS PUSH HANDLE moves the current condition mappings, ignored-condition set,
and active/canceled typed ABEND exits into one bounded task-local frame, leaving a
clear specification set for the nested routine. POP HANDLE discards the nested
set and restores exactly one prior frame. The stack permits 64 frames, rejects
growth before mutation, and treats an unmatched POP as INVREQ rather than as an
empty success.

CICS IGNORE CONDITION carries one canonical newline-separated list of 1–16
unique names from the generated 121-name EIBRESP authority. A matching provider
condition returns the ignored disposition so execution continues after EIB
state is updated. HANDLE CONDITION for the same name removes that ignore, and
the ignored set participates in the same PUSH/POP snapshot as label handlers.

CICS HANDLE CONDITION carries one canonical `mainframe-env.cics.condition-handlers@1`
payload containing 1–16 strictly name-ordered records. Each record is a reviewed
EIBRESP name, a tab, and its optional COBOL label; an empty label deactivates
the condition-specific handler and restores the default action. The complete
payload is validated before any state changes. A labeled condition removes a
matching ignore and replaces the prior handler. If an otherwise unhandled
condition has a terminating default action, the generalized `ERROR` ignore or
handler applies after the specific condition action and before abnormal
termination.

CICS HANDLE AID carries one canonical `mainframe-env.cics.aid-handlers@1`
payload containing zero to 16 strictly name-ordered records from the generated
34-name AID authority. Each record contains the AID name, a tab, and an optional
COBOL label. An empty label is retained as an explicit deactivation tombstone,
so a specific key such as PF10 can continue after input even when ANYKEY has a
label. Exact AID specifications precede ANYKEY; ANYKEY applies only to PA1–PA3,
PF1–PF24, and CLEAR, never ENTER or the other special AIDs. The AID state joins
the same bounded PUSH/POP stack. A DPL server attempt fails with INVREQ, RESP
16, and RESP2 200 before changing the state.

Condition, AID, ignored-condition, active/canceled ABEND, and nested PUSH/POP
state is task-local while a run is live and session-durable across a terminal
input handoff. Every state-changing command first validates the complete
request, then commits the next `MECS9` session version by CAS; a failed write
restores the prior in-memory state. The next terminal task restores the exact
state before replaying the machine checkpoint. Normal task completion and
non-handoff terminal recovery clear it, preventing specifications from leaking
  into an unrelated task. Historical `MECS8` rows retain one abend code as both
  original and current, `MECS7` rows retain typed HANDLE state with no abend
  history, `MECS6` rows retain label-only HANDLE state, and `MECS1`–`MECS5`
  decode with empty HANDLE state.

`CALL`, `INVOKE`, and LE callable services resolve through versioned typed ABI
catalog entries. Names choose a registered program only after the ABI identity
is known; no copybook, application, or program-name special case selects host
semantics. Arguments carry explicit mode, direction, class, length, and bounded
payload identity. Cancellation and unknown outcomes remain explicit across the
existing effect intent/result journal and checkpoint envelope.

The product invocation boundary installs the four accepted compatible LE
selectors as `mainframe-env.runtime-service-selector@1` bindings. The
interpreter copies the exact kind/name/version selector into `ProgramRequest`;
the program provider dispatches compatible LE handlers only from that selector
and rejects typed selectors on the ordinary program-name router. Conflicting
bindings, wrong service kinds, and unknown ABI versions fail closed.

## Checkpoint compatibility

New values serialize in owned canonical byte form, never Rust debug or a
third-party type layout. Restore authenticates the existing envelope, validates
artifact/runtime/host compatibility and all bounds, then upgrades through one
versioned reader. Writers emit only the current schema. Pending effects are
reconciled through the existing idempotency record; they are never silently
reissued from a decoded machine snapshot.
