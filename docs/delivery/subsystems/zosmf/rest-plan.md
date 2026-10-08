# z/OSMF — REST portfolio

Subsystem: **zosmf**
Phase: **rest**

Status: **Proposed**
Start gate: normalized operation catalog and backend ownership frozen; each adapter waits for its accepted backend
Completion dependencies: racf.security, dataset.data, jes.execution, cics.system-api
Estimate: 12–20 engineer-months

The [shared validation contract](../README.md#shared-validation-contract) and
[hardened slice acceptance](../../../prompts/subsystems/README.md#hardened-slice-acceptance)
apply, including early participant-contract and licensed-harness preparation.
These requirements do not themselves certify implementation or waive an exit gate.

## Outcome

Advertise and implement all 27 pinned z/OSMF 3.2 REST service families with
exact protocol, authorization, backend, failure, and recovery behavior.

## Owned scope

- Generate service-family, operation, method, route, media-type, schema,
  response, and error identities from the pinned official portfolio.
- Cover jobs, files/data sets, console, workflows, software management,
  configuration, systems, security-facing, capacity/performance, and remaining
  pinned service families through typed backend adapters.
- Implement exact authentication, SAF authorization, CSRF/cookie/token policy,
  headers, negotiation, pagination, async operations, cancellation, and audit.
- Keep product-compatible routes separate from custom mainframe-env management
  routes and advertise capabilities truthfully.

## Work packages

| ID | Deliverable |
|---|---|
| ZMF-1101 | Normalize 27 families/189 source headings; freeze operation/schema/error and backend-owner/obligation mapping |
| ZMF-1102 | Jobs, spool, console, workflow, and asynchronous-operation adapters |
| ZMF-1103 | Dataset, file, software, configuration, and systems adapters |
| ZMF-1104 | Authentication, SAF, CSRF, negotiation, pagination, and audit |
| ZMF-1105 | Custom-route separation, capability discovery, and compatibility |
| ZMF-1106 | Protocol, failure, scale, cancellation, and IBM differentials |

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

## Parallelization

Service-family adapters can run in parallel after generated route and error
contracts freeze. Authentication, error normalization, and route publication
remain shared authorities. An adapter may be built before a backend version is
released, but cannot be advertised until that backend passes its exit gate.

zosmf.rest can overlap with db2.programming–mq.programming completion work. Its final full-profile gate is
serialized over one exact route catalog and one accepted backend closure.

## Exit gate

- 27/27 service families close every required normalized operation and mandatory
  obligation through accepted typed backends; no placeholder or generic-success
  path or unowned required operation remains.
- Exact status, header, media-type, schema, pagination, authn/authz, async,
  cancellation, malformed-input, resource-bound, and recovery matrices pass.
- Custom extensions cannot shadow or be mistaken for IBM-compatible routes.
- Licensed z/OSMF 3.2 differentials pass for every pinned family.
- Route/profile closure, OpenAPI artifacts, and capability reports are generated
  from the same candidate identity.

## Non-goals

- z/OSMF plug-ins or service families not present in the pinned 27-family
  official inventory.
