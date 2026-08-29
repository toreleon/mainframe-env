# 0.1 Package Map

Status: **Accepted by repository owner, with the V0 consolidation below**

## Scope rule

The 0.1 workspace contains only packages required to deliver COBOL, CICS,
JCL/JES, datasets, RACF/security, z/OSMF, and their runtime infrastructure.

A current package outside that surface has no migration obligation. It is not
added as a placeholder, empty facade, optional feature, test dependency, or
future plugin stub.

## Package-boundary rule

A crate exists only when it enforces dependency direction, a versioned public
contract, an independently selected provider, or an application boundary.
Readability alone is handled with modules.

## Proposed 0.1 workspace

```text
crates/
  foundation/
    mainframe-env-source
    mainframe-env-diagnostics
    mainframe-env-encoding
    mainframe-env-ir
    mainframe-env-ir-codec

  contracts/
    mainframe-env-compiler-api
    mainframe-env-execution-api
    mainframe-env-host-api
    mainframe-env-store-api
    mainframe-env-cics-api

  kernel/
    mainframe-env-compiler
    mainframe-env-execution
    mainframe-env-interpreter

  cobol/
    mainframe-env-cobol-syntax
    mainframe-env-cobol

  batch/
    mainframe-env-jcl
    mainframe-env-jes

  providers/
    mainframe-env-dataset
    mainframe-env-cics
    mainframe-env-racf

  stores/
    mainframe-env-store-memory
    mainframe-env-store-sql
    mainframe-env-artifacts

  apps/
    mainframe-env-zosmf
    mainframe-env-server
    mainframe-env-cli

  tooling/
    mainframe-env-conformance
    xtask
```

The exact count may shrink when two proposed packages do not enforce a real
dependency boundary. It may grow only through an ADR demonstrating an in-scope
0.1 requirement.

## Accepted V0 consolidation

The machine package inventory consolidates the proposed map to 20 packages.
IR codecs remain with `mainframe-env-ir`; CICS contracts remain with
`mainframe-env-host-api`; execution coordination remains with the interpreter
kernel; COBOL syntax, semantics, HIR, and lowering remain private modules of the
compiler kernel; JCL and JES share the batch state authority; and
memory/SQL/artifact adapters share the store package. These units do not require
independent 0.1 publication or provider selection boundaries. ADR 0005 records
the decision, and the exact accepted mapping and justification is
`conformance/0.1/inventory/packages.json`.

## Foundation packages

| Package | Owns | Must not depend on |
|---|---|---|
| `mainframe-env-source` | source bytes, IDs, formats, maps, COPY/precompiler provenance | language semantics, Tokio, stores |
| `mainframe-env-diagnostics` | stable diagnostic/problem DTOs and codes | Miette renderers, gateway types |
| `mainframe-env-encoding` | CCSID/EBCDIC conversion and collation primitives | compiler/execution engines |
| `mainframe-env-ir` | in-memory IR, verifier, versioned text/binary/envelope codecs | COBOL AST, backends |

## Contract packages

| Package | Owns |
|---|---|
| `mainframe-env-compiler-api` | compiler stages, requests/results, legality, artifact descriptors |
| `mainframe-env-execution-api` | invocation, context, limits, outcomes, events, lifecycle identity |
| `mainframe-env-host-api` | dataset, program, JES/spool, terminal, security, clock, audit, and typed CICS requests/results |
| `mainframe-env-store-api` | execution, event, work, checkpoint, session, artifact metadata, idempotency stores |

These packages expose only mainframe-env-owned types and remain independent of
Axum, Tokio, SQLx, concrete providers, and current-workspace crates.

## Kernel packages

### `mainframe-env-compiler`

Owns pipeline planning, compiler registration, pass execution, legality gates,
artifact fingerprinting, publication, and bounded caches.

### `mainframe-env-interpreter`

Owns the deterministic reference MIR machine and common execution coordinator:
admission, run units, machine driving, suspension/resume, cancellation,
host-effect sequencing, and transactional lifecycle journaling. It depends on
IR and execution/host/store contracts, never on COBOL syntax or concrete
providers/stores.

### `mainframe-env-application`

Owns content-addressed application manifests, atomic install selection, named
program generations, BMS/CSD resource forms, and validated dataset catalogs.
Dataset catalogs represent exact organization, record-format, record-length,
key, PDS member-extension, and GDG roll-policy metadata through host contracts;
they contain no provider state or local corpus paths.

## COBOL compiler modules

### `mainframe-env-compiler::syntax`

Owns source formats, preprocessing, COPY expansion, lexer, lossless CST, typed
AST, syntax diagnostics, and source provenance.

### `mainframe-env-compiler::{semantic,hir,lower}`

Owns semantic analysis, layouts, storage/alias meaning, HIR, verification,
lowering, and compiler integration. Its lowering modules are grouped by control,
data, arithmetic, strings, files, program control, and CICS.

## Batch package

### `mainframe-env-batch::jcl`

Owns JCL syntax, procedure/symbol expansion, DD and step model, conditions,
workflow plan, and dispatch through `ProgramService`. It does not instantiate
utility or COBOL implementations directly.

### `mainframe-env-batch::service`

Owns job lifecycle, admission, job/step identity, spool/SYSOUT, cancellation,
purge, status, and JES-facing host operations. JCL describes workflow; JES owns
job execution state.

## Provider packages

### `mainframe-env-dataset`

Owns the 0.1 dataset/catalog authority and storage adapters required by accepted
fixtures. Key-sequenced records are addressed by stable primary identity;
alternate-index paths retain ordered alternate/base identities and duplicate
policy. A record mutation and every affected index generation commit in one
provider-state transaction. Filesystem paths remain provider-private. Callers
use typed dataset host requests and DD bindings.

### `mainframe-env-cics`

Owns typed CICS provider behavior required by the 0.1 operation inventory,
including terminal/session, file, program control, conditions, transactions,
and EIB outcomes. The provider persists pseudo-conversational continuations,
transient-data records, and idempotent syncpoint intent/result decisions;
unresolved decisions remain unknown outcomes until explicit reconciliation.
Protocol/UI implementations are not dependencies.

### `mainframe-env-racf`

Owns identity, authentication, SAF authorization, profiles, audit decisions,
and security conditions required by 0.1. Callers never access its database or
locks directly.

## Stores

| Package | Purpose |
|---|---|
| `mainframe-env-store` | bounded memory, SQLite/PostgreSQL metadata/state, and immutable local artifact adapters behind separate owned interfaces |

No messaging broker package is required for 0.1. Store state is authoritative;
in-process notifications are bounded and reconstructible.

## Applications

### `mainframe-env-zosmf`

Owns route DTOs and IBM-compatible protocol translation for accepted 0.1
information, job, dataset, and security/console surfaces. Handlers contain no
compiler, job, dataset, or RACF business authority.

### `mainframe-env-server`

Owns configuration loading, provider/store construction, readiness, lifecycle,
graceful shutdown, and Axum server startup.

### `mainframe-env-cli`

Provides local compilation, execution, inspection, and administration for the
same 0.1 services. It does not embed an alternate execution path.

## Tooling

`mainframe-env-conformance` owns 0.1 fixtures, current-workspace oracle
adapters, differential reports, protocol tests, and evidence models. It is not a
production dependency.

`xtask` owns deterministic code generation, architecture/profile checks,
schema checks, and release evidence orchestration.

## Explicitly absent from 0.1

No 0.1 package or feature is created for:

```text
ADABAS, CLIST, crypto provider pack, deployment generator, DRDA,
Easytrieve, FOCUS, Gym product, HLASM, IDMS, IMS, ISPF, MQ, MVS,
Natural, networking/TN3270, PL/I, program-management experiments,
REXX, standalone SMF pack, symbolic execution, system commands,
TSO product, TUI, USS, Wiki, WLM product policy, Wasm plugins,
process plugins, Cranelift, LLVM, or multi-node distribution.
```

A runtime dependency is admitted only when an accepted 0.1 selector reaches it
through the target architecture and no smaller owned contract satisfies the
need.

## In-scope migration matrix

Before implementation, only current packages/selectors contributing to the 0.1
surface receive migration rows:

```text
current package and selector
accepted 0.1 fixture
observable current behavior
mainframe-env target package
reimplement | port algorithm | replace | retire
compatibility oracle command
cutover/removal condition
owner
```

Out-of-scope current packages are recorded once as `excluded_from_v1` and are
not analyzed selector by selector.
