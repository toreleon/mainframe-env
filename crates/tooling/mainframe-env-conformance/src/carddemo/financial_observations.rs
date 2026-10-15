//! Complete observations for the expressly qualified current manual-seed route.
//! Expected digests come from independent pinned-source arithmetic and bytes,
//! never from the captured provider. Raw expected fixtures remain external.

mod fixtures;

use super::{
    CorpusProblem, ProductServer, RouteObservations, base_batch_control_invocation,
    journey_closure::AuthorityKind, read_corpus_file, terminal_problem,
};
use mainframe_env_batch::{JobState, StepState, StepTermination};
use mainframe_env_host_api::{
    DatasetAttributes, DatasetName, DatasetOrganization, DatasetRequest, DatasetResult,
    HostProblem, RecordFormat,
};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::Path};

const PAGE: u32 = 4_096;
const JOBS: [&str; 8] = [
    "POSTTRAN", "INTCALC", "TRANBKP", "COMBTRAN", "TRANIDX", "TRANREPT", "PRTCATBL", "CREASTMT",
];

#[derive(Clone, Copy)]
struct DatasetExpected {
    phase: &'static str,
    name: &'static str,
    width: u32,
    record_format: RecordFormat,
    records: usize,
    key: Option<(u32, u32)>,
    framed_sha256: &'static str,
}

#[derive(Clone, Copy)]
struct SpoolExpected {
    job: &'static str,
    file: &'static str,
    records: usize,
    bytes: usize,
    framed_sha256: &'static str,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CapturedDataset {
    attributes: DatasetAttributes,
    version: u64,
    records: usize,
    framed_sha256: String,
}

#[derive(Default)]
pub(super) struct FinancialChecks {
    initial: bool,
    jobs: Vec<(String, String)>,
    final_datasets: BTreeMap<&'static str, (DatasetExpected, CapturedDataset)>,
    restart_sealed: bool,
    reopened: bool,
    comparisons: BTreeMap<String, serde_json::Value>,
}

fn drift(detail: impl Into<String>) -> CorpusProblem {
    CorpusProblem::new("carddemo.batch_financial.source_drift", detail)
}

pub(super) fn require_source_input(relative: &str, bytes: &[u8]) -> Result<(), CorpusProblem> {
    let Some((_, expected)) = fixtures::SOURCE_PINS
        .iter()
        .find(|(path, _)| *path == relative)
    else {
        return Err(drift(format!(
            "{relative} has no independently frozen source pin"
        )));
    };
    if format!("{:x}", Sha256::digest(bytes)) != *expected {
        return Err(drift(format!(
            "{relative} differs from its independent source pin"
        )));
    }
    Ok(())
}

pub(super) fn source_pin_matches(relative: &str, digest: &str) -> bool {
    fixtures::SOURCE_PINS
        .iter()
        .any(|&(path, expected)| path == relative && expected == digest)
}

pub(super) fn require_statement_input(bytes: &[u8]) -> Result<(), CorpusProblem> {
    if format!("{:x}", Sha256::digest(bytes)) != fixtures::CORRECTED_STATEMENT_SHA256 {
        return Err(drift(
            "offered CREASTMT differs from the existing exact source correction",
        ));
    }
    Ok(())
}

impl FinancialChecks {
    pub(super) fn prepare(corpus: &Path, commit: &str, clock: &str) -> Result<Self, CorpusProblem> {
        if commit != fixtures::CORPUS_COMMIT || clock != "2022070600000000+0000" {
            return Err(drift("financial source commit or offered clock differs"));
        }
        for (relative, _) in fixtures::SOURCE_PINS {
            require_source_input(relative, &read_corpus_file(corpus, &corpus.join(relative))?)?;
        }
        Ok(Self::default())
    }

    pub(super) fn capture_initial(&mut self, server: &ProductServer) -> Result<(), CorpusProblem> {
        if self.initial || !self.jobs.is_empty() {
            return Err(drift("initial financial state was captured out of order"));
        }
        self.capture_phase(server, "initial")?;
        self.initial = true;
        Ok(())
    }

    fn capture_phase(&mut self, server: &ProductServer, phase: &str) -> Result<(), CorpusProblem> {
        for expected in fixtures::DATASETS
            .iter()
            .filter(|value| value.phase == phase)
        {
            let captured =
                capture_dataset(expected, |request| server.dataset_service().invoke(request))?;
            if let Some(name) =
                super::base_batch_evidence::comparison_name(expected.phase, expected.name)
            {
                self.comparisons.insert(
                    name.into(),
                    serde_json::json!({
                        "dataset": expected.name,
                        "attributes": captured.attributes,
                        "version": captured.version,
                        "records": captured.records,
                        "record_u32_framed_sha256": captured.framed_sha256,
                    }),
                );
            }
            self.final_datasets
                .insert(expected.name, (*expected, captured));
        }
        Ok(())
    }

    pub(super) fn job_done(
        &mut self,
        server: &ProductServer,
        label: &str,
        id: &str,
    ) -> Result<(), CorpusProblem> {
        if !self.initial
            || self.reopened
            || JOBS.get(self.jobs.len()) != Some(&label)
            || self.jobs.iter().any(|(_, prior)| prior == id)
        {
            return Err(drift(
                "financial jobs are missing, repeated or out of offered order",
            ));
        }
        let job = server.batch_service().get(id).map_err(terminal_problem)?;
        let code = if label == "POSTTRAN" { 4 } else { 0 };
        if job.id != id
            || job.name != label
            || job.owner != "IBMUSER"
            || job.state != JobState::Completed
            || job.return_code != Some(code)
            || job.abend_code.is_some()
            || job.active_step.is_some()
        {
            return Err(drift(format!(
                "{label} is not the actual completed owner job"
            )));
        }
        if matches!(label, "POSTTRAN" | "INTCALC")
            && (job.steps.len() != 1
                || job.steps[0].name != "STEP15"
                || job.steps[0].state != StepState::Completed
                || job.steps[0].termination != Some(StepTermination::ReturnCode { code }))
        {
            return Err(drift(format!("{label} STEP15 outcome differs")));
        }
        self.capture_phase(server, label)?;
        if let Some(expected) = fixtures::SPOOLS.iter().find(|value| value.job == label) {
            capture_spool(server, id, expected)?;
        }
        self.jobs.push((label.into(), id.into()));
        Ok(())
    }

    pub(super) fn compare_reopened(&mut self, server: &ProductServer) -> Result<(), CorpusProblem> {
        if !self.restart_sealed || self.reopened {
            return Err(drift(
                "reopen comparison requires every actual offered financial job",
            ));
        }
        for (expected, before) in self.final_datasets.values() {
            let reopened =
                capture_dataset(expected, |request| server.dataset_service().invoke(request))?;
            if &reopened != before {
                return Err(drift(format!("{} changed across reopen", expected.name)));
            }
        }
        for expected in fixtures::SPOOLS {
            let id = &self
                .jobs
                .iter()
                .find(|(label, _)| label == expected.job)
                .ok_or_else(|| drift("reopened program job identity is missing"))?
                .1;
            capture_spool(server, id, expected)?;
        }
        self.reopened = true;
        Ok(())
    }

    pub(super) fn seal_before_restart(
        &mut self,
        server: &ProductServer,
    ) -> Result<(), CorpusProblem> {
        if !self.initial || self.jobs.len() != JOBS.len() || self.restart_sealed || self.reopened {
            return Err(drift("final financial state was sealed out of order"));
        }
        for (expected, captured) in self.final_datasets.values_mut() {
            *captured =
                capture_dataset(expected, |request| server.dataset_service().invoke(request))?;
        }
        self.restart_sealed = true;
        Ok(())
    }

    /// The real wrapper calls this only after both checked drains and checked
    /// exact owned-root removal. No partial proof escapes a failed wrapper.
    pub(super) fn finish(
        self,
        observations: &mut RouteObservations,
    ) -> Result<super::base_batch_evidence::FinancialProof, CorpusProblem> {
        if !self.initial
            || self.jobs.len() != JOBS.len()
            || !self.restart_sealed
            || !self.reopened
            || !self
                .final_datasets
                .keys()
                .copied()
                .eq(fixtures::FINAL_NAMES.iter().copied())
        {
            return Err(drift("financial source comparisons are incomplete"));
        }
        let proof = super::base_batch_evidence::FinancialProof::complete(self.comparisons)?;
        for name in [
            "POSTTRAN step outcomes",
            "INTCALC decimal results",
            "dataset mutations",
            "return codes and spool",
        ] {
            observations.compare(AuthorityKind::Journey, "CD.J11", name, false, || {
                Ok(drift("financial proof is incomplete"))
            })?;
        }
        for name in ["SORT ordering", "report records"] {
            observations.compare(AuthorityKind::Journey, "CD.J12", name, false, || {
                Ok(drift("report proof is incomplete"))
            })?;
        }
        // No GDG lifecycle, AIX, compiled linkage, TIOT/full statement-spool,
        // licensed IBM, inline carrier or upstream signed-EDIT observation.
        Ok(proof)
    }
}

fn attributes(
    dataset: &DatasetName,
    invoke: &mut impl FnMut(DatasetRequest) -> Result<DatasetResult, HostProblem>,
) -> Result<(DatasetAttributes, u64), CorpusProblem> {
    match invoke(DatasetRequest::Attributes {
        dataset: dataset.clone(),
    })
    .map_err(terminal_problem)?
    {
        DatasetResult::Attributes {
            attributes,
            version,
        } if version != 0 => Ok((attributes, version)),
        _ => Err(drift(
            "financial dataset attributes/version are unavailable",
        )),
    }
}

fn capture_dataset(
    expected: &DatasetExpected,
    mut invoke: impl FnMut(DatasetRequest) -> Result<DatasetResult, HostProblem>,
) -> Result<CapturedDataset, CorpusProblem> {
    let dataset =
        DatasetName::new(expected.name, 128).map_err(|_| drift("invalid expected dataset"))?;
    let before = attributes(&dataset, &mut invoke)?;
    let organization = if expected.key.is_some() {
        DatasetOrganization::KeySequenced
    } else {
        DatasetOrganization::Sequential
    };
    if before.0.organization != organization
        || before.0.record_format != expected.record_format
        || before.0.logical_record_length != expected.width
        || before.0.ccsid != Some(37)
        || before.0.key_offset != expected.key.map(|key| key.0)
        || before.0.key_length != expected.key.map(|key| key.1)
    {
        return Err(drift(format!("{} layout/CCSID differs", expected.name)));
    }
    let mut digest = Sha256::new();
    let mut position = 0usize;
    while position < expected.records {
        let DatasetResult::Records {
            records,
            identities,
            version,
        } = invoke(DatasetRequest::ReadSequential {
            dataset: dataset.clone(),
            member: None,
            start: Some(position as u64),
            reverse: false,
            max_records: PAGE,
        })
        .map_err(terminal_problem)?
        else {
            return Err(drift("financial sequential page returned the wrong result"));
        };
        let count = (expected.records - position).min(PAGE as usize);
        require_page(
            expected,
            position,
            count,
            before.1,
            &records,
            &identities,
            version,
        )?;
        for record in records {
            digest.update(expected.width.to_be_bytes());
            digest.update(record);
        }
        position += count;
    }
    let end = invoke(DatasetRequest::ReadSequential {
        dataset: dataset.clone(),
        member: None,
        start: Some(expected.records as u64),
        reverse: false,
        max_records: PAGE,
    });
    require_end(end, expected.records, before.1)?;
    if attributes(&dataset, &mut invoke)? != before {
        return Err(drift(format!(
            "{} changed during page/EOF capture",
            expected.name
        )));
    }
    let framed_sha256 = format!("{:x}", digest.finalize());
    if framed_sha256 != expected.framed_sha256 {
        return Err(drift(format!(
            "{} full independent record hash differs",
            expected.name
        )));
    }
    Ok(CapturedDataset {
        attributes: before.0,
        version: before.1,
        records: position,
        framed_sha256,
    })
}

// A distinct test-only SQLite initialization producer observes the same strict
// initial oracle. Metadata is saved before each original capture predicate;
// no raw customer/user record or key leaves this boundary.
#[cfg(all(test, target_os = "linux"))]
pub(super) fn capture_initial_diagnostic(
    mut invoke: impl FnMut(DatasetRequest) -> Result<DatasetResult, HostProblem>,
    mut retain: impl FnMut(serde_json::Value) -> Result<(), CorpusProblem>,
) -> Result<serde_json::Value, CorpusProblem> {
    let expected = fixtures::DATASETS
        .iter()
        .filter(|row| row.phase == "initial")
        .collect::<Vec<_>>();
    if expected.len() != 9 {
        return Err(drift("independent nine initial expectations differ"));
    }
    let mut complete = Vec::new();
    for expected in expected {
        let captured = capture_dataset(expected, |request| {
            let outcome = invoke(request.clone());
            let metadata = match (&request, &outcome) {
                (
                    DatasetRequest::Attributes { .. },
                    Ok(DatasetResult::Attributes {
                        attributes,
                        version,
                    }),
                ) => {
                    serde_json::json!({"operation":"attributes","attributes":attributes,"version":version})
                }
                (
                    DatasetRequest::ReadSequential {
                        start,
                        max_records,
                        reverse,
                        ..
                    },
                    Ok(DatasetResult::Records {
                        records,
                        identities,
                        version,
                    }),
                ) => {
                    let rows = if records.len() <= PAGE as usize
                        && identities.len() <= PAGE as usize
                    {
                        records.iter().zip(identities).enumerate().map(|(index,(row,key))|serde_json::json!({
                            "ordinal":start.and_then(|start|start.checked_add(index as u64)),
                            "row_bytes":row.len(),"row_sha256":format!("{:x}",Sha256::digest(row)),
                            "key_bytes":key.len(),"key_sha256":format!("{:x}",Sha256::digest(key)),
                        })).collect::<Vec<_>>()
                    } else {
                        Vec::new()
                    };
                    serde_json::json!({"operation":"page","start":start,"max_records":max_records,"reverse":reverse,
                        "actual_records":records.len(),"actual_identities":identities.len(),"version":version,"rows":rows})
                }
                (
                    _,
                    Err(HostProblem::Condition {
                        name,
                        response,
                        response2,
                    }),
                ) => {
                    serde_json::json!({"operation":"condition","condition":name,"response":response,"response2":response2})
                }
                (_, Err(_)) => serde_json::json!({"operation":"provider-refused"}),
                _ => serde_json::json!({"operation":"unexpected-result"}),
            };
            retain(serde_json::json!({"phase":"initial","dataset":expected.name,"expected_records":expected.records,
                "expected_width":expected.width,"expected_key":expected.key,"expected_framed_sha256":expected.framed_sha256,"actual":metadata}))
                .map_err(|_|HostProblem::InfrastructureFailure)?;
            outcome
        })?;
        complete.push(serde_json::json!({"dataset":expected.name,"attributes":captured.attributes,"version":captured.version,
            "records":captured.records,"framed_sha256":captured.framed_sha256}));
    }
    Ok(serde_json::json!(complete))
}

fn require_page(
    expected: &DatasetExpected,
    start: usize,
    count: usize,
    expected_version: u64,
    records: &[Vec<u8>],
    identities: &[Vec<u8>],
    version: u64,
) -> Result<(), CorpusProblem> {
    if version != expected_version || records.len() != count || identities.len() != count {
        return Err(drift(format!(
            "financial page is incomplete or changed version: phase={} dataset={} start={start} wanted_records={count} actual_records={} actual_identities={} expected_version={expected_version} actual_version={version}",
            expected.phase,
            expected.name,
            records.len(),
            identities.len(),
        )));
    }
    for (index, (record, identity)) in records.iter().zip(identities).enumerate() {
        if record.len() != expected.width as usize {
            return Err(drift("financial record has the wrong fixed length"));
        }
        let wanted = match expected.key {
            Some((offset, width)) => {
                let end = offset
                    .checked_add(width)
                    .ok_or_else(|| drift("key length overflows"))?;
                record
                    .get(offset as usize..end as usize)
                    .ok_or_else(|| drift("financial key lies outside its source layout"))?
                    .to_vec()
            }
            None => ((start + index) as u64).to_be_bytes().to_vec(),
        };
        if identity != &wanted {
            return Err(drift(
                "financial record identity differs from exact source ordinal/key",
            ));
        }
    }
    Ok(())
}

fn require_end(
    outcome: Result<DatasetResult, HostProblem>,
    records: usize,
    version: u64,
) -> Result<(), CorpusProblem> {
    match outcome {
        Err(HostProblem::Condition {
            name,
            response: 13,
            response2: 0,
        }) if records != 0 && name == "NOTFND" => Ok(()),
        Ok(DatasetResult::Records {
            records: values,
            identities,
            version: actual,
        }) if records == 0 && values.is_empty() && identities.is_empty() && actual == version => {
            Ok(())
        }
        _ => Err(drift(
            "financial record EOF differs or unseen extra records remain",
        )),
    }
}

fn require_spool(
    expected: &SpoolExpected,
    records: &[Vec<u8>],
    more: bool,
) -> Result<(), CorpusProblem> {
    if more
        || records.len() != expected.records
        || records.iter().map(Vec::len).sum::<usize>() != expected.bytes
    {
        return Err(drift("financial program spool is incomplete"));
    }
    let mut digest = Sha256::new();
    for record in records {
        let length = u32::try_from(record.len())
            .map_err(|_| drift("spool record length overflows framing"))?;
        digest.update(length.to_be_bytes());
        digest.update(record);
    }
    if format!("{:x}", digest.finalize()) != expected.framed_sha256 {
        return Err(drift(
            "complete program DISPLAY spool differs from independent source rows",
        ));
    }
    Ok(())
}

fn capture_spool(
    server: &ProductServer,
    id: &str,
    expected: &SpoolExpected,
) -> Result<(), CorpusProblem> {
    let invocation = base_batch_control_invocation()?;
    let files = server
        .batch_service()
        .spool_files(&invocation, id)
        .map_err(terminal_problem)?;
    let matching = files
        .iter()
        .filter(|(_, file, _, _)| file == expected.file)
        .collect::<Vec<_>>();
    if matching.len() != 1 || matching[0].2 != expected.records || matching[0].3 != expected.bytes {
        return Err(drift("actual owner program spool descriptor differs"));
    }
    let (records, more) = server
        .batch_service()
        .spool(&invocation, id, expected.file, 0, PAGE as usize)
        .map_err(terminal_problem)?;
    require_spool(expected, &records, more)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> (DatasetExpected, Vec<Vec<u8>>, Vec<Vec<u8>>) {
        let expected = DatasetExpected {
            phase: "test-only",
            name: "AWS.M2.CARDDEMO.TEST",
            width: 2,
            record_format: RecordFormat::Fixed,
            records: 2,
            key: None,
            // Independently frozen Python SHA256 of u32(2)+[1,2]+u32(2)+[3,4].
            framed_sha256: "93596cbcbd47e65c74edd35e35cbf23cd81d520f8bd04987ae9449c22a4632ba",
        };
        (
            expected,
            vec![vec![1, 2], vec![3, 4]],
            vec![0u64.to_be_bytes().to_vec(), 1u64.to_be_bytes().to_vec()],
        )
    }

    #[test]
    fn truncation_and_extra_rows_are_refused() {
        let (expected, mut rows, mut ids) = sample();
        assert!(require_page(&expected, 0, 2, 7, &rows[..1], &ids[..1], 7).is_err());
        rows.push(vec![5, 6]);
        ids.push(2u64.to_be_bytes().to_vec());
        assert!(require_page(&expected, 0, 2, 7, &rows, &ids, 7).is_err());
    }

    #[test]
    fn page_refusal_retains_exact_owner_counts_and_versions_without_record_bytes() {
        let (expected, rows, ids) = sample();
        for (count, actual_rows, actual_ids, version) in [
            (2, rows[..1].to_vec(), ids[..1].to_vec(), 7),
            (2, rows.clone(), ids[..1].to_vec(), 7),
            (2, rows.clone(), ids.clone(), 8),
        ] {
            let error = require_page(
                &expected,
                4096,
                count,
                7,
                &actual_rows,
                &actual_ids,
                version,
            )
            .unwrap_err();
            assert_eq!(error.code, "carddemo.batch_financial.source_drift");
            assert_eq!(
                error.detail,
                format!(
                    "financial page is incomplete or changed version: phase=test-only dataset=AWS.M2.CARDDEMO.TEST start=4096 wanted_records=2 actual_records={} actual_identities={} expected_version=7 actual_version={version}",
                    actual_rows.len(),
                    actual_ids.len(),
                )
            );
        }
    }

    #[test]
    fn diagnostic_keeps_exact_count_version_and_identity_admission() {
        let (expected, rows, ids) = sample();
        assert!(require_page(&expected, 0, 2, 7, &rows, &ids, 7).is_ok());
        assert!(require_page(&expected, 0, 1, 7, &rows, &ids, 7).is_err());
        assert!(require_page(&expected, 0, 2, 8, &rows, &ids, 7).is_err());
        assert!(require_page(&expected, 1, 2, 7, &rows, &ids, 7).is_err());
        assert!(require_page(&expected, 0, 2, 7, &rows, &ids[..1], 7).is_err());
    }

    fn provider_capture(
        keyed: bool,
        count: usize,
        wanted: usize,
        framed_sha256: &'static str,
    ) -> Result<CapturedDataset, CorpusProblem> {
        use mainframe_env_dataset::DatasetService;
        use mainframe_env_execution_api::{IdempotencyKey, InvocationLimits};
        use mainframe_env_host_api::Mutation;
        use mainframe_env_store::MemoryStore;
        use std::sync::Arc;
        let service = DatasetService::open(
            Arc::new(MemoryStore::new(Default::default())),
            Default::default(),
        )
        .unwrap();
        let mut attributes = layout();
        if keyed {
            attributes.organization = DatasetOrganization::KeySequenced;
            attributes.key_offset = Some(0);
            attributes.key_length = Some(2);
        }
        let dataset = DatasetName::new("AWS.M2.CARDDEMO.TEST", 128).unwrap();
        let mutation = |sequence| Mutation {
            sequence,
            idempotency_key: IdempotencyKey::new(
                format!("financial-provider-control-{sequence}"),
                InvocationLimits::default(),
            )
            .unwrap(),
            transaction: None,
        };
        let created = service
            .invoke(DatasetRequest::Create {
                dataset: dataset.clone(),
                attributes,
                mutation: mutation(1),
            })
            .unwrap();
        let DatasetResult::Created { version } = created else {
            panic!("actual provider create result differs");
        };
        service
            .invoke(DatasetRequest::Write {
                dataset,
                member: None,
                records: (0..count)
                    .map(|i| (i as u16).to_be_bytes().to_vec())
                    .collect(),
                expected_version: Some(version),
                mutation: mutation(2),
            })
            .unwrap();
        let expected = DatasetExpected {
            records: wanted,
            key: keyed.then_some((0, 2)),
            framed_sha256,
            ..sample().0
        };
        let captured = capture_dataset(&expected, |request| {
            if let DatasetRequest::ReadSequential {
                max_records,
                reverse,
                ..
            } = &request
            {
                assert_eq!(*max_records, PAGE);
                assert!(!*reverse);
            }
            service.invoke(request)
        })?;
        assert_eq!(captured.version, 2);
        Ok(captured)
    }

    #[test]
    fn actual_default_provider_ps_pagination_matches6632_full_rows_and_exact_eof() {
        // First execution of the actual bounded API control; zero application credit.
        let hash = "2ce02d624a7814ebdc88a53f3a11bb7958bbc77d402548eed75aa89d37668bcf";
        assert_eq!(
            provider_capture(false, 6632, 6632, hash).unwrap().records,
            6632
        );
        assert!(provider_capture(false, 6631, 6632, hash).is_err());
        assert!(provider_capture(false, 6633, 6632, hash).is_err());
    }

    #[test]
    fn actual_default_provider_ksds_ordinal_pages_keep_full_keys_and_version() {
        // Independent source preparation frames each u16 ordinal with u32(2).
        let hash = "49470c22112872bcd982185c588061c65a592e27b1d962285114fc061ecc1132";
        assert_eq!(provider_capture(true, 50, 50, hash).unwrap().records, 50);
        assert!(provider_capture(true, 49, 50, hash).is_err());
        assert!(provider_capture(true, 51, 50, hash).is_err());
    }

    #[test]
    fn stale_version_and_malformed_monotonic_identities_are_refused() {
        let (expected, rows, mut ids) = sample();
        assert!(require_page(&expected, 0, 2, 7, &rows, &ids, 8).is_err());
        ids = vec![1u64.to_be_bytes().to_vec(), 2u64.to_be_bytes().to_vec()];
        assert!(require_page(&expected, 0, 2, 7, &rows, &ids, 7).is_err());
    }

    #[test]
    fn wrong_eof_and_extra_tail_are_refused() {
        assert!(
            require_end(
                Err(HostProblem::Condition {
                    name: "NOTFND".into(),
                    response: 13,
                    response2: 80
                }),
                2,
                7
            )
            .is_err()
        );
        assert!(require_end(Err(HostProblem::ProviderFailure), 2, 7).is_err());
        assert!(
            require_end(
                Ok(DatasetResult::Records {
                    records: vec![vec![1, 2]],
                    identities: vec![vec![0; 8]],
                    version: 7
                }),
                2,
                7
            )
            .is_err()
        );
    }

    #[test]
    fn source_pin_changes_are_refused() {
        assert!(require_source_input(fixtures::SOURCE_PINS[0].0, b"changed source").is_err());
        assert!(require_source_input("unoffered/source.cbl", b"anything").is_err());
    }

    #[test]
    fn incomplete_lifecycle_has_no_credit() {
        for checks in [
            FinancialChecks::default(),
            FinancialChecks {
                initial: true,
                jobs: JOBS
                    .iter()
                    .map(|name| (name.to_string(), name.to_string()))
                    .collect(),
                reopened: false,
                ..Default::default()
            },
            FinancialChecks {
                initial: true,
                restart_sealed: true,
                reopened: true,
                jobs: JOBS
                    .iter()
                    .map(|name| (name.to_string(), name.to_string()))
                    .collect(),
                ..Default::default()
            },
        ] {
            let mut observations = RouteObservations::default();
            assert!(checks.finish(&mut observations).is_err());
            assert!(observations.journeys().is_empty());
            assert!(observations.issues().is_empty());
        }
    }

    fn layout() -> DatasetAttributes {
        DatasetAttributes {
            organization: DatasetOrganization::Sequential,
            record_format: RecordFormat::Fixed,
            logical_record_length: 2,
            key_offset: None,
            key_length: None,
            ccsid: Some(37),
        }
    }

    fn capture_sample(
        rows: Vec<Vec<u8>>,
        changed_after_eof: bool,
        changed_version: bool,
    ) -> Result<CapturedDataset, CorpusProblem> {
        let (expected, _, _) = sample();
        let mut attribute_reads = 0;
        capture_dataset(&expected, |request| match request {
            DatasetRequest::Attributes { .. } => {
                attribute_reads += 1;
                let mut attributes = layout();
                if attribute_reads == 2 && changed_after_eof {
                    attributes.ccsid = Some(1208);
                }
                Ok(DatasetResult::Attributes {
                    attributes,
                    version: if attribute_reads == 2 && changed_version {
                        8
                    } else {
                        7
                    },
                })
            }
            DatasetRequest::ReadSequential {
                start: Some(0),
                max_records: PAGE,
                reverse: false,
                ..
            } => Ok(DatasetResult::Records {
                records: rows.clone(),
                identities: (0..rows.len())
                    .map(|i| (i as u64).to_be_bytes().to_vec())
                    .collect(),
                version: 7,
            }),
            DatasetRequest::ReadSequential {
                start: Some(2),
                max_records: PAGE,
                reverse: false,
                ..
            } => Err(HostProblem::Condition {
                name: "NOTFND".into(),
                response: 13,
                response2: 0,
            }),
            _ => Err(HostProblem::Unsupported),
        })
    }

    #[test]
    fn metadata_or_version_change_after_eof_refuses() {
        let (_, rows, _) = sample();
        assert!(capture_sample(rows.clone(), true, false).is_err());
        assert!(capture_sample(rows, false, true).is_err());
    }

    #[test]
    fn source_layout_mutation_refuses_before_read() {
        let expected = sample().0;
        let captured = capture_dataset(&expected, |request| match request {
            DatasetRequest::Attributes { .. } => {
                let mut attributes = layout();
                attributes.logical_record_length = 3;
                Ok(DatasetResult::Attributes {
                    attributes,
                    version: 7,
                })
            }
            _ => panic!("invalid source layout must refuse before record capture"),
        });
        assert!(captured.is_err());
        assert_exact_source_formats_and_encoding();
    }

    fn assert_exact_source_formats_and_encoding() {
        let blocked_names = [
            "AWS.M2.CARDDEMO.TRANSACT.BKUP.G0001V00",
            "AWS.M2.CARDDEMO.TRANSACT.BKUP.G0002V00",
            "AWS.M2.CARDDEMO.TRANSACT.COMBINED.G0001V00",
            "AWS.M2.CARDDEMO.TRANSACT.DALY.G0001V00",
            "AWS.M2.CARDDEMO.TRANREPT.G0001V00",
            "AWS.M2.CARDDEMO.TCATBALF.BKUP.G0001V00",
            "AWS.M2.CARDDEMO.TCATBALF.REPT",
            "AWS.M2.CARDDEMO.TRXFL.SEQ",
            "AWS.M2.CARDDEMO.STATEMNT.PS",
            "AWS.M2.CARDDEMO.STATEMNT.HTML",
        ];
        for expected in fixtures::DATASETS {
            assert_eq!(
                expected.record_format,
                if blocked_names.contains(&expected.name) {
                    RecordFormat::FixedBlocked
                } else {
                    RecordFormat::Fixed
                }
            );
        }
        for record_format in [RecordFormat::Fixed, RecordFormat::FixedBlocked] {
            let expected = DatasetExpected {
                record_format,
                ..sample().0
            };
            let (_, rows, ids) = sample();
            let mut attributes = layout();
            attributes.record_format = record_format;
            let captured = capture_dataset(&expected, |request| match request {
                DatasetRequest::Attributes { .. } => Ok(DatasetResult::Attributes {
                    attributes: attributes.clone(),
                    version: 7,
                }),
                DatasetRequest::ReadSequential { start: Some(0), .. } => {
                    Ok(DatasetResult::Records {
                        records: rows.clone(),
                        identities: ids.clone(),
                        version: 7,
                    })
                }
                DatasetRequest::ReadSequential { start: Some(2), .. } => {
                    Err(HostProblem::Condition {
                        name: "NOTFND".into(),
                        response: 13,
                        response2: 0,
                    })
                }
                _ => panic!("unexpected format-control request"),
            })
            .unwrap();
            assert_eq!(captured.attributes, attributes);
            assert_eq!(captured.records, 2);
            assert_eq!(captured.framed_sha256, expected.framed_sha256);
            for actual_format in [
                RecordFormat::Fixed,
                RecordFormat::FixedBlocked,
                RecordFormat::FixedBlockedStandard,
                RecordFormat::Variable,
                RecordFormat::VariableBlocked,
                RecordFormat::VariableSpanned,
                RecordFormat::VariableBlockedSpanned,
                RecordFormat::Undefined,
                RecordFormat::Line,
            ] {
                if actual_format == record_format {
                    continue;
                }
                let mut wrong = attributes.clone();
                wrong.record_format = actual_format;
                assert_format_refuses_before_rows(&expected, wrong);
            }
            for ccsid in [None, Some(1047), Some(1208)] {
                let mut wrong = attributes.clone();
                wrong.ccsid = ccsid;
                assert_format_refuses_before_rows(&expected, wrong);
            }
        }
    }

    fn assert_format_refuses_before_rows(
        expected: &DatasetExpected,
        attributes: DatasetAttributes,
    ) {
        let captured = capture_dataset(expected, |request| match request {
            DatasetRequest::Attributes { .. } => Ok(DatasetResult::Attributes {
                attributes: attributes.clone(),
                version: 7,
            }),
            _ => panic!("wrong exact format or CCSID must refuse before rows"),
        });
        assert!(captured.is_err());
    }

    #[test]
    fn keyed_identities_require_the_exact_embedded_primary_key() {
        let expected = DatasetExpected {
            key: Some((0, 1)),
            ..sample().0
        };
        let (_, rows, _) = sample();
        let ids = vec![vec![1], vec![3]];
        assert!(require_page(&expected, 0, 2, 7, &rows, &ids, 7).is_ok());
        let wrong = vec![vec![1], vec![4]];
        assert!(require_page(&expected, 0, 2, 7, &rows, &wrong, 7).is_err());
    }

    #[test]
    fn whole_record_order_and_padding_are_independent_of_count() {
        let (_, mut rows, _) = sample();
        assert!(capture_sample(rows.clone(), false, false).is_ok());
        rows.reverse();
        assert!(capture_sample(rows.clone(), false, false).is_err());
        rows.reverse();
        rows[1][1] ^= 1;
        assert!(capture_sample(rows, false, false).is_err());
    }

    #[test]
    fn complete_html_sized_two_page_capture_keeps4096_requests_and_exact_end() {
        let expected = DatasetExpected {
            records: 6632,
            // Frozen independent Python framing of each ordinal encoded as u16.
            framed_sha256: "2ce02d624a7814ebdc88a53f3a11bb7958bbc77d402548eed75aa89d37668bcf",
            ..sample().0
        };
        let mut pages = Vec::new();
        let mut attributes = 0;
        let captured = capture_dataset(&expected, |request| match request {
            DatasetRequest::Attributes { .. } => {
                attributes += 1;
                Ok(DatasetResult::Attributes {
                    attributes: layout(),
                    version: 7,
                })
            }
            DatasetRequest::ReadSequential {
                start: Some(start),
                max_records,
                reverse: false,
                ..
            } => {
                assert_eq!(max_records, PAGE);
                pages.push(start);
                if start == 6632 {
                    return Err(HostProblem::Condition {
                        name: "NOTFND".into(),
                        response: 13,
                        response2: 0,
                    });
                }
                let end = (start + u64::from(PAGE)).min(6632);
                Ok(DatasetResult::Records {
                    records: (start..end)
                        .map(|i| (i as u16).to_be_bytes().to_vec())
                        .collect(),
                    identities: (start..end).map(|i| i.to_be_bytes().to_vec()).collect(),
                    version: 7,
                })
            }
            _ => Err(HostProblem::Unsupported),
        })
        .expect("synthetic page/control proof only; zero application credit");
        assert_eq!(captured.records, 6632);
        assert_eq!(pages, [0, 4096, 6632]);
        assert_eq!(attributes, 2);
    }

    #[test]
    fn spool_framing_padding_count_and_more_are_refused() {
        let expected = SpoolExpected {
            job: "test-only",
            file: "TEST:SYSOUT",
            records: 2,
            bytes: 4,
            framed_sha256: sample().0.framed_sha256,
        };
        let (_, mut rows, _) = sample();
        assert!(require_spool(&expected, &rows, false).is_ok());
        assert!(require_spool(&expected, &rows, true).is_err());
        assert!(require_spool(&expected, &rows[..1], false).is_err());
        rows[0][1] ^= 1;
        assert!(require_spool(&expected, &rows, false).is_err());
        assert!(require_spool(&expected, &[vec![1], vec![2, 3, 4]], false).is_err());
    }
}
