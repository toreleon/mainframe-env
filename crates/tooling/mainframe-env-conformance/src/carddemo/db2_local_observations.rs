//! Complete local metadata/rows and bounded real TSO step/spool comparisons.
//! No journey tokens, SQL type claims, new provider behavior or licensed credit.
use super::*;
use mainframe_env_batch::{JobSnapshot, StepState, StepTermination};

mod expected;
#[cfg(test)]
mod tests;

#[derive(Clone, Copy)]
pub(super) enum RowsPhase {
    Empty,
    Loaded,
    Maintained,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct LocalTables(Vec<(Db2TableDefinition, Vec<Vec<Vec<u8>>>)>);

fn drift(detail: &str) -> CorpusProblem {
    CorpusProblem::new("carddemo.db2.local_comparison_drift", detail)
}

fn pinned_source(corpus: &Path, path: &str, identity: &str) -> Result<Vec<u8>, CorpusProblem> {
    let bytes = read_corpus_file(corpus, &corpus.join(path))?;
    require_source_identity(&bytes, identity)?;
    Ok(bytes)
}

fn require_source_identity(bytes: &[u8], identity: &str) -> Result<(), CorpusProblem> {
    if format!("{:x}", Sha256::digest(bytes)) != identity {
        return Err(drift("local metadata or utility source identity differs"));
    }
    Ok(())
}

pub(super) fn check_inputs(corpus: &Path) -> Result<(), CorpusProblem> {
    for (path, identity) in expected::SOURCE_INPUTS {
        pinned_source(corpus, path, identity)?;
    }
    Ok(())
}

pub(super) fn require_controls(server: &ProductServer, corpus: &Path) -> Result<(), CorpusProblem> {
    for (path, identity) in expected::SOURCE_INPUTS {
        if !path.starts_with("app/app-transaction-type-db2/ctl/") {
            continue;
        }
        let bytes = pinned_source(corpus, path, identity)?;
        let member = Path::new(path)
            .file_stem()
            .and_then(|value| value.to_str())
            .ok_or_else(|| drift("pinned Db2 control member is invalid"))?;
        let records = utility_records(server, "AWS.M2.CARDDEMO.CNTL", Some(member))?;
        require_control_records(&records, &bytes)?;
    }
    Ok(())
}

fn require_control_records(records: &[Vec<u8>], source: &[u8]) -> Result<(), CorpusProblem> {
    let source =
        std::str::from_utf8(source).map_err(|_| drift("pinned Db2 control source is not UTF-8"))?;
    if !records
        .iter()
        .map(Vec::as_slice)
        .eq(source.lines().map(str::as_bytes))
    {
        return Err(drift(
            "actual UTF-8 control member differs from pinned source",
        ));
    }
    Ok(())
}

fn capture(server: &ProductServer) -> Result<LocalTables, CorpusProblem> {
    let service = server.db2_service();
    expected::TABLE_NAMES
        .iter()
        .map(|name| {
            Ok((
                service.table_definition(name).map_err(terminal_problem)?,
                service.table_rows(name).map_err(terminal_problem)?,
            ))
        })
        .collect::<Result<Vec<_>, _>>()
        .map(LocalTables)
}

fn require_tables(tables: &LocalTables, phase: RowsPhase) -> Result<(), CorpusProblem> {
    if tables != &expected::tables(phase) {
        return Err(drift(
            "complete ordered local table metadata or rows differ",
        ));
    }
    Ok(())
}

pub(super) fn require_state(server: &ProductServer, phase: RowsPhase) -> Result<(), CorpusProblem> {
    require_tables(&capture(server)?, phase)
}

#[derive(Clone, Copy)]
pub(super) enum UtilityJob {
    Create,
    Extract,
}

impl UtilityJob {
    fn name(self) -> &'static str {
        match self {
            Self::Create => "CREADB2",
            Self::Extract => "TRANEXTR",
        }
    }

    fn steps(self) -> &'static [&'static str] {
        match self {
            Self::Create => &["FREEPLN", "CRCRDDB", "LDTTYPE", "RUNTEP2", "LDTCCAT"],
            Self::Extract => &["STEP10", "STEP20", "STEP30", "STEP40", "STEP50"],
        }
    }

    fn spool(self) -> &'static [(&'static str, &'static [&'static [u8]])] {
        match self {
            Self::Create => expected::CREATE_SPOOL,
            Self::Extract => expected::EXTRACT_SPOOL,
        }
    }
}

fn require_job(id: &str, job: &JobSnapshot, selected: UtilityJob) -> Result<(), CorpusProblem> {
    if id.is_empty()
        || job.id != id
        || job.name != selected.name()
        || job.owner != "IBMUSER"
        || job.state != JobState::Completed
        || job.return_code != Some(0)
        || job.abend_code.is_some()
        || job.active_step.is_some()
        || job.cancellation.is_some()
        || job.attempt != 1
        || job.version == 0
        || job.kind != Default::default()
        || job.origin != Default::default()
        || job.steps.len() != selected.steps().len()
        || !job.steps.iter().zip(selected.steps()).all(|(step, name)| {
            step.name == *name
                && step.state == StepState::Completed
                && step.attempt == 1
                && step.termination == Some(StepTermination::ReturnCode { code: 0 })
        })
    {
        return Err(drift(
            "owned utility job or complete ordered terminal steps differ",
        ));
    }
    Ok(())
}

fn byte_count(records: &[&[u8]]) -> Result<usize, CorpusProblem> {
    records
        .iter()
        .try_fold(0_usize, |total, record| total.checked_add(record.len()))
        .ok_or_else(|| drift("selected spool byte count overflowed"))
}

fn require_descriptor(
    files: &[(usize, String, usize, usize)],
    file: &str,
    expected: &[&[u8]],
) -> Result<(), CorpusProblem> {
    let matching = files
        .iter()
        .filter(|(_, name, _, _)| name == file)
        .collect::<Vec<_>>();
    if matching.len() != 1
        || matching[0].2 != expected.len()
        || matching[0].3 != byte_count(expected)?
    {
        return Err(drift(
            "selected step spool descriptor is missing, duplicate or incomplete",
        ));
    }
    Ok(())
}

fn require_records(
    records: &[Vec<u8>],
    more: bool,
    expected: &[&[u8]],
) -> Result<(), CorpusProblem> {
    if more
        || !records
            .iter()
            .map(Vec::as_slice)
            .eq(expected.iter().copied())
    {
        return Err(drift("complete selected step spool bytes differ"));
    }
    Ok(())
}

fn require_stable_job(before: &JobSnapshot, after: &JobSnapshot) -> Result<(), CorpusProblem> {
    if after != before {
        return Err(drift(
            "owned terminal utility job changed during spool readback",
        ));
    }
    Ok(())
}

pub(super) fn require_utility(
    server: &ProductServer,
    id: &str,
    selected: UtilityJob,
) -> Result<(), CorpusProblem> {
    let batch = server.batch_service();
    let before = batch.get(id).map_err(terminal_problem)?;
    require_job(id, &before, selected)?;
    let invocation = base_batch_control_invocation()?;
    let files = batch
        .spool_files(&invocation, id)
        .map_err(terminal_problem)?;
    for (file, expected) in selected.spool() {
        require_descriptor(&files, file, expected)?;
        // The exact small oracle length bounds reads before any untrusted count
        // can request allocation. Exact descriptor/bytes/more checks prove completion.
        let (records, more) = batch
            .spool(&invocation, id, file, 0, expected.len())
            .map_err(terminal_problem)?;
        require_records(&records, more, expected)?;
    }
    let after = batch.get(id).map_err(terminal_problem)?;
    require_job(id, &after, selected)?;
    require_stable_job(&before, &after)
}
