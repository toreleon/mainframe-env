# Conformance inputs

Specifications, fixture inputs, and executable checks are organized by subsystem.
Pinned IBM product baselines and contract schema revisions are compatibility
identities, independent of framework package versions.

| Path | Owns |
|---|---|
| `subsystems/platform/` | Shared profiles, contracts, inventory, and foundational fixtures |
| `subsystems/coverage/` | Official source catalogs and coverage specifications |
| `subsystems/cobol/` | Structural and execution fixtures |
| `subsystems/racf/`, `dataset/`, `jcl/`, `jes/` | Security, dataset and batch specifications |
| `subsystems/cics/`, `zosmf/`, `ims/`, `mq/` | Provider and gateway catalogs, schemas, fixtures, and source tools |
| `subsystems/integration/`, `certification/` | Transaction and licensed harness contracts |
| `profiles/carddemo/` | Workload inventory, resource definitions, and operator fixtures |
| `spec/` | Typed conformance specifications and shared test bindings |
| `tools/` | Shared pinned-source and fixture tooling |
| `standards/` | Retained third-party compatibility schemas |

Subsystem ownership, plans, status, and dependencies are registered in
[documentation-registry.json](../docs/documentation-registry.json). See the
[subsystem index](../docs/delivery/subsystems/README.md) and
[verification workflow](../docs/runbooks/VERIFICATION-WORKFLOW.md).

Historical execution receipts and release records are removed. Generate fresh
results outside Git; report current scope, blockers, and unavailable environments
in the owning status document. Source locators, publication hashes, schema
contracts, and expected fixture values remain reproducible inputs. A local test
or source catalog never supplies licensed IBM differential credit.
