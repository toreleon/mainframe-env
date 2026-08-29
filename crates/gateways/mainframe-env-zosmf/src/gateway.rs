use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue, Response, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post, put};
use base64::Engine;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;
use tower::limit::ConcurrencyLimitLayer;
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;

pub enum Authentication {
    Anonymous,
    Basic { user: String, secret: Vec<u8> },
    Bearer(String),
}

impl Drop for Authentication {
    fn drop(&mut self) {
        if let Self::Basic { secret, .. } = self {
            secret.fill(0);
        }
    }
}

pub enum GatewayRequest {
    Info,
    Authenticate,
    Logout,
    DatasetList {
        pattern: String,
        start: Option<String>,
        max: usize,
    },
    DatasetRead {
        dataset: String,
        member: Option<String>,
    },
    DatasetWrite {
        dataset: String,
        member: Option<String>,
        bytes: Vec<u8>,
    },
    DatasetCreate {
        dataset: String,
        attributes: Value,
    },
    DatasetDelete {
        dataset: String,
        member: Option<String>,
    },
    MemberList {
        dataset: String,
        start: Option<String>,
        max: usize,
    },
    DatasetSearch {
        dataset: String,
        search: String,
        max: usize,
    },
    Ams {
        control: Vec<u8>,
    },
    JobList {
        owner: Option<String>,
        prefix: Option<String>,
        max: usize,
    },
    JobSubmit {
        jcl: Vec<u8>,
    },
    JobStatus {
        jobname: String,
        jobid: String,
    },
    JobCancel {
        jobname: String,
        jobid: String,
    },
    JobPurge {
        jobname: String,
        jobid: String,
    },
    SpoolList {
        jobname: String,
        jobid: String,
    },
    SpoolRead {
        jobname: String,
        jobid: String,
        file: usize,
        start: usize,
        max: usize,
    },
    ConsoleIssue {
        name: String,
        command: Vec<u8>,
    },
    ConsoleSolicited {
        name: String,
        key: String,
    },
    ConsoleDetection {
        name: String,
        key: String,
    },
    ConsoleLogs,
    ConsoleLog,
}

pub struct GatewayResponse {
    pub status: StatusCode,
    pub headers: BTreeMap<String, String>,
    pub body: GatewayBody,
}

pub enum GatewayBody {
    Empty,
    Json(Value),
    Bytes(Vec<u8>),
}

impl GatewayResponse {
    #[must_use]
    pub fn json(status: StatusCode, body: Value) -> Self {
        Self {
            status,
            headers: BTreeMap::new(),
            body: GatewayBody::Json(body),
        }
    }

    #[must_use]
    pub fn empty(status: StatusCode) -> Self {
        Self {
            status,
            headers: BTreeMap::new(),
            body: GatewayBody::Empty,
        }
    }

    #[must_use]
    pub fn bytes(status: StatusCode, body: Vec<u8>) -> Self {
        Self {
            status,
            headers: BTreeMap::new(),
            body: GatewayBody::Bytes(body),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GatewayProblem {
    pub status: StatusCode,
    pub code: String,
    pub message: String,
}

impl GatewayProblem {
    #[must_use]
    pub fn new(status: StatusCode, code: &str, message: &str) -> Self {
        Self {
            status,
            code: code.into(),
            message: message.into(),
        }
    }
}

pub trait ZosmfBackend: Send + Sync {
    fn call(
        &self,
        authentication: Authentication,
        request: GatewayRequest,
    ) -> Result<GatewayResponse, GatewayProblem>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ZosmfLimits {
    pub max_body_bytes: usize,
    pub max_concurrency: usize,
    pub timeout: Duration,
    pub max_page_items: usize,
}

impl Default for ZosmfLimits {
    fn default() -> Self {
        Self {
            max_body_bytes: 4 * 1024 * 1024,
            max_concurrency: 256,
            timeout: Duration::from_secs(30),
            max_page_items: 1000,
        }
    }
}

#[derive(Clone)]
struct GatewayState {
    backend: Arc<dyn ZosmfBackend>,
    limits: ZosmfLimits,
}

pub fn router(backend: Arc<dyn ZosmfBackend>, limits: ZosmfLimits) -> Router {
    let state = GatewayState { backend, limits };
    Router::new()
        .route("/zosmf/info", get(info))
        .route(
            "/zosmf/services/authenticate",
            post(authenticate).delete(logout),
        )
        .route("/zosmf/restfiles/ds", get(dataset_list))
        .route(
            "/zosmf/restfiles/ds/{dsn}",
            get(dataset_read)
                .put(dataset_write)
                .post(dataset_create)
                .delete(dataset_delete),
        )
        .route("/zosmf/restfiles/ds/{dsn}/member", get(member_list))
        .route("/zosmf/restfiles/ds/{dsn}/search", get(dataset_search))
        .route("/zosmf/restfiles/ams", put(ams))
        .route("/zosmf/restjobs/jobs", get(job_list).put(job_submit))
        .route(
            "/zosmf/restjobs/jobs/{jobname}/{jobid}",
            get(job_status).put(job_cancel).delete(job_purge),
        )
        .route(
            "/zosmf/restjobs/jobs/{jobname}/{jobid}/files",
            get(spool_list),
        )
        .route(
            "/zosmf/restjobs/jobs/{jobname}/{jobid}/files/{file}/records",
            get(spool_read),
        )
        .route("/zosmf/restconsoles/consoles/{name}", put(console_issue))
        .route(
            "/zosmf/restconsoles/consoles/{name}/solmsgs/{key}",
            get(console_solicited),
        )
        .route(
            "/zosmf/restconsoles/consoles/{name}/detections/{key}",
            get(console_detection),
        )
        .route("/zosmf/logs", get(console_logs))
        .route("/zosmf/restconsoles/v1/log", get(console_log))
        .fallback(not_found)
        .with_state(state)
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            limits.timeout,
        ))
        .layer(ConcurrencyLimitLayer::new(limits.max_concurrency))
        .layer(RequestBodyLimitLayer::new(limits.max_body_bytes))
        .layer(TraceLayer::new_for_http())
}

async fn info(State(state): State<GatewayState>) -> Response<Body> {
    dispatch(&state, Authentication::Anonymous, GatewayRequest::Info)
}

async fn authenticate(State(state): State<GatewayState>, headers: HeaderMap) -> Response<Body> {
    dispatch(
        &state,
        authentication(&headers),
        GatewayRequest::Authenticate,
    )
}

async fn logout(State(state): State<GatewayState>, headers: HeaderMap) -> Response<Body> {
    dispatch(&state, authentication(&headers), GatewayRequest::Logout)
}

#[derive(Deserialize)]
struct DatasetListQuery {
    dslevel: Option<String>,
    start: Option<String>,
    max: Option<usize>,
}

async fn dataset_list(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Query(query): Query<DatasetListQuery>,
) -> Response<Body> {
    let max = query.max.unwrap_or(100).min(state.limits.max_page_items);
    dispatch(
        &state,
        authentication(&headers),
        GatewayRequest::DatasetList {
            pattern: query.dslevel.unwrap_or_else(|| "**".into()),
            start: query.start,
            max,
        },
    )
}

#[derive(Deserialize, Default)]
struct DatasetQuery {
    member: Option<String>,
}

async fn dataset_read(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Path(dsn): Path<String>,
    Query(query): Query<DatasetQuery>,
) -> Response<Body> {
    let (dataset, member) = split_member(&dsn, query.member);
    dispatch(
        &state,
        authentication(&headers),
        GatewayRequest::DatasetRead { dataset, member },
    )
}

async fn dataset_write(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Path(dsn): Path<String>,
    bytes: Bytes,
) -> Response<Body> {
    if let Some(response) = csrf(&headers) {
        return response;
    }
    let (dataset, member) = split_member(&dsn, None);
    dispatch(
        &state,
        authentication(&headers),
        GatewayRequest::DatasetWrite {
            dataset,
            member,
            bytes: bytes.to_vec(),
        },
    )
}

async fn dataset_create(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Path(dsn): Path<String>,
    bytes: Bytes,
) -> Response<Body> {
    if let Some(response) = csrf(&headers) {
        return response;
    }
    let attributes = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    dispatch(
        &state,
        authentication(&headers),
        GatewayRequest::DatasetCreate {
            dataset: dsn,
            attributes,
        },
    )
}

async fn dataset_delete(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Path(dsn): Path<String>,
) -> Response<Body> {
    if let Some(response) = csrf(&headers) {
        return response;
    }
    let (dataset, member) = split_member(&dsn, None);
    dispatch(
        &state,
        authentication(&headers),
        GatewayRequest::DatasetDelete { dataset, member },
    )
}

#[derive(Deserialize, Default)]
struct PageQuery {
    start: Option<String>,
    max: Option<usize>,
}

async fn member_list(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Path(dsn): Path<String>,
    Query(query): Query<PageQuery>,
) -> Response<Body> {
    dispatch(
        &state,
        authentication(&headers),
        GatewayRequest::MemberList {
            dataset: dsn,
            start: query.start,
            max: query.max.unwrap_or(100).min(state.limits.max_page_items),
        },
    )
}

#[derive(Deserialize)]
struct SearchQuery {
    search: String,
    max: Option<usize>,
}

async fn dataset_search(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Path(dsn): Path<String>,
    Query(query): Query<SearchQuery>,
) -> Response<Body> {
    dispatch(
        &state,
        authentication(&headers),
        GatewayRequest::DatasetSearch {
            dataset: dsn,
            search: query.search,
            max: query.max.unwrap_or(100).min(state.limits.max_page_items),
        },
    )
}

async fn ams(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    bytes: Bytes,
) -> Response<Body> {
    if let Some(response) = csrf(&headers) {
        return response;
    }
    dispatch(
        &state,
        authentication(&headers),
        GatewayRequest::Ams {
            control: bytes.to_vec(),
        },
    )
}

#[derive(Deserialize, Default)]
struct JobListQuery {
    owner: Option<String>,
    prefix: Option<String>,
    max: Option<usize>,
}

async fn job_list(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Query(query): Query<JobListQuery>,
) -> Response<Body> {
    dispatch(
        &state,
        authentication(&headers),
        GatewayRequest::JobList {
            owner: query.owner,
            prefix: query.prefix,
            max: query.max.unwrap_or(100).min(state.limits.max_page_items),
        },
    )
}

async fn job_submit(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    bytes: Bytes,
) -> Response<Body> {
    if let Some(response) = csrf(&headers) {
        return response;
    }
    dispatch(
        &state,
        authentication(&headers),
        GatewayRequest::JobSubmit {
            jcl: bytes.to_vec(),
        },
    )
}

async fn job_status(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Path((jobname, jobid)): Path<(String, String)>,
) -> Response<Body> {
    dispatch(
        &state,
        authentication(&headers),
        GatewayRequest::JobStatus { jobname, jobid },
    )
}

async fn job_cancel(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Path((jobname, jobid)): Path<(String, String)>,
) -> Response<Body> {
    if let Some(response) = csrf(&headers) {
        return response;
    }
    dispatch(
        &state,
        authentication(&headers),
        GatewayRequest::JobCancel { jobname, jobid },
    )
}

async fn job_purge(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Path((jobname, jobid)): Path<(String, String)>,
) -> Response<Body> {
    if let Some(response) = csrf(&headers) {
        return response;
    }
    dispatch(
        &state,
        authentication(&headers),
        GatewayRequest::JobPurge { jobname, jobid },
    )
}

async fn spool_list(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Path((jobname, jobid)): Path<(String, String)>,
) -> Response<Body> {
    dispatch(
        &state,
        authentication(&headers),
        GatewayRequest::SpoolList { jobname, jobid },
    )
}

#[derive(Deserialize, Default)]
struct SpoolQuery {
    start: Option<usize>,
    max: Option<usize>,
}

async fn spool_read(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Path((jobname, jobid, file)): Path<(String, String, usize)>,
    Query(query): Query<SpoolQuery>,
) -> Response<Body> {
    dispatch(
        &state,
        authentication(&headers),
        GatewayRequest::SpoolRead {
            jobname,
            jobid,
            file,
            start: query.start.unwrap_or(0),
            max: query.max.unwrap_or(1000).min(state.limits.max_page_items),
        },
    )
}

async fn console_issue(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Path(name): Path<String>,
    bytes: Bytes,
) -> Response<Body> {
    if let Some(response) = csrf(&headers) {
        return response;
    }
    dispatch(
        &state,
        authentication(&headers),
        GatewayRequest::ConsoleIssue {
            name,
            command: bytes.to_vec(),
        },
    )
}

async fn console_solicited(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Path((name, key)): Path<(String, String)>,
) -> Response<Body> {
    dispatch(
        &state,
        authentication(&headers),
        GatewayRequest::ConsoleSolicited { name, key },
    )
}

async fn console_detection(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Path((name, key)): Path<(String, String)>,
) -> Response<Body> {
    dispatch(
        &state,
        authentication(&headers),
        GatewayRequest::ConsoleDetection { name, key },
    )
}

async fn console_logs(State(state): State<GatewayState>, headers: HeaderMap) -> Response<Body> {
    dispatch(
        &state,
        authentication(&headers),
        GatewayRequest::ConsoleLogs,
    )
}

async fn console_log(State(state): State<GatewayState>, headers: HeaderMap) -> Response<Body> {
    dispatch(&state, authentication(&headers), GatewayRequest::ConsoleLog)
}

async fn not_found() -> Response<Body> {
    problem(GatewayProblem::new(
        StatusCode::NOT_FOUND,
        "route_not_supported",
        "route is not part of the mainframe-env 0.1 profile",
    ))
}

fn dispatch(
    state: &GatewayState,
    authentication: Authentication,
    request: GatewayRequest,
) -> Response<Body> {
    match state.backend.call(authentication, request) {
        Ok(result) => response(result),
        Err(problem_value) => problem(problem_value),
    }
}

fn response(value: GatewayResponse) -> Response<Body> {
    let mut builder = Response::builder().status(value.status);
    for (name, value) in value.headers {
        if let (Ok(name), Ok(value)) = (HeaderName::try_from(name), HeaderValue::try_from(value)) {
            builder = builder.header(name, value);
        }
    }
    let (body, content_type) = match value.body {
        GatewayBody::Empty => (Body::empty(), None),
        GatewayBody::Json(value) => (
            Body::from(serde_json::to_vec(&value).unwrap_or_default()),
            Some("application/json"),
        ),
        GatewayBody::Bytes(bytes) => (Body::from(bytes), Some("text/plain; charset=utf-8")),
    };
    if let Some(content_type) = content_type {
        builder = builder.header("content-type", content_type);
    }
    builder
        .body(body)
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

fn problem(value: GatewayProblem) -> Response<Body> {
    response(GatewayResponse::json(
        value.status,
        json!({
            "category": value.code,
            "message": value.message,
            "status": value.status.as_u16()
        }),
    ))
}

fn authentication(headers: &HeaderMap) -> Authentication {
    let Some(value) = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
    else {
        return Authentication::Anonymous;
    };
    if let Some(value) = value.strip_prefix("Basic ")
        && let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(value)
        && let Some(separator) = decoded.iter().position(|byte| *byte == b':')
    {
        return Authentication::Basic {
            user: String::from_utf8_lossy(&decoded[..separator]).into_owned(),
            secret: decoded[separator + 1..].to_vec(),
        };
    }
    value
        .strip_prefix("Bearer ")
        .map(|value| Authentication::Bearer(value.into()))
        .unwrap_or(Authentication::Anonymous)
}

fn csrf(headers: &HeaderMap) -> Option<Response<Body>> {
    if headers.contains_key("x-csrf-zosmf-header") {
        None
    } else {
        Some(problem(GatewayProblem::new(
            StatusCode::FORBIDDEN,
            "csrf_required",
            "X-CSRF-ZOSMF-HEADER is required",
        )))
    }
}

fn split_member(dataset: &str, query_member: Option<String>) -> (String, Option<String>) {
    if let Some(open) = dataset.rfind('(')
        && dataset.ends_with(')')
    {
        return (
            dataset[..open].to_string(),
            Some(dataset[open + 1..dataset.len() - 1].to_string()),
        );
    }
    (dataset.to_string(), query_member)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{Method, Request};
    use base64::Engine;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tower::ServiceExt;

    struct Backend {
        calls: AtomicUsize,
    }

    impl ZosmfBackend for Backend {
        fn call(
            &self,
            authentication: Authentication,
            request: GatewayRequest,
        ) -> Result<GatewayResponse, GatewayProblem> {
            if !matches!(request, GatewayRequest::Info)
                && matches!(authentication, Authentication::Anonymous)
            {
                return Err(GatewayProblem::new(
                    StatusCode::UNAUTHORIZED,
                    "authentication_required",
                    "authentication is required",
                ));
            }
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(match request {
                GatewayRequest::DatasetWrite { .. }
                | GatewayRequest::DatasetDelete { .. }
                | GatewayRequest::JobCancel { .. }
                | GatewayRequest::JobPurge { .. } => GatewayResponse::empty(StatusCode::NO_CONTENT),
                GatewayRequest::DatasetCreate { .. } | GatewayRequest::JobSubmit { .. } => {
                    GatewayResponse::json(StatusCode::CREATED, json!({"ok":true}))
                }
                GatewayRequest::DatasetRead { .. } | GatewayRequest::SpoolRead { .. } => {
                    GatewayResponse::bytes(StatusCode::OK, b"content".to_vec())
                }
                _ => GatewayResponse::json(StatusCode::OK, json!({"ok":true})),
            })
        }
    }

    fn basic() -> String {
        format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode("IBMUSER:TESTPASS")
        )
    }

    #[tokio::test]
    async fn every_frozen_route_reaches_typed_backend() {
        let backend = Arc::new(Backend {
            calls: AtomicUsize::new(0),
        });
        let app = router(backend.clone(), ZosmfLimits::default());
        let routes = [
            (Method::GET, "/zosmf/info", false),
            (Method::POST, "/zosmf/services/authenticate", false),
            (Method::DELETE, "/zosmf/services/authenticate", false),
            (Method::GET, "/zosmf/restfiles/ds", false),
            (Method::GET, "/zosmf/restfiles/ds/USER.DATA", false),
            (Method::PUT, "/zosmf/restfiles/ds/USER.DATA", true),
            (Method::POST, "/zosmf/restfiles/ds/USER.DATA", true),
            (Method::DELETE, "/zosmf/restfiles/ds/USER.DATA", true),
            (Method::GET, "/zosmf/restfiles/ds/USER.DATA/member", false),
            (
                Method::GET,
                "/zosmf/restfiles/ds/USER.DATA/search?search=X",
                false,
            ),
            (Method::PUT, "/zosmf/restfiles/ams", true),
            (Method::GET, "/zosmf/restjobs/jobs", false),
            (Method::PUT, "/zosmf/restjobs/jobs", true),
            (Method::GET, "/zosmf/restjobs/jobs/JOB/JOB00001", false),
            (Method::PUT, "/zosmf/restjobs/jobs/JOB/JOB00001", true),
            (Method::DELETE, "/zosmf/restjobs/jobs/JOB/JOB00001", true),
            (
                Method::GET,
                "/zosmf/restjobs/jobs/JOB/JOB00001/files",
                false,
            ),
            (
                Method::GET,
                "/zosmf/restjobs/jobs/JOB/JOB00001/files/0/records",
                false,
            ),
            (Method::PUT, "/zosmf/restconsoles/consoles/CONS", true),
            (
                Method::GET,
                "/zosmf/restconsoles/consoles/CONS/solmsgs/1",
                false,
            ),
            (
                Method::GET,
                "/zosmf/restconsoles/consoles/CONS/detections/1",
                false,
            ),
            (Method::GET, "/zosmf/logs", false),
            (Method::GET, "/zosmf/restconsoles/v1/log", false),
        ];
        for (method, uri, csrf_required) in routes {
            let mut request = Request::builder()
                .method(method)
                .uri(uri)
                .header("authorization", basic());
            if csrf_required {
                request = request.header("x-csrf-zosmf-header", "true");
            }
            let response = app
                .clone()
                .oneshot(request.body(Body::from("{}")).unwrap())
                .await
                .unwrap();
            assert!(
                response.status().is_success(),
                "{uri}: {}",
                response.status()
            );
        }
        assert_eq!(backend.calls.load(Ordering::SeqCst), 23);
    }

    #[tokio::test]
    async fn auth_csrf_body_limit_and_excluded_route_fail_closed() {
        let app = router(
            Arc::new(Backend {
                calls: AtomicUsize::new(0),
            }),
            ZosmfLimits {
                max_body_bytes: 4,
                ..Default::default()
            },
        );
        let unauthenticated = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/zosmf/restjobs/jobs")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);
        let csrf = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::PUT)
                    .uri("/zosmf/restjobs/jobs")
                    .header("authorization", basic())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(csrf.status(), StatusCode::FORBIDDEN);
        let large = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::PUT)
                    .uri("/zosmf/restjobs/jobs")
                    .header("authorization", basic())
                    .header("x-csrf-zosmf-header", "true")
                    .body(Body::from("12345"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(large.status(), StatusCode::PAYLOAD_TOO_LARGE);
        let excluded = app
            .oneshot(
                Request::builder()
                    .uri("/zosmf/tsoApp/tso")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(excluded.status(), StatusCode::NOT_FOUND);
    }
}
