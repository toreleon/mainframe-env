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
and normalized catalogs, compiler/IR/source/diagnostic contracts, and relevant
ADRs. Verify accepted 0.2.0 receipts for catalog generation, packages, coverage,
source provenance, and handler identity before public integration.

If 0.2.0 is not accepted, only isolated grammar/catalog/fixture preparation is
allowed. Do not merge a new public compiler artifact format or claim coverage.

## Implement in this order

1. Freeze lossless syntax-node, source-span, diagnostic, name, type, layout, and
   executable-blocking contracts.
2. Run **CB-301/CB-303** for lexer, preprocessing/directives, complete statement
   AST, options, scopes, and recovery; run **CB-302/CB-304** for divisions,
   declarations, clauses, types, layouts, aliases, and tables.
3. Implement **CB-305** from the reviewed intrinsic/special-register catalog,
   with exact signatures, type inference, context, and diagnostics.
4. Implement **CB-306** malformed, limit, recovery, deterministic-generation,
   and prior-artifact compatibility suites.

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

## Completion gate

Do not finish until 44/44 statement families recognize valid forms and reject
invalid ones; 82/82 functions have exact signature/type metadata; every pinned
clause/directive validates; every syntax/semantic node has stable bounded
provenance; incomplete execution cannot publish; and accepted 0.1.1 artifacts
remain readable within the declared compatibility window.

Run complete compiler/source/IR diagnostics, malformed/boundary/property/fuzz,
artifact round-trip, architecture, coverage-ledger, and full repository gates.
At handoff, report denominators and recognition/validation numerators separately;
leave execution/recovery/differential pending where 0.4 owns them.
