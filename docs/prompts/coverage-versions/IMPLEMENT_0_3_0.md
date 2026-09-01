# Execution Prompt — Implement mainframe-env 0.3.0

Target version: **0.3.0**
Completion dependencies: 0.2.0

Use this prompt from the repository root. The
[common execution contract](README.md#common-execution-contract) is normative.

---

You are implementing **mainframe-env 0.3.0: complete COBOL structure and type
system**. Deliver complete recognition and validation for the pinned Enterprise
COBOL 6.5 structure without claiming 0.4 execution semantics.

## Read and verify first

Read `docs/prompts/coverage-versions/README.md`,
`docs/delivery/coverage-versions/0.3.0.md`, the official COBOL baseline receipt
and normalized catalogs, `docs/architecture/CONFORMANCE-IR.md`,
compiler/IR/source/diagnostic contracts, and relevant ADRs. Verify accepted
0.2.0 receipts for catalog generation, packages, coverage, source provenance,
and handler identity before public integration.

If 0.2.0 is not accepted, only isolated grammar/catalog/fixture preparation is
allowed. Do not merge a new public compiler artifact format or claim coverage.

## Implement in this order

1. Freeze the shared minimal typed Conformance IR, bounded operation/predicate/
   observation registries, executable test binding, verdict event, derived
   coverage ledger, and `spec --check`/focused runner interfaces. This is the
   common foundation reused by every later subsystem minor.
2. Freeze lossless syntax-node, source-span, diagnostic, name, type, layout, and
   executable-blocking contracts.
3. Run **CB-301/CB-303** for lexer, preprocessing/directives, complete statement
   AST, options, scopes, and recovery; run **CB-302/CB-304** for divisions,
   declarations, clauses, types, layouts, aliases, and tables.
4. Implement **CB-305** from the reviewed intrinsic/special-register catalog,
   with exact signatures, type inference, context, and diagnostics.
5. Implement **CB-306** malformed, limit, recovery, deterministic-generation,
   prior-artifact compatibility, row-binding, verdict, and ledger suites.

## Reuse and architecture guardrails

- Extend the accepted Rowan-style lossless syntax and shared source/provenance,
  diagnostic, catalog-generation, and bounded-arena facilities. Do not create a
  second source model, diagnostic model, catalog compiler, or durable AST codec.
- COBOL fixed-column/contextual lexing, COPY/precompiler provenance, recovery,
  typed AST, layout, and IBM dialect validation remain compiler-owned. A
  Tree-sitter or parser-combinator frontend may support editor or bounded local
  grammars only; it cannot be compiler or publication authority.
- Evaluate an incremental-query framework only if an accepted consumer requires
  incremental analysis. Keep it behind the compiler service, convert all values
  to owned stages, and prove deterministic clean-build equivalence; otherwise do
  not add it speculatively.
- Generate function, statement, clause, directive, and special-register
  identities from the shared contract compiler and close their registry against
  explicit validators. Do not hand-maintain matching enums and lookup tables.
- Keep one generic Conformance IR. It contains typed bounded references to owned
  product drivers and observations, not Rust/shell snippets, arbitrary
  expressions, COBOL-specific duplicated algorithms, or process evidence.
- Generate or register executable cases from the formal rows. A broad compiler
  or CardDemo test contributes coverage only through explicit row/gate verdicts.
  Do not add per-review schemas or manually edited coverage pass counts.

## Version-specific invariants

- Cover 44 statement families, 82 intrinsic functions, 15 compiler-directing
  statements, five directive groups, ten file-description clauses, and seventeen
  data-description clauses with immutable row identities.
- Preserve every accepted token/form and bounded provenance through AST/HIR.
- Unsupported 0.4 runtime semantics must block executable publication with a
  typed diagnostic; never lower them to no-op or generic success.
- Keep CICS, Db2, IMS, MQ, LE, DFHAID, DFHBMSCA, SQLCA, and MQ ABI knowledge out
  of compiler ownership.
- One semantic owner integrates qualification, scopes, name resolution, types,
  PICTURE/USAGE, alignment, aliasing, and layouts.
- Every recognition or validation claim is traceable as official row -> typed
  Conformance IR case -> executable test -> verdict event -> derived ledger.
  Execution, recovery, and IBM differential gates owned by later versions remain
  explicitly pending rather than being rounded up or marked non-applicable.

## Completion gate

Do not finish until 44/44 statement families recognize valid forms and reject
invalid ones; 82/82 functions have exact signature/type metadata; every pinned
clause/directive validates; every syntax/semantic node has stable bounded
provenance; incomplete execution cannot publish; and accepted 0.1.1 artifacts
remain readable within the declared compatibility window.

Require `cargo xtask spec --check` to compile the complete COBOL specification,
registries, schemas, and test bindings quickly without product environments.
Run focused COBOL recognition/validation conformance during implementation, then
tier-3 complete compiler/source/IR diagnostics, malformed/boundary/property/fuzz,
artifact round-trip, and affected-scope repository validation once on the
unchanged minor candidate. Generate the ledger exclusively from verdict events.
At handoff, report denominators and recognition/validation numerators with exact
row/test bindings; leave execution/recovery/differential pending where 0.4 or
0.17 owns them.
