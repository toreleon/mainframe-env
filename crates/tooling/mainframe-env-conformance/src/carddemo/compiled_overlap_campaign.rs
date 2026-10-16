//! Request orchestration for the actual compiled overlap owner only.
//! The passive store capture and completed-effect proof remain independent.
use super::*;
use std::future::Future;
use tokio::task::{JoinError, JoinHandle};

type Submission = Result<(StatusCode, Vec<u8>), CorpusProblem>;
type Exchanges = Result<(serde_json::Value, serde_json::Value), CorpusProblem>;

pub(super) async fn stage_input(
    app: &axum::Router,
    session: &CardDemoOnlineSession,
    aid: u8,
    fields: BTreeMap<String, String>,
) -> Result<(), CorpusProblem> {
    let input = terminal_http(
        app,
        Method::PUT,
        &format!("/mainframe-env/cics/v1/sessions/{}/input", session.session),
        session.headers.clone(),
        serde_json::to_vec(&serde_json::json!({"aid":aid,"fields":fields}))
            .map_err(|error| CorpusProblem::new("carddemo.online.request", error.to_string()))?,
    )
    .await?;
    if input.0 != StatusCode::OK {
        return Err(CorpusProblem::new(
            "carddemo.online.exchange_failed",
            format!(
                "terminal input returned {}: {}",
                input.0,
                String::from_utf8_lossy(&input.1)
            ),
        ));
    }
    Ok(())
}

pub(super) async fn resume_input(
    app: &axum::Router,
    session: &CardDemoOnlineSession,
) -> Result<serde_json::Value, CorpusProblem> {
    let resumed = terminal_http(
        app,
        Method::POST,
        &format!("/mainframe-env/cics/v1/sessions/{}/resume", session.session),
        session.headers.clone(),
        Vec::new(),
    )
    .await?;
    if resumed.0 != StatusCode::OK {
        return Err(CorpusProblem::new(
            "carddemo.online.exchange_failed",
            format!(
                "terminal exchange returned {}: {}",
                resumed.0,
                String::from_utf8_lossy(&resumed.1)
            ),
        ));
    }
    serde_json::from_slice(&resumed.1)
        .map_err(|error| CorpusProblem::new("carddemo.online.response", error.to_string()))
}

pub(super) async fn join_campaign(
    submissions: Vec<JoinHandle<Submission>>,
    exchanges: impl Future<Output = Exchanges>,
) -> (Vec<Result<Submission, JoinError>>, Exchanges) {
    let submissions = async move {
        let mut joined = Vec::with_capacity(submissions.len());
        // Collect every result, including task panic and request failure, before
        // the caller interprets either workload or enters drain/cleanup.
        for task in submissions {
            joined.push(task.await);
        }
        joined
    };
    tokio::join!(submissions, exchanges)
}

#[path = "compiled_overlap_campaign_tests.rs"]
mod tests;
