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
- The bounded legacy CICS `ASSIGN` route returns APPLID, SYSID, USERID, and the
  current TASKPRIORITY from owned invocation context. Because no CICS
  application, platform, operation, version, or channel context is currently
  bound, their source-defined absence values are returned as fixed blanks or
  fullword `-1`; absent CWA/TWA lengths are zero, compatibility OPERKEYS are
  eight null bytes, and unsupported emergency task restart remains `X'00'`.
  Product installation names are not reinterpreted as IBM application context.
  Output references resolve before dispatch, numeric and fixed binary fields
  require their exact shapes, and no command can exceed IBM's 16-option limit.
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
  Other source-valid ASSIGN options remain rejected until their terminal,
  program-level, or failure context has an owned runtime representation.
- Terminal execution outcomes carry an explicit transaction-dump disposition.
  CICS ABEND results translate the provider's typed `ABEND.DUMP` metadata to
  requested or suppressed while retained responses from before that metadata
  remain unspecified. The interpreter preserves the supplied ABCODE as the
  terminal code instead of replacing it with the generic CICS condition name.

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
request, then commits the next `MECS7` session version by CAS; a failed write
restores the prior in-memory state. The next terminal task restores the exact
state before replaying the machine checkpoint. Normal task completion and
non-handoff terminal recovery clear it, preventing specifications from leaking
into an unrelated task. Historical `MECS6` rows retain label-only HANDLE state;
`MECS1`–`MECS5` decode with empty HANDLE state.

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
