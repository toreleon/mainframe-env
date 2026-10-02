# ADR-0029: COBOL storage-entry identity within one CICS task

Status: **Proposed; private codec prerequisite; runtime writers disabled**
Owner: **Selected execution and CICS task maintainers**
Scope: **language storage identity, before atomic instance admission and close**
Applies from: **mainframe-env 0.9.0 development**

## Context and decision

Two compiled SQLite diagnostics show that repeated LINK calls currently reuse
working storage and that native CALL and LINK share the same retained instance.
The CICS task and UOW correctly retain one core run identity. Changing that core
identity to separate language storage would break task resources, effects and
retention ownership. Instead keep the existing root RunState authority and its
instance namespace. Future writers index members by storage scope as well as
program. This prerequisite does not enable those writers or fix the diagnostics.

Use the existing bounded Invocation binding `cobol.storage-entry`, schema
`mainframe-env.cobol.storage-entry@1`. A root creates the level-1 storage scope.
An independently attested actual CICS LINK occurrence creates a fresh child
scope. Native CALL inherits its caller's scope, including its immutable creation
identity, while receiving a distinct entry actor and CALL occurrence. Repeated
LINKs and same-name members at higher/lower levels remain distinct. No scope
digest becomes Invocation.run_unit_id or a provider/core owner run.

The strict ordered compact JSON entry fields are `schema_version`, `scope`,
`kind`, `execution`, `source_execution`, `call_key`, `program`, `artifact`,
`attempt`, `metadata_digest`. Kind is `root`, `native_call` or `cics_link`.
The ordered immutable scope fields are `root_execution`, `task_run`, `principal`,
`id`, `owner_execution`, `owner_selector`, `owner_artifact`, `owner_attempt`,
`parent_scope`, `source_execution`, `creation_call`,
`logical_level`, `selection`. Optional fields are explicit JSON nulls. Selection
has `artifact`, `generation`, `content_identity`, in that order. Program/selector
must agree exactly; artifact/content are lowercase `sha256:` identities and
generation is positive in the durable signed-integer range. Logical level is
bounded at 16. Binding bytes are independently capped at 8,192.

A root scope has no parent/source/creation CALL/selection, level 1 and root as
owner. LINK creation has all four fields, a parent scope, source actor, exact
CALL key and frozen selection. The owner is the deterministic original child
`online-call-execution-<CALL key>`. Its entry must agree with that owner and
selection. The creator selector, artifact and attempt are immutable as well. A native
entry has its own immediate parent and CALL key and cannot
claim to be the scope creator. It preserves the creator's original parent,
source, CALL, level and selection. These shape checks do not prove ancestry or
that a selected generation was actually invoked.

## Canonical encoders and trust boundary

SHA-256 uses a literal domain, one zero byte, then u64 big-endian byte-length
framing for each field. `mainframe-env.cobol-storage-scope@1` hashes one compact
JSON field containing the ordered scope with `id` empty.
`mainframe-env.cobol-storage-entry@1` hashes one compact JSON field containing the
ordered entry with `metadata_digest` empty. For member identity,
`mainframe-env.cobol-storage-member@1` frames the existing root RunState key,
scope ID and normalized program, in that order. The root key continues to use
the existing installed-call encoder over instance-owner, actual core run and
principal. Every digest is lowercase 64-character hex. Six independently
calculated vectors freeze root/LINK scope, entry and member encodings.

Readers reject unknown fields/versions, duplicate fields, invalid or rehashed
foreign owner/phase metadata, mismatched invocation identities and noncanonical
JSON. Absence remains absence. Binding an identical value is idempotent;
capacity failure or an existing unequal value leaves the invocation unchanged.
This module has no provider writer, dispatch or schema-promotion path.

Canonical bytes and digests provide integrity only. A syntactically valid
changed selection produces a different scope; it does not gain authority.
The future consumer must independently require actual typed LINK origin and
the top selected live provider loan, original source invocation before COMMAREA
replacement, immutable selection, trusted root/core authority and source lease.
Grants, audit, generations, controls and nonwidening resource/deadline limits
must be checked through those live authorities. Native CALL needs its source
lease and exact inherited scope registry. Serialized busy rows cannot recreate
either token after a cold restart.

## Reserved writer generations and rollout

The manager reserves RunState JSON 3, Instance JSON 3, CALL protocol JSON 4 and
ordinary scope-aware CALL receipt JSON 5 for the subsequent atomic writer.
This is a reservation only: this change reads/writes none of these generations.
Existing RunState1/2, Instance1/2, protocol1/2/3, ordinary receipt1/2 and pending
Transfer receipt3/4 retain their existing strict codecs. Earlier unreserved
Transfer proposals cannot consume receipt5. A later Transfer disposition must
use a separately reviewed version and contract.

Deploy strict readers and retention ownership before enabling writers. Do not
upgrade active legacy state on read, reinterpret bare program keys as scoped
members or allow missing scope metadata to fall back to old LINK behavior.
Legacy known replies remain historical behavior with existing validation; busy
and pending rows remain fenced. A fresh scoped protocol or a separately proven
quiescent migration is required for corrected admissions. Old binaries must
reject new generations. Downgrade requires drained writers plus a compatible
reader or verified pre-change backup, preserving pending/unknown obligations;
never strip metadata or relabel unresolved calls as completed.

Before reservation writes, settle aggregate root count/byte/receipt charges,
root machine allocation versus concurrent members, exact source CAS adoption,
and atomic pending CALL plus root/scope/member reservation. Scope close must
validate actual executed return kind and exact core terminal event/version,
attempt and checkpoint, closed resources, member CAS versions and child scope
quiescence. Delete only that scope's members, update root counters and record
the original terminal CALL reply/close proof in one mixed CAS transaction.
Higher same-name state is preserved. Unknown outcomes keep pending fences and
never redispatch. CICS native command subloans, RETURN propagation, batch-file
exclusions, CANCEL/ABEND, task-end ownership and retention remain separate
required contracts. The existing 256-member root bound is not reset per LINK.

## Scoped row reader contract before writer rollout

The serialized manager lane adds strict RunState3 and Instance3 readers in the
existing root namespace. The runtime still cannot create, migrate, acquire or
close those rows. Existing writers reject the new generations. Retention
recognizes canonical generation-3 records and preserves the actual root/core
run owner, root protocol, scope creators and busy original CALL dependencies.
Malformed, reordered, duplicate, oversized and foreign records fail closed.
No pending row recreates a live source lease or a normal-return witness.

RunState3 retains the immutable root entry, root-derived member/frame/receipt
bounds, a scope-creator map, exact member index and unique original CALL keys.
Every LINK scope requires its own busy creator member and a parent exactly one
logical level higher; native entry actors cannot replace scope creation. The
index includes exact member row versions, full payload digests, active flags
and charges. Complete bounded member validation rejects missing, extra,
duplicate, stale, foreign or differently owned rows. Instance3 retains the
immutable scope-creator entry even after its transient native owner becomes
idle; its key, artifact, root, size and phase agree with the root index.

The existing 256-member bound applies across the root, with at most 16 scopes
and the root's smaller declared frame bound. Busy members reserve the root's
per-execution storage allowance; idle members charge actual retained payload
bytes. The unmanaged top machine reserves its full declared allowance. This
bounds member storage by 257 times that allowance; it does not claim a new
single-execution storage entitlement. The shared store's actual capacity still
applies. A checksum does not authorize widened bounds: live admission must
compare them with the trusted root invocation.

CALL history has one root-wide monotonic allowance capped by the root's
max_effects. Each unique original key reserves max_output_bytes plus 128 KiB
for bounded close metadata. Scope close cannot replenish that allowance or
erase keys. The future reservation transaction must validate existing indexed
CALL rows and atomically publish the new key/charge, pending original receipt,
root/member updates and exact source CAS; this reader implements only pure
in-memory charge preparation. Combined maximum storage and receipt charges
must fit the positive signed durable range. JSON bodies are capped at 64 MiB
before generation-3 deserialization; tighter store/payload limits still apply.

RunState3 orders schema_version, root, max_member_bytes, max_scopes, max_calls,
max_receipt_bytes, calls, receipt_charge, root_charge, active, charged_bytes,
scopes, members, ended_tick and metadata_digest. Member values order scope,
program, artifact, row_version, payload_digest, charged_bytes and busy.
Instance3 orders schema_version, run_key, scope_entry, max_state_bytes, program,
artifact, owner, initial, state and metadata_digest. Nulls are explicit; maps
and CALL sets have sorted canonical keys. Metadata uses the existing
mainframe-env.installed-call@1 domain, framing scoped-run-metadata@3 or
scoped-instance-metadata@3, row key and compact JSON with empty digest. Full
payload references use ordinary SHA-256. Independent Python vectors freeze
root and busy-member encodings; hashes provide integrity, not execution trust.

These reader/retention and bound checks do not fix the compiled storage routes.
Runtime admission, live source-version adoption, original CALL5 proof, terminal
checkpoint publication, atomic close and task-end/recovery remain pending.
No public registration, official gate or parent completion is granted.

## Source authority and acceptance

Catalog `ibm-cics-ts-6x-2026-08-31:api-commands:0138` LINK, command-body baseline
`ibm-cics-ts-6x-application-api-sources-b-2026-09-10`. Storage and return authority:
`ibm-cics-ts-6x-cobol-calling-context-candidates-2026-09-12`,
`SSJL4D_6.x/applications/developing/cobol/dfhp3_cobol_subprog_rules.html`, SHA-256
`ff6330589c9ff88c9292aac548c37d3c50de369fdcef485125d6165fca414d9c`, lines 74–102;
and `dfhp3_cobol_subprog_flow.html`, SHA-256
`19c888aa15e7d8b01333dd099c20f8bcb80ac1b507668346939e140b2ceb8f8b`, lines 9–20.
Both were searched and read hash-verified offline before implementation.

Require independent vectors, immutable creator/native inheritance and distinct
occurrence/member tests; rehashed owner/phase/selection/context mutations;
canonical/schema/duplicate/byte bounds; pure reads and idempotent/conflicting
bindings; and mandatory policy gates on the integration candidate. Those are
codec proofs, with zero execution, recovered, licensed or parent coverage.
Runtime acceptance still requires the independently asserted repeated LINK
37/37/37 and mixed CALL/LINK/CALL 37/37/47 routes, higher-state byte/CAS
preservation, atomic rollback and physical SQLite/PostgreSQL reopen. Pending
gate counts and the user's retained CICSMESSAGE internal requirement stay intact.
