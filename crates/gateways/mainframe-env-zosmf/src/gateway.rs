use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue, Response, StatusCode};
use axum::response::IntoResponse;
use base64::Engine;
use mainframe_env_execution_api::CancellationProbe;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc::{SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::oneshot;
use tower::limit::ConcurrencyLimit;
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;
use zeroize::{Zeroize, Zeroizing};

#[path = "generated/custom_routes.rs"]
mod custom_routes;
#[path = "generated/official_routes.rs"]
mod official_routes;

pub enum Authentication {
    Anonymous,
    Basic { user: String, secret: Vec<u8> },
    Bearer(String),
}

impl Drop for Authentication {
    fn drop(&mut self) {
        match self {
            Self::Basic { secret, .. } => secret.zeroize(),
            Self::Bearer(token) => token.zeroize(),
            Self::Anonymous => {}
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
        attributes: bool,
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
        jobid: Option<String>,
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
    CicsLaunch {
        transaction: String,
        rows: u16,
        columns: u16,
    },
    CicsScreen {
        session: String,
        tn3270: bool,
    },
    CicsInput {
        session: String,
        csrf_token: String,
        aid: u8,
        fields: BTreeMap<String, Vec<u8>>,
    },
    CicsTn3270Input {
        session: String,
        csrf_token: String,
        record: Vec<u8>,
    },
    CicsResume {
        session: String,
        csrf_token: String,
    },
    CicsDisconnect {
        session: String,
        csrf_token: String,
    },
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
        context: GatewayCallContext,
    ) -> Result<GatewayResponse, GatewayProblem>;
}

#[derive(Clone, Debug)]
pub struct GatewayCallContext {
    deadline_tick: u64,
    cancellation: CancellationProbe,
}

impl GatewayCallContext {
    pub fn new(deadline_tick: u64) -> Result<Self, GatewayProblem> {
        if deadline_tick == 0 {
            return Err(backend_unavailable());
        }
        Ok(Self {
            deadline_tick,
            cancellation: CancellationProbe::new(),
        })
    }

    fn for_timeout(timeout: Duration) -> Result<Self, GatewayProblem> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| backend_unavailable())?;
        let now = u64::try_from(now.as_millis()).map_err(|_| backend_unavailable())?;
        let timeout = u64::try_from(timeout.as_millis()).map_err(|_| backend_unavailable())?;
        let deadline_tick = now.checked_add(timeout).ok_or_else(backend_unavailable)?;
        Self::new(deadline_tick)
    }

    #[must_use]
    pub const fn deadline_tick(&self) -> u64 {
        self.deadline_tick
    }

    #[must_use]
    pub fn cancellation_probe(&self) -> CancellationProbe {
        self.cancellation.clone()
    }

    #[must_use]
    pub fn cancellation_requested(&self) -> bool {
        self.cancellation.is_requested()
    }

    #[must_use]
    pub fn deadline_elapsed(&self) -> bool {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|duration| u64::try_from(duration.as_millis()).ok())
            .is_none_or(|now| now >= self.deadline_tick)
    }
}

struct CancelOnDrop {
    cancellation: CancellationProbe,
    completed: bool,
}

impl CancelOnDrop {
    fn new(cancellation: CancellationProbe) -> Self {
        Self {
            cancellation,
            completed: false,
        }
    }

    fn complete(&mut self) {
        self.completed = true;
    }
}

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        if !self.completed {
            self.cancellation.request();
        }
    }
}

struct BackendJob {
    authentication: Authentication,
    request: GatewayRequest,
    context: GatewayCallContext,
    response: oneshot::Sender<Result<GatewayResponse, GatewayProblem>>,
}

struct BlockingLane {
    sender: SyncSender<BackendJob>,
}

impl BlockingLane {
    fn new(backend: Arc<dyn ZosmfBackend>, workers: usize, queue: usize) -> Self {
        let (sender, receiver) = sync_channel::<BackendJob>(queue.max(1));
        let receiver = Arc::new(Mutex::new(receiver));
        for index in 0..workers.max(1) {
            let backend = backend.clone();
            let receiver = receiver.clone();
            let _ = std::thread::Builder::new()
                .name(format!("zosmf-backend-{index}"))
                .spawn(move || {
                    loop {
                        let job = {
                            let Ok(receiver) = receiver.lock() else {
                                return;
                            };
                            let Ok(job) = receiver.recv() else {
                                return;
                            };
                            job
                        };
                        if job.context.cancellation_requested() || job.context.deadline_elapsed() {
                            let _ = job.response.send(Err(GatewayProblem::new(
                                StatusCode::REQUEST_TIMEOUT,
                                "request_cancelled",
                                "the request deadline elapsed before backend dispatch",
                            )));
                            continue;
                        }
                        let result = catch_unwind(AssertUnwindSafe(|| {
                            backend.call(job.authentication, job.request, job.context)
                        }))
                        .unwrap_or_else(|_| Err(backend_unavailable()));
                        let _ = job.response.send(result);
                    }
                });
        }
        Self { sender }
    }

    fn submit(
        &self,
        authentication: Authentication,
        request: GatewayRequest,
        context: GatewayCallContext,
    ) -> Result<oneshot::Receiver<Result<GatewayResponse, GatewayProblem>>, GatewayProblem> {
        let (response, receiver) = oneshot::channel();
        let job = BackendJob {
            authentication,
            request,
            context,
            response,
        };
        match self.sender.try_send(job) {
            Ok(()) => Ok(receiver),
            Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => {
                Err(backend_unavailable())
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ZosmfLimits {
    pub max_body_bytes: usize,
    pub max_concurrency: usize,
    pub max_blocking: usize,
    pub timeout: Duration,
    pub max_page_items: usize,
}

impl Default for ZosmfLimits {
    fn default() -> Self {
        Self {
            max_body_bytes: 4 * 1024 * 1024,
            max_concurrency: 256,
            max_blocking: 4,
            timeout: Duration::from_secs(30),
            max_page_items: 1000,
        }
    }
}

#[derive(Clone)]
struct GatewayState {
    limits: ZosmfLimits,
    blocking_lane: Arc<BlockingLane>,
}

pub fn router(backend: Arc<dyn ZosmfBackend>, limits: ZosmfLimits) -> Router {
    let state = GatewayState {
        limits,
        blocking_lane: Arc::new(BlockingLane::new(
            backend,
            limits.max_blocking,
            limits.max_concurrency,
        )),
    };
    let routes = official_routes::register(Router::<GatewayState>::new());
    let routes = custom_routes::register(routes)
        .fallback(not_found)
        .with_state(state)
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            limits.timeout,
        ));
    Router::new()
        .fallback_service(ConcurrencyLimit::new(routes, limits.max_concurrency))
        .layer(RequestBodyLimitLayer::new(limits.max_body_bytes))
        .layer(TraceLayer::new_for_http())
}

#[must_use]
pub const fn official_route_ids() -> &'static [&'static str] {
    official_routes::OFFICIAL_ROUTE_IDS
}

#[must_use]
pub const fn custom_route_ids() -> &'static [&'static str] {
    custom_routes::CUSTOM_ROUTE_IDS
}

async fn info(State(state): State<GatewayState>) -> Response<Body> {
    dispatch(&state, Authentication::Anonymous, GatewayRequest::Info).await
}

async fn authenticate(State(state): State<GatewayState>, headers: HeaderMap) -> Response<Body> {
    dispatch(
        &state,
        authentication(&headers),
        GatewayRequest::Authenticate,
    )
    .await
}

async fn logout(State(state): State<GatewayState>, headers: HeaderMap) -> Response<Body> {
    dispatch(&state, authentication(&headers), GatewayRequest::Logout).await
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
            attributes: headers.contains_key("x-ibm-attributes"),
            max,
        },
    )
    .await
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
    .await
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
    .await
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
    .await
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
    .await
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
    .await
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
    .await
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
    .await
}

#[derive(Deserialize, Default)]
struct JobListQuery {
    owner: Option<String>,
    prefix: Option<String>,
    jobid: Option<String>,
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
            jobid: query.jobid,
            max: query.max.unwrap_or(100).min(state.limits.max_page_items),
        },
    )
    .await
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
    .await
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
    .await
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
    .await
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
    .await
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
    .await
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
    .await
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
    .await
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
    .await
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
    .await
}

async fn console_logs(State(state): State<GatewayState>, headers: HeaderMap) -> Response<Body> {
    dispatch(
        &state,
        authentication(&headers),
        GatewayRequest::ConsoleLogs,
    )
    .await
}

async fn console_log(State(state): State<GatewayState>, headers: HeaderMap) -> Response<Body> {
    dispatch(&state, authentication(&headers), GatewayRequest::ConsoleLog).await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CicsLaunchBody {
    transaction: String,
    rows: Option<u16>,
    columns: Option<u16>,
}

async fn cics_launch(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    bytes: Bytes,
) -> Response<Body> {
    if let Some(response) = csrf(&headers) {
        return response;
    }
    let Ok(body) = serde_json::from_slice::<CicsLaunchBody>(&bytes) else {
        return malformed("CICS launch body is malformed");
    };
    dispatch(
        &state,
        authentication(&headers),
        GatewayRequest::CicsLaunch {
            transaction: body.transaction,
            rows: body.rows.unwrap_or(24),
            columns: body.columns.unwrap_or(80),
        },
    )
    .await
}

async fn cics_screen(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Path(session): Path<String>,
) -> Response<Body> {
    dispatch(
        &state,
        authentication(&headers),
        GatewayRequest::CicsScreen {
            session,
            tn3270: false,
        },
    )
    .await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CicsInputBody {
    aid: u8,
    fields: BTreeMap<String, String>,
}

async fn cics_input(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Path(session): Path<String>,
    bytes: Bytes,
) -> Response<Body> {
    if let Some(response) = csrf(&headers) {
        return response;
    }
    let Some(csrf_token) = terminal_csrf(&headers) else {
        return malformed_csrf();
    };
    let Ok(body) = serde_json::from_slice::<CicsInputBody>(&bytes) else {
        return malformed("CICS input body is malformed");
    };
    dispatch(
        &state,
        authentication(&headers),
        GatewayRequest::CicsInput {
            session,
            csrf_token,
            aid: body.aid,
            fields: body
                .fields
                .into_iter()
                .map(|(name, value)| (name, value.into_bytes()))
                .collect(),
        },
    )
    .await
}

async fn cics_resume(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Path(session): Path<String>,
) -> Response<Body> {
    if let Some(response) = csrf(&headers) {
        return response;
    }
    let Some(csrf_token) = terminal_csrf(&headers) else {
        return malformed_csrf();
    };
    dispatch(
        &state,
        authentication(&headers),
        GatewayRequest::CicsResume {
            session,
            csrf_token,
        },
    )
    .await
}

async fn cics_disconnect(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Path(session): Path<String>,
) -> Response<Body> {
    if let Some(response) = csrf(&headers) {
        return response;
    }
    let Some(csrf_token) = terminal_csrf(&headers) else {
        return malformed_csrf();
    };
    dispatch(
        &state,
        authentication(&headers),
        GatewayRequest::CicsDisconnect {
            session,
            csrf_token,
        },
    )
    .await
}

async fn cics_tn3270_screen(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Path(session): Path<String>,
) -> Response<Body> {
    dispatch(
        &state,
        authentication(&headers),
        GatewayRequest::CicsScreen {
            session,
            tn3270: true,
        },
    )
    .await
}

async fn cics_tn3270_input(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Path(session): Path<String>,
    bytes: Bytes,
) -> Response<Body> {
    if let Some(response) = csrf(&headers) {
        return response;
    }
    let Some(csrf_token) = terminal_csrf(&headers) else {
        return malformed_csrf();
    };
    dispatch(
        &state,
        authentication(&headers),
        GatewayRequest::CicsTn3270Input {
            session,
            csrf_token,
            record: bytes.to_vec(),
        },
    )
    .await
}

async fn not_found() -> Response<Body> {
    problem(GatewayProblem::new(
        StatusCode::NOT_FOUND,
        "route_not_supported",
        "route is not part of the mainframe-env 0.1 profile",
    ))
}

async fn dispatch(
    state: &GatewayState,
    authentication: Authentication,
    request: GatewayRequest,
) -> Response<Body> {
    let context = match GatewayCallContext::for_timeout(state.limits.timeout) {
        Ok(context) => context,
        Err(value) => return problem(value),
    };
    let mut cancellation = CancelOnDrop::new(context.cancellation_probe());
    let receiver = match state.blocking_lane.submit(authentication, request, context) {
        Ok(receiver) => receiver,
        Err(value) => return problem(value),
    };
    let result = match receiver.await {
        Ok(result) => result,
        Err(_) => Err(backend_unavailable()),
    };
    cancellation.complete();
    match result {
        Ok(result) => response(result),
        Err(problem_value) => problem(problem_value),
    }
}

fn backend_unavailable() -> GatewayProblem {
    GatewayProblem::new(
        StatusCode::SERVICE_UNAVAILABLE,
        "backend_unavailable",
        "the bounded backend lane is unavailable",
    )
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
    {
        let decoded = Zeroizing::new(decoded);
        if let Some(separator) = decoded.iter().position(|byte| *byte == b':') {
            return Authentication::Basic {
                user: String::from_utf8_lossy(&decoded[..separator]).into_owned(),
                secret: decoded[separator + 1..].to_vec(),
            };
        }
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

fn terminal_csrf(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-csrf-token")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty() && value.len() <= 256)
        .map(str::to_string)
}

fn malformed_csrf() -> Response<Body> {
    problem(GatewayProblem::new(
        StatusCode::FORBIDDEN,
        "csrf_required",
        "X-CSRF-TOKEN is required for this CICS session mutation",
    ))
}

fn malformed(message: &str) -> Response<Body> {
    problem(GatewayProblem::new(
        StatusCode::BAD_REQUEST,
        "malformed",
        message,
    ))
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
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use tower::ServiceExt;

    struct Backend {
        calls: AtomicUsize,
    }

    impl ZosmfBackend for Backend {
        fn call(
            &self,
            authentication: Authentication,
            request: GatewayRequest,
            _context: GatewayCallContext,
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

    struct BlockingBackend {
        calls: AtomicUsize,
        completed: AtomicBool,
        observed_cancellation: AtomicBool,
    }

    impl ZosmfBackend for BlockingBackend {
        fn call(
            &self,
            _authentication: Authentication,
            _request: GatewayRequest,
            context: GatewayCallContext,
        ) -> Result<GatewayResponse, GatewayProblem> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(150));
            self.observed_cancellation
                .store(context.cancellation_requested(), Ordering::SeqCst);
            self.completed.store(true, Ordering::SeqCst);
            Ok(GatewayResponse::json(StatusCode::OK, json!({"ok":true})))
        }
    }

    struct MixedLoadBackend {
        calls: AtomicUsize,
        active: AtomicUsize,
        peak_active: AtomicUsize,
    }

    impl ZosmfBackend for MixedLoadBackend {
        fn call(
            &self,
            _authentication: Authentication,
            request: GatewayRequest,
            _context: GatewayCallContext,
        ) -> Result<GatewayResponse, GatewayProblem> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.peak_active.fetch_max(active, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(75));
            self.active.fetch_sub(1, Ordering::SeqCst);
            let status = if matches!(request, GatewayRequest::JobSubmit { .. }) {
                StatusCode::CREATED
            } else {
                StatusCode::OK
            };
            Ok(GatewayResponse::json(status, json!({"ok":true})))
        }
    }

    #[tokio::test]
    async fn mixed_routes_share_one_global_concurrency_limit() {
        let backend = Arc::new(MixedLoadBackend {
            calls: AtomicUsize::new(0),
            active: AtomicUsize::new(0),
            peak_active: AtomicUsize::new(0),
        });
        let app = router(
            backend.clone(),
            ZosmfLimits {
                max_concurrency: 8,
                max_blocking: 4,
                timeout: Duration::from_secs(2),
                ..Default::default()
            },
        );
        let mut tasks = Vec::new();
        for index in 0..8 {
            let route = app.clone();
            tasks.push(tokio::spawn(async move {
                route
                    .oneshot(
                        Request::builder()
                            .uri("/zosmf/info")
                            .body(Body::empty())
                            .unwrap(),
                    )
                    .await
                    .unwrap()
                    .status()
            }));
            let route = app.clone();
            tasks.push(tokio::spawn(async move {
                route
                    .oneshot(
                        Request::builder()
                            .method(Method::PUT)
                            .uri("/zosmf/restjobs/jobs")
                            .header("x-csrf-zosmf-header", "true")
                            .body(Body::from(format!("//JOB{index} JOB\n")))
                            .unwrap(),
                    )
                    .await
                    .unwrap()
                    .status()
            }));
        }
        let mut ok = 0;
        let mut created = 0;
        for task in tasks {
            match task.await.unwrap() {
                StatusCode::OK => ok += 1,
                StatusCode::CREATED => created += 1,
                status => panic!("mixed-route admission returned {status}"),
            }
        }
        assert_eq!((ok, created), (8, 8));
        assert_eq!(backend.calls.load(Ordering::SeqCst), 16);
        assert!(backend.peak_active.load(Ordering::SeqCst) <= 4);
        assert_eq!(backend.active.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn timeout_preempts_blocking_backend_and_retains_the_bounded_lane() {
        let backend = Arc::new(BlockingBackend {
            calls: AtomicUsize::new(0),
            completed: AtomicBool::new(false),
            observed_cancellation: AtomicBool::new(false),
        });
        let app = router(
            backend.clone(),
            ZosmfLimits {
                max_concurrency: 2,
                max_blocking: 1,
                timeout: Duration::from_millis(20),
                ..Default::default()
            },
        );
        let first = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/zosmf/info")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(first.status(), StatusCode::REQUEST_TIMEOUT);
        assert!(!backend.completed.load(Ordering::SeqCst));

        let second = app
            .oneshot(
                Request::builder()
                    .uri("/zosmf/info")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(second.status(), StatusCode::REQUEST_TIMEOUT);
        assert_eq!(backend.calls.load(Ordering::SeqCst), 1);

        for _ in 0..50 {
            if backend.completed.load(Ordering::SeqCst) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert!(backend.completed.load(Ordering::SeqCst));
        assert!(backend.observed_cancellation.load(Ordering::SeqCst));
    }

    fn basic() -> String {
        format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode("IBMUSER:TESTPASS")
        )
    }

    #[tokio::test]
    async fn every_frozen_route_reaches_typed_backend() {
        assert_eq!(official_route_ids().len(), 23);
        assert_eq!(custom_route_ids().len(), 7);
        assert!(
            official_route_ids()
                .iter()
                .all(|id| id.contains(" /zosmf/"))
        );
        assert!(
            custom_route_ids()
                .iter()
                .all(|id| id.contains(" /mainframe-env/"))
        );
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

    #[tokio::test]
    async fn cics_session_routes_are_typed_bounded_and_csrf_guarded() {
        let backend = Arc::new(Backend {
            calls: AtomicUsize::new(0),
        });
        let app = router(backend.clone(), ZosmfLimits::default());
        let launch = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/mainframe-env/cics/v1/sessions")
                    .header("authorization", basic())
                    .header("x-csrf-zosmf-header", "true")
                    .body(Body::from(r#"{"transaction":"CC00"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(launch.status(), StatusCode::OK);
        for uri in [
            "/mainframe-env/cics/v1/sessions/terminal-1",
            "/mainframe-env/cics/v1/sessions/terminal-1/tn3270",
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(uri)
                        .header("authorization", basic())
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
        }
        for (method, uri, body) in [
            (
                Method::PUT,
                "/mainframe-env/cics/v1/sessions/terminal-1/input",
                r#"{"aid":125,"fields":{"USERID":"USER"}}"#,
            ),
            (
                Method::PUT,
                "/mainframe-env/cics/v1/sessions/terminal-1/tn3270",
                "record",
            ),
            (
                Method::POST,
                "/mainframe-env/cics/v1/sessions/terminal-1/resume",
                "",
            ),
            (
                Method::DELETE,
                "/mainframe-env/cics/v1/sessions/terminal-1",
                "",
            ),
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(uri)
                        .header("authorization", basic())
                        .header("x-csrf-zosmf-header", "true")
                        .header("x-csrf-token", "session-csrf")
                        .body(Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
        }
        let missing_csrf = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::PUT)
                    .uri("/mainframe-env/cics/v1/sessions/terminal-1/input")
                    .header("authorization", basic())
                    .header("x-csrf-zosmf-header", "true")
                    .body(Body::from(r#"{"aid":125,"fields":{}}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(missing_csrf.status(), StatusCode::FORBIDDEN);
        let malformed = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/mainframe-env/cics/v1/sessions")
                    .header("authorization", basic())
                    .header("x-csrf-zosmf-header", "true")
                    .body(Body::from("{"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(malformed.status(), StatusCode::BAD_REQUEST);
        assert_eq!(backend.calls.load(Ordering::SeqCst), 7);
    }
}
