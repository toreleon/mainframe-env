# Subsystem engineering review

This review guide defines the risk boundaries to inspect when changing a
subsystem. The owning contracts, plans, fixtures, and tests define the exact
requirements. Check the current candidate and retain execution logs outside Git.

| Subsystem | Review focus | Authority |
|---|---|---|
| Execution and host effects | Post-dispatch uncertainty, durable audit, canonical replay identity, cancellation, and recovery | [Execution and durability](../architecture/EXECUTION-AND-DURABILITY.md) |
| Storage | Atomic journal invariants, hostile-record rollback, quotas, artifact publication, and backend parity | [Durable storage profile](../contracts/DURABLE-STORAGE-PROFILE.md) |
| Security | Typed SAF decisions before mutation, per-resource listing filters, secret cleanup, expiring sessions, and principal binding | [Security architecture](../architecture/PLUGIN-AND-SECURITY.md) |
| JES and HTTP composition | Worker ownership, real clocks, fencing, expired leases, effective deadlines, and graceful shutdown | [Operations](../runbooks/OPERATIONS.md) |
| CICS application APIs | Durable online execution, task and resource ownership, continuation state, and partial-command boundaries | [Application API status](../delivery/subsystems/cics/application-api-status.md) |
| Db2, IMS, and MQ | Independently owned provider rows, bounded state transitions, restart, replay, and source-defined conditions | [Package map](../architecture/PACKAGE-MAP.md) |
| Compiler and artifacts | Sealed type-state transitions, semantic identity versus payload digest, and verification before execution | [Artifact contract](../architecture/COMPILER-AND-IR.md) |
| Sandbox and agent access | Profile capability admission, workspace bounds, staged publication, credential routing, child-process cleanup, and independent application generations | [Mainframe Sandbox](../guides/MAINFRAME-SANDBOX.md) |
| Tooling and documentation | Complete test discovery, offline schema resolution, pinned dependencies, legal notices, accurate commands, and explicit external prerequisites | [Verification strategy](../delivery/VERIFICATION-STRATEGY.md) |

## Required validation

Run the affected subsystem's focused regressions and required policy checks.
Security and persistence changes need negative, restart, uncertainty, and backend
parity cases for the changed boundary. Schema checks must reject invalid inputs;
coverage checks must distinguish route presence from behavioral equivalence.

A local test result cannot establish licensed IBM equivalence. PostgreSQL,
CardDemo, Zowe, fuzz, load, and licensed campaigns require their stated inputs.
Declare any unavailable required environment in the change report. Do not carry
forward historical acceptance as validation of a new candidate.

## Public code and documentation review

Review public entry points manually alongside repository-wide convention and
policy checks. Formatting and filename consistency do not prove maintainability
or application completeness.

- Follow the [naming conventions](../../CONTRIBUTING.md#naming-and-code-conventions),
  retaining schema-pinned paths and compatibility identities.
- Check that each guide names its prerequisites, available profile capabilities,
  executable commands, state changes, and remaining limitations.
- Distinguish subsystem work records from acceptance on the current candidate;
  describe pending work by subsystem rather than historical release labels.
- Inspect duplication, large enums, deeply nested control flow, unnecessary
  copies, and mixed ownership in production code. Count production lines
  separately from colocated tests. Use the module-budget registry's existing
  split boundaries for legacy modules rather than raising their ceilings.
- Run strict Clippy on the pinned toolchain and report failures. A warning-only
  inventory is diagnostic; it does not satisfy the strict gate. Review API and
  layout implications before boxing public enum variants or changing contracts.
- Exercise real application routes for the advertised profile. A batch engine
  conformance run does not establish that a sandbox profile installs batch
  programs, datasets, authorities, or extension providers.

Keep review logs outside Git and state unresolved findings in the pull request.
Do not replace executable specifications or negative regressions with prose
claims, or suppress failures to describe a candidate as release-ready.

## CICS promotion boundaries

The [CICS command boundaries](CICS-COMMAND-BOUNDARIES.md) retain the PROGRAM,
RETRIEVE, and PURGE MESSAGE requirements. Consult the current application API
status before implementing or crediting a command family.
