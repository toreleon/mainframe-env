# Subsystem delivery plans

Status: **Subsystem delivery tracking**

The [documentation registry](../../documentation-registry.json) owns each
subsystem, phase, plan, progress record, implementation prompt, and completion
dependency. Work is tracked by these names rather than release milestones.
Specifications, fixtures, and tests live in `conformance/subsystems/<name>/`;
workload-specific inputs live in `conformance/profiles/<name>/`.

## Subsystem plans

<!-- BEGIN GENERATED SUBSYSTEM INDEX -->
| Subsystem | Phase | Plan | Progress |
|---|---|---|---|
| Coverage and conformance | Coverage authority | [Plan](coverage/foundation-plan.md) | [Progress](coverage/foundation-status.md) |
| COBOL | Grammar and types | [Plan](cobol/structure-plan.md) | [Progress](cobol/structure-status.md) |
| COBOL | Execution semantics | [Plan](cobol/execution-plan.md) | [Progress](cobol/execution-status.md) |
| RACF / SAF | Commands and authorization | [Plan](racf/security-plan.md) | [Progress](racf/security-status.md) |
| Datasets / VSAM / AMS | Dataset services | [Plan](dataset/data-plan.md) | [Progress](dataset/data-status.md) |
| JCL | Converter and planner | [Plan](jcl/planning-plan.md) | [Progress](jcl/planning-status.md) |
| JES2 and utilities | Jobs, spool and utilities | [Plan](jes/execution-plan.md) | [Progress](jes/execution-status.md) |
| CICS | Application API | [Plan](cics/application-api-plan.md) | [Progress](cics/application-api-status.md) |
| CICS | SPI and FEPI | [Plan](cics/system-api-plan.md) | [Progress](cics/system-api-status.md) |
| z/OSMF | REST portfolio | [Plan](zosmf/rest-plan.md) | [Progress](zosmf/rest-status.md) |
| Db2 | Engine and common SQL | [Plan](db2/core-plan.md) | [Progress](db2/core-status.md) |
| Db2 | Complete programming surface | [Plan](db2/programming-plan.md) | [Progress](db2/programming-status.md) |
| IMS | DB / TM programming surface | [Plan](ims/programming-plan.md) | [Progress](ims/programming-status.md) |
| IBM MQ | MQI programming surface | [Plan](mq/programming-plan.md) | [Progress](mq/programming-status.md) |
| Cross-resource integration | Transactions and recovery | [Plan](integration/transactions-plan.md) | [Progress](integration/transactions-status.md) |
| Licensed certification | Differential certification | [Plan](certification/licensed-plan.md) | [Progress](certification/licensed-status.md) |
<!-- END GENERATED SUBSYSTEM INDEX -->

## Shared validation contract

This anchor is retained for existing references to the common acceptance rules.
Acceptance now belongs to the owning subsystem phase. Define its bounded scope,
source authorities, contract owner, consumed dependencies, and executable checks.
A passing local suite covers only the operations and cases it actually exercises.

## Recorded licensed-pending dispositions

Licensed differentials remain pending wherever the owning status record says so.
Local modeling, parser recognition, and merged changes do not establish IBM
equivalence. Run required campaigns against the current candidate and retain
outputs outside Git. Historical execution receipts are no longer stored here.

## Maintain a subsystem

1. Read the plan and current progress record before selecting a bounded slice.
2. Verify the consumed dependencies with their current executable checks.
3. Update specifications and fixtures alongside behavior and regression tests.
4. Record current scope, blockers, and unavailable checks in the status document.
5. Run `cargo xtask docs` and `cargo xtask subsystems --check`.

Use the [dependency map](DEPENDENCIES.md), [project workflow](GITHUB-PROJECT.md),
[profile track](PROFILE-TRACK.md), and
[implementation prompts](../../prompts/subsystems/README.md).
