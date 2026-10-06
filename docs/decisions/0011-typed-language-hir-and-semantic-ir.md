# ADR-0011: Adopt language-specific HIR and versioned semantic IR dialects

Status: **Accepted by repository owner**
Owner: **compiler, execution, and subsystem maintainers**
Scope: **language HIR, executable semantic IR, host effects, and compatibility boundaries**
Applies from: **mainframe-env current subsystem contracts**

## Context

mainframe-env already separates source processing, semantic analysis, HIR,
lowering, executable artifacts, deterministic execution, host effects,
providers, stores, and conformance. Those large-scale boundaries remain the
right foundation for a multi-language mainframe runtime.

Some executable paths nevertheless carry source-shaped token lists farther
than necessary. A runtime handler may rediscover a statically knowable COBOL
separator or option such as `TO`, `GIVING`, `ROUNDED`, `RIDFLD`, or `RESP`.
That makes parsing an accidental runtime responsibility, weakens IR
verification, and permits the compiler and interpreter to disagree about the
same grammar.

Removing those token lists by forcing every language and subsystem through one
minimal instruction set would create the opposite problem. COBOL, PL/I,
HLASM, REXX, JCL, BMS, Db2 SQL, CICS, IMS, and MQ have semantic boundaries
that must remain visible when they affect observable behavior, provenance,
conditions, transactions, or verification.

This decision defines the target architecture and an incremental migration. It
does not authorize a rewrite of the existing coordinator, effect journal,
stores, providers, artifact model, or conformance authority.

## Decision

The normative program path is:

```text
SourceBundle
  -> language-specific frontend and semantic model
  -> language-specific typed HIR
  -> typed executable semantic IR dialects
  -> versioned executable artifact
  -> deterministic reference machine
  -> execution coordinator
  -> typed host effects
  -> subsystem authorities
```

Each arrow is an owned validation boundary. No later layer may infer that an
earlier layer successfully resolved a construct merely because source text or
an untyped string survived into the artifact.

### Responsibility and dependency matrix

| Component | Responsibility | Dependency boundary |
| --- | --- | --- |
| Generic IR framework | Common representation, registration, codec infrastructure, and verification mechanisms | Has no dependency on a language frontend, runtime adapter, interpreter, or provider implementation |
| Dialect | Operation schemas, typed plans, dialect-specific codecs, and static validation rules | Depends on generic contracts only; it does not import compiler, interpreter, or provider implementations |
| Language frontend/lowering | Language-specific interpretation, binding, policies, condition semantics, and source provenance | May depend on generic IR and dialect contracts; it must not depend on interpreter or provider implementations |
| Runtime adapter | Execute an already-resolved contract and evaluate genuinely dynamic inputs | May depend on generic IR, dialect contracts, and typed host APIs; it must not import language HIR or provider implementations |
| Provider | Own subsystem resources, authorization, transitions, recovery, and its resource-generation state | Consumes typed host requests through the coordinator; it must not import frontend/HIR types or create a second execution/state authority |

This matrix governs ownership, not file placement. A dialect need not live in a
separate crate, but validator reuse must follow the dependency boundary: a
compiler cannot reuse validation by importing an interpreter or provider, and
a runtime adapter cannot become a second provider or execution coordinator.

### Language HIR remains language-specific

Each programming-language frontend owns its own HIR types, verification rules,
and source provenance. There is no universal language enum and no universal HIR
shared by COBOL, PL/I, HLASM, REXX, JCL, and resource-definition languages.

COBOL HIR owns COBOL meaning, including resolved data references and layouts,
receiving-field policies, arithmetic condition edges, `ADD CORRESPONDING`
pairing, `PERFORM`, `REDEFINES`, `OCCURS`, declaratives, and effective compiler
options. A future PL/I or HLASM frontend may reuse the generic IR framework and
shared semantic primitives without importing COBOL HIR.

For a migrated semantic family, statically knowable grammar is resolved before
execution. The HIR and executable operation carry typed values, storage
references, policies, and control edges. Runtime handlers must not search a
token list to rediscover separators, phrase order, or output bindings.

Dynamic semantics remain dynamic. A compiler may resolve the identity and
shape of a table reference while retaining a runtime subscript expression. It
must not freeze current storage values, authorization, provider generation,
resource state, transaction state, dynamic SQL, or any other execution input.

### Executable IR is a multi-dialect framework

The generic IR container owns IDs, regions, blocks, values, attributes, storage
references, effects, locations, limits, catalogs, and verification. It does not
define one lowest-common-denominator language.

Operations use explicit, versioned dialect identities. Shared primitives are
used only where their observable contracts genuinely match, for example:

```text
memory.*
decimal.*
string.*
control.*
program.*
```

Specialized operations remain explicit when language or subsystem meaning is
observable, for example:

```text
cobol.*
cics.*
db2.*
ims.*
mq.*
dataset.*
```

A specialized operation may compose shared decimal, storage, or control
semantics. That reuse must not erase COBOL receiver rules, CICS conditions,
Db2 transaction behavior, aliasing, provenance, or failure taxonomy.

#### Executable COBOL layout ABI

`mainframe.core.cobol@1.define` owns the executable COBOL layout vocabulary.
Its `CobolLayoutDefinition` semantic contract checks every definition, including
ones not referenced by a migrated plan, before MIR legalization or artifact
admission. The contract validates the runtime-required field types and domains,
numeric and nonnumeric PICTURE/category/representation widths, sign metadata,
BYTE-LENGTH and object-class ownership, LP-dependent opaque widths, static
occurrence extents, DYNAMIC limits, storage-view topology, and unique qualified
definition names. Decimal and
CICS contracts then add exact plan-slot-to-definition-to-storage binding and
numeric/writable usage checks.

The verifier builds one bounded layout index per module. It validates and caches
each definition ABI once, then reuses that record for all decimal/CICS slots;
PICTURE repetition is checked without expansion and is capped at the compiler's
one-million-symbol boundary.

The pinned IBM HTML cache is evidence input, not executable parser code.
`extract_cobol_html_grammar.py` currently projects procedure-statement diagrams
for a zero-credit comparison artifact, while `cargo xtask cobol-language`
generates descriptors from the independently reviewed `language.json` catalog.
The data-description scanners consume those generated descriptors for clause
openers, but still implement clause operands and defaults in the frontend. For
the affected OCCURS/KEY/INDEXED/DYNAMIC slice, source forms, scanner tests,
resolved layout identities, and the executable ABI validator are therefore
checked together. Extending HTML projection into a generated data-clause parser
is later #130 work, not claimed by this remediation.

`TYPEDEF` templates remain semantic-HIR declarations; lowering emits only their
allocated `TYPE` instances as executable layout/storage identities. The current
bounded qualifier ABI begins at the data-record root. FD/SD outer qualifiers and
the high-to-low `::` spelling are rejected before publication until a later
#130 slice carries that owner metadata explicitly.

The same dialect layer validates the currently executable level-88 subset:
canonical `VALUE [IS]`/`VALUES [ARE]` operands, numeric assignment fit,
strictly increasing numeric ranges, and bounded quoted/figurative values for
alphanumeric variables. It rejects incompatible pointer/index/object parents.
Complete VALUE format 2 behavior (`WHEN SET TO FALSE`, `ALL`, symbolic
characters, NATIONAL/DBCS/UTF-8 conversion and collation, edited/group cases,
and an explicit versioned level-78 kind) remains a later #130 slice and is not
silently interpreted by this one.

The schema, identity, and bounded unbounded-table capacities are registered
once in the IR dialect layer and are consumed by compiler and defensive VM
admission without either importing the other. Historical `@2` artifact fields
that predate `alias_of` and `occurs_clause` retain their documented absent-value
defaults; the absent `occurs_clause` default is false and is accepted only for
a scalar definition with no table metadata. HIR remains a storage-binding proof
because executable `define` operations do not exist at that stage; full
layout-ABI proof begins at executable MIR and artifact boundaries.

#### Decimal assignment responsibility and policy

`mainframe.decimal@2.assign` is a shared executable operation, not a universal
HIR and not a claim that every language has COBOL arithmetic. Its canonical
`mainframe-env.decimal-assignment-plan@2` payload separates provenance from
the policies that select observable execution:

| Contract element | Responsibility | Current reviewed value |
| --- | --- | --- |
| `semantic_origin` | Frontend provenance only; it never selects v2 dispatch or behavior | COBOL emits `cobol.add@1` or `cobol.compute@1`; the bounded ledger adapter emits `ledger.formula@1` |
| arithmetic context | Shared decimal evaluator precision, exponent range, primitive rounding, and division guard scale | `Decimal18V1` or `Decimal34V1`: 18 or 34 digits, exponent range -9999 through 9999, primitive truncation, nine division guard places |
| storage ABI | Adapter-visible layout metadata and physical numeric bytes | `CobolNumericV1`; reuse is explicit and does not rename this ABI as language-neutral |
| receiver update | Operand visibility and conversion-failure commit behavior | `CapturedOperandsReceiverLocalV1`: capture all operands and commit receiver-local results; preserve a failed receiver when `ON SIZE ERROR` is declared, otherwise store its truncated result |
| condition policy | Mapping of evaluation/conversion failures to executable control | `CobolSizeErrorV1`, paired with the typed `cobol.arithmetic-size-error@1` branch contract; the condition is selected only after all receiver work |
| receiver rounding | Per-destination conversion behavior | The plan carries one `DecimalRoundingPolicy` for each receiver |

The independent `ledger.formula@1` conformance adapter parses its own bounded
input, selects the same declared policy types, emits `mainframe.decimal@2.assign`,
passes ordinary IR semantic verification, and executes in the production
reference machine. It imports neither COBOL HIR nor the COBOL ADD/COMPUTE
producer identities. Its exact 18-versus-34-digit result demonstrates reuse of
the declared arithmetic boundary; its selection of `CobolNumericV1` and
`CobolSizeErrorV1` keeps the remaining language-specific obligations visible.
This is a bounded adapter proof, not implementation of a second complete
programming language.

The historical pair `mainframe.decimal@1.assign` and
`mainframe-env.decimal-assignment-plan@1` retains its reviewed implicit COBOL
module arithmetic context, whole-batch receiver atomicity, and COBOL-origin
allowlist. The v1 reader maps those bytes only to a read-only legacy policy.
Writers emit v2. Operation/plan version mismatches, unknown policy tags, a v1
origin outside its allowlist, and attempts to emit the legacy policy in v2 all
fail closed; v2 never dispatches by `semantic_origin`.

The unconditional whole-batch behavior in the older source-token arithmetic
handler predates the typed-HIR change (it is present at the PR base
`8b7459a`). It is not an expected-result authority. Current token-compatible
ADD/SUBTRACT execution captures every operand and commits successful receivers.
When a receiver conversion fails, it preserves that receiver only when an
`ON SIZE ERROR` phrase is present; without the phrase it stores the truncated
result. Condition selection happens after receiver processing. This bug fix is
distinct from the immutable decimal-plan `@1` reader above, whose published
byte contract remains intentionally unchanged.

### Typed host effects are the integration boundary

Providers consume owned typed requests, never source text, frontend syntax, or
language HIR. The executable operation supplies pre-resolved binding
descriptors; the machine evaluates only the genuinely dynamic values and emits
the typed request.

A host request makes the applicable resource identity, principal and
capability context, input/output representation, transaction or unit-of-work
identity, effect sequence and idempotency identity, deadline and cancellation,
condition taxonomy, and reconciliation requirements explicit.

The existing `ExecutionCoordinator` remains the authority for admission,
ordered dispatch, cancellation, intent/result journaling, unknown outcomes,
checkpointing, replay, and recovery. A typed-IR migration must pass through the
existing provider and store authorities; it must not create a direct
interpreter-to-store path or a second subsystem implementation.

For the migrated CICS `READ`, `REWRITE`, and `SYNCPOINT` pilot, ownership is
explicit and deliberately narrow:

| Fact | Authoritative owner | Consumers/check |
| --- | --- | --- |
| executable command identity, semantic major, exact effect sequence, and `host.cics` import | typed CICS dialect descriptor registry in generic IR | compiler catalog, static verifier, and runtime registry consume the same descriptor |
| typed operand/option/output direction and operation-specific static shape | `CicsEffectPlan` codec and its dialect validator | COBOL lowering must construct a canonical valid plan; artifact admission rechecks it |
| source spelling, COBOL reference resolution, and static storage binding | COBOL frontend/HIR lowering | no provider or runtime token parser participates |
| executable registration and legal host binding | compiler MIR catalog materialized from the dialect descriptors | the compiler/interpreter registry consistency test checks every descriptor and the architecture guard rejects duplicated identity/effect tables |
| evaluation of dynamic storage values and mapping to typed `HostRequest::Cics` | reference-machine CICS runtime adapter | the adapter consumes an already-resolved plan and does not own provider state |
| resource authorization, file/update context, unit-of-work transition, rollback/commit, and recovery | existing scoped host, CICS/dataset providers, coordinator, and stores | the durable CICS pilot checks coordinator lifecycle, ordered effect journal, audit sequence, and mutating-effect completion |

Adding another CICS command to this mechanism requires extending the descriptor,
plan-shape rules, frontend translation, adapter mapping, and focused
transition/recovery tests together. This bounded consistency contract avoids a
second allowlist without introducing a universal semantic DSL; broader command
families remain follow-up work under #130/#15.

### JCL and resource DSLs remain separate

JCL lowers to a typed immutable job/workflow plan executed by JES and batch
services. It is not wrapped in a pretend program operation and is not forced
through the ordinary program machine.

BMS, CSD, and similar resource-definition languages compile to versioned
resource artifacts consumed by their owning subsystem. They may share source,
provenance, diagnostics, artifact lifecycle, security, store, and conformance
contracts without sharing a programming-language HIR or execution loop.

Workflow independence is proven by the existing versioned JCL `JobPlan`/JES
path. IR-framework and shared-decimal reuse are separately proven by the
bounded `ledger.formula@1` adapter described above. BMS and CSD remain separate,
subsystem-owned parser/resource paths and are not represented as program
operations; standalone serialization of their current in-memory resource
models is a later vertical slice, not a claim made by the ADD/COMPUTE, decimal
adapter, and CICS pilot migration. Complete PL/I, HLASM, and REXX frontends also
remain unimplemented.

### Compatibility is explicit and fail-closed

The current writer uses `mainframe-env.artifact@3`. Its manifest records the IR
envelope contract, an exact `dialect_contracts` set containing every executable
operation namespace/major pair in the payload, and every required host ABI.
Publication derives the dialect set from the payload and rejects a stale,
missing, or extra manifest entry. Operation identity includes dialect,
operation name, and semantic major version.

For COBOL payloads, publication and read admission also compare all three
normalized semantic options with the immutable `mainframe.core.cobol@1.config`
operation. Current `@3` payloads must carry arithmetic, display-sign, and
address-mode markers. The retained `@2` path alone applies its documented
compatible-sign and LP(32) defaults when those historical fields are absent;
the admitted view changes, but the historical payload does not.

Once published, an operation identity and major version are immutable semantic
contracts. An incompatible operand shape or behavioral change uses a new
operation identity or major version. A reader must either execute the exact
registered contract, invoke a reviewed explicit migration, or reject the
artifact. It must not guess a version from optional attributes or interpret an
old operation under new rules.

`mainframe-env.artifact@2` is the historical pre-`dialect_contracts` manifest
contract. Readers continue to support the documented historical artifact range
through a version-selected boundary that validates canonical IR and the legal
operation profile, derives the absent dialect set, and retains the original
payload and source-contract identity. Writers emit `mainframe-env.artifact@3`.
Legacy and current handlers may coexist during migration, but fallback between
them is explicit and tested.
Historical artifacts, checkpoints, conformance verdicts, and differential
evidence remain bound to the contract versions that produced them; a compiler
upgrade does not rewrite that history.

The product-owned COBOL reference environment currently admits the following
exact compatibility profile before installation, execution, or resume:

| Profile field | Admitted value/range | Decision |
| --- | --- | --- |
| environment profile | `mainframe-env.cobol.reference@1` | Host compatibility is owned by the product environment, not deferred to a provider effect |
| source artifact contract | `mainframe-env.artifact@2` or `mainframe-env.artifact@3` | `@2` is the retained pre-dialect-manifest reader; writers emit only `@3`; every other version is rejected |
| compiler generation and target | exactly `mainframe-env-cobol-0.8.3` and `reference` | No cross-generation semantic compatibility is inferred |
| normalized options | exactly `cobol.effective-arith` (`compatible` or `extended`), `cobol.effective-dispsign` (`compatible` or `separate`), and `cobol.effective-lp` (`32` or `64`) | Unknown, missing, or payload-config-mismatched semantic options are rejected |
| host ABI set | exactly `mainframe-env.host@1` and `mainframe-env.cics@1` | Capability and resource authorization still occur at dispatch |
| IR envelope | exactly `mainframe-env.ir-envelope@1` | Canonical bytes and the registered executable profile are revalidated |

For `@2`, the reader derives the missing dialect set only in the admitted
in-memory view; it preserves the original payload, content digest, source
contract, and recorded semantic identity. For `@3`, the declared dialect set
must equal the immutable payload's executable dialects. Persisted metadata is
canonically bound to its semantic identity and payload digest. A checkpoint
resume additionally requires the same artifact identity and recorded provider
generations before any terminal, exchange, coordinator, or provider mutation.

Artifact semantic identity includes frontend/compiler generation, normalized
options, dialect requirements, and host-interface requirements. Exact payload
identity remains separate. Checkpoint restoration validates the artifact and
required host interfaces before execution resumes.

## Incremental migration

Migration proceeds as vertical semantic slices rather than as a flag day:

1. Freeze the current contract and reviewed obligations for one small family,
   beginning with COBOL `ADD`/`COMPUTE` arithmetic.
2. Introduce typed HIR and executable operands for that family, retain source
   provenance, and remove runtime parsing of its static grammar.
3. Reuse at least one shared decimal, storage, or control primitive across two
   distinct operations while preserving their observable language rules.
4. Apply the same boundary to one host-integrated CICS file/unit-of-work family:
   compile static command options and bindings, evaluate dynamic values in the
   machine, and emit the existing typed effect through the coordinator.
5. Exercise a bounded second frontend or clearly different family against the
   same IR/effect framework without importing COBOL HIR or forking the
   coordinator/provider contracts.

The current family routes and retirement conditions are intentionally bounded:

| Family/forms | Current selected route | Deliberate compatibility scope | Cutover/deprecation criterion |
| --- | --- | --- | --- |
| COBOL `ADD` and `COMPUTE` with statically resolved numeric operands, receivers, and supported bounded expressions | Typed COBOL HIR to `mainframe.decimal@2.assign`; tests assert both operation selection and exact values/bytes | Runtime subscripts, special registers/intrinsics, and expressions beyond the current typed depth remain explicit `mainframe.core.cobol` operations | Model remaining dynamic references as typed deferred expressions with runtime bounds checks, verify their static shape, and land route-plus-result and mutation tests before removing the source-token route |
| COBOL `ADD CORRESPONDING` with unmodified, unsubscripted eligible group operands | COBOL HIR resolves relative-qualified, bilaterally unique pairs and emits `mainframe.decimal@2.assign` | A valid selected table-group subscript uses the explicit core-COBOL route; missing required subscripts, reference modification, UTF-8 groups in this slice, and numeric national-byte pairs fail before publication rather than falling through | Add a versioned typed selected-group reference/offset contract, runtime bounds checks, qualification/exclusion tests on both routes, and exact byte evidence before retiring this compatibility route |
| COBOL executable layout forms touched by typed plans | Scalar `PIC X`/`PIC U` DYNAMIC items and bounded fixed/ODO/unbounded tables retain the existing machine | DYNAMIC items with their own OCCURS, beneath any table, or participating in REDEFINES fail before HIR/publication because the current machine has neither per-occurrence dynamic buffers nor dynamic alias storage; omission of a DYNAMIC LIMIT remains an inherited unsupported frontend form | Add an explicit per-occurrence dynamic-storage and alias contract with execution/restart tests before admitting those combinations; separately materialize the baseline default limit before accepting omitted LIMIT |
| COBOL level-88 values reached by current execution | Numeric DISPLAY/PACKED/BINARY literals and increasing ranges plus bounded alphanumeric literals/figuratives are normalized and validated in the frontend and executable ABI | NATIONAL/DBCS/UTF-8, edited/group, `WHEN SET TO FALSE`, `ALL`, symbolic-character, and collation-dependent ranges fail closed rather than using incomplete raw-token behavior | Add explicit literal class/encoding/collation policies and byte-exact SET/condition tests before admitting each deferred class |
| Migrated CICS commands with statically resolved option direction and bindings | Typed CICS plan through the existing machine, coordinator, typed host request, and provider | Commands/options outside the declared pilot remain on their documented existing route; a malformed typed plan never falls back to token interpretation | Extend the authoritative descriptors and binding verifier with command-specific transition/recovery tests before moving each additional command family; do not infer completion of all CICS cics.application-api work |
| Published decimal operation/plan major `@1` | Read-only version-selected compatibility handler | Historical bytes and semantic identities remain immutable | Retain for the documented artifact range; removal requires an explicit compatibility/version decision and is independent of source-route cutover |

There is one default route for each recognized form: typed selection happens in
the frontend, and a malformed or unsupported typed artifact fails its declared
contract. Compatibility execution is selected only for the forms named above;
the runtime does not retry source-token interpretation after typed validation or
execution fails. Thus migration may be incremental without leaving two
interchangeable default interpretations indefinitely.

Every migrated slice requires parser recognition, static semantic validation,
HIR verification/legalization, exact value or byte execution, applicable
condition/failure behavior, provider integration where applicable, and
representative product-runtime mutation adequacy. Differential evidence uses a
trusted oracle when available and remains pending rather than synthetic when it
has not run.

## Consequences

- Compiler work increases because executable operands and policies must be
  modeled and verified instead of copied as statement strings.
- Runtime handlers become smaller semantic executors and reject malformed or
  incompatible typed operations before mutation.
- Shared primitives are reusable across frontends without making providers or
  the execution coordinator language-aware.
- Old artifact execution remains testable alongside new typed operations, so
  compatibility cost is visible rather than hidden in permissive decoding.
- Architecture and conformance checks must detect a return to runtime grammar
  discovery for each migrated family.

## Non-goals

- Rewriting all statements or subsystem commands at once.
- Replacing the execution coordinator, effect protocol, providers, or stores
  for architectural symmetry.
- Lowering every specialized semantic operation to the smallest primitive.
- Making runtime resource, value, authorization, or transaction decisions at
  compile time.
- Requiring complete PL/I, HLASM, or REXX support before the first typed slice.
- Treating local/reference execution as licensed IBM differential evidence.
