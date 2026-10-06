# Coverage and conformance — Coverage authority progress

Subsystem: **coverage**
Phase: **foundation**

Status: **Complete implementation candidate**

## Implemented boundaries

The shared Conformance IR separates official catalog rows, obligations,
executable bindings, verdicts, and licensed-credit policy. Nine pinned IBM
baselines define the 1,506 mandatory catalog identities. Publication bodies
remain in an external cache; the repository retains bounded locators, topic
manifests, schemas, and fixture specifications.

| Work package | Owned implementation |
|---|---|
| CV-201 | Pinned topic manifests and normalized official catalogs |
| CV-202 | Independent six-gate coverage rows and append-only verdict storage |
| CV-203 | Generated semantic identities and explicit handler registry |
| CV-204 | Signed package generations, reference validation, atomic selection, and rollback |
| CV-205 | Catalog-driven Db2 and authorization routes |
| CV-206 | Typed batch controllers decoded from selected signed packages |
| CV-207 | Provider-owned compatibility source libraries for CICS, Db2, and MQ |
| CV-208 | Generated utility/system-service and official/custom route registration |
| CV-209 | Cross-subsystem validation and non-destructive migration boundaries |

Regression requirements include non-destructive Db2 install, upgrade, and
rollback; primary-key integrity; HMAC-verified package publication; exact
controller artifact binding; durable restart; raw Db2 bytes and defaults;
compiled JSON Schema validation; signed program identities; allocation-safe
package preflight; and bounded package/controller admission. Specifications,
fixtures, and executable tests remain in the owning subsystem folders.

## Validate the current candidate

```bash
cargo xtask spec --check
cargo xtask coverage --check
cargo xtask semantic-identities --check
cargo xtask application-packages --check
cargo xtask abi-libraries --check
cargo xtask dehardcoding --check
```

Run applicable backend, CardDemo, and licensed checks with their required inputs.
Historical release identities, execution receipts, and acceptance tables are
removed. Keep fresh output outside Git and describe its candidate and scope in
the change report. A local fixture, catalog, or source review cannot establish
licensed IBM equivalence.

## Source and compatibility boundaries

All source authority comes from the pinned IBM topic manifests under
`conformance/subsystems/coverage/manifests/`. Raw topic bytes and retrieval
observations remain outside the repository. The topic reader distinguishes
republished or unexplained changes from an older cached revision; an ordinary
review never silently re-pins a topic. See the
[publication source investigation](../../../research/publication-source-probe.md)
and [cache runbook](../../../runbooks/IBM-DOCS-CACHE.md).

The original z/OSMF heading-level denominator remains frozen. Endpoint
normalization is owned by the z/OSMF subsystem and creates its own source-bound
projection; it does not rewrite the shared catalog's identities.
