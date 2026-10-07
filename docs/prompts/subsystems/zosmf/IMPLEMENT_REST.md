# Execution Prompt — z/OSMF — REST portfolio

Subsystem: **zosmf**
Phase: **rest**

Completion dependencies: racf.security, dataset.data, jes.execution, cics.system-api

Use this prompt from the repository root. The
[common execution contract](../README.md#common-execution-contract) is normative.
Apply its [hardened slice acceptance](../README.md#hardened-slice-acceptance),
[early participant contract](../README.md#early-transaction-participant-contract),
and [licensed-harness preparation](../README.md#licensed-harness-preparation)
requirements alongside the version-specific boundaries below.

---

You are implementing **mainframe-env zosmf.rest: complete z/OSMF 3.2 REST
portfolio** for all 27 pinned service families.

## Read and verify first

Read `docs/prompts/subsystems/README.md`,
`docs/delivery/subsystems/zosmf/rest-plan.md`, the generated route/operation/error
catalog, gateway/security/protocol contracts, and accepted evidence for racf.security,
dataset.data, jes.execution, and cics.system-api. Verify required backend capability versions and route
closure before advertising any operation.

Route catalog/schema and private adapters may be prepared earlier in an isolated
lane, but no route may publish against a placeholder or incomplete backend.

## Implement in this order

1. Implement **ZMF-1101** generated 27-family service, method, route, operation,
   media-type, schema, response, and error catalog with collision checks.
2. Freeze shared HTTP, authentication, SAF, CSRF/cookie/token, negotiation,
   pagination, async-operation, cancellation, correlation and audit contracts.
3. Implement **ZMF-1102/ZMF-1103** jobs/spool/console/workflow and dataset/file/
   software/configuration/systems adapters through typed accepted backends.
4. Implement **ZMF-1104/ZMF-1105** exact protocol/security behavior, capability
   discovery, OpenAPI generation, and strict custom-route namespace separation.
5. Implement **ZMF-1106** malformed, protocol, authn/authz, failure, timeout,
   cancellation, scale, bounds, restart and licensed differential suites.

## Operation normalization and backend ownership

ZMF-1101 is not complete with a family list or generated HTTP handlers. The
pinned catalog contains 27 service families and 189 guide-heading rows marked
`heading-only-pending-endpoint-normalization`; those headings are not 189
proven distinct executable endpoints. Preserve source provenance and normalize
headings, aliases, overviews and asynchronous operation lifecycles explicitly.

Before broad ZMF-1102/ZMF-1103 work, freeze a reviewed mapping from each source
locator to normalized operation, method/path/request/response/error schema,
backend owner and capability version, authorization intent, state/failure model,
existing work package/slice and mandatory Conformance IR obligations. Resolve
ownership for cloud provisioning, software management, sysplex, TSO/E address
spaces, WLM pooling and RMF metering as well as jobs, files, console and workflows.
The listed SAF/data/JES/CICS dependencies do not by themselves establish these
additional backend capabilities. Record concrete existing owners or explicit
prerequisite implementation slices; do not claim absent or accepted backends
without inspecting the current source and evidence.

A missing required backend blocks its adapter's public integration and remains
pending. Truthful unsupported responses are necessary failure behavior but do
not satisfy required execution. ZMF-1106 must close every required normalized
operation and obligation across all 27 families, not merely the advertised
subset. Keep one catalog/codegen and typed backend authority, without shadow
administrative state or a second asynchronous-operation scheduler.

## Reuse and architecture guardrails

- Keep the pinned IBM route/schema/error catalog as the normative authority.
  Use reviewed OpenAPI 3.1 and Axum integration libraries to assemble and check
  artifacts, but reject code-first drift: the generated OpenAPI document must
  diff cleanly against the pinned catalog and candidate identity.
- Compile request/response schemas with the shared Draft 2020-12 validator and
  validate positive, negative, malformed, bounded, and content-negotiation
  fixtures. Derive macros and generated Rust types do not replace runtime schema
  or cross-field validation.
- Reuse Axum, Tower, Tower HTTP, Rustls, the common principal/SAF authority,
  execution/work stores, cancellation, audit, pagination/continuation identity,
  route registry, and evidence harness. Do not hand-build HTTP parsing, TLS,
  generic middleware, or another async-operation scheduler.
- HTTP/OpenAPI framework types stop at the gateway. Exact z/OSMF statuses,
  headers, media types, cookies/CSRF policy, errors, async outcomes, and route
  separation remain owned compatibility DTOs and handlers.

## Version-specific invariants

- The same reviewed catalog generates router registration, capability reports,
  schemas/OpenAPI, coverage identities, and collision gates.
- Product-compatible routes and custom mainframe-env management routes cannot
  shadow, alias, or impersonate one another.
- Exact statuses, headers, media types, body schemas, pagination and errors are
  semantic behavior; successful JSON shape alone is insufficient.
- Authentication failure, denied SAF decision, missing capability, saturation,
  cancellation or indeterminate backend outcome must fail closed and audit safely.
- No direct store/provider access bypasses the typed backend authority.

## Completion gate

Do not finish until 27/27 service families close every required normalized
operation and mandatory obligation through reviewed executable routes and
accepted backends, with no placeholder/generic-success path; exact protocol, schema, pagination,
authn/authz, async, cancellation, malformed, bound and recovery matrices pass;
custom routes remain separate; licensed z/OSMF 3.2 differentials pass; and route,
profile, OpenAPI and capability artifacts share one candidate identity.

At handoff, provide per-family/per-operation counts, collision and closure
reports, protocol/security/recovery evidence, oracle receipts, generated artifact
digests and full unchanged-candidate validation.
