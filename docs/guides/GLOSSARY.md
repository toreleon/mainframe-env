# Framework glossary

These definitions orient new readers. Versioned contracts and IBM product
baselines own the exact semantics.

| Term | Meaning in this project |
|---|---|
| ABEND | Abnormal termination, distinct from normal completion or a handled condition |
| Artifact | Immutable executable payload with compatibility metadata and content identity |
| BMS / CSD | CICS map definitions / resource definitions |
| CCSID / EBCDIC | Declared character-set identity / mainframe encoding family |
| CICS | Transaction-processing surface with provider-owned task, terminal, file and program behavior |
| COPY / copybook | COBOL source inclusion through explicitly ordered source libraries |
| DD | JCL definition binding a step to a dataset or input/output resource |
| Db2 / SQLCA | Database programming surface / SQL result and diagnostic communication area |
| Deterministic core | Compiler and machine transitions using explicit inputs and bounds |
| Effect | Typed host-operation request with ordered identity and retained outcome |
| HIR / MIR | Typed high-level language representation / lowered executable intermediate representation |
| IMS / DL/I / PCB / PSB | Database and transaction surface / call interface / program communication block / program specification block |
| JES / JCL | Job lifecycle and spool authority / batch workflow language |
| MQ / MQI | Message-queue provider / its programming interface |
| Oracle | Independent reference system for differential observations; provenance determines credit |
| Profile | Explicit dependency and capability closure, such as `core-server` or `conformance` |
| RACF / SAF | Security provider / typed authorization boundary |
| Run unit | One owned execution context containing frames, bindings and transaction context |
| Semantic identity | Source/compiler/manifest identity, distinct from exact artifact-byte digest |
| Suspension / checkpoint | Execution paused outside a CPU worker / versioned state needed to resume |
| Syncpoint / unit of work | Provider transaction decision / staged operations with commit or rollback |
| Unknown outcome | A mutation may have occurred but its result is unresolved; requires reconciliation |
| VSAM / AMS | Dataset access organizations / access-method utility surface |
| z/OSMF | HTTP compatibility gateway over registered application services |

Continue with [getting started](GETTING-STARTED.md),
[capabilities](CAPABILITIES.md), or the [architecture](../architecture/OVERVIEW.md).
