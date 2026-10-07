# mainframe-env Rewrite Charter

Status: **Accepted by repository owner**
Owner: **repository owner**
Scope: **product charter, initial boundary, and governance invariants**
Applies from: **mainframe-env current subsystem contracts**
Program identity: **mainframe-env**
Management model: **named subsystems and phases**
Delivery mode: **Greenfield rewrite with executable-oracle compatibility**

## Objective

Build a new, production-shaped mainframe environment around one coherent compiler,
execution, host-service, state, security, and product-profile architecture. The
first release supports only COBOL, CICS, JCL/JES, datasets, RACF/security,
z/OSMF, and their mandatory runtime dependencies. The resulting system must be
understandable by maintainers and AI agents, deterministic where semantics
require it, bounded under overload, recoverable after failure, and capable of
evolving through versioned contracts.

The current workspace is preserved as a behavioral oracle during development.
It is not linked into production mainframe-env binaries and does not dictate
mainframe-env internal organization.

## Why a greenfield rewrite

The current system contains valuable mainframe behavior and fixtures, but its
implementation has accumulated broad composition roots, wave-specific public
modules, duplicated authority, mixed compiler stages, large mixed-responsibility
modules, and migration paths that are difficult to reason about independently.

The rewrite is intended to remove those structural constraints rather than
copying them into cleaner filenames.

## Base profile scope

The platform.runtime-integration product scope is limited to:

- COBOL source, preprocessing, parsing, semantic analysis, HIR/MIR, compilation,
  and reference execution;
- typed CICS terminal, file, program-control, condition, transaction, and
  security interactions required by accepted platform.runtime-integration fixtures;
- JCL parsing and execution through JES job, step, spool, DD, and condition
  lifecycle;
- dataset behavior required by COBOL, CICS, JCL, JES, and z/OSMF fixtures;
- RACF/SAF identity, authentication, authorization, audit, and security
  conditions required by supported paths;
- z/OSMF information, job, dataset, console/security, and selected runtime
  routes;
- execution, compiler, host-service, artifact, configuration, store,
  observability, and compatibility infrastructure required by those paths; and
- deterministic conformance tooling for the platform.runtime-integration surface.

Every current selector within this scope receives one explicit disposition:

- **reimplement** in the mainframe-env architecture;
- **port algorithm** behind a new owned boundary without retaining old
  architecture;
- **replace** with a new provider; or
- **retire** from the platform.runtime-integration public surface through an explicit compatibility
  decision.

No in-scope selector or fixture is silently omitted. Unsupported in-scope
behavior remains explicit and diagnostic.

## Outside the base profile

The following current areas may be ignored completely and create no platform.runtime-integration
compatibility, packaging, build, documentation, or migration obligation:

- PL/I, REXX, CLIST, HLASM, Easytrieve, Natural, FOCUS, and other language
  packs;
- IMS, IDMS, ADABAS, MQ, USS, ISPF, TSO beyond an unavoidable selected runtime
  dependency, MVS experiments, and other subsystem packs;
- DRDA, TN3270 listeners, Ratatui/Crossterm TUI, Wiki, Gym product features,
  deployment generators, symbolic execution, native/JIT backends, and LLVM;
- external Wasm/process plugin ecosystem and distributed multi-node execution;
  and
- migration of a current crate solely because it exists.

An out-of-scope component is added later only through a new versioned product
decision. platform.runtime-integration must not contain speculative abstractions whose only consumer is an
out-of-scope future component.

## Non-negotiable invariants

1. mainframe-env production crates do not depend on current OpenMainframe crates.
2. Compiler and execution semantics do not depend on Tokio, Axum, SQLx,
   OpenTelemetry, or another infrastructure framework.
3. Every queue, mailbox, stream, cache, output, payload, nesting depth,
   checkpoint, and resource pool has a configured bound.
4. Normal program control is represented as data, never as an untyped error.
5. Host access is typed, capability-limited, authorized, observable, and
   idempotent or explicitly non-retryable where mutation occurs.
6. Durable delivery is at least once. The product never claims exactly-once
   execution across process or node failure.
7. Artifacts are immutable and content addressed by a canonical semantic
   fingerprint, not by unstable serialization bytes.
8. Executable IR is verified and fully legalized before publication and again
   at trust boundaries where required.
9. platform.runtime-integration uses statically linked, reviewed Rust implementations. Any later external
   plugin system has no ambient authority and does not use the native Rust
   dynamic-library ABI as a stable contract.
10. One observable authority exists for each selector, protocol state,
    configuration family, capability resolution family, and durable state
    class.
11. Product code contains no architectural names tied to a roadmap wave or
    compatibility fixture.
12. Historical compatibility evidence is preserved; prose cannot relabel a
    failed or scoped result.

## Product principles

### Deterministic semantics, asynchronous infrastructure

Compilers, validators, IR passes, interpreters, and state transitions are
deterministic functions over explicit input and state. Asynchronous runtime,
networking, persistence, scheduling, and provider I/O live in an outer shell.

### Owned contracts, replaceable frameworks

Public and durable types are owned by mainframe-env. External library types do
not cross stable boundaries. Frameworks may be upgraded or replaced without
changing mainframe semantics.

### Static base profile

Built-in Rust implementations establish the contracts and reference behavior.
Wasm components and supervised process plugins are not platform.runtime-integration deliverables.

### Reference interpreter before optimization

The deterministic interpreter is the platform.runtime-integration semantic oracle. Other execution
backends are outside the platform.runtime-integration product.

### Compatibility is observable

Every adapter, fallback, canary, rollback route, version window, and removal
condition is represented in machine-readable state. Hidden fallback is
prohibited.

## Success criteria

mainframe-env is ready for product cutover only when:

- all retained in-scope selectors have one accepted default authority;
- the `core-server` profile excludes optional UI, analysis, test, deployment, and
  compatibility packages from its dependency closure;
- the compiler and execution kernels pass deterministic replay, malformed
  input, resource, cancellation, failure, and compatibility gates;
- durable work survives the declared worker, coordinator, and store failures;
- every platform.runtime-integration package has ownership, support, versioning, and retirement policy;
  and
- the superseded default implementation is removed from the production
  workspace.

## Governance

The following changes require an ADR and compatibility impact statement:

- contract or schema version changes;
- new dependency direction between architectural layers;
- new durable state authority;
- new capability or permission family;
- new external plugin surface;
- semantic changes to an existing operation;
- change to delivery, retry, idempotency, or consistency behavior;
- product, contract, schema, migration, or release-channel compatibility
  changes; and
- removal of a selector, adapter, schema reader, or compatibility fixture.
