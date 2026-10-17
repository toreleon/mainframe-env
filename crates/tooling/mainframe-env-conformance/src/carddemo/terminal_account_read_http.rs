//! Bounded account-owner HTTP transport and finite failure-only diagnostics.
use super::*;

fn http_refusal(phase: &'static str, status: StatusCode, body: &[u8]) {
    let value = serde_json::from_slice::<serde_json::Value>(body).ok();
    let category = value
        .as_ref()
        .and_then(|value| value["category"].as_str())
        .filter(|category| {
            matches!(
                *category,
                "malformed"
                    | "unsupported"
                    | "unsupported_capability"
                    | "not_found"
                    | "not_authorized"
                    | "cancelled"
                    | "timed_out"
                    | "resource_exhausted"
                    | "conflict"
                    | "unknown_outcome"
                    | "condition"
                    | "infrastructure_failure"
                    | "missing_idempotency"
            )
        })
        .unwrap_or("unclassified");
    // Fixed phases/categories and hashes only: never headers, routes, auth or body bytes.
    eprintln!(
        "account-read HTTP refusal phase={phase} status={} category={category} body_bytes={} body_sha256={}",
        status.as_u16(),
        body.len(),
        evidence::hash(body),
    );
}

pub(super) struct Http {
    pub(super) app: axum::Router,
    pub(super) calls: usize,
}
impl Http {
    pub(super) async fn call(
        &mut self,
        method: Method,
        route: &str,
        headers: BTreeMap<String, String>,
        body: Vec<u8>,
    ) -> Result<(StatusCode, Vec<u8>), CorpusProblem> {
        self.calls += 1;
        require(self.calls <= 16 && body.len() <= 65536 && route.len() <= 512)?;
        let mut builder = Request::builder().method(method).uri(route);
        for (key, value) in headers {
            builder = builder.header(key, value);
        }
        let response = self
            .app
            .clone()
            .oneshot(builder.body(Body::from(body)).map_err(|_| problem())?)
            .await
            .map_err(|_| problem())?;
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 65536)
            .await
            .map_err(|_| problem())?;
        Ok((status, bytes.to_vec()))
    }
    pub(super) async fn read(
        &mut self,
        route: &CardDemoOnlineSession,
    ) -> Result<serde_json::Value, CorpusProblem> {
        let (status, body) = self
            .call(
                Method::GET,
                &format!("/mainframe-env/cics/v1/sessions/{}", route.session),
                route.headers.clone(),
                Vec::new(),
            )
            .await?;
        if status != StatusCode::OK {
            http_refusal("read", status, &body);
        }
        require(status == StatusCode::OK)?;
        serde_json::from_slice(&body).map_err(|_| problem())
    }
    pub(super) async fn resume(
        &mut self,
        route: &CardDemoOnlineSession,
    ) -> Result<serde_json::Value, CorpusProblem> {
        let (status, body) = self
            .call(
                Method::POST,
                &format!("/mainframe-env/cics/v1/sessions/{}/resume", route.session),
                route.headers.clone(),
                Vec::new(),
            )
            .await?;
        if status != StatusCode::OK {
            http_refusal("resume", status, &body);
        }
        require(status == StatusCode::OK)?;
        serde_json::from_slice(&body).map_err(|_| problem())
    }
    pub(super) async fn exchange(
        &mut self,
        route: &CardDemoOnlineSession,
        aid: u8,
        fields: BTreeMap<String, String>,
    ) -> Result<serde_json::Value, CorpusProblem> {
        let (status, _) = self
            .call(
                Method::PUT,
                &format!("/mainframe-env/cics/v1/sessions/{}/input", route.session),
                route.headers.clone(),
                encode(&serde_json::json!({"aid":aid,"fields":fields}))?,
            )
            .await?;
        require(status == StatusCode::OK)?;
        self.resume(route).await
    }
}
