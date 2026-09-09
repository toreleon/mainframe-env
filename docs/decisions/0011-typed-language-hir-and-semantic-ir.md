# ADR-0011: Adopt language-specific HIR and versioned semantic IR dialects

Status: **Accepted by repository owner**
Owner: **compiler, execution, and subsystem maintainers**
Scope: **language HIR, executable semantic IR, host effects, and compatibility boundaries**
Applies from: **mainframe-env 0.9.0 development**

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

### JCL and resource DSLs remain separate

JCL lowers to a typed immutable job/workflow plan executed by JES and batch
services. It is not wrapped in a pretend program operation and is not forced
through the ordinary program machine.

BMS, CSD, and similar resource-definition languages compile to versioned
resource artifacts consumed by their owning subsystem. They may share source,
provenance, diagnostics, artifact lifecycle, security, store, and conformance
contracts without sharing a programming-language HIR or execution loop.

The initial adoption proof for this decision is the existing versioned JCL
`JobPlan`/JES path. BMS and CSD remain separate, subsystem-owned parser/resource
paths and are not represented as program operations; standalone serialization
of their current in-memory resource models is a later vertical slice, not a
claim made by the ADD/COMPUTE and CICS pilot migration.

### Compatibility is explicit and fail-closed

The current writer uses `mainframe-env.artifact@3`. Its manifest records the IR
envelope contract, an exact `dialect_contracts` set containing every executable
operation namespace/major pair in the payload, and every required host ABI.
Publication derives the dialect set from the payload and rejects a stale,
missing, or extra manifest entry. Operation identity includes dialect,
operation name, and semantic major version.

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
