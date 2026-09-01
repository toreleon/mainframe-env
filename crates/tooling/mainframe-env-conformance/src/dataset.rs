use mainframe_env_batch::{
    AmsStatement, BatchService, JclBundle, JobState, ProgramRouter, parse_idcams_control,
    validate_idcams_control,
};
use mainframe_env_coverage::{
    CompiledSpec, ConformanceDriver, ConformanceLimits, ConformanceObservation,
    ConformanceRunReport, ConformanceRunner, DriverOutput, DriverRef, FixtureRef, ObservationCheck,
    ObservationRef, RunnerContext, RunnerSelection, RuntimeRegistry, SpecProblem,
};
use mainframe_env_dataset::{DatasetLimits, DatasetService, dataset_providers};
use mainframe_env_execution_api::{
    ArtifactRef, CapabilityId, ExecutionId, IdempotencyKey, Invocation, InvocationLimits,
    Principal, PrincipalId, RequestId, ResourceLimits, RunUnitId, Selector, ServiceClass, TraceId,
};
use mainframe_env_host_api::{
    CapabilityDescriptor, CatalogKind, DatasetAttributes, DatasetLifecycleState, DatasetName,
    DatasetOrganization, DatasetRequest, DatasetResult, EffectRequest, EffectResult, HostLimits,
    HostProblem, HostProvider, HostRequest, HostResult, Mutation, RecordFormat, RegistrySnapshot,
    ScopedHostService, SecurityDecision,
};
use mainframe_env_store::{MemoryStore, StoreLimits};
use mainframe_env_store_api::ProviderStateStore;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::sync::Arc;

const FIXTURES: &str =
    include_str!("../../../../conformance/0.6/fixtures/dataset-organizations.json");
const AMS_FIXTURES: &str = include_str!("../../../../conformance/0.6/fixtures/ams-commands.json");

const FIXTURE_IDS: [&str; 10] = [
    "dataset.esds.invalid",
    "dataset.esds.valid",
    "dataset.ksds.invalid",
    "dataset.ksds.valid",
    "dataset.lds.invalid",
    "dataset.lds.valid",
    "dataset.rrds.invalid",
    "dataset.rrds.valid",
    "dataset.vrrds.invalid",
    "dataset.vrrds.valid",
];
const AMS_FIXTURE_IDS: [&str; 31] = [
    "ams.allocate",
    "ams.alter",
    "ams.alter-libraryentry",
    "ams.alter-volumeentry",
    "ams.bldindex",
    "ams.create-libraryentry",
    "ams.create-volumeentry",
    "ams.dcollect",
    "ams.define-alias",
    "ams.define-alternateindex",
    "ams.define-cluster",
    "ams.define-generationdatagroup",
    "ams.define-nonvsam",
    "ams.define-pagespace",
    "ams.define-path",
    "ams.define-usercatalog",
    "ams.delete",
    "ams.diagnose",
    "ams.examine",
    "ams.export",
    "ams.export-disconnect",
    "ams.import",
    "ams.import-connect",
    "ams.listcat",
    "ams.listdata",
    "ams.print",
    "ams.repro",
    "ams.recover",
    "ams.setcache",
    "ams.shcds",
    "ams.verify",
];

pub fn run_dataset_conformance(
    spec: &CompiledSpec,
    selection: &RunnerSelection,
    context: &RunnerContext,
) -> Result<ConformanceRunReport, String> {
    let simulation = crate::run_dataset_reference_simulation()?;
    if simulation.official_rows != 36
        || simulation.organization_rows != 5
        || simulation.command_rows != 31
        || simulation.differential_credit != 0
    {
        return Err("dataset reference simulation denominator or evidence boundary drifted".into());
    }
    let limits = ConformanceLimits::default();
    let handlers = dataset_conformance_runtime();
    let runtime = RuntimeRegistry::new(
        spec,
        handlers
            .drivers(limits)
            .map_err(|problem| problem.to_string())?,
        Vec::new(),
        handlers
            .observations(limits)
            .map_err(|problem| problem.to_string())?,
        limits,
    )
    .map_err(|problem| problem.to_string())?;
    ConformanceRunner::new(spec, runtime, limits)
        .run(selection, context)
        .map_err(|problem| problem.to_string())
}

#[must_use]
pub fn dataset_conformance_runtime() -> DatasetConformanceRuntime {
    DatasetConformanceRuntime {
        organization: DatasetOrganizationDriver,
        ams: AmsCommandDriver,
        observations: FIXTURE_IDS
            .into_iter()
            .chain(AMS_FIXTURE_IDS)
            .map(|fixture| ExpectedObservation { fixture })
            .collect(),
    }
}

pub struct DatasetConformanceRuntime {
    organization: DatasetOrganizationDriver,
    ams: AmsCommandDriver,
    observations: Vec<ExpectedObservation>,
}

impl DatasetConformanceRuntime {
    pub fn drivers(
        &self,
        limits: ConformanceLimits,
    ) -> Result<Vec<(DriverRef, &dyn ConformanceDriver)>, SpecProblem> {
        Ok(vec![
            (
                DriverRef::new("dataset.organization.driver", limits)?,
                &self.organization as &dyn ConformanceDriver,
            ),
            (
                DriverRef::new("dataset.ams.driver", limits)?,
                &self.ams as &dyn ConformanceDriver,
            ),
        ])
    }

    pub fn observations(
        &self,
        limits: ConformanceLimits,
    ) -> Result<Vec<(ObservationRef, &dyn ConformanceObservation)>, SpecProblem> {
        self.observations
            .iter()
            .map(|observation| {
                Ok((
                    ObservationRef::new(format!("observe.{}", observation.fixture), limits)?,
                    observation as &dyn ConformanceObservation,
                ))
            })
            .collect()
    }
}

struct DatasetOrganizationDriver;

struct AmsCommandDriver;

impl ConformanceDriver for AmsCommandDriver {
    fn execute(&self, fixture: &FixtureRef) -> Result<DriverOutput, String> {
        let document: Value =
            serde_json::from_str(AMS_FIXTURES).map_err(|error| error.to_string())?;
        let case = document["cases"]
            .as_array()
            .and_then(|cases| {
                cases
                    .iter()
                    .find(|case| case["id"].as_str() == Some(fixture.as_str()))
            })
            .ok_or_else(|| format!("unknown AMS fixture {}", fixture.as_str()))?;
        let control = case["control"]
            .as_str()
            .ok_or_else(|| "AMS fixture control is missing".to_string())?;
        validate_idcams_control(control.as_bytes()).map_err(|problem| problem.to_string())?;
        let parsed =
            parse_idcams_control(control.as_bytes()).map_err(|problem| problem.to_string())?;
        let [AmsStatement::Command(command)] = parsed.as_slice() else {
            return Err("AMS fixture did not produce exactly one command".into());
        };
        execute_ams_command_fixture(control, command)?;
        let output = format!(
            "command={};label={};capability={}",
            command.id(),
            command.label(),
            command.capability().unwrap_or("none")
        );
        DriverOutput::new(output.into_bytes(), ConformanceLimits::default())
            .map_err(|problem| problem.to_string())
    }
}

impl ConformanceDriver for DatasetOrganizationDriver {
    fn execute(&self, fixture: &FixtureRef) -> Result<DriverOutput, String> {
        let output = match fixture.as_str() {
            "dataset.ksds.valid" => valid_ksds()?,
            "dataset.ksds.invalid" => invalid_ksds()?,
            "dataset.esds.valid" => valid_esds()?,
            "dataset.esds.invalid" => invalid_esds()?,
            "dataset.lds.valid" => valid_lds()?,
            "dataset.lds.invalid" => invalid_lds()?,
            "dataset.rrds.valid" => valid_rrds()?,
            "dataset.rrds.invalid" => invalid_rrds()?,
            "dataset.vrrds.valid" => valid_vrrds()?,
            "dataset.vrrds.invalid" => invalid_vrrds()?,
            other => return Err(format!("unknown dataset fixture {other}")),
        };
        DriverOutput::new(output.into_bytes(), ConformanceLimits::default())
            .map_err(|problem| problem.to_string())
    }
}

struct ExpectedObservation {
    fixture: &'static str,
}

impl ConformanceObservation for ExpectedObservation {
    fn evaluate(&self, output: &DriverOutput) -> Result<ObservationCheck, String> {
        let expected = fixture_expected(self.fixture)?;
        let actual = String::from_utf8(output.bytes().to_vec())
            .map_err(|_| "dataset driver output is not UTF-8".to_string())?;
        ObservationCheck::new(
            actual == expected,
            expected,
            actual,
            ConformanceLimits::default(),
        )
        .map_err(|problem| problem.to_string())
    }
}

fn fixture_expected(id: &str) -> Result<String, String> {
    let source = if id.starts_with("ams.") {
        AMS_FIXTURES
    } else {
        FIXTURES
    };
    let document: Value = serde_json::from_str(source).map_err(|error| error.to_string())?;
    document["cases"]
        .as_array()
        .and_then(|cases| cases.iter().find(|case| case["id"].as_str() == Some(id)))
        .and_then(|case| case["expected"].as_str())
        .map(str::to_string)
        .ok_or_else(|| format!("dataset fixture expectation is missing for {id}"))
}

struct AllowSecurityProvider {
    descriptor: CapabilityDescriptor,
}

impl HostProvider for AllowSecurityProvider {
    fn descriptor(&self) -> &CapabilityDescriptor {
        &self.descriptor
    }

    fn invoke(&self, _: &Invocation, effect: EffectRequest) -> EffectResult {
        let outcome = match effect.request {
            HostRequest::Security(_) => Ok(HostResult::Security(SecurityDecision::Allow)),
            _ => Err(HostProblem::Malformed),
        };
        EffectResult {
            sequence: effect.sequence,
            outcome,
        }
    }
}

fn ams_host(dataset: Arc<DatasetService>) -> Result<Arc<ScopedHostService>, String> {
    let limits = InvocationLimits::default();
    let security: Arc<dyn HostProvider> = Arc::new(AllowSecurityProvider {
        descriptor: CapabilityDescriptor {
            capability: CapabilityId::new("host.security.authorize", limits)
                .map_err(|problem| problem.to_string())?,
            provider_id: "dataset-conformance-security".into(),
            generation: "1".into(),
            request_schema: "security@1".into(),
            result_schema: "decision@1".into(),
            max_request_bytes: 65_536,
            max_result_bytes: 65_536,
            ready: true,
        },
    });
    let program: Arc<dyn HostProvider> = ProgramRouter::with_builtins(limits);
    let mut providers = vec![security, program];
    providers.extend(dataset_providers(dataset, limits));
    Ok(Arc::new(ScopedHostService::new(
        Arc::new(
            RegistrySnapshot::new(1, providers, limits).map_err(|problem| problem.to_string())?,
        ),
        HostLimits::default(),
    )))
}

fn ams_invocation() -> Result<Invocation, String> {
    let limits = InvocationLimits::default();
    let grants = [
        "host.security.authorize",
        "host.program.invoke",
        "host.dataset.read",
        "host.dataset.write",
    ]
    .into_iter()
    .map(|capability| CapabilityId::new(capability, limits))
    .collect::<Result<BTreeSet<_>, _>>()
    .map_err(|problem| problem.to_string())?;
    Invocation::new(
        RequestId::new("ams-conformance-request", limits).map_err(|problem| problem.to_string())?,
        ExecutionId::new("ams-conformance-execution", limits)
            .map_err(|problem| problem.to_string())?,
        RunUnitId::new("ams-conformance-run", limits).map_err(|problem| problem.to_string())?,
        None,
        Selector::new("jes:submit", limits).map_err(|problem| problem.to_string())?,
        ArtifactRef::new("ams-conformance-jcl", limits).map_err(|problem| problem.to_string())?,
        Principal::new(
            PrincipalId::new("IBMUSER", limits).map_err(|problem| problem.to_string())?,
            grants,
            limits,
        )
        .map_err(|problem| problem.to_string())?,
        ServiceClass::Batch,
        0,
        100,
        TraceId::new("ams-conformance-trace", limits).map_err(|problem| problem.to_string())?,
        IdempotencyKey::new("ams-conformance-invocation", limits)
            .map_err(|problem| problem.to_string())?,
        1,
        ResourceLimits::default(),
        Default::default(),
        limits,
    )
    .map_err(|problem| problem.to_string())
}

fn ams_fixture_mutation(command: &str, sequence: &mut u64) -> Result<Mutation, String> {
    *sequence = sequence
        .checked_add(1)
        .ok_or_else(|| "AMS fixture sequence overflow".to_string())?;
    Ok(Mutation {
        sequence: *sequence,
        idempotency_key: IdempotencyKey::new(
            format!("ams-{command}-setup-{sequence}"),
            InvocationLimits::default(),
        )
        .map_err(|problem| problem.to_string())?,
        transaction: None,
    })
}

fn seed_ams_dataset(
    service: &DatasetService,
    command: &str,
    sequence: &mut u64,
    dataset: &str,
    organization: DatasetOrganization,
    records: Vec<Vec<u8>>,
) -> Result<(), String> {
    let attributes = DatasetAttributes {
        organization,
        record_format: if organization == DatasetOrganization::Linear {
            RecordFormat::Undefined
        } else if matches!(
            organization,
            DatasetOrganization::Sequential | DatasetOrganization::EntrySequenced
        ) {
            RecordFormat::Variable
        } else {
            RecordFormat::Fixed
        },
        logical_record_length: if matches!(
            organization,
            DatasetOrganization::Sequential | DatasetOrganization::EntrySequenced
        ) {
            1_024
        } else {
            4
        },
        key_offset: (organization == DatasetOrganization::KeySequenced).then_some(0),
        key_length: (organization == DatasetOrganization::KeySequenced).then_some(2),
        ccsid: Some(37),
    };
    let dataset = name(dataset);
    service
        .invoke(DatasetRequest::Create {
            dataset: dataset.clone(),
            attributes,
            mutation: ams_fixture_mutation(command, sequence)?,
        })
        .map_err(|problem| problem.to_string())?;
    if !records.is_empty() {
        service
            .invoke(DatasetRequest::Write {
                dataset,
                member: None,
                records,
                expected_version: Some(1),
                mutation: ams_fixture_mutation(command, sequence)?,
            })
            .map_err(|problem| problem.to_string())?;
    }
    Ok(())
}

fn define_ams_catalog(
    service: &DatasetService,
    command: &str,
    sequence: &mut u64,
) -> Result<(), String> {
    service
        .invoke(DatasetRequest::DefineCatalog {
            catalog: name("USER.CAT"),
            kind: CatalogKind::User,
            mutation: ams_fixture_mutation(command, sequence)?,
        })
        .map_err(|problem| problem.to_string())?;
    Ok(())
}

fn define_ams_aix(
    service: &DatasetService,
    command: &str,
    sequence: &mut u64,
) -> Result<(), String> {
    service
        .invoke(DatasetRequest::DefineAlternateIndex {
            base: name("USER.A"),
            index: name("USER.A.AIX"),
            key_offset: 2,
            key_length: 2,
            allow_duplicates: false,
            upgrade: true,
            mutation: ams_fixture_mutation(command, sequence)?,
        })
        .map_err(|problem| problem.to_string())?;
    Ok(())
}

fn snapshot_digest(domain: &[u8], core: &str, manifest: &[u8]) -> String {
    let mut digest = Sha256::new();
    digest.update(domain);
    digest.update((core.len() as u64).to_be_bytes());
    digest.update(core.as_bytes());
    digest.update((manifest.len() as u64).to_be_bytes());
    digest.update(manifest);
    digest.update(0u64.to_be_bytes());
    format!("sha256:{:x}", digest.finalize())
}

fn write_ams_input(
    service: &DatasetService,
    command: &str,
    sequence: &mut u64,
    records: Vec<Vec<u8>>,
) -> Result<(), String> {
    service
        .invoke(DatasetRequest::Write {
            dataset: name("CONF.INPUT"),
            member: None,
            records,
            expected_version: Some(1),
            mutation: ams_fixture_mutation(command, sequence)?,
        })
        .map_err(|problem| problem.to_string())?;
    Ok(())
}

fn prepare_definition_snapshot_input(
    service: &DatasetService,
    command: &str,
    sequence: &mut u64,
) -> Result<(), String> {
    let DatasetResult::Snapshot { snapshot, .. } = service
        .invoke(DatasetRequest::Snapshot {
            dataset: name("USER.A"),
            max_records: 32,
            max_members: 32,
        })
        .map_err(|problem| problem.to_string())?
    else {
        return Err("AMS snapshot setup returned an unexpected result".into());
    };
    let manifest = serde_json::to_vec(snapshot.as_ref()).map_err(|error| error.to_string())?;
    let chunks = manifest.chunks(192).map(<[u8]>::to_vec).collect::<Vec<_>>();
    let core = format!("MEAMS2|USER.A|{}", chunks.len());
    let header = format!(
        "{core}|{}",
        snapshot_digest(b"mainframe-env.ams-definition-snapshot@2", &core, &manifest,)
    )
    .into_bytes();
    let mut records = Vec::with_capacity(chunks.len() + 1);
    records.push(header);
    records.extend(chunks);
    write_ams_input(service, command, sequence, records)
}

fn prepare_catalog_snapshot_input(
    service: &DatasetService,
    command: &str,
    sequence: &mut u64,
) -> Result<(), String> {
    define_ams_catalog(service, command, sequence)?;
    service
        .invoke(DatasetRequest::SetCatalogConnection {
            catalog: name("USER.CAT"),
            connected: false,
            expected_version: Some(1),
            mutation: ams_fixture_mutation(command, sequence)?,
        })
        .map_err(|problem| problem.to_string())?;
    let core = "MEAMSCAT1|USER.CAT";
    let mut digest = Sha256::new();
    digest.update(b"mainframe-env.ams-snapshot@1");
    digest.update((core.len() as u64).to_be_bytes());
    digest.update(core.as_bytes());
    digest.update(0u64.to_be_bytes());
    write_ams_input(
        service,
        command,
        sequence,
        vec![format!("{core}|sha256:{:x}", digest.finalize()).into_bytes()],
    )
}

fn execute_ams_command_fixture(
    control: &str,
    command: &mainframe_env_batch::AmsCommand,
) -> Result<(), String> {
    let store = Arc::new(MemoryStore::new(StoreLimits::default()));
    let provider_store: Arc<dyn ProviderStateStore> = store.clone();
    let dataset = DatasetService::open(provider_store, DatasetLimits::default())
        .map_err(|problem| problem.to_string())?;
    let mut sequence = 0u64;
    seed_ams_dataset(
        &dataset,
        command.id(),
        &mut sequence,
        "CONF.INPUT",
        DatasetOrganization::Sequential,
        Vec::new(),
    )?;
    seed_ams_dataset(
        &dataset,
        command.id(),
        &mut sequence,
        "CONF.OUTPUT",
        DatasetOrganization::Sequential,
        Vec::new(),
    )?;
    if !matches!(
        command.id(),
        "allocate" | "define-cluster" | "define-nonvsam"
    ) {
        seed_ams_dataset(
            &dataset,
            command.id(),
            &mut sequence,
            "USER.A",
            DatasetOrganization::KeySequenced,
            vec![b"AA11".to_vec()],
        )?;
    }
    match command.id() {
        "bldindex" | "define-path" => {
            define_ams_aix(&dataset, command.id(), &mut sequence)?;
        }
        "export-disconnect" => {
            define_ams_catalog(&dataset, command.id(), &mut sequence)?;
        }
        "import" | "recover" => {
            prepare_definition_snapshot_input(&dataset, command.id(), &mut sequence)?;
        }
        "import-connect" => {
            prepare_catalog_snapshot_input(&dataset, command.id(), &mut sequence)?;
        }
        "repro" => {
            seed_ams_dataset(
                &dataset,
                command.id(),
                &mut sequence,
                "USER.B",
                DatasetOrganization::KeySequenced,
                Vec::new(),
            )?;
        }
        _ => {}
    }
    let host = ams_host(dataset.clone())?;
    let batch_store: Arc<dyn ProviderStateStore> = store.clone();
    let batch = BatchService::open(host, batch_store, Default::default(), Default::default())
        .map_err(|problem| problem.to_string())?;
    let invocation = ams_invocation()?;
    let job = batch
        .submit(
            &invocation,
            &JclBundle {
                primary: format!(
                    "//AMSJOB JOB CLASS=A\n//AMS EXEC PGM=IDCAMS\n//IN DD DSN=CONF.INPUT,DISP=SHR\n//OUT DD DSN=CONF.OUTPUT,DISP=OLD\n//SYSIN DD *\n {control}\n/*\n"
                ),
                ..Default::default()
            },
            &IdempotencyKey::new(
                format!("ams-conformance-{}", command.id()),
                InvocationLimits::default(),
            )
            .map_err(|problem| problem.to_string())?,
            false,
        )
        .map_err(|problem| problem.to_string())?;
    let completed = batch
        .run_next(&invocation, false)
        .map_err(|problem| problem.to_string())?
        .ok_or_else(|| "AMS conformance job did not run".to_string())?;
    let expected_cc = if command.capability().is_some() {
        12
    } else {
        0
    };
    if completed.state != JobState::Completed || completed.return_code != Some(expected_cc) {
        return Err(format!(
            "{} completed with state {:?} and return code {:?}, expected {expected_cc}",
            command.id(),
            completed.state,
            completed.return_code
        ));
    }
    let reopened_provider_store: Arc<dyn ProviderStateStore> = store.clone();
    let reopened_dataset = DatasetService::open(reopened_provider_store, DatasetLimits::default())
        .map_err(|problem| problem.to_string())?;
    let reopened_host = ams_host(reopened_dataset.clone())?;
    let reopened_batch_store: Arc<dyn ProviderStateStore> = store;
    let reopened_batch = BatchService::open(
        reopened_host,
        reopened_batch_store,
        Default::default(),
        Default::default(),
    )
    .map_err(|problem| problem.to_string())?;
    let recovered = reopened_batch
        .get(&job.id)
        .map_err(|problem| problem.to_string())?;
    if recovered.state != JobState::Completed || recovered.return_code != Some(expected_cc) {
        return Err(format!(
            "{} terminal result changed after restart",
            command.id()
        ));
    }
    verify_ams_command_effect(command, &reopened_batch, &reopened_dataset, &job.id)
}

fn catalog_contains(service: &DatasetService, entry_name: &str) -> Result<bool, String> {
    match service
        .invoke(DatasetRequest::ListCatalog {
            pattern: entry_name.into(),
            start: None,
            max_items: 32,
        })
        .map_err(|problem| problem.to_string())?
    {
        DatasetResult::CatalogEntries { entries, .. } => Ok(entries
            .iter()
            .any(|entry| entry.name.as_str() == entry_name)),
        result => Err(format!("unexpected catalog result {result:?}")),
    }
}

fn dataset_records(service: &DatasetService, dataset: &str) -> Result<Vec<Vec<u8>>, String> {
    match service
        .invoke(DatasetRequest::Read {
            dataset: name(dataset),
            member: None,
            key: None,
            max_records: 4_096,
        })
        .map_err(|problem| problem.to_string())?
    {
        DatasetResult::Records { records, .. } => Ok(records),
        result => Err(format!("unexpected record result {result:?}")),
    }
}

fn verify_ams_command_effect(
    command: &mainframe_env_batch::AmsCommand,
    batch: &BatchService,
    dataset: &DatasetService,
    job_id: &str,
) -> Result<(), String> {
    if let Some(capability) = command.capability() {
        let (records, _) = batch
            .spool(job_id, "SYSPRINT", 0, 64)
            .map_err(|problem| problem.to_string())?;
        return records
            .iter()
            .map(|record| String::from_utf8_lossy(record))
            .any(|record| record.contains("UnsupportedCapability") && record.contains(capability))
            .then_some(())
            .ok_or_else(|| format!("{} did not retain its capability condition", command.id()));
    }
    let ok = match command.id() {
        "allocate" | "define-cluster" => dataset
            .invoke(DatasetRequest::Attributes {
                dataset: name("USER.A"),
            })
            .is_ok(),
        "define-nonvsam" => dataset
            .invoke(DatasetRequest::Attributes {
                dataset: name("USER.PS"),
            })
            .is_ok(),
        "alter" => matches!(
            dataset.invoke(DatasetRequest::Describe {
                dataset: name("USER.A"),
            }),
            Ok(DatasetResult::Description(description))
                if description.definition.lifecycle.state == DatasetLifecycleState::Open
        ),
        "bldindex" => catalog_contains(dataset, "USER.A.AIX")?,
        "dcollect" => !dataset_records(dataset, "CONF.OUTPUT")?.is_empty(),
        "define-alias" => catalog_contains(dataset, "USER.ALIAS")?,
        "define-alternateindex" => catalog_contains(dataset, "USER.A.AIX")?,
        "define-generationdatagroup" => catalog_contains(dataset, "USER.GDG")?,
        "define-path" => catalog_contains(dataset, "USER.A.PATH")?,
        "define-usercatalog" => catalog_contains(dataset, "USER.CAT")?,
        "delete" => {
            dataset.invoke(DatasetRequest::Attributes {
                dataset: name("USER.A"),
            }) == Err(HostProblem::NotFound)
        }
        "export" => dataset_records(dataset, "CONF.OUTPUT")?
            .first()
            .is_some_and(|record| record.starts_with(b"MEAMS2|USER.A|")),
        "export-disconnect" => dataset_records(dataset, "CONF.OUTPUT")?
            .first()
            .is_some_and(|record| record.starts_with(b"MEAMSCAT1|USER.CAT|")),
        "import" => dataset_records(dataset, "USER.A")? == [b"AA11".to_vec()],
        "import-connect" => catalog_contains(dataset, "USER.CAT")?,
        "recover" => matches!(
            dataset.invoke(DatasetRequest::Describe {
                dataset: name("USER.A"),
            }),
            Ok(DatasetResult::Description(description))
                if description.definition.lifecycle.state == DatasetLifecycleState::Closed
                    && dataset_records(dataset, "USER.A")? == [b"AA11".to_vec()]
        ),
        "repro" => dataset_records(dataset, "USER.B")? == [b"AA11".to_vec()],
        "diagnose" | "examine" | "listcat" | "listdata" | "print" | "shcds" => !batch
            .spool(job_id, "SYSPRINT", 0, 64)
            .map_err(|problem| problem.to_string())?
            .0
            .is_empty(),
        "verify" => dataset
            .invoke(DatasetRequest::Attributes {
                dataset: name("USER.A"),
            })
            .is_ok(),
        _ => false,
    };
    ok.then_some(())
        .ok_or_else(|| format!("{} did not retain its typed effect", command.id()))
}

fn dataset_service() -> (Arc<dyn ProviderStateStore>, Arc<DatasetService>) {
    let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(StoreLimits::default()));
    let service = DatasetService::open(store.clone(), DatasetLimits::default())
        .expect("bounded fixture store opens");
    (store, service)
}

fn mutation(sequence: u64) -> Mutation {
    Mutation {
        sequence,
        idempotency_key: IdempotencyKey::new(
            format!("dataset-conformance-{sequence}"),
            InvocationLimits::default(),
        )
        .expect("fixture idempotency key"),
        transaction: None,
    }
}

fn attributes(
    organization: DatasetOrganization,
    record_format: RecordFormat,
    length: u32,
) -> DatasetAttributes {
    DatasetAttributes {
        organization,
        record_format,
        logical_record_length: length,
        key_offset: (organization == DatasetOrganization::KeySequenced).then_some(0),
        key_length: (organization == DatasetOrganization::KeySequenced).then_some(2),
        ccsid: Some(37),
    }
}

fn name(value: &str) -> DatasetName {
    DatasetName::new(value, 44).expect("fixture dataset name")
}

fn version(service: &DatasetService, dataset: DatasetName) -> Result<u64, String> {
    match service
        .invoke(DatasetRequest::Attributes { dataset })
        .map_err(|problem| problem.to_string())?
    {
        DatasetResult::Attributes { version, .. } => Ok(version),
        result => Err(format!("unexpected attributes result {result:?}")),
    }
}

fn valid_ksds() -> Result<String, String> {
    let (store, service) = dataset_service();
    let dataset = name("CONF.KSDS");
    service
        .invoke(DatasetRequest::Create {
            dataset: dataset.clone(),
            attributes: attributes(DatasetOrganization::KeySequenced, RecordFormat::Fixed, 4),
            mutation: mutation(1),
        })
        .map_err(|problem| problem.to_string())?;
    service
        .invoke(DatasetRequest::Write {
            dataset: dataset.clone(),
            member: None,
            records: vec![b"AA11".to_vec()],
            expected_version: Some(1),
            mutation: mutation(2),
        })
        .map_err(|problem| problem.to_string())?;
    let restarted = DatasetService::open(store, DatasetLimits::default())
        .map_err(|problem| problem.to_string())?;
    match restarted
        .invoke(DatasetRequest::Read {
            dataset,
            member: None,
            key: Some(b"AA".to_vec()),
            max_records: 1,
        })
        .map_err(|problem| problem.to_string())?
    {
        DatasetResult::Records {
            records,
            identities,
            ..
        } if records == [b"AA11".to_vec()] && identities == [b"AA".to_vec()] => Ok(
            "organization=ksds;outcome=valid;access=key;identity=AA;bytes=AA11;restart=true".into(),
        ),
        result => Err(format!("unexpected KSDS result {result:?}")),
    }
}

fn invalid_ksds() -> Result<String, String> {
    let (_, service) = dataset_service();
    let dataset = name("CONF.KSDS");
    service
        .invoke(DatasetRequest::Create {
            dataset: dataset.clone(),
            attributes: attributes(DatasetOrganization::KeySequenced, RecordFormat::Fixed, 4),
            mutation: mutation(1),
        })
        .map_err(|problem| problem.to_string())?;
    service
        .invoke(DatasetRequest::Write {
            dataset: dataset.clone(),
            member: None,
            records: vec![b"AA11".to_vec()],
            expected_version: Some(1),
            mutation: mutation(2),
        })
        .map_err(|problem| problem.to_string())?;
    let condition = service.invoke(DatasetRequest::Write {
        dataset: dataset.clone(),
        member: None,
        records: vec![b"AA22".to_vec()],
        expected_version: Some(2),
        mutation: mutation(3),
    });
    if !matches!(condition, Err(HostProblem::Condition { ref name, response: 14, .. }) if name == "DUPREC")
        || version(&service, dataset)? != 2
    {
        return Err(format!(
            "KSDS duplicate mutation was not rejected: {condition:?}"
        ));
    }
    Ok("organization=ksds;outcome=condition;condition=DUPREC;version=2;unchanged=true".into())
}

fn valid_esds() -> Result<String, String> {
    let (store, service) = dataset_service();
    let dataset = name("CONF.ESDS");
    service
        .invoke(DatasetRequest::Create {
            dataset: dataset.clone(),
            attributes: attributes(
                DatasetOrganization::EntrySequenced,
                RecordFormat::Variable,
                8,
            ),
            mutation: mutation(1),
        })
        .map_err(|problem| problem.to_string())?;
    service
        .invoke(DatasetRequest::Write {
            dataset: dataset.clone(),
            member: None,
            records: vec![b"AA".to_vec(), b"BBB".to_vec()],
            expected_version: Some(1),
            mutation: mutation(2),
        })
        .map_err(|problem| problem.to_string())?;
    let restarted = DatasetService::open(store, DatasetLimits::default())
        .map_err(|problem| problem.to_string())?;
    match restarted
        .invoke(DatasetRequest::ReadRba {
            dataset,
            rba: 2,
            max_bytes: 8,
        })
        .map_err(|problem| problem.to_string())?
    {
        DatasetResult::Rba { data, rba: 2, .. } if data == b"BBB" => {
            Ok("organization=esds;outcome=valid;access=rba;rba=2;bytes=BBB;restart=true".into())
        }
        result => Err(format!("unexpected ESDS result {result:?}")),
    }
}

fn invalid_esds() -> Result<String, String> {
    let (_, service) = dataset_service();
    let dataset = name("CONF.ESDS");
    service
        .invoke(DatasetRequest::Create {
            dataset: dataset.clone(),
            attributes: attributes(
                DatasetOrganization::EntrySequenced,
                RecordFormat::Variable,
                8,
            ),
            mutation: mutation(1),
        })
        .map_err(|problem| problem.to_string())?;
    service
        .invoke(DatasetRequest::Write {
            dataset: dataset.clone(),
            member: None,
            records: vec![b"AA".to_vec(), b"BBB".to_vec()],
            expected_version: Some(1),
            mutation: mutation(2),
        })
        .map_err(|problem| problem.to_string())?;
    let condition = service.invoke(DatasetRequest::ReadRba {
        dataset: dataset.clone(),
        rba: 1,
        max_bytes: 8,
    });
    if !matches!(condition, Err(HostProblem::Condition { ref name, response: 13, .. }) if name == "NOTFND")
        || version(&service, dataset)? != 2
    {
        return Err(format!("ESDS interior RBA was not rejected: {condition:?}"));
    }
    Ok("organization=esds;outcome=condition;condition=NOTFND;version=2;unchanged=true".into())
}

fn valid_lds() -> Result<String, String> {
    let (store, service) = dataset_service();
    let dataset = name("CONF.LDS");
    service
        .invoke(DatasetRequest::Create {
            dataset: dataset.clone(),
            attributes: attributes(DatasetOrganization::Linear, RecordFormat::Undefined, 1024),
            mutation: mutation(1),
        })
        .map_err(|problem| problem.to_string())?;
    service
        .invoke(DatasetRequest::WriteRba {
            dataset: dataset.clone(),
            rba: 0,
            data: b"HELLO".to_vec(),
            expected_version: Some(1),
            mutation: mutation(2),
        })
        .map_err(|problem| problem.to_string())?;
    let restarted = DatasetService::open(store, DatasetLimits::default())
        .map_err(|problem| problem.to_string())?;
    match restarted
        .invoke(DatasetRequest::ReadRba {
            dataset,
            rba: 0,
            max_bytes: 5,
        })
        .map_err(|problem| problem.to_string())?
    {
        DatasetResult::Rba { data, rba: 0, .. } if data == b"HELLO" => {
            Ok("organization=lds;outcome=valid;access=rba;rba=0;bytes=HELLO;restart=true".into())
        }
        result => Err(format!("unexpected LDS result {result:?}")),
    }
}

fn invalid_lds() -> Result<String, String> {
    let (_, service) = dataset_service();
    let dataset = name("CONF.LDS");
    service
        .invoke(DatasetRequest::Create {
            dataset: dataset.clone(),
            attributes: attributes(DatasetOrganization::Linear, RecordFormat::Undefined, 1024),
            mutation: mutation(1),
        })
        .map_err(|problem| problem.to_string())?;
    let condition = service.invoke(DatasetRequest::WriteRba {
        dataset: dataset.clone(),
        rba: 1,
        data: b"X".to_vec(),
        expected_version: Some(1),
        mutation: mutation(2),
    });
    if !matches!(condition, Err(HostProblem::Condition { ref name, response: 13, .. }) if name == "NOTFND")
        || version(&service, dataset)? != 1
    {
        return Err(format!("LDS sparse RBA was not rejected: {condition:?}"));
    }
    Ok("organization=lds;outcome=condition;condition=NOTFND;version=1;unchanged=true".into())
}

fn valid_rrds() -> Result<String, String> {
    valid_relative(
        DatasetOrganization::Relative,
        RecordFormat::Fixed,
        "rrds",
        b"F002",
    )
}

fn valid_vrrds() -> Result<String, String> {
    valid_relative(
        DatasetOrganization::VariableRelative,
        RecordFormat::Variable,
        "vrrds",
        b"FOUR",
    )
}

fn valid_relative(
    organization: DatasetOrganization,
    record_format: RecordFormat,
    label: &str,
    record: &[u8],
) -> Result<String, String> {
    let (store, service) = dataset_service();
    let dataset = name(if label == "rrds" {
        "CONF.RRDS"
    } else {
        "CONF.VRRDS"
    });
    service
        .invoke(DatasetRequest::Create {
            dataset: dataset.clone(),
            attributes: attributes(organization, record_format, 4),
            mutation: mutation(1),
        })
        .map_err(|problem| problem.to_string())?;
    service
        .invoke(DatasetRequest::WriteRelative {
            dataset: dataset.clone(),
            record_number: if label == "rrds" { 2 } else { 4 },
            record: record.to_vec(),
            expected_version: Some(1),
            mutation: mutation(2),
        })
        .map_err(|problem| problem.to_string())?;
    let restarted = DatasetService::open(store, DatasetLimits::default())
        .map_err(|problem| problem.to_string())?;
    let rrn = if label == "rrds" { 2 } else { 4 };
    match restarted
        .invoke(DatasetRequest::ReadRelative {
            dataset,
            record_number: rrn,
        })
        .map_err(|problem| problem.to_string())?
    {
        DatasetResult::Records { records, .. } if records == [record.to_vec()] => Ok(format!(
            "organization={label};outcome=valid;access=rrn;rrn={rrn};bytes={};restart=true",
            String::from_utf8_lossy(record)
        )),
        result => Err(format!("unexpected {label} result {result:?}")),
    }
}

fn invalid_rrds() -> Result<String, String> {
    let (_, service) = dataset_service();
    let dataset = name("CONF.RRDS");
    service
        .invoke(DatasetRequest::Create {
            dataset: dataset.clone(),
            attributes: attributes(DatasetOrganization::Relative, RecordFormat::Fixed, 4),
            mutation: mutation(1),
        })
        .map_err(|problem| problem.to_string())?;
    let condition = service.invoke(DatasetRequest::WriteRelative {
        dataset: dataset.clone(),
        record_number: 0,
        record: b"F000".to_vec(),
        expected_version: Some(1),
        mutation: mutation(2),
    });
    if condition != Err(HostProblem::Malformed) || version(&service, dataset)? != 1 {
        return Err(format!("RRDS zero RRN was not rejected: {condition:?}"));
    }
    Ok("organization=rrds;outcome=condition;condition=MALFORMED;version=1;unchanged=true".into())
}

fn invalid_vrrds() -> Result<String, String> {
    let (_, service) = dataset_service();
    let dataset = name("CONF.VRRDS");
    service
        .invoke(DatasetRequest::Create {
            dataset: dataset.clone(),
            attributes: attributes(
                DatasetOrganization::VariableRelative,
                RecordFormat::Variable,
                4,
            ),
            mutation: mutation(1),
        })
        .map_err(|problem| problem.to_string())?;
    let condition = service.invoke(DatasetRequest::WriteRelative {
        dataset: dataset.clone(),
        record_number: 1,
        record: b"TOO-LONG".to_vec(),
        expected_version: Some(1),
        mutation: mutation(2),
    });
    if !matches!(condition, Err(HostProblem::Condition { ref name, response: 22, .. }) if name == "LENGERR")
        || version(&service, dataset)? != 1
    {
        return Err(format!("VRRDS long record was not rejected: {condition:?}"));
    }
    Ok("organization=vrrds;outcome=condition;condition=LENGERR;version=1;unchanged=true".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_coverage::{CoverageGate, RunnerContext, RunnerSelection};

    #[test]
    fn fixture_expectations_are_unique_and_drivers_match() {
        for id in FIXTURE_IDS {
            let expected = fixture_expected(id).unwrap();
            let output = DatasetOrganizationDriver
                .execute(&FixtureRef::new(id, ConformanceLimits::default()).unwrap())
                .unwrap();
            assert_eq!(output.bytes(), expected.as_bytes());
        }
        for id in AMS_FIXTURE_IDS {
            let expected = fixture_expected(id).unwrap();
            let output = AmsCommandDriver
                .execute(&FixtureRef::new(id, ConformanceLimits::default()).unwrap())
                .unwrap();
            assert_eq!(output.bytes(), expected.as_bytes());
        }
    }

    #[test]
    fn exact_observations_kill_generic_success_and_byte_mutants() {
        let observation = ExpectedObservation {
            fixture: "dataset.ksds.valid",
        };
        for mutant in [
            b"success".as_slice(),
            b"organization=ksds;outcome=valid;access=key;identity=AA;bytes=AA12;restart=true",
        ] {
            let output = DriverOutput::new(mutant.to_vec(), ConformanceLimits::default()).unwrap();
            assert!(!observation.evaluate(&output).unwrap().matched);
        }
    }

    #[allow(dead_code)]
    fn runner_types_remain_linked(
        spec: &CompiledSpec,
        context: &RunnerContext,
    ) -> Result<ConformanceRunReport, String> {
        let selection = RunnerSelection::focused(
            "dataset-vsam-ams",
            Some(CoverageGate::Executed),
            None,
            ConformanceLimits::default(),
        )
        .map_err(|problem| problem.to_string())?;
        run_dataset_conformance(spec, &selection, context)
    }
}
