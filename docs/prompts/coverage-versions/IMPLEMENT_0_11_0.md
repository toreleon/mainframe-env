# Execution Prompt — Implement mainframe-env 0.11.0

Target version: **0.11.0**
Completion dependencies: 0.5.0, 0.6.0, 0.8.0, 0.10.0

Use this prompt from the repository root. The
[common execution contract](README.md#common-execution-contract) is normative.

---

You are implementing **mainframe-env 0.11.0: complete z/OSMF 3.2 REST
portfolio** for all 27 pinned service families.

## Read and verify first

Read `docs/prompts/coverage-versions/README.md`,
`docs/delivery/coverage-versions/0.11.0.md`, the generated route/operation/error
catalog, gateway/security/protocol contracts, and accepted evidence for 0.5.0,
0.6.0, 0.8.0, and 0.10.0. Verify required backend capability versions and route
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

Do not finish until 27/27 service families have reviewed executable routes with
no placeholder/generic-success path; exact protocol, schema, pagination,
authn/authz, async, cancellation, malformed, bound and recovery matrices pass;
custom routes remain separate; licensed z/OSMF 3.2 differentials pass; and route,
profile, OpenAPI and capability artifacts share one candidate identity.

At handoff, provide per-family/per-operation counts, collision and closure
reports, protocol/security/recovery evidence, oracle receipts, generated artifact
digests and full unchanged-candidate validation.
