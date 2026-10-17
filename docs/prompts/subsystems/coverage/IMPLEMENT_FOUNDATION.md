# Execution Prompt — Coverage and conformance — Coverage authority

Subsystem: **coverage**
Phase: **foundation**

Completion dependencies: none

Use this prompt from the repository root. The
[common execution contract](../README.md#common-execution-contract) is normative.

---

You are implementing **mainframe-env coverage.foundation: coverage authority and
de-hardcoding foundation**. Continue until the complete coverage.foundation exit gate passes
or a genuine stop-the-line condition is recorded.

## Read and verify first

Read `docs/prompts/subsystems/README.md`,
`docs/delivery/subsystems/coverage/foundation-plan.md`,
`docs/research/IBM-OFFICIAL-COVERAGE-ROADMAP.md`,
`conformance/roadmap/ibm-official-coverage-roadmap.json`, and all referenced
architecture, evidence, application-package, profile, and public distribution contracts.

The accepted profile.carddemo source/evidence is the baseline. Record its exact identity
and inventory the current tree before editing. There is no earlier phase
dependency, but no profile.carddemo behavior may regress.

## Implement in this order

1. **CV-201/CV-202:** pin reviewed official receipts for all nine baselines;
   define normalized catalog rows, the six independent coverage gates, immutable
   evidence records, denominator checks, schemas, and deterministic validators.
2. Freeze the catalog, evidence, generated-identity, handler, application-package,
   source/ABI, program-registry, route-registry, and migration contracts.
3. Then implement **CV-203**, **CV-204**, **CV-205**, **CV-206**, **CV-207**, and
   **CV-208** in parallel-safe slices: generated semantic identities and
   `SubsystemHandlerRegistry`; versioned application-package sections; generic
   Db2 schema/row loading; installed batch controllers; subsystem-owned ABI
   libraries; generated route/program registries; and architecture scans.
4. Implement **CV-209:** ledger consistency, migration/rollback, full profile
   closure, and regression evidence.

## Reuse and architecture guardrails

- CV-201/CV-202 must freeze the four shared authorities used by every later
  phase: one contract/catalog compiler, one application-package trust/install
  runtime, one store/migration/artifact adapter family, and one
  conformance/oracle evidence harness. Do not leave each subsystem to recreate
  these facilities.
- Compile the normative Draft 2020-12 schemas with a reviewed validator and
  validate every catalog, package, evidence, migration, profile, and distribution
  artifact mapped by the schema gate. Handwritten top-level field checks are not
  schema validation.
- Use the accepted bounded canonical COSE_Mac0 profile through the reviewed
  library and existing HMAC-SHA256 trust owner; see ADR-0055 and the application
  package contract. Preserve exact retained verification without resigning or
  implicit profile conversion. Mainframe-env owns manifest/identity closure,
  generation selection, rollback and trust policy; MAC authentication does not
  provide public-key signing, nonrepudiation or arbitrary snapshot authentication.
- Keep immutable large-blob storage behind the artifact-store port and evaluate
  a reviewed multi-backend object-store library rather than implementing local,
  S3, Azure, or GCS clients. This phase need only ship the adapters required by
  its accepted profiles.
- Record dependency/license/MSRV and semantic-gap decisions in CV-201 evidence.
  Generated DTOs, schema libraries, signature libraries, and storage adapters
  remain replaceable and do not enter durable public contracts.

## Foundation invariants

- Eliminate all 29 audited Db2 and three batch production H1/H3 hits. CardDemo
  identities may remain only in application packages, fixtures, tests, and
  conformance assets.
- Move DFHAID, DFHBMSCA, SQLCA, and MQ compatibility assets out of the COBOL
  compiler into subsystem-owned, licensed-compatible ABI libraries.
- Generated catalog presence never increments semantic execution coverage.
- Application packages are bounded, signed/versioned, fully reference-validated,
  and atomically installed. Crash/retry cannot select a partial generation.
- Official z/OSMF-compatible routes and custom routes have separate namespaces.
- Do not implement future subsystem completeness merely to make this gate pass.

## Completion gate

Do not finish until:

- all nine official baselines parse with publication identity, digest, immutable
  denominator, provenance, and review status;
- coverage rows cannot become complete without every applicable gate;
- production H1 application-hardcode and application string-dispatch counts are
  exactly zero under deterministic scans;
- CardDemo must pass all 20 journeys, all 26 transaction requirements and all
  105 issue acceptance requirements through installed package data, not
  production branches; those gates remain pending until actually observed;
- the existing 260-test floor, PostgreSQL controls, CardDemo, live Zowe route,
  architecture/profile/schema/evidence and public distribution checks pass; and
- no compatibility numerator is credited solely from code generation.

At handoff, provide per-work-package evidence, before/after hardcode scan counts,
catalog/schema digests, migration/rollback results, full validation results, and
the exact unchanged candidate identity. After the full gate passes, push only the
assigned branch and open the pull request required by the common contract. Do
not tag, publish, or deploy.
