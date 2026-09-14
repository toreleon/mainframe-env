//! Waits for a CardDemo batch job to reach a terminal state, and the utility
//! spool-log diagnostic for a job that did not finish as expected.
//!
//! Split out of `carddemo.rs` to keep it under its ADR-0010 module-review
//! budget (`conformance/0.9/inventory/module-budgets.json`). Covers the
//! z/OSMF poll (`wait_for_submitted_job`), the listed-job poll used by the
//! internal-reader wait (`wait_for_listed_job`), the ABEND submit-and-wait
//! helper (`submit_expected_abend`), and the shared timeout and poll
//! interval they use.

use super::{CorpusProblem, base_batch_control_invocation, terminal_http, terminal_problem};
use axum::http::{Method, StatusCode};
use base64::Engine;
use mainframe_env_batch::JobSnapshot;
use mainframe_env_execution_api::PrincipalId;
use mainframe_env_server::ProductServer;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

const JOB_COMPLETION_TIMEOUT: Duration = Duration::from_secs(120);
const JOB_POLL_INTERVAL: Duration = Duration::from_millis(10);

pub(super) async fn wait_for_submitted_job(
    server: &ProductServer,
    app: &axum::Router,
    headers: &BTreeMap<String, String>,
    mut job: serde_json::Value,
) -> Result<serde_json::Value, CorpusProblem> {
    let jobname = job["jobname"]
        .as_str()
        .ok_or_else(|| CorpusProblem::new("carddemo.utility.job_failed", "job name is missing"))?
        .to_string();
    let jobid = job["jobid"]
        .as_str()
        .ok_or_else(|| CorpusProblem::new("carddemo.utility.job_failed", "job ID is missing"))?
        .to_string();
    let deadline = tokio::time::Instant::now() + JOB_COMPLETION_TIMEOUT;
    loop {
        let (status, body) = terminal_http(
            app,
            Method::GET,
            &format!("/zosmf/restjobs/jobs/{jobname}/{jobid}"),
            headers.clone(),
            Vec::new(),
        )
        .await?;
        if status != StatusCode::OK {
            return Err(CorpusProblem::new(
                "carddemo.utility.job_failed",
                format!(
                    "utility job status returned {status}: {}",
                    String::from_utf8_lossy(&body)
                ),
            ));
        }
        job = serde_json::from_slice(&body).map_err(|error| {
            CorpusProblem::new("carddemo.utility.job_failed", error.to_string())
        })?;
        if !matches!(job["status"].as_str(), Some("INPUT" | "ACTIVE")) {
            return Ok(job);
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(utility_job_failure(server, &job)?);
        }
        tokio::time::sleep(JOB_POLL_INTERVAL).await;
    }
}

pub(super) async fn wait_for_listed_job(
    server: &ProductServer,
    principal: &PrincipalId,
    name: &str,
    bound: usize,
    missing_code: &str,
    incomplete_code: &str,
) -> Result<JobSnapshot, CorpusProblem> {
    let deadline = tokio::time::Instant::now() + JOB_COMPLETION_TIMEOUT;
    let mut last_state = None;
    loop {
        let (jobs, _) = server
            .batch_service()
            .list(Some(principal), None, bound)
            .map_err(terminal_problem)?;
        // JES job IDs increase monotonically, so the greatest ID is the newest matching job.
        if let Some(job) = jobs
            .into_iter()
            .filter(|job| job.name == name)
            .max_by(|left, right| left.id.cmp(&right.id))
        {
            last_state = Some(job.state);
            if job.state.terminal() {
                return Ok(job);
            }
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(last_state.map_or_else(
                || {
                    CorpusProblem::new(
                        missing_code,
                        format!(
                            "{name} did not reach a terminal state in time because it never appeared"
                        ),
                    )
                },
                |state| {
                    CorpusProblem::new(
                        incomplete_code,
                        format!(
                            "{name} did not reach a terminal state in time; last observed state: {state:?}"
                        ),
                    )
                },
            ));
        }
        tokio::time::sleep(JOB_POLL_INTERVAL).await;
    }
}

pub(super) fn utility_job_failure(
    server: &ProductServer,
    job: &serde_json::Value,
) -> Result<CorpusProblem, CorpusProblem> {
    let spool_invocation = base_batch_control_invocation()?;
    let detail = job["jobid"].as_str().map_or_else(BTreeMap::new, |id| {
        [
            "JESMSGLG", "JOBLOG", "SYSPRINT", "SYSOUT", "CMDOUT", "ISFOUT",
        ]
        .into_iter()
        .filter_map(|name| {
            server
                .batch_service()
                .spool(&spool_invocation, id, name, 0, 4096)
                .ok()
                .map(|(records, _)| {
                    (
                        name.to_string(),
                        records
                            .into_iter()
                            .map(|record| String::from_utf8_lossy(&record).into_owned())
                            .collect::<Vec<_>>(),
                    )
                })
        })
        .collect()
    });
    Ok(CorpusProblem::new(
        "carddemo.utility.job_failed",
        format!("utility job did not complete: {job}; spool={detail:?}"),
    ))
}

pub(super) async fn submit_expected_abend(
    server: &Arc<ProductServer>,
    app: &axum::Router,
    jcl: &str,
    expected: &str,
) -> Result<(), CorpusProblem> {
    server
        .start_background_workers()
        .map_err(terminal_problem)?;
    let headers = BTreeMap::from([
        (
            "authorization".into(),
            format!(
                "Basic {}",
                base64::engine::general_purpose::STANDARD.encode("IBMUSER:TESTPASS")
            ),
        ),
        ("x-csrf-zosmf-header".into(), "true".into()),
    ]);
    let (status, body) = terminal_http(
        app,
        Method::PUT,
        "/zosmf/restjobs/jobs",
        headers.clone(),
        jcl.as_bytes().to_vec(),
    )
    .await?;
    let job: serde_json::Value = serde_json::from_slice(&body)
        .map_err(|error| CorpusProblem::new("carddemo.batch_program.abend", error.to_string()))?;
    let job = if status == StatusCode::CREATED {
        wait_for_submitted_job(server, app, &headers, job).await?
    } else {
        job
    };
    if status != StatusCode::CREATED || job["status"] != "OUTPUT" || job["retcode"] != expected {
        return Err(CorpusProblem::new(
            "carddemo.batch_program.abend",
            format!("ABEND job returned {status}: {job}"),
        ));
    }
    Ok(())
}
