use mainframe_env_batch::{AmsStatement, parse_idcams_control, validate_idcams_control};
use mainframe_env_coverage::{
    CompiledSpec, ConformanceDriver, ConformanceLimits, ConformanceObservation,
    ConformanceRunReport, ConformanceRunner, DriverOutput, DriverRef, FixtureRef, ObservationCheck,
    ObservationRef, RunnerContext, RunnerSelection, RuntimeRegistry,
};
use mainframe_env_dataset::{DatasetLimits, DatasetService};
use mainframe_env_execution_api::{IdempotencyKey, InvocationLimits};
use mainframe_env_host_api::{
    DatasetAttributes, DatasetName, DatasetOrganization, DatasetRequest, DatasetResult,
    HostProblem, Mutation, RecordFormat,
};
use mainframe_env_store::{MemoryStore, StoreLimits};
use mainframe_env_store_api::ProviderStateStore;
use serde_json::Value;
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
    let limits = ConformanceLimits::default();
    let driver = DatasetOrganizationDriver;
    let ams_driver = AmsCommandDriver;
    let observations = FIXTURE_IDS
        .into_iter()
        .chain(AMS_FIXTURE_IDS)
        .map(|fixture| ExpectedObservation { fixture })
        .collect::<Vec<_>>();
    let runtime = RuntimeRegistry::new(
        spec,
        vec![
            (
                DriverRef::new("dataset.organization.driver", limits)
                    .map_err(|problem| problem.to_string())?,
                &driver as &dyn ConformanceDriver,
            ),
            (
                DriverRef::new("dataset.ams.driver", limits)
                    .map_err(|problem| problem.to_string())?,
                &ams_driver as &dyn ConformanceDriver,
            ),
        ],
        Vec::new(),
        observations
            .iter()
            .map(|observation| {
                Ok((
                    ObservationRef::new(format!("observe.{}", observation.fixture), limits)
                        .map_err(|problem| problem.to_string())?,
                    observation as &dyn ConformanceObservation,
                ))
            })
            .collect::<Result<Vec<_>, String>>()?,
        limits,
    )
    .map_err(|problem| problem.to_string())?;
    ConformanceRunner::new(spec, runtime, limits)
        .run(selection, context)
        .map_err(|problem| problem.to_string())
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
