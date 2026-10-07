# Subsystem implementation status

Progress is grouped by subsystem and phase. The table is generated from each
registered progress document. Edit the owning record, then run `cargo xtask docs`.
Recorded implementation scope and pending licensed work remain explicit; rerun
executable checks for the current checkout. Historical receipts and release
records are not part of the repository's management model.

<!-- BEGIN GENERATED SUBSYSTEM INDEX -->
| Subsystem | Phase | Recorded progress |
|---|---|---|
| Coverage and conformance | Coverage authority | [Complete implementation candidate](subsystems/coverage/foundation-status.md) |
| COBOL | Grammar and types | [CB-306 complete; full-phase acceptance next](subsystems/cobol/structure-status.md) |
| COBOL | Execution semantics | [CB-401 through CB-406 locally complete — pass-with-licensed-differential-pending](subsystems/cobol/execution-status.md) |
| RACF / SAF | Commands and authorization | [Implementation candidate; pass with licensed differential pending](subsystems/racf/security-status.md) |
| Datasets / VSAM / AMS | Dataset services | [DAT-601 through DAT-606 complete — pass-with-licensed-differential-pending](subsystems/dataset/data-status.md) |
| JCL | Converter and planner | [JCL-701 through JCL-706 complete; phase exit gate passed](subsystems/jcl/planning-status.md) |
| JES2 and utilities | Jobs, spool and utilities | [JES-801 through JES-806 complete — pass-with-licensed-differential-pending](subsystems/jes/execution-status.md) |
| CICS | Application API | [Implementation in progress; application API acceptance and licensed differential remain incomplete](subsystems/cics/application-api-status.md) |
| CICS | SPI and FEPI | [Source-backed private preparation implemented; public SPI/FEPI execution and licensed acceptance pending](subsystems/cics/system-api-status.md) |
| z/OSMF | REST portfolio | [ZMF-1101 operation-normalization foundation complete; no new routes advertised](subsystems/zosmf/rest-status.md) |
| Db2 | Engine and common SQL | [In progress — second/third-wave pure surfaces sealed; catalog/binder and execution pending](subsystems/db2/core-status.md) |
| Db2 | Complete programming surface | [No progress record](subsystems/db2/programming-plan.md) |
| IMS | DB / TM programming surface | [Proposed](subsystems/ims/programming-status.md) |
| IBM MQ | MQI programming surface | [Implementation active](subsystems/mq/programming-status.md) |
| Cross-resource integration | Transactions and recovery | [Early INT-1601 participant boundary sealed; integration.transactions is not complete](subsystems/integration/transactions-status.md) |
| Licensed certification | Differential certification | [CER-1701 shared harness foundation in progress; all licensed differentials pending](subsystems/certification/licensed-status.md) |
<!-- END GENERATED SUBSYSTEM INDEX -->

Use the [plans](subsystems/README.md), [dependencies](subsystems/DEPENDENCIES.md),
and [implementation prompts](../prompts/subsystems/README.md) to select work.
