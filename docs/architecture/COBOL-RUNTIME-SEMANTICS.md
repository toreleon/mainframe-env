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

CICS HANDLE ABEND label state distinguishes active from canceled. Selecting an
active exit moves it to canceled before the interpreter branches, preventing a
recursive abend from immediately selecting the same exit. RESET moves that
single-level exit back to active, and an explicit or default CANCEL deactivates
it. Program exits and nested logical-level search remain separate continuation
work and are not represented as labels.

CICS PUSH HANDLE moves the current condition mappings, ignored-condition set,
and active/canceled ABEND labels into one bounded task-local frame, leaving a
clear specification set for the nested routine. POP HANDLE discards the nested
set and restores exactly one prior frame. The stack permits 64 frames, rejects
growth before mutation, and treats an unmatched POP as INVREQ rather than as an
empty success.

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
