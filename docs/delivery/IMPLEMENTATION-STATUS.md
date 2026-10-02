# Subsystem implementation status

Progress is grouped by subsystem and phase. The table is generated from the
registered progress records with `cargo xtask docs`; edit the linked record to
update a phase. A record describes its named candidate and is not automatically
proof for the current workspace. Target releases identify compatibility scope.
Published releases do not imply licensed differential completion.

<!-- BEGIN GENERATED SUBSYSTEM INDEX -->
| Subsystem | Phase | Recorded progress | Target release |
|---|---|---|---|
| Coverage and conformance | Coverage authority | [Complete implementation candidate; not released](subsystems/coverage/foundation-status.md) | 0.2.0 |
| COBOL | Grammar and types | [CB-306 complete; full-minor acceptance next](subsystems/cobol/structure-status.md) | 0.3.0 |
| COBOL | Execution semantics | [CB-401 through CB-406 locally complete — pass-with-licensed-differential-pending](subsystems/cobol/execution-status.md) | 0.4.0 |
| RACF / SAF | Commands and authorization | [Implementation candidate; pass with licensed differential pending; not released](subsystems/racf/security-status.md) | 0.5.0 |
| Datasets / VSAM / AMS | Dataset services | [DAT-601 through DAT-606 complete — pass-with-licensed-differential-pending](subsystems/dataset/data-status.md) | 0.6.0 |
| JCL | Converter and planner | [JCL-701 through JCL-706 complete; minor exit gate passed](subsystems/jcl/planning-status.md) | 0.7.0 |
| JES2 and utilities | Jobs, spool and utilities | [JES-801 through JES-806 complete — pass-with-licensed-differential-pending](subsystems/jes/execution-status.md) | 0.8.0 |
| CICS | Application API | [Implementation in progress; application API acceptance and licensed differential remain incomplete](subsystems/cics/application-api-status.md) | 0.9.0 |
| CICS | SPI and FEPI | [SPI-1001 identity foundation sealed; semantic source dependency blocked; SPI-1001 and 0.10.0 remain Proposed](subsystems/cics/system-api-status.md) | 0.10.0 |
| z/OSMF | REST portfolio | [ZMF-1101 operation-normalization foundation complete; no new routes advertised](subsystems/zosmf/rest-status.md) | 0.11.0 |
| Db2 | Engine and common SQL | [In progress — resumed original lane on current main for the declared second wave](subsystems/db2/core-status.md) | 0.12.0 |
| Db2 | Complete programming surface | [No progress record](subsystems/db2/programming-plan.md) | 0.13.0 |
| IMS | DB / TM programming surface | [Proposed](subsystems/ims/programming-status.md) | 0.14.0 |
| IBM MQ | MQI programming surface | [Implementation active](subsystems/mq/programming-status.md) | 0.15.0 |
| Cross-resource integration | Transactions and recovery | [Early INT-1601 participant boundary sealed; 0.16.0 is not complete](subsystems/integration/transactions-status.md) | 0.16.0 |
| Licensed certification | Differential certification | [CER-1701 shared harness foundation in progress; all licensed differentials pending](subsystems/certification/licensed-status.md) | 0.17.0 |
| Licensed certification | Stable release promotion | [No progress record](subsystems/certification/stable-release-plan.md) | 1.0.0 |
<!-- END GENERATED SUBSYSTEM INDEX -->

Use the [subsystem plans](subsystems/README.md) for scope and acceptance gates,
the [dependency map](subsystems/DEPENDENCIES.md) for sequencing, and the
[implementation prompts](../prompts/subsystems/README.md) to start bounded work.
Phases without a record have no implementation progress recorded here.

The original [0.1 / CardDemo implementation status](history/INITIAL-IMPLEMENTATION-STATUS.md)
is retained as a historical record. Release state is owned by `VERSION`,
`release.toml`, release notes, tags, and their original evidence.
