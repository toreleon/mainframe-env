# Execution Prompt — Implement mainframe-env 0.4.0

Target version: **0.4.0**
Completion dependencies: 0.3.0

Use this prompt from the repository root. The
[common execution contract](README.md#common-execution-contract) is normative.

---

You are implementing **mainframe-env 0.4.0: complete COBOL execution and
differential semantics** over the accepted 0.3 typed language model.

## Read and verify first

Read `docs/prompts/coverage-versions/README.md`,
`docs/delivery/coverage-versions/0.4.0.md`, the 0.3 coverage/evidence package,
the accepted 0.3 Conformance IR/verdict/ledger contracts,
compiler/interpreter/execution/host contracts, and the pinned Enterprise COBOL
6.5 receipt. Verify 0.3.0 has accepted all structural rows and artifact formats.

Do not reinterpret or bypass 0.3 nodes in a second parser. Any structural defect
found during execution work must be fixed at its single compiler authority and
must rerun the affected 0.3 gate.

## Implement in this order

1. Freeze deterministic value, storage, control-transfer, exception, I/O-effect,
   program-call, locale/CCSID, clock, and runtime-limit contracts.
2. Implement **CB-401** statement-family semantics and exact control flow.
3. Implement **CB-402/CB-403** intrinsic functions, decimal/floating/national/
   UTF-8/DBCS/pointer/object behavior, table/search/string/date/time semantics.
4. Implement **CB-404/CB-405** file/report/XML/JSON/locale behavior and bounded
   subsystem-neutral LE/runtime services.
5. Implement **CB-406** condition, resource, cancellation, restart, property,
   metamorphic, prior-artifact, and licensed differential suites.

## Approved 2026-09-02 completion policy

The user-approved 0.4 disposition is
`pass-with-licensed-differential-pending`. A licensed Enterprise COBOL 6.5
environment remains unavailable, so:

- preserve the licensed differential numerator at exactly 0/153 and keep the
  real licensed adapter fail-closed;
- run GnuCOBOL 3.2.0 out of process with `-std=ibm-strict` only for the explicit
  bounded portable allowlist, recording its exact binary/runtime identities,
  flags, locale, encoding, source format, normalization, fixture digest, exit
  statuses, observations, and candidate identity;
- label the result `reference=gnucobol`, grant it zero IBM differential credit,
  and never use it for IBM option/storage, national/UTF-8/DBCS, JSON/XML, LE,
  VSAM, or subsystem semantics;
- require exact independently projected mainframe-env-versus-reference
  comparisons, a portability rationale and pinned IBM source locator for every
  case, and representative harness mutants; and
- defer the real licensed 153-row campaign to the 0.17 release-certification
  hard gate, where it remains mandatory before 1.0.

## Reuse and architecture guardrails

- Before implementing decimal and floating arithmetic primitives, run a frozen
  semantic-gap spike against reviewed general-decimal/IEEE libraries, including
  precision, scale, rounding, traps/status, overflow, signed zero, NaN, and
  determinism. Reuse a fitting primitive library; do not write big-number or
  floating-point algorithms without a recorded rejection decision.
- COBOL packed/zoned/binary representation, PICTURE behavior, intermediate
  precision, compiler options, size-error conditions, aliasing, and IBM-visible
  results remain owned semantic adapters and must pass licensed differentials.
- Reuse the 0.2 host registry, effect protocol, checkpoint envelope, migration
  runner, and the 0.3 Conformance IR/runner. LE and host extensions add typed
  catalog rows and handlers rather than introducing a second invocation or
  conformance mechanism.
- Locale, code-page, date/time, sort/merge, and file adapters may use reviewed
  libraries internally, but normalize immediately to owned values and keep
  source bytes, CCSID, conditions, and replay identity explicit.

## Version-specific invariants

- All semantic execution is deterministic over explicit input/state/effects;
  providers and async I/O remain outside the interpreter kernel.
- Preserve COBOL storage, rounding, truncation, overflow, size-error, sign,
  comparison, collation, reference modification, alias, and control-flow rules.
- No unimplemented statement/function may execute as no-op or success.
- Host services are selected by typed ABI contracts, never copybook/program names.
- Resource exhaustion, cancellation, provider failure, invalid data, and runtime
  limits produce exact typed conditions without forbidden mutation.
- Extend the accepted 0.3 row specifications with execution, condition,
  recovery, and oracle obligations. Generate obligation-level verdicts and the
  ledger; never infer execution coverage from a broad interpreter/CardDemo
  pass.

## Completion gate

Do not finish until every pinned COBOL row executes all applicable behavior,
every applicable mutation/restart/recovery row passes, the approved bounded
GnuCOBOL campaign and harness mutants pass, and the complete 0.3
recognition/validation and 0.1.1 compatibility suites remain green. The
licensed Enterprise COBOL 6.5 positive, negative, boundary, condition, and
interaction differential stays explicitly pending at 0/153 under the approved
policy and is a hard 0.17 release-certification dependency.

At handoff, report coverage by all six gates, the GnuCOBOL reference receipt and
normalization rules, interpreter resource bounds, recovery results, exact
row/obligation/test verdict bindings, and the 0/153 licensed handoff. Never mark
the unavailable IBM oracle work passed.
