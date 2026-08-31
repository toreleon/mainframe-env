# Execution Prompt — Implement mainframe-env 0.7.0

Target version: **0.7.0**
Completion dependencies: 0.2.0

Use this prompt from the repository root. The
[common execution contract](README.md#common-execution-contract) is normative.

---

You are implementing **mainframe-env 0.7.0: complete JCL converter and planner**.
Produce a lossless, typed execution plan; JES runtime execution belongs to 0.8.

## Read and verify first

Read `docs/prompts/coverage-versions/README.md`,
`docs/delivery/coverage-versions/0.7.0.md`, the pinned JCL/JES2 JECL catalogs,
source/diagnostic/application-package contracts, and current batch parser tests.
Verify accepted 0.2 catalog, diagnostic, provenance, plan, and generation
receipts. Coordinate DD vocabulary with the 0.6 lane without depending on its
runtime completion.

## Implement in this order

1. Freeze plan-node, source-span, diagnostic, symbol, procedure, statement,
   parameter, capability, and generated-identity schemas.
2. Implement **JCL-701/JCL-702** lexer, continuation, in-stream data, comments,
   delimiters, all statement forms, and generated parameter catalogs.
3. Implement **JCL-703** procedures, INCLUDE/search, symbols and substitution
   timing, overrides, backward references, nesting, and provenance.
4. Implement **JCL-704/JCL-705** exact JOB/EXEC/DD/OUTPUT and JES2 JECL
   validation plus immutable typed planning and capability requirements.
5. Implement **JCL-706** malformed, boundary, recovery, scale, deterministic
   plan, compatibility, and licensed converter differential suites.

## Reuse and architecture guardrails

- Reuse the compiler's source-byte/provenance, Rowan-style syntax, diagnostics,
  bounded arenas, generated identities, and contract compiler. Do not create a
  second generic parser framework or source-position authority for JCL.
- JCL columns, continuations, in-stream data, symbols, procedure search,
  overrides, backward references, and JECL timing remain a JCL-owned converter.
  Parser-combinator libraries may implement bounded local operand grammars only.
- Use one reviewed graph/cycle utility for INCLUDE/procedure dependencies and
  plan validation, but convert results to the owned immutable job-plan schema
  with deterministic order and source provenance.
- The generated statement/parameter inventory must also drive validation,
  documentation, coverage rows, and planner-closure checks. Do not hand-code
  parallel operand tables or accept parsed-but-ignored parameters.

## Version-specific invariants

- Cover exactly 20 statement forms, 74 DD, 19 EXEC, 35 JOB, 76 OUTPUT
  parameters, and 13 JES2 JECL statements from reviewed generated catalogs.
- Preserve source provenance through expansion and overrides; diagnostics point
  to both use and definition where required.
- Every accepted operand changes the plan or produces an explicit unsupported-
  capability diagnostic. Do not silently discard parameters.
- Planning performs no scheduling, utility summary, dataset mutation, or JES
  success simulation.
- Procedure and symbol expansion are bounded and cycle-safe.

## Completion gate

Do not finish until the exact denominators above are recognized and validated;
procedures, symbols, overrides, conditions, provenance, malformed input and
recovery pass their full matrices; valid plans are stable and immutable; and
existing CardDemo JCL produces an equivalent typed plan.

At handoff, report counts separately for each statement/parameter family,
generated catalog and plan-schema digests, explicit deferred capabilities,
differential results, and full validation on the unchanged candidate. Do not
credit JES execution or utility semantics in this version.
