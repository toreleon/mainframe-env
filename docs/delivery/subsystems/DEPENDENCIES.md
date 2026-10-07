# Subsystem dependencies

Status: **Accepted subsystem sequencing**

The registry defines completion dependencies. An edge means the receiving phase
consumes accepted behavior from the preceding phase. Catalog preparation and
private parser work may proceed within a plan's declared boundaries before all
dependencies complete; public behavior requires the consumed contracts to pass.

```mermaid
flowchart TB
    coverage_foundation["coverage.foundation"]
    cobol_structure["cobol.structure"]
    cobol_execution["cobol.execution"]
    racf_security["racf.security"]
    dataset_data["dataset.data"]
    jcl_planning["jcl.planning"]
    jes_execution["jes.execution"]
    cics_application_api["cics.application-api"]
    cics_system_api["cics.system-api"]
    zosmf_rest["zosmf.rest"]
    db2_core["db2.core"]
    db2_programming["db2.programming"]
    ims_programming["ims.programming"]
    mq_programming["mq.programming"]
    integration_transactions["integration.transactions"]
    certification_licensed["certification.licensed"]
    coverage_foundation --> cobol_structure
    cobol_structure --> cobol_execution
    coverage_foundation --> racf_security
    coverage_foundation --> dataset_data
    coverage_foundation --> jcl_planning
    racf_security --> jes_execution
    dataset_data --> jes_execution
    jcl_planning --> jes_execution
    cobol_execution --> cics_application_api
    racf_security --> cics_application_api
    dataset_data --> cics_application_api
    cics_application_api --> cics_system_api
    racf_security --> zosmf_rest
    dataset_data --> zosmf_rest
    jes_execution --> zosmf_rest
    cics_system_api --> zosmf_rest
    coverage_foundation --> db2_core
    cobol_execution --> db2_core
    racf_security --> db2_core
    db2_core --> db2_programming
    cobol_execution --> ims_programming
    racf_security --> ims_programming
    dataset_data --> ims_programming
    cobol_execution --> mq_programming
    racf_security --> mq_programming
    jes_execution --> integration_transactions
    cics_system_api --> integration_transactions
    db2_programming --> integration_transactions
    ims_programming --> integration_transactions
    mq_programming --> integration_transactions
    zosmf_rest --> certification_licensed
    integration_transactions --> certification_licensed
```

## Completion dependencies

<!-- BEGIN GENERATED SUBSYSTEM INDEX -->
| Subsystem phase | Completion dependencies |
|---|---|
| [coverage.foundation](coverage/foundation-plan.md) | Accepted initial baseline |
| [cobol.structure](cobol/structure-plan.md) | coverage.foundation |
| [cobol.execution](cobol/execution-plan.md) | cobol.structure |
| [racf.security](racf/security-plan.md) | coverage.foundation |
| [dataset.data](dataset/data-plan.md) | coverage.foundation |
| [jcl.planning](jcl/planning-plan.md) | coverage.foundation |
| [jes.execution](jes/execution-plan.md) | racf.security, dataset.data, jcl.planning |
| [cics.application-api](cics/application-api-plan.md) | cobol.execution, racf.security, dataset.data |
| [cics.system-api](cics/system-api-plan.md) | cics.application-api |
| [zosmf.rest](zosmf/rest-plan.md) | racf.security, dataset.data, jes.execution, cics.system-api |
| [db2.core](db2/core-plan.md) | coverage.foundation, cobol.execution, racf.security |
| [db2.programming](db2/programming-plan.md) | db2.core |
| [ims.programming](ims/programming-plan.md) | cobol.execution, racf.security, dataset.data |
| [mq.programming](mq/programming-plan.md) | cobol.execution, racf.security |
| [integration.transactions](integration/transactions-plan.md) | jes.execution, cics.system-api, db2.programming, ims.programming, mq.programming |
| [certification.licensed](certification/licensed-plan.md) | zosmf.rest, integration.transactions |
<!-- END GENERATED SUBSYSTEM INDEX -->

## Integration discipline

Keep ownership of language, provider, host, persistence, and transport contracts
explicit. Merge one bounded slice with its regressions. Revalidate consumers when
shared contracts change. Cross-resource recovery depends on participating
providers; a provider's private transaction cannot establish aggregate atomicity.
