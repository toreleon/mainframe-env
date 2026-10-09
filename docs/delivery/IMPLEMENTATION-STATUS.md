# Subsystem implementation status

Progress is grouped by subsystem and phase. The table is generated from each
registered progress document. Edit the owning record, then run `cargo xtask docs`.
Recorded implementation scope and pending licensed work remain explicit; rerun
executable checks for the current checkout. Historical receipts and release
records are not part of the repository's management model.

The [unreleased review](UNRELEASED-STATUS.md) distinguishes merged bounded
implementation from preparation, blocked dependencies, and incomplete
acceptance. A recorded local pass applies to its stated scope and producing
inputs; it does not establish a fresh full-suite pass or licensed equivalence.

<!-- BEGIN GENERATED SUBSYSTEM INDEX -->
| Subsystem | Phase | Recorded progress |
|---|---|---|
| Coverage and conformance | Coverage authority | [Bounded implementation accepted; integrated acceptance pending](subsystems/coverage/foundation-status.md) |
| COBOL | Grammar and types | [CB-306 complete; full-phase acceptance next](subsystems/cobol/structure-status.md) |
| COBOL | Execution semantics | [CB-401 through CB-406 locally complete — pass-with-licensed-differential-pending](subsystems/cobol/execution-status.md) |
| RACF / SAF | Commands and authorization | [Implementation candidate; pass with licensed differential pending](subsystems/racf/security-status.md) |
| Datasets / VSAM / AMS | Dataset services | [DAT-601 through DAT-606 complete — pass-with-licensed-differential-pending](subsystems/dataset/data-status.md) |
| JCL | Converter and planner | [Local converter/planner implementation complete; licensed differential pending](subsystems/jcl/planning-status.md) |
| JES2 and utilities | Jobs, spool and utilities | [JES-801 through JES-806 complete — pass-with-licensed-differential-pending](subsystems/jes/execution-status.md) |
| CICS | Application API | [Implementation in progress; application API acceptance and licensed differential remain incomplete](subsystems/cics/application-api-status.md) |
| CICS | SPI and FEPI | [Source-backed private preparation implemented; public SPI/FEPI execution and licensed acceptance pending](subsystems/cics/system-api-status.md) |
| z/OSMF | REST portfolio | [ZMF-1101 operation-normalization foundation complete; no new routes advertised](subsystems/zosmf/rest-status.md) |
| Db2 | Engine and common SQL | [Bounded syntax, type/value and column-default surfaces implemented; generic binder/catalog/execution acceptance pending](subsystems/db2/core-status.md) |
| Db2 | Complete programming surface | [Blocked on db2.core acceptance; complete programming surface not implemented](subsystems/db2/programming-status.md) |
| IMS | DB / TM programming surface | [Implementation in progress; bounded DB/TM surfaces implemented; full programming acceptance pending](subsystems/ims/programming-status.md) |
| IBM MQ | MQI programming surface | [Bounded selected and installed MQI flows implemented; all 26 complete-call gates pending](subsystems/mq/programming-status.md) |
| Cross-resource integration | Transactions and recovery | [Early INT-1601 participant boundary sealed; integration.transactions is not complete](subsystems/integration/transactions-status.md) |
| Licensed certification | Differential certification | [Shared harness foundation implemented; licensed environments and campaigns pending](subsystems/certification/licensed-status.md) |
<!-- END GENERATED SUBSYSTEM INDEX -->

The [Foundation progress record](subsystems/coverage/foundation-status.md)
separates qualified supervisor, MQ and selected navigation results from pending
strict/application and final-candidate gates. Full CardDemo closure still requires
20 journeys with 114 observations and 26 selected issue rows with 105 acceptance
requirements; scoped passes do not complete these obligations.

Use the [plans](subsystems/README.md), [dependencies](subsystems/DEPENDENCIES.md),
and [implementation prompts](../prompts/subsystems/README.md) to select work.
