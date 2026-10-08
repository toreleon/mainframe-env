//! Bounded transport of the shared runner's authoritative canonical records.

use crate::TaskResult;
use mainframe_env_coverage::{
    CONFORMANCE_LEDGER_CONTRACT, CONFORMANCE_RUNNER_VERSION_V1, CONFORMANCE_SPEC_VERSION_V1,
    CONFORMANCE_VERDICT_CONTRACT, ConformanceLimits, ConformanceRunReport, CoverageGate,
    DerivedConformanceLedger, VerdictEvent,
};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const STAGING_BYTES: usize = 64 * 1024 * 1024;
const RECORD_BYTES: usize = 512 * 1024 * 1024;
const TEMP_ATTEMPTS: usize = 32;
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy)]
struct OutputLimits {
    shape: ConformanceLimits,
    staging_bytes: usize,
    record_bytes: usize,
}

impl Default for OutputLimits {
    fn default() -> Self {
        Self {
            shape: ConformanceLimits::default(),
            staging_bytes: STAGING_BYTES,
            record_bytes: RECORD_BYTES,
        }
    }
}

pub(super) fn validate_selection(
    output: Option<&Path>,
    selected: bool,
    prepare_candidates: bool,
) -> TaskResult {
    if output.is_some() && !selected {
        return Err("--output requires focused --subsystem or --replay".into());
    }
    if output.is_some() && prepare_candidates {
        return Err("--output cannot be combined with candidate preparation".into());
    }
    Ok(())
}

pub(super) fn diagnostic(message: fmt::Arguments<'_>) -> TaskResult {
    let mut stderr = io::stderr().lock();
    stderr
        .write_fmt(message)
        .and_then(|()| stderr.write_all(b"\n"))
        .and_then(|()| stderr.flush())
        .map_err(|error| format!("focused conformance diagnostic: {error}"))
}

pub(super) fn emit_report(report: &ConformanceRunReport, output: Option<&Path>) -> TaskResult {
    let staged = encode_report(report, OutputLimits::default())?;
    if let Some(path) = output {
        publish_file(path, &staged)
    } else {
        write_stream(&mut io::stdout().lock(), &staged)
    }
}

fn bounded_count(value: usize, maximum: usize, name: &str) -> TaskResult {
    if value > maximum {
        return Err(format!("conformance output {name} exceeds {maximum}"));
    }
    Ok(())
}

fn checked_add(left: usize, right: usize) -> TaskResult<usize> {
    left.checked_add(right)
        .ok_or_else(|| "conformance output size overflow".into())
}

struct Projection {
    bytes: usize,
    maximum: usize,
}

impl Projection {
    fn new(maximum: usize) -> Self {
        Self { bytes: 0, maximum }
    }

    fn add(&mut self, bytes: usize) -> TaskResult {
        self.bytes = checked_add(self.bytes, bytes)?;
        bounded_count(self.bytes, self.maximum, "record projection")
    }

    fn text(&mut self, text: &str, maximum: usize) -> TaskResult {
        bounded_count(text.len(), maximum, "field bytes")?;
        // Six bytes per UTF-8 byte bounds JSON escaping, including public DTO mutations.
        let escaped = text
            .len()
            .checked_mul(6)
            .ok_or("conformance output size overflow")?;
        self.add(escaped)
    }
}

fn preflight_event(event: &VerdictEvent, limits: OutputLimits) -> TaskResult {
    if event.schema_version != CONFORMANCE_VERDICT_CONTRACT
        || event.spec_version != CONFORMANCE_SPEC_VERSION_V1
        || event.runner_version != CONFORMANCE_RUNNER_VERSION_V1
    {
        return Err("conformance output has unsupported verdict shape/version".into());
    }
    let shape = limits.shape;
    let mut wire = Projection::new(limits.record_bytes);
    // Covers the 21 fixed keys, delimiters, quotes, enum strings, null and LF.
    wire.add(2048)?;
    for text in [
        event.schema_version,
        &event.spec_version,
        event.runner_version,
        &event.candidate_digest,
        &event.catalog_digest,
        &event.spec_digest,
        &event.environment_manifest_digest,
        event.key.row_id.as_str(),
        event.key.obligation_id.as_str(),
        event.test_id.as_str(),
        &event.observation_digest,
        &event.cache_identity,
        event.driver.as_str(),
        event.fixture_or_seed.as_str(),
    ] {
        wire.text(text, shape.max_identity_bytes)?;
    }
    if let Some(receipt) = &event.oracle_receipt_digest {
        wire.text(receipt, shape.max_identity_bytes)?;
    }
    wire.text(
        &event.replay,
        checked_add(
            shape.max_identity_bytes,
            "cargo xtask conformance --replay ".len(),
        )?,
    )?;
    wire.text(&event.source_locator, shape.max_locator_bytes)?;
    wire.text(&event.expected, shape.max_observation_bytes)?;
    wire.text(&event.actual, shape.max_observation_bytes)
}

fn preflight_ledger(ledger: &DerivedConformanceLedger, limits: OutputLimits) -> TaskResult {
    if ledger.schema_version != CONFORMANCE_LEDGER_CONTRACT
        || ledger.spec_version != CONFORMANCE_SPEC_VERSION_V1
    {
        return Err("conformance output has unsupported ledger shape/version".into());
    }
    let shape = limits.shape;
    bounded_count(ledger.rows.len(), shape.max_catalog_rows, "ledger rows")?;
    let mut wire = Projection::new(limits.record_bytes);
    // Root/counts overhead includes all six gates, usize decimal fields and LF.
    wire.add(4096)?;
    for text in [
        ledger.schema_version,
        &ledger.spec_version,
        &ledger.catalog_digest,
        &ledger.spec_digest,
    ] {
        wire.text(text, shape.max_identity_bytes)?;
    }
    for gate in CoverageGate::ALL {
        let count = ledger
            .counts
            .get(&gate)
            .ok_or_else(|| format!("conformance output missing ledger count {}", gate.slug()))?;
        for value in [count.pass, count.fail, count.pending, count.non_applicable] {
            bounded_count(value, shape.max_catalog_rows, "ledger count")?;
        }
    }
    let mut bindings = 0;
    for row in ledger.rows.values() {
        wire.add(512)?;
        for text in [row.row_id.as_str(), &row.subsystem, &row.family] {
            wire.text(text, shape.max_identity_bytes)?;
        }
        wire.text(&row.source_locator, shape.max_locator_bytes)?;
        for gate in CoverageGate::ALL {
            let result = row.gates.get(&gate).ok_or_else(|| {
                format!(
                    "conformance output missing row gate {} {}",
                    row.row_id,
                    gate.slug()
                )
            })?;
            bounded_count(
                result.mandatory_obligations,
                shape.max_obligations,
                "obligations",
            )?;
            bounded_count(
                result.passed_obligations,
                shape.max_obligations,
                "passed obligations",
            )?;
            bindings = checked_add(bindings, result.bindings.len())?;
            bounded_count(bindings, shape.max_bindings, "ledger bindings")?;
            // Each gate's fixed keys, state/enum strings, two usize numbers and delimiters.
            wire.add(512)?;
            for binding in &result.bindings {
                wire.add(256)?;
                for text in [
                    binding.obligation_id.as_str(),
                    binding.test_id.as_str(),
                    &binding.observation_digest,
                ] {
                    wire.text(text, shape.max_identity_bytes)?;
                }
            }
        }
    }
    Ok(())
}

fn preflight(report: &ConformanceRunReport, limits: OutputLimits) -> TaskResult {
    bounded_count(report.batches.len(), limits.shape.max_bindings, "batches")?;
    let mut events = 0;
    for batch in &report.batches {
        events = checked_add(events, batch.events.len())?;
        bounded_count(events, limits.shape.max_bindings, "events")?;
        for event in &batch.events {
            preflight_event(event, limits)?;
        }
    }
    if events == 0 {
        return Err("conformance output refuses an empty report; no executable verdicts".into());
    }
    preflight_ledger(&report.ledger, limits)
}

fn encode_report(report: &ConformanceRunReport, limits: OutputLimits) -> TaskResult<Vec<u8>> {
    encode_with(
        report,
        limits,
        |event| event.canonical_json().map_err(|error| error.to_string()),
        |ledger| ledger.canonical_json().map_err(|error| error.to_string()),
    )
}

fn encode_with(
    report: &ConformanceRunReport,
    limits: OutputLimits,
    mut event_encoder: impl FnMut(&VerdictEvent) -> TaskResult<Vec<u8>>,
    mut ledger_encoder: impl FnMut(&DerivedConformanceLedger) -> TaskResult<Vec<u8>>,
) -> TaskResult<Vec<u8>> {
    preflight(report, limits)?;
    let mut staged = Vec::new();
    for event in report.batches.iter().flat_map(|batch| &batch.events) {
        append_record(&mut staged, &event_encoder(event)?, limits.staging_bytes)?;
    }
    append_record(
        &mut staged,
        &ledger_encoder(&report.ledger)?,
        limits.staging_bytes,
    )?;
    Ok(staged)
}

fn append_record(staged: &mut Vec<u8>, record: &[u8], maximum: usize) -> TaskResult {
    let additional = checked_add(record.len(), 1)?;
    let total = checked_add(staged.len(), additional)?;
    bounded_count(total, maximum, "staged JSONL bytes")?;
    staged
        .try_reserve_exact(additional)
        .map_err(|error| format!("conformance output staging allocation refused: {error}"))?;
    staged.extend_from_slice(record);
    staged.push(b'\n');
    Ok(())
}

fn write_stream(writer: &mut impl Write, staged: &[u8]) -> TaskResult {
    writer
        .write_all(staged)
        .and_then(|()| writer.flush())
        .map_err(|error| format!("conformance output write/flush failed: {error}"))
}

// Private filesystem seam exercises publication failures without product hooks or dependencies.
trait Publication {
    type Sink: Write;
    fn create_new(&self, path: &Path) -> io::Result<Self::Sink>;
    fn sync(&self, sink: &Self::Sink) -> io::Result<()>;
    fn rename(&self, source: &Path, destination: &Path) -> io::Result<()>;
    fn remove(&self, path: &Path) -> io::Result<()>;
}

struct Filesystem;

impl Publication for Filesystem {
    type Sink = File;

    fn create_new(&self, path: &Path) -> io::Result<File> {
        OpenOptions::new().write(true).create_new(true).open(path)
    }

    fn sync(&self, sink: &File) -> io::Result<()> {
        sink.sync_all()
    }

    fn rename(&self, source: &Path, destination: &Path) -> io::Result<()> {
        fs::rename(source, destination)
    }

    fn remove(&self, path: &Path) -> io::Result<()> {
        fs::remove_file(path)
    }
}

fn regular_destination(path: &Path) -> TaskResult {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_file() => Ok(()),
        Ok(_) => Err(format!(
            "conformance output destination is not a regular file: {}",
            path.display()
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!(
            "conformance output destination {}: {error}",
            path.display()
        )),
    }
}

fn publish_file(path: &Path, staged: &[u8]) -> TaskResult {
    publish_with(path, staged, &Filesystem)
}

fn temporary_path(path: &Path) -> TaskResult<PathBuf> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    if path.file_name().is_none() {
        return Err("conformance output requires a filename".into());
    }
    // Fixed-length name also accommodates maximum-length destination filenames.
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    Ok(parent.join(format!(
        ".xtask-conformance-{}-{sequence}.tmp",
        std::process::id()
    )))
}

fn publish_with(path: &Path, staged: &[u8], operations: &impl Publication) -> TaskResult {
    regular_destination(path)?;
    for _ in 0..TEMP_ATTEMPTS {
        let temporary = temporary_path(path)?;
        let mut sink = match operations.create_new(&temporary) {
            Ok(sink) => sink,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(format!(
                    "conformance output create {}: {error}",
                    temporary.display()
                ));
            }
        };
        let result = write_stream(&mut sink, staged).and_then(|()| {
            operations
                .sync(&sink)
                .map_err(|error| format!("conformance output sync: {error}"))
        });
        // Rust's safe File API closes on drop; sync above is the fallible durability operation.
        drop(sink);
        let result = result
            .and_then(|()| regular_destination(path))
            .and_then(|()| {
                operations.rename(&temporary, path).map_err(|error| {
                    format!("conformance output rename to {}: {error}", path.display())
                })
            });
        if let Err(original) = result {
            return match operations.remove(&temporary) {
                Ok(()) => Err(original),
                Err(cleanup) => Err(format!(
                    "{original}; cleanup failed for {}: {cleanup}",
                    temporary.display()
                )),
            };
        }
        return Ok(());
    }
    Err(format!(
        "conformance output exhausted {TEMP_ATTEMPTS} create-new sibling attempts"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ConformanceDriver, ConformanceObservation, ConformancePredicate, ConformanceRunner,
        DriverOutput, FixtureRef, GateState, ObservationCheck, RunnerContext, RunnerSelection,
        RuntimeRegistry, Verdict, compile_shared_spec,
    };
    use std::cell::{Cell, RefCell};
    use std::sync::OnceLock;

    const REPLAY: &str = "cobol.frontend.basis.valid";
    const ROW: &str = "ibm-enterprise-cobol-6.5-2026-05-31:compiler-directing-statements:0001";

    fn root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .to_path_buf()
    }

    fn real_report() -> ConformanceRunReport {
        static REPORT: OnceLock<ConformanceRunReport> = OnceLock::new();
        REPORT
            .get_or_init(|| {
                let limits = ConformanceLimits::default();
                let spec = compile_shared_spec(&root()).unwrap();
                let dataset = crate::dataset_conformance_runtime();
                let jcl = crate::jcl_conformance::runtime();
                let cics = crate::cics_pilot_runtime();
                let movement = crate::cobol_move_pilot_runtime();
                let arithmetic = crate::cobol_arithmetic_pilot_runtime();
                let runtime = crate::combined_conformance_runtime(
                    &spec,
                    &dataset,
                    &jcl,
                    &cics,
                    &movement,
                    &arithmetic,
                    limits,
                )
                .unwrap();
                let context =
                    RunnerContext::new(crate::candidate_digest(&root()).unwrap(), "local", limits)
                        .unwrap();
                let selection = RunnerSelection::replay(REPLAY, limits).unwrap();
                ConformanceRunner::new(&spec, runtime, limits)
                    .run(&selection, &context)
                    .unwrap()
            })
            .clone()
    }

    fn canonical_bytes(report: &ConformanceRunReport) -> Vec<u8> {
        let mut expected = Vec::new();
        for batch in &report.batches {
            for event in &batch.events {
                expected.extend(event.canonical_json().unwrap());
                expected.push(b'\n');
            }
        }
        expected.extend(report.ledger.canonical_json().unwrap());
        expected.push(b'\n');
        expected
    }

    struct Directory(PathBuf);

    impl Directory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "cv209-report-{}-{}",
                std::process::id(),
                TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed),
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn sentinel(&self) -> PathBuf {
            let path = self.0.join("report.jsonl");
            fs::write(&path, b"previous report\n").unwrap();
            path
        }
    }

    impl Drop for Directory {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn canonical_records_keep_original_bytes() {
        let report = real_report();
        assert_eq!(report.batches.len(), 1);
        let event = &report.batches[0].events[0];
        assert_eq!(report.batches[0].events.len(), 1);
        assert_eq!(event.key.row_id.as_str(), ROW);
        assert_eq!(event.key.obligation_id.as_str(), "valid-forms");
        assert_eq!(event.key.gate, CoverageGate::Recognized);
        assert_eq!(event.test_id.as_str(), REPLAY);
        assert_eq!(event.verdict, Verdict::Pass);
        assert_eq!(report.ledger.rows.len(), 1506);
        let staged = encode_report(&report, OutputLimits::default()).unwrap();
        assert_eq!(staged, canonical_bytes(&report));
        let records: Vec<_> = staged.split(|byte| *byte == b'\n').collect();
        assert_eq!(records.len(), 3);
        assert_eq!(records[1], report.ledger.canonical_json().unwrap());
        for count in report.ledger.counts.values() {
            assert_eq!(
                count.pass + count.fail + count.pending + count.non_applicable,
                1506
            );
        }
    }

    struct IndependentDriver;
    impl ConformanceDriver for IndependentDriver {
        fn execute(&self, _: &FixtureRef) -> Result<DriverOutput, String> {
            DriverOutput::new(
                b"different-observed-byte".to_vec(),
                ConformanceLimits::default(),
            )
            .map_err(|error| error.to_string())
        }
    }
    struct Ready;
    impl ConformancePredicate for Ready {
        fn evaluate(&self, _: &FixtureRef) -> Result<bool, String> {
            Ok(true)
        }
    }
    struct Exact;
    impl ConformanceObservation for Exact {
        fn evaluate(&self, output: &DriverOutput) -> Result<ObservationCheck, String> {
            ObservationCheck::new(
                output.bytes() == b"independent-required-byte",
                "independent-required-byte",
                String::from_utf8(output.bytes().to_vec()).unwrap(),
                ConformanceLimits::default(),
            )
            .map_err(|error| error.to_string())
        }
    }

    #[test]
    fn failed_verdict_and_ledger_survive_nonzero_completion() {
        // A test-only driver mismatch runs through the actual runner, not a mutated pass event.
        let limits = ConformanceLimits::default();
        let spec = compile_shared_spec(&root()).unwrap();
        let driver = IndependentDriver;
        let predicate = Ready;
        let observation = Exact;
        let runtime = RuntimeRegistry::new(
            &spec,
            spec.registries()
                .drivers()
                .iter()
                .map(|id| (id.clone(), &driver as &dyn ConformanceDriver))
                .collect(),
            spec.registries()
                .predicates()
                .iter()
                .map(|id| (id.clone(), &predicate as &dyn ConformancePredicate))
                .collect(),
            spec.registries()
                .observations()
                .iter()
                .map(|id| (id.clone(), &observation as &dyn ConformanceObservation))
                .collect(),
            limits,
        )
        .unwrap();
        let context =
            RunnerContext::new(crate::candidate_digest(&root()).unwrap(), "local", limits).unwrap();
        let report = ConformanceRunner::new(&spec, runtime, limits)
            .run(&RunnerSelection::replay(REPLAY, limits).unwrap(), &context)
            .unwrap();
        let event = &report.batches[0].events[0];
        assert_eq!(event.verdict, Verdict::Fail);
        assert_eq!(event.key.row_id.as_str(), ROW);
        assert_eq!(
            report.ledger.rows[&event.key.row_id].gates[&CoverageGate::Recognized].state,
            GateState::Failed
        );
        assert!(event.expected.contains("independent-required-byte"));
        assert!(event.actual.contains("different-observed-byte"));
        let directory = Directory::new();
        let destination = directory.sentinel();
        let completion = crate::finish_focused_report(
            &report,
            Some(&destination),
            spec.spec_digest(),
            "conformance-ledger",
            "focused conformance produced failing verdicts",
        );
        assert!(
            completion
                .unwrap_err()
                .contains("produced failing verdicts")
        );
        let emitted = fs::read(destination).unwrap();
        assert_eq!(emitted, canonical_bytes(&report));
        if let Some(receipts) = std::env::var_os("CV209_CLI_RECEIPTS") {
            fs::create_dir_all(&receipts).unwrap();
            fs::write(
                PathBuf::from(receipts).join("independent-runner-fail.jsonl"),
                emitted,
            )
            .unwrap();
        }
    }

    #[test]
    fn runner_refusal_never_creates_output() {
        let directory = Directory::new();
        let destination = directory.sentinel();
        let args = crate::ConformanceArgs {
            subsystem: None,
            gate: None,
            shard: None,
            replay: Some("cobol.output-boundary-does-not-exist".into()),
            output: Some(destination.clone()),
            prepare_candidates: false,
            check: true,
        };
        let error = crate::check_focused_conformance_interface(&root(), &args).unwrap_err();
        assert!(error.contains("selection has no executable bindings"));
        assert_eq!(fs::read(destination).unwrap(), b"previous report\n");
    }

    #[test]
    fn required_count_and_row_gate_maps_refuse_before_encoding() {
        for gate in CoverageGate::ALL {
            for missing_count in [true, false] {
                let mut report = real_report();
                if missing_count {
                    report.ledger.counts.remove(&gate);
                } else {
                    report
                        .ledger
                        .rows
                        .values_mut()
                        .next()
                        .unwrap()
                        .gates
                        .remove(&gate);
                }
                let called = Cell::new(false);
                let result = encode_with(
                    &report,
                    OutputLimits::default(),
                    |_| {
                        called.set(true);
                        Err("encoder must not run".into())
                    },
                    |_| {
                        called.set(true);
                        Err("encoder must not run".into())
                    },
                );
                assert!(result.unwrap_err().contains("missing"));
                assert!(!called.get());
                let directory = Directory::new();
                let destination = directory.sentinel();
                assert!(emit_report(&report, Some(&destination)).is_err());
                assert_eq!(fs::read(destination).unwrap(), b"previous report\n");
            }
        }
    }

    #[test]
    fn empty_report_refuses_without_sinks() {
        let mut report = real_report();
        report.batches.clear();
        let directory = Directory::new();
        let destination = directory.0.join("absent.jsonl");
        assert!(
            emit_report(&report, Some(&destination))
                .unwrap_err()
                .contains("empty report")
        );
        assert!(!destination.exists());
        assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 0);
    }

    #[test]
    fn unsupported_versions_and_oversized_fields_refuse_without_sinks() {
        for mutation in 0..5 {
            let mut report = real_report();
            match mutation {
                0 => report.batches[0].events[0].schema_version = "unsupported-verdict@2",
                1 => report.batches[0].events[0].runner_version = "unsupported-runner@2",
                2 => report.ledger.schema_version = "unsupported-ledger@2",
                3 => {
                    report.batches[0].events[0].actual =
                        "x".repeat(ConformanceLimits::default().max_observation_bytes + 1)
                }
                _ => {
                    report
                        .ledger
                        .rows
                        .values_mut()
                        .next()
                        .unwrap()
                        .source_locator =
                        "x".repeat(ConformanceLimits::default().max_locator_bytes + 1)
                }
            }
            let directory = Directory::new();
            let destination = directory.sentinel();
            assert!(emit_report(&report, Some(&destination)).is_err());
            assert_eq!(fs::read(destination).unwrap(), b"previous report\n");
        }
    }

    #[test]
    fn typed_malformed_verdict_is_refused_by_its_owner() {
        let event = real_report().batches.remove(0).events.remove(0);
        for missing_oracle in [false, true] {
            let mut key = event.key.clone();
            if missing_oracle {
                key.gate = CoverageGate::Differential;
            }
            let result = VerdictEvent::new(
                event.spec_version.clone(),
                if missing_oracle {
                    event.candidate_digest.clone()
                } else {
                    "malformed-digest".into()
                },
                event.catalog_digest.clone(),
                event.spec_digest.clone(),
                event.environment_manifest_digest.clone(),
                key,
                event.test_id.clone(),
                Verdict::Pass,
                event.observation_digest.clone(),
                event.cache_identity.clone(),
                event.source_locator.clone(),
                event.driver.clone(),
                event.fixture_or_seed.clone(),
                event.expected.clone(),
                event.actual.clone(),
                None,
                ConformanceLimits::default(),
            );
            assert!(result.is_err());
        }
    }

    #[test]
    fn later_encoding_failure_never_reaches_any_sink() {
        let report = real_report();
        let directory = Directory::new();
        let destination = directory.sentinel();
        let calls = Cell::new(0);
        let mut stdout = Vec::new();
        let encoded = encode_with(
            &report,
            OutputLimits::default(),
            |event| {
                calls.set(calls.get() + 1);
                event.canonical_json().map_err(|error| error.to_string())
            },
            |_| {
                calls.set(calls.get() + 1);
                Err("independent terminal encoder refusal".into())
            },
        );
        let result = encoded.and_then(|bytes| {
            write_stream(&mut stdout, &bytes)?;
            publish_file(&destination, &bytes)
        });
        assert!(result.unwrap_err().contains("terminal encoder refusal"));
        assert_eq!(calls.get(), 2);
        assert!(stdout.is_empty());
        assert_eq!(fs::read(destination).unwrap(), b"previous report\n");
        assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 1);
    }

    #[test]
    fn exact_staging_boundary_includes_every_lf() {
        let report = real_report();
        let expected = canonical_bytes(&report);
        let mut limits = OutputLimits {
            staging_bytes: expected.len(),
            ..OutputLimits::default()
        };
        assert_eq!(encode_report(&report, limits).unwrap(), expected);
        limits.staging_bytes -= 1;
        assert!(
            encode_report(&report, limits)
                .unwrap_err()
                .contains("staged JSONL bytes")
        );
        let mut tiny = Vec::new();
        append_record(&mut tiny, b"abc", 4).unwrap();
        assert_eq!(tiny, b"abc\n");
        assert!(append_record(&mut tiny, b"", 4).is_err());
        assert_eq!(tiny, b"abc\n");
        assert!(checked_add(usize::MAX, 1).is_err());
    }

    #[test]
    fn record_projection_and_cardinalities_refuse_before_canonical_allocation() {
        let report = real_report();
        let mut limits = OutputLimits {
            record_bytes: 1,
            ..OutputLimits::default()
        };
        let calls = Cell::new(0);
        assert!(
            encode_with(
                &report,
                limits,
                |_| {
                    calls.set(1);
                    Ok(vec![])
                },
                |_| {
                    calls.set(1);
                    Ok(vec![])
                }
            )
            .is_err()
        );
        assert_eq!(calls.get(), 0);
        let mut projection = Projection::new(12);
        projection.text("é", 2).unwrap();
        assert_eq!(projection.bytes, 12);
        assert!(projection.add(1).is_err());
        let mut overflow = Projection {
            bytes: usize::MAX,
            maximum: usize::MAX,
        };
        assert!(overflow.add(1).is_err());
        limits = OutputLimits::default();
        limits.shape.max_catalog_rows = 1505;
        assert!(encode_report(&report, limits).is_err());
        limits = OutputLimits::default();
        limits.shape.max_bindings = 0;
        assert!(encode_report(&report, limits).is_err());
        limits = OutputLimits::default();
        limits.shape.max_bindings = 1;
        let mut repeated = report.clone();
        repeated.batches[0]
            .events
            .push(report.batches[0].events[0].clone());
        assert!(
            encode_report(&repeated, limits)
                .unwrap_err()
                .contains("events")
        );
        let mut ledger_bindings = report.clone();
        let row = ledger_bindings
            .ledger
            .rows
            .values_mut()
            .find(|row| row.gates.values().any(|gate| !gate.bindings.is_empty()))
            .unwrap();
        let gate = row
            .gates
            .values_mut()
            .find(|gate| !gate.bindings.is_empty())
            .unwrap();
        gate.bindings.push(gate.bindings[0].clone());
        assert!(
            encode_report(&ledger_bindings, limits)
                .unwrap_err()
                .contains("ledger bindings")
        );
        limits = OutputLimits::default();
        limits.shape.max_obligations = 0;
        assert!(
            encode_report(&report, limits)
                .unwrap_err()
                .contains("obligations")
        );
    }

    #[test]
    fn approved_record_projection_cap_has_exact_and_over_boundaries() {
        assert_eq!(OutputLimits::default().staging_bytes, 64 * 1024 * 1024);
        assert_eq!(OutputLimits::default().record_bytes, 512 * 1024 * 1024);
        let mut wire = Projection::new(RECORD_BYTES);
        wire.add(RECORD_BYTES).unwrap();
        assert_eq!(wire.bytes, RECORD_BYTES);
        assert!(wire.add(1).unwrap_err().contains("record projection"));
    }

    struct BrokenWriter {
        remaining: usize,
        flush_failure: bool,
        bytes: Vec<u8>,
    }
    impl Write for BrokenWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.remaining == 0 {
                return Err(io::Error::other("independent write failure"));
            }
            let count = bytes.len().min(self.remaining);
            self.bytes.extend_from_slice(&bytes[..count]);
            self.remaining -= count;
            Ok(count)
        }
        fn flush(&mut self) -> io::Result<()> {
            if self.flush_failure {
                Err(io::Error::other("independent flush failure"))
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn stdout_write_partial_write_and_flush_errors_are_nonzero_results() {
        for (remaining, flush_failure, prefix) in [(0, false, 0), (3, false, 3), (100, true, 6)] {
            let mut writer = BrokenWriter {
                remaining,
                flush_failure,
                bytes: Vec::new(),
            };
            assert!(write_stream(&mut writer, b"record").is_err());
            assert_eq!(writer.bytes, b"record"[..prefix]);
        }
    }

    #[derive(Clone, Copy, PartialEq)]
    enum Fault {
        Write,
        Flush,
        Sync,
        Rename,
        Cleanup,
        Collision,
        Create,
    }
    struct FaultSink {
        file: File,
        fault: Fault,
    }
    impl Write for FaultSink {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.fault == Fault::Write {
                Err(io::Error::other("independent write refusal"))
            } else {
                self.file.write(bytes)
            }
        }
        fn flush(&mut self) -> io::Result<()> {
            if self.fault == Fault::Flush {
                Err(io::Error::other("independent flush refusal"))
            } else {
                self.file.flush()
            }
        }
    }
    struct FaultPublication {
        fault: Fault,
        attempts: Cell<usize>,
        owned: RefCell<Vec<PathBuf>>,
        removals: Cell<usize>,
    }
    impl FaultPublication {
        fn new(fault: Fault) -> Self {
            Self {
                fault,
                attempts: Cell::new(0),
                owned: RefCell::new(Vec::new()),
                removals: Cell::new(0),
            }
        }
    }
    impl Publication for FaultPublication {
        type Sink = FaultSink;
        fn create_new(&self, path: &Path) -> io::Result<FaultSink> {
            self.attempts.set(self.attempts.get() + 1);
            if self.fault == Fault::Collision {
                return Err(io::Error::new(io::ErrorKind::AlreadyExists, "collision"));
            }
            if self.fault == Fault::Create {
                return Err(io::Error::other("independent create refusal"));
            }
            let file = Filesystem.create_new(path)?;
            self.owned.borrow_mut().push(path.to_path_buf());
            Ok(FaultSink {
                file,
                fault: self.fault,
            })
        }
        fn sync(&self, sink: &FaultSink) -> io::Result<()> {
            if self.fault == Fault::Sync {
                Err(io::Error::other("independent sync refusal"))
            } else {
                sink.file.sync_all()
            }
        }
        fn rename(&self, _: &Path, _: &Path) -> io::Result<()> {
            Err(io::Error::other("independent rename refusal"))
        }
        fn remove(&self, path: &Path) -> io::Result<()> {
            assert!(self.owned.borrow().iter().any(|owned| owned == path));
            self.removals.set(self.removals.get() + 1);
            if self.fault == Fault::Cleanup {
                Err(io::Error::other("independent cleanup refusal"))
            } else {
                fs::remove_file(path)
            }
        }
    }

    #[test]
    fn file_write_flush_sync_and_rename_failures_preserve_old_file_and_clean_only_owned_sibling() {
        for fault in [Fault::Write, Fault::Flush, Fault::Sync, Fault::Rename] {
            let directory = Directory::new();
            let path = directory.sentinel();
            let unrelated = directory.0.join("unrelated.tmp");
            fs::write(&unrelated, b"keep").unwrap();
            let operations = FaultPublication::new(fault);
            assert!(publish_with(&path, b"new report\n", &operations).is_err());
            assert_eq!(operations.attempts.get(), 1);
            assert_eq!(operations.removals.get(), 1);
            assert_eq!(fs::read(path).unwrap(), b"previous report\n");
            assert_eq!(fs::read(unrelated).unwrap(), b"keep");
            assert!(operations.owned.borrow().iter().all(|path| !path.exists()));
        }
    }

    #[test]
    fn cleanup_failure_reports_both_errors_and_exact_owned_path() {
        let directory = Directory::new();
        let destination = directory.sentinel();
        let operations = FaultPublication::new(Fault::Cleanup);
        let error = publish_with(&destination, b"new report\n", &operations).unwrap_err();
        assert!(error.contains("independent rename refusal"));
        assert!(error.contains("independent cleanup refusal"));
        let owned = operations.owned.borrow();
        assert_eq!(owned.len(), 1);
        assert!(error.contains(&owned[0].display().to_string()));
        assert!(owned[0].is_file());
        assert_eq!(fs::read(destination).unwrap(), b"previous report\n");
    }

    #[test]
    fn create_new_collisions_are_bounded_and_never_clean_unowned_paths() {
        for fault in [Fault::Collision, Fault::Create] {
            let directory = Directory::new();
            let destination = directory.sentinel();
            let operations = FaultPublication::new(fault);
            assert!(publish_with(&destination, b"new report\n", &operations).is_err());
            assert_eq!(
                operations.attempts.get(),
                if fault == Fault::Collision { 32 } else { 1 }
            );
            assert_eq!(operations.removals.get(), 0);
            assert_eq!(fs::read(destination).unwrap(), b"previous report\n");
        }
    }

    #[test]
    fn regular_file_replacement_uses_exact_bytes_and_leaves_no_sibling() {
        let directory = Directory::new();
        let destination = directory.sentinel();
        let report = real_report();
        emit_report(&report, Some(&destination)).unwrap();
        assert_eq!(fs::read(&destination).unwrap(), canonical_bytes(&report));
        assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 1);
        let absent = directory.0.join("new.jsonl");
        emit_report(&report, Some(&absent)).unwrap();
        assert_eq!(fs::read(absent).unwrap(), canonical_bytes(&report));
    }

    #[test]
    fn literal_dash_is_an_ordinary_filename() {
        let directory = Directory::new();
        let destination = directory.0.join("-");
        publish_file(&destination, b"literal filename\n").unwrap();
        assert_eq!(fs::read(destination).unwrap(), b"literal filename\n");
    }

    #[test]
    fn directories_missing_parents_and_symlinks_are_refused() {
        let directory = Directory::new();
        assert!(publish_file(&directory.0, b"report").is_err());
        let missing = directory.0.join("missing/report.jsonl");
        assert!(publish_file(&missing, b"report").is_err());
        assert!(!missing.parent().unwrap().exists());
        #[cfg(unix)]
        {
            let destination = directory.sentinel();
            let link = directory.0.join("link.jsonl");
            std::os::unix::fs::symlink(&destination, &link).unwrap();
            assert!(publish_file(&link, b"report").is_err());
            assert_eq!(fs::read(destination).unwrap(), b"previous report\n");
        }
    }
}
