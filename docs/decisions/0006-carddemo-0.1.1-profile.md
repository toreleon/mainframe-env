# ADR-0006: Add an additive CardDemo-full profile for 0.1.1

Status: **Accepted by repository owner**
Owner: **repository owner**
Scope: **additive CardDemo-full product profile and package boundaries**
Applies from: **mainframe-env 0.1.1**
Target product: **mainframe-env 0.1.1**
Supersedes: the 0.1 exclusions only for the optional `carddemo-full` profile

## Context

The 0.1 implementation froze the 31-program AWS CardDemo base corpus mainly as
an operation inventory. It did not install, compile, launch, or execute the
application. The owner has now requested a 0.1.1 enhancement that can execute
the complete clean local CardDemo repository at commit
`59cc6c2fd7ebd7ef7925cad552a01a4b8b6e4d5e`.

The complete repository includes more than the base CICS screens. It also
contains the base batch cycle, two assembler compatibility routines, Db2
transaction-type management, IMS/Db2/MQ authorization processing, VSAM/MQ
request-response programs, extra utilities, operator scripts, and two runtime
archives. Treating a login-screen pass as full-repository support would be a
false completion claim.

The current product identity remains `0.1.0-alpha.0`. This ADR authorizes the
0.1.1 implementation target, but it does not authorize skipping the 0.1.0
release ordering gate, publishing artifacts, tagging, pushing, or deployment.

## Decision

### Additive profiles

0.1.1 adds the following cumulative profiles without removing or redefining the
existing `core-server` profile:

1. `carddemo-base-online`: base CICS transactions, BMS maps, VSAM data, program
   transfer, application sign-on, and interactive terminal access.
2. `carddemo-base`: base online plus the declared initialization and operational
   batch cycle, GDG/PDS and selected utilities.
3. `carddemo-db2`: transaction-type Db2 online and batch extension.
4. `carddemo-mq`: VSAM/MQ date and account request-response extension.
5. `carddemo-authorization`: IMS/Db2/MQ pending-authorization online and batch
   extension, including cross-resource commit/rollback behavior.
6. `carddemo-full`: all profiles above plus the bounded operator compatibility
   routes needed to drive the repository workflows.

Each lower profile must remain independently testable. A higher profile cannot
turn an unavailable dependency into generic success.

### Meaning of full execution

`carddemo-full` means:

- every included COBOL source closure either publishes through the owned
  compiler or has an explicit accepted source-level correction;
- every declared reachable transaction, program, map, file, queue, database,
  job, procedure, and dataset has one installed authority;
- the base online and batch journeys and all three optional extension journeys
  execute through product entry points;
- terminal, dataset, SQL, DLI, MQ, JES, condition, EIB, COMMAREA, linkage, and
  transaction observations are compared against pinned application evidence;
- restart, cancellation, authorization, overload, and mutation-recovery tests
  pass on the same routes; and
- unsupported or orphan upstream artifacts are named explicitly and cannot be
  counted as executed.

Historical helper scripts and prebuilt migration binaries are oracle and
operator inputs. They are not linked into production. Where a repository script
depends on a bounded standard protocol, 0.1.1 provides a compatible adapter or
an owned equivalent command with an explicit compatibility decision.

### Generic application packaging

Production code must not hard-code CardDemo behavior. Add an owned, generic
application-package contract that can carry source bundles, copybook libraries,
BMS and CSD resources, data catalogs, seed objects, program/transaction
registrations, profiles, and installation migrations. The CardDemo descriptor
and fixtures live under `conformance/0.1.1/` or another non-core application
asset boundary.

Application installation is content-addressed, transactional, idempotent, and
versioned. A partially installed application is never ready.

### Compiler and runtime authority

The accepted compiler architecture remains authoritative. 0.1.1 completes the
language-aware preprocessor, scoped data model, typed AST, structured control
flow, storage/linkage semantics, file semantics, and embedded-host lowering
actually reached by the pinned corpus. It must not add fixture-output shortcuts.

COBDATFT, MVSWAIT, CEEDAYS, and CEE3ABD may be implemented as owned compatible
program/runtime services when their reached behavior is smaller and safer than
adding a general HLASM or Language Environment implementation. The source and
behavioral disposition remains explicit.

### New provider and protocol boundaries

The existing `mainframe-env-host-api` gains additive typed Db2, IMS/DLI, and MQ
request/result families. Independent provider-selection boundaries justify
three provider packages:

- `mainframe-env-db2`;
- `mainframe-env-ims`; and
- `mainframe-env-mq`.

The generic application-package contract justifies
`mainframe-env-application`. Original-client compatibility justifies bounded
`mainframe-env-tn3270` and FTP/JES gateway packages only when their accepted
workloads require those protocols. This is a documented split under ADR-0005,
not permission for broad subsystem emulation.

### Oracle policy

The clean local CardDemo checkout is an out-of-process/application-data oracle.
The Micro Focus and UniKix archives may provide catalog, screen, data, and
behavioral observations. Their native objects and shared libraries are never a
production fallback or dependency.

The orphan base CSD mapping `CDV1 -> COCRDSEC` has no corresponding source or
runtime object in the pinned repository. It remains a stop-the-line scope
decision: provide the missing accepted source, or record an owner-approved
retirement/disabled-resource correction before `carddemo-full` can pass.

## Compatibility and release consequences

- Existing 0.1 selectors, contracts, artifacts, checkpoints, configuration,
  datasets, and routes remain readable and behaviorally compatible.
- Additive Rust enum or durable-schema work must use a negotiated contract
  version where exhaustive old readers would otherwise break.
- The 0.1.1 version is not written to `VERSION`, Cargo manifests, or release
  artifacts until the implementation and release-ordering gates pass.
- No CardDemo compatibility claim is derived from inventory counts alone.
- Every implementation issue uses one focused local commit and its own
  regression evidence before the cumulative gate advances.
