# COBOL — Grammar and types

Subsystem: **cobol**
Phase: **structure**

Status: **Proposed**
Start gate: coverage.foundation catalog, generation, package, and coverage contracts frozen
Completion dependencies: coverage.foundation
Estimate: 10–15 engineer-months

## Outcome

Recognize and validate the complete pinned Enterprise COBOL 6.5 language
structure without claiming execution completeness. The compiler must produce a
typed, lossless, bounded model for every official form.

## Owned scope

- Introduce the shared thin typed Conformance IR v1, mandatory obligation model,
  executable row/obligation/gate binding, replayable canonical verdict events,
  derived coverage ledger, deterministic sharding/cache identity, fast spec
  compiler, and focused subsystem runner used by all later phases.
- Complete grammar and typed AST for all 44 PROCEDURE DIVISION statement
  families.
- Catalog all 82 intrinsic functions, 15 compiler-directing statements, five
  compiler-directive groups, ten file-description clauses, and seventeen
  data-description clauses.
- Complete scope, qualification, repository, nested program, method/function,
  declarative, file, table, pointer/object, national/UTF-8/DBCS, floating,
  dynamic-length, external/global, and local-storage semantic models.
- Complete PICTURE/USAGE, alignment, alias, condition, table, and file layout
  validation.
- Produce executable-blocking diagnostics for rows whose runtime semantics are
  intentionally assigned to cobol.execution rather than publishing partial IR.
- Remove the last host-product ABI knowledge from the COBOL compiler.

## Work packages

| ID | Deliverable |
|---|---|
| CI-300 | Shared thin Conformance IR/obligation/runner/ledger foundation |
| CB-301 | Complete lexer, preprocessor, and directive grammar |
| CB-302 | Complete divisions, declarations, clauses, and scopes |
| CB-303 | Complete statement AST and option legality |
| CB-304 | Complete data classes, usages, layout, aliases, and tables |
| CB-305 | Complete function/special-register catalog and type inference |
| CB-306 | Malformed, limit, recovery, prior-artifact, row-binding, verdict, and ledger suites |

## Parallelization

Complete and accept CI-300 first. CB-301/CB-303 and CB-302/CB-304 can then run as
two compiler sublanes after AST node and source-provenance contracts freeze.
CB-305 can proceed independently from the generated function catalog. One
semantic owner must integrate name resolution and layouts.

Freeze the generic Conformance IR/obligation/verdict/ledger contract in the
small CI-300 milestone. The racf.security, dataset.data, and jcl.planning lanes may prepare catalogs and
product code in parallel; after CI-300 is accepted they may integrate claims
against it without waiting for the remaining COBOL work. They may not create
subsystem-local conformance frameworks.

This version can be built in parallel with racf.security RACF, dataset.data dataset, and jcl.planning JCL.
It shares no provider state with those versions.

## Exit gate

- 44/44 statement families recognize valid forms and reject invalid forms.
- 82/82 intrinsic functions have exact signature/type metadata.
- All pinned clauses and directives are recognized and validated.
- Every AST/HIR node has bounded source provenance and stable identity.
- Recovery or execution-incomplete nodes cannot publish executable artifacts.
- No CICS, Db2, IMS, MQ, or LE copybook/layout is owned by the compiler.
- Existing profile.carddemo artifacts remain readable under the declared compatibility
  window.
- Every claimed recognition/validation gate is connected by official row ->
  typed row specification -> mandatory obligation -> executable binding ->
  verdict event -> generated ledger.
- `spec --check` rejects unknown rows, missing/duplicate bindings, stale spec
  versions, incompatible gates, incomplete obligation/shard sets, unsafe cache
  reuse, and manually asserted pass counts.
- Product behavior never dispatches on conformance IDs, expected observations
  are not derived from the compiler implementation under test, and
  representative harness mutants are killed.
- Every failed generated/property case reports a deterministic replay command,
  IBM source locator, row/obligation/gate, fixture or seed, and bounded
  expected/actual observations.
- Execution, recovery, and differential gates assigned to cobol.execution/certification.licensed remain
  explicitly pending.

## Non-goals

- Claiming all statements/functions execute; that belongs to cobol.execution.
- Native/JIT optimization.
- CICS, SQL, DLI, or MQ provider semantics.
- A general predicate/expression or workflow DSL, shell-in-spec, subsystem-local
  IRs, and per-row/obligation committed verdict files.
