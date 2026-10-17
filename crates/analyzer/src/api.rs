use std::{net::SocketAddr, path::PathBuf, str::FromStr, sync::Arc};

use anyhow::{anyhow, Context};
use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, put},
    Extension, Json, Router,
};
use serde::{Deserialize, Serialize};
use tokio::sync::watch;

use crate::storage::chains::{
    decode_cursor, ChainCursor, ChainPage, ChainReader, MemberCursor, MemberPage,
};
use crate::{
    auth::{self, AuthConfig},
    collector::{CollectorRegistry, CollectorStatus},
    config::AnalyzerMode,
    domain::{SessionProtocol, SessionSummary, Source, SourceInput},
    payload_search::{self, PayloadSearch, PayloadSearchLimits},
    storage::{parse_optional_ip, read_payload, Catalog, SessionFilter},
};

#[derive(Clone)]
pub struct ApiState {
    pub catalog: Catalog,
    pub data_dir: PathBuf,
    pub source_revision: watch::Sender<u64>,
    pub flag_regex: regex::bytes::Regex,
    pub collectors: CollectorRegistry,
    pub analyzer_mode: AnalyzerMode,
}

#[derive(Clone)]
struct RuleOwner(String);

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Bucket {
    Known,
    Unknown,
}
impl Bucket {
    fn accepts(self, known: bool) -> bool {
        matches!(self, Self::Known) == known
    }
}

async fn list_rules(
    State(state): State<Arc<ApiState>>,
    Extension(owner): Extension<RuleOwner>,
) -> Result<Json<Vec<crate::attack_rules::AttackRule>>, ApiError> {
    Ok(Json(
        blocking(move || state.catalog.list_rules(&owner.0)).await?,
    ))
}
async fn create_rule(
    State(state): State<Arc<ApiState>>,
    Extension(owner): Extension<RuleOwner>,
    Json(input): Json<crate::attack_rules::RuleInput>,
) -> Result<(StatusCode, Json<crate::attack_rules::AttackRule>), ApiError> {
    let rule = blocking(move || state.catalog.save_rule(&owner.0, None, input))
        .await?
        .ok_or_else(|| ApiError::not_found("rule not found"))?;
    Ok((StatusCode::CREATED, Json(rule)))
}
async fn update_rule(
    State(state): State<Arc<ApiState>>,
    Extension(owner): Extension<RuleOwner>,
    Path(id): Path<i64>,
    Json(input): Json<crate::attack_rules::RuleInput>,
) -> Result<Json<crate::attack_rules::AttackRule>, ApiError> {
    let rule = blocking(move || state.catalog.save_rule(&owner.0, Some(id), input))
        .await?
        .ok_or_else(|| ApiError::not_found("rule not found"))?;
    Ok(Json(rule))
}
async fn delete_rule(
    State(state): State<Arc<ApiState>>,
    Extension(owner): Extension<RuleOwner>,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    if !blocking(move || state.catalog.delete_rule(&owner.0, id)).await? {
        return Err(ApiError::not_found("rule not found"));
    }
    Ok(StatusCode::NO_CONTENT)
}
async fn get_http_events(
    State(state): State<Arc<ApiState>>,
    Path(id): Path<i64>,
) -> Result<Json<Vec<crate::attack_rules::HttpEvent>>, ApiError> {
    let catalog = state.catalog.clone();
    let stored = blocking(move || catalog.get_session(id))
        .await?
        .ok_or_else(|| ApiError::not_found("session not found"))?;
    Ok(Json(
        blocking(move || {
            crate::attack_rules::extract_events(id, &read_payload(state.data_dir.clone(), &stored)?)
        })
        .await?,
    ))
}

#[derive(Deserialize)]
struct RuleHighlightQuery {
    selection: String,
}

async fn get_rule_highlights(
    State(state): State<Arc<ApiState>>,
    Extension(owner): Extension<RuleOwner>,
    Path(id): Path<i64>,
    Query(query): Query<RuleHighlightQuery>,
) -> Result<Json<crate::attack_rules::RuleHighlights>, ApiError> {
    if query.selection.len() > 16_384 {
        return Err(ApiError::bad_request("rule selection is too large"));
    }
    let selection: Vec<crate::attack_rules::RuleSelection> = serde_json::from_str(&query.selection)
        .map_err(|_| ApiError::bad_request("invalid rule selection"))?;
    crate::attack_rules::validate_selection(&selection).map_err(ApiError::bad_request_error)?;
    Ok(Json(
        chain_blocking(move || {
            let stored = state
                .catalog
                .get_session(id)?
                .ok_or_else(|| anyhow!(crate::attack_rules::StaleRuleMatch))?;
            let rules = crate::attack_rules::compile(state.catalog.list_rules(&owner.0)?)?;
            let payload = read_payload(state.data_dir.clone(), &stored)?;
            crate::attack_rules::explain(id, stored.summary.source_id, &payload, &rules, &selection)
        })
        .await?,
    ))
}

pub fn router(state: ApiState, config: AuthConfig) -> Router {
    let owner = RuleOwner(config.username().to_owned());
    let app = Router::new()
        .route("/api/sources", get(list_sources).post(create_source))
        .route("/api/collectors", get(list_collectors))
        .route("/api/payload-search/limits", get(payload_search_limits))
        .route(
            "/api/sources/{id}",
            put(update_source).delete(delete_source),
        )
        .route("/api/sessions", get(list_sessions))
        .route("/api/chains", get(list_chains))
        .route("/api/chains/{id}/sessions", get(list_chain_members))
        .route("/api/sessions/{id}", get(get_session))
        .route("/api/sessions/{id}/payload/{direction}", get(get_payload))
        .route("/api/sessions/{id}/flag-matches", get(get_flag_matches))
        .route("/api/sessions/{id}/http-events", get(get_http_events))
        .route("/api/sessions/{id}/rule-matches", get(get_rule_highlights))
        .route("/api/attack-rules", get(list_rules).post(create_rule))
        .route(
            "/api/attack-rules/{id}",
            put(update_rule).delete(delete_rule),
        )
        .layer(Extension(owner))
        .with_state(Arc::new(state));
    auth::protect(app, config)
}

#[derive(Serialize)]
struct CollectorsResponse {
    mode: AnalyzerMode,
    collectors: Vec<CollectorStatus>,
}

async fn list_collectors(State(state): State<Arc<ApiState>>) -> Json<CollectorsResponse> {
    Json(CollectorsResponse {
        mode: state.analyzer_mode,
        collectors: state.collectors.list(),
    })
}

pub async fn serve(address: SocketAddr, state: ApiState, config: AuthConfig) -> anyhow::Result<()> {
    anyhow::ensure!(address.ip().is_loopback(), "API listener must be loopback");
    let app = router(state, config);
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .with_context(|| format!("bind API to {address}"))?;
    tracing::info!(%address, "API listening");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await
    .context("serve API")
}

async fn list_sources(State(state): State<Arc<ApiState>>) -> Result<Json<Vec<Source>>, ApiError> {
    let catalog = state.catalog.clone();
    let sources = blocking(move || catalog.list_sources(false)).await?;
    Ok(Json(sources))
}

async fn create_source(
    State(state): State<Arc<ApiState>>,
    Json(input): Json<SourceInput>,
) -> Result<(StatusCode, Json<Source>), ApiError> {
    validate_source_input(&input)?;
    let catalog = state.catalog.clone();
    let source = blocking(move || catalog.create_source(&input)).await?;
    notify_source_change(&state.source_revision);
    Ok((StatusCode::CREATED, Json(source)))
}

async fn update_source(
    State(state): State<Arc<ApiState>>,
    Path(id): Path<i64>,
    Json(input): Json<SourceInput>,
) -> Result<Json<Source>, ApiError> {
    validate_source_input(&input)?;
    let catalog = state.catalog.clone();
    let source = blocking(move || catalog.update_source(id, &input)).await?;
    let source = source.ok_or_else(|| ApiError::not_found("source not found"))?;
    notify_source_change(&state.source_revision);
    Ok(Json(source))
}

async fn delete_source(
    State(state): State<Arc<ApiState>>,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    let catalog = state.catalog.clone();
    let deleted = blocking(move || catalog.delete_source(id)).await?;
    if !deleted {
        return Err(ApiError::not_found("source not found"));
    }
    notify_source_change(&state.source_revision);
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
struct SessionQuery {
    bucket: Option<Bucket>,
    source_id: Option<i64>,
    contains_flag: Option<bool>,
    protocol: Option<String>,
    client_ip: Option<String>,
    server_ip: Option<String>,
    client_port: Option<u16>,
    server_port: Option<u16>,
    started_after: Option<i64>,
    cursor: Option<i64>,
    limit: Option<usize>,
    #[serde(default, deserialize_with = "deserialize_payload_terms")]
    payload: Vec<String>,
    chain_cursor: Option<String>,
}

// A JSON list in one URL-encoded query value preserves commas, quotes and
// duplicate terms without relying on repeated-key decoding in serde_urlencoded.
fn deserialize_payload_terms<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    serde_json::from_str(&value).map_err(serde::de::Error::custom)
}

async fn payload_search_limits() -> Json<PayloadSearchLimits> {
    Json(payload_search::limits())
}

#[derive(Serialize)]
struct SessionPage {
    items: Vec<SessionSummary>,
    next_cursor: Option<i64>,
}

impl SessionQuery {
    fn filter(&self) -> Result<SessionFilter, ApiError> {
        Ok(SessionFilter {
            source_id: self.source_id,
            contains_flag: self.contains_flag,
            protocol: self
                .protocol
                .as_deref()
                .map(SessionProtocol::from_str)
                .transpose()
                .map_err(|_| ApiError::bad_request("invalid protocol filter"))?,
            client_ip: parse_optional_ip(self.client_ip.as_deref())
                .map_err(ApiError::bad_request_error)?,
            server_ip: parse_optional_ip(self.server_ip.as_deref())
                .map_err(ApiError::bad_request_error)?,
            client_port: self.client_port,
            server_port: self.server_port,
            started_after: self.started_after,
            cursor: self.cursor,
            limit: self.limit.unwrap_or(100).clamp(1, 200),
        })
    }
}

async fn list_chains(
    State(state): State<Arc<ApiState>>,
    Extension(owner): Extension<RuleOwner>,
    Query(query): Query<SessionQuery>,
) -> Result<Json<ChainPage>, ApiError> {
    let filter = query.filter()?;
    let cursor = query
        .chain_cursor
        .as_deref()
        .map(decode_cursor::<ChainCursor>)
        .transpose()
        .map_err(ApiError::bad_request_error)?;
    if cursor.as_ref().is_some_and(|cursor| cursor.snapshot < 0) {
        return Err(ApiError::bad_request("invalid chain snapshot"));
    }
    let search = PayloadSearch::new(query.payload).map_err(ApiError::bad_request)?;
    let catalog = state.catalog.clone();
    let data_dir = state.data_dir.clone();
    Ok(Json(
        chain_blocking(move || {
            let reader = ChainReader::open(&catalog)?;
            let rules = if query.bucket.is_some() {
                crate::attack_rules::compile(reader.rules(&owner.0)?)?
            } else {
                Vec::new()
            };
            if matches!(query.bucket, Some(Bucket::Known))
                && !rules.iter().any(|rule| {
                    filter
                        .source_id
                        .is_none_or(|source| rule.rule.input.source_id == source)
                })
            {
                return Ok(ChainPage {
                    items: Vec::new(),
                    next_cursor: None,
                });
            }
            let mut page = reader.list(&filter, cursor, &search, &data_dir)?;
            if let Some(bucket) = query.bucket {
                for chain in &mut page.items {
                    chain.known_matches = reader.classify(chain, &rules, &data_dir)?;
                }
                page.items
                    .retain(|chain| bucket.accepts(!chain.known_matches.is_empty()));
            }
            Ok(page)
        })
        .await?,
    ))
}

#[derive(Deserialize)]
struct MemberQuery {
    cursor: Option<String>,
    snapshot_id: Option<i64>,
    limit: Option<usize>,
}

async fn list_chain_members(
    State(state): State<Arc<ApiState>>,
    Path(id): Path<i64>,
    Query(query): Query<MemberQuery>,
) -> Result<Json<MemberPage>, ApiError> {
    let cursor = query
        .cursor
        .as_deref()
        .map(decode_cursor::<MemberCursor>)
        .transpose()
        .map_err(ApiError::bad_request_error)?;
    if query.snapshot_id.is_some_and(|value| value < 0)
        || cursor.as_ref().is_some_and(|value| value.snapshot < 0)
    {
        return Err(ApiError::bad_request("invalid chain snapshot"));
    }
    let catalog = state.catalog.clone();
    let page = chain_blocking(move || {
        ChainReader::open(&catalog)?.member_page(
            id,
            cursor,
            query.snapshot_id,
            query.limit.unwrap_or(20),
        )
    })
    .await?;
    Ok(Json(
        page.ok_or_else(|| ApiError::not_found("chain not found"))?,
    ))
}

async fn list_sessions(
    State(state): State<Arc<ApiState>>,
    Extension(owner): Extension<RuleOwner>,
    Query(query): Query<SessionQuery>,
) -> Result<Json<SessionPage>, ApiError> {
    let base_filter = query.filter()?;

    let search = PayloadSearch::new(query.payload).map_err(ApiError::bad_request)?;
    let page = if !search.is_empty() {
        search_payload(state.clone(), base_filter, search).await?
    } else {
        let catalog = state.catalog.clone();
        let requested = base_filter.limit;
        let sessions = blocking(move || catalog.list_sessions(&base_filter)).await?;
        let next_cursor = (sessions.len() == requested)
            .then(|| sessions.last().map(|session| session.id))
            .flatten();
        SessionPage {
            items: sessions,
            next_cursor,
        }
    };
    let mut page = page;
    if let Some(bucket) = query.bucket {
        let catalog = state.catalog.clone();
        let data_dir = state.data_dir.clone();
        page = chain_blocking(move || {
            let reader = ChainReader::open(&catalog)?;
            let snapshot = reader.snapshot()?;
            let rules = crate::attack_rules::compile(reader.rules(&owner.0)?)?;
            if rules.is_empty() {
                if matches!(bucket, Bucket::Known) {
                    page.items.clear();
                    page.next_cursor = None;
                }
                return Ok(page);
            }
            let anchors: Vec<_> = page
                .items
                .iter()
                .filter(|session| {
                    rules
                        .iter()
                        .any(|r| r.rule.input.source_id == session.source_id)
                })
                .map(|session| session.id)
                .collect();
            for chain in reader.summaries_for_sessions(&anchors, snapshot)? {
                let matches = reader.classify(&chain, &rules, &data_dir)?;
                for session in page
                    .items
                    .iter_mut()
                    .filter(|session| chain.contains_session(session))
                {
                    session.known_matches = matches.clone();
                }
            }
            page.items
                .retain(|session| bucket.accepts(!session.known_matches.is_empty()));
            Ok(page)
        })
        .await?;
    }
    Ok(Json(page))
}

async fn search_payload(
    state: Arc<ApiState>,
    filter: SessionFilter,
    search: PayloadSearch,
) -> Result<SessionPage, ApiError> {
    let desired = filter.limit;
    let catalog = state.catalog.clone();
    let data_dir = state.data_dir.clone();
    blocking(move || {
        let mut matches = Vec::with_capacity(desired);
        let mut cursor = filter.cursor;
        let mut scanned = 0usize;
        const MAX_CANDIDATES: usize = 2_000;

        while matches.len() < desired && scanned < MAX_CANDIDATES {
            let mut page_filter = filter.clone();
            page_filter.cursor = cursor;
            page_filter.limit = 200.min(MAX_CANDIDATES - scanned);
            let page = catalog.list_sessions(&page_filter)?;
            if page.is_empty() {
                cursor = None;
                break;
            }
            for summary in page {
                scanned += 1;
                cursor = Some(summary.id);
                let Some(stored) = catalog.get_session(summary.id)? else {
                    continue;
                };
                let payload = read_payload(data_dir.clone(), &stored)?;
                if search.matches(&payload.c2s, &payload.s2c) {
                    matches.push(summary);
                    if matches.len() == desired {
                        break;
                    }
                }
            }
        }
        Ok(SessionPage {
            items: matches,
            next_cursor: cursor,
        })
    })
    .await
}

async fn get_session(
    State(state): State<Arc<ApiState>>,
    Path(id): Path<i64>,
) -> Result<Json<SessionSummary>, ApiError> {
    let catalog = state.catalog.clone();
    let session = blocking(move || catalog.get_session(id)).await?;
    Ok(Json(
        session
            .ok_or_else(|| ApiError::not_found("session not found"))?
            .summary,
    ))
}

async fn get_payload(
    State(state): State<Arc<ApiState>>,
    Path((id, direction)): Path<(i64, String)>,
) -> Result<Response, ApiError> {
    if direction != "c2s" && direction != "s2c" {
        return Err(ApiError::bad_request("direction must be c2s or s2c"));
    }
    let stored = {
        let catalog = state.catalog.clone();
        blocking(move || catalog.get_session(id))
            .await?
            .ok_or_else(|| ApiError::not_found("session not found"))?
    };
    let data_dir = state.data_dir.clone();
    let bytes = blocking(move || {
        let payload = read_payload(data_dir, &stored)?;
        Ok(if direction == "c2s" {
            payload.c2s
        } else {
            payload.s2c
        })
    })
    .await?;

    let mut response = Response::new(Body::from(bytes));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    Ok(response)
}

#[derive(Serialize)]
struct FlagMatches {
    representation: &'static str,
    c2s: Vec<ByteRange>,
    s2c: Vec<ByteRange>,
}

#[derive(Serialize)]
struct ByteRange {
    start: usize,
    end: usize,
    value: String,
}

async fn get_flag_matches(
    State(state): State<Arc<ApiState>>,
    Path(id): Path<i64>,
) -> Result<Json<FlagMatches>, ApiError> {
    let stored = {
        let catalog = state.catalog.clone();
        blocking(move || catalog.get_session(id))
            .await?
            .ok_or_else(|| ApiError::not_found("session not found"))?
    };
    let data_dir = state.data_dir.clone();
    let regex = state.flag_regex.clone();
    let matches = blocking(move || {
        let payload = read_payload(data_dir, &stored)?;
        let ranges = |bytes: &[u8]| {
            let text = crate::payload_text::display(bytes, false);
            regex
                .find_iter(text.as_bytes())
                .map(|found| {
                    let mut start = found.start();
                    let mut end = found.end();
                    while !text.is_char_boundary(start) {
                        start -= 1;
                    }
                    while !text.is_char_boundary(end) {
                        end += 1;
                    }
                    ByteRange {
                        start: found.start(),
                        end: found.end(),
                        value: text[start..end].to_owned(),
                    }
                })
                .collect()
        };
        Ok(FlagMatches {
            representation: crate::payload_text::VERSION,
            c2s: ranges(&payload.c2s),
            s2c: ranges(&payload.s2c),
        })
    })
    .await?;
    Ok(Json(matches))
}

fn notify_source_change(sender: &watch::Sender<u64>) {
    sender.send_modify(|revision| *revision = revision.wrapping_add(1));
}

fn validate_source_input(input: &SourceInput) -> Result<(), ApiError> {
    if input.name.trim().is_empty() {
        return Err(ApiError::bad_request("source name must not be empty"));
    }
    if input.name.trim().len() > 80 {
        return Err(ApiError::bad_request(
            "source name must not exceed 80 bytes",
        ));
    }
    if input.port == 0 {
        return Err(ApiError::bad_request(
            "source port must be between 1 and 65535",
        ));
    }
    Ok(())
}

async fn blocking<T, F>(operation: F) -> Result<T, ApiError>
where
    T: Send + 'static,
    F: FnOnce() -> anyhow::Result<T> + Send + 'static,
{
    tokio::task::spawn_blocking(operation)
        .await
        .map_err(|error| ApiError::internal(anyhow!(error)))?
        .map_err(ApiError::internal)
}

// Chain reads can sort a large retained catalog. Reject excess requests instead
// of letting them occupy an unbounded number of blocking threads/connections.
static CHAIN_READS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);

async fn chain_blocking<T, F>(operation: F) -> Result<T, ApiError>
where
    T: Send + 'static,
    F: FnOnce() -> anyhow::Result<T> + Send + 'static,
{
    let permit = CHAIN_READS.try_acquire().map_err(|_| ApiError {
        status: StatusCode::SERVICE_UNAVAILABLE,
        message: "chain queries are busy; retry shortly".to_owned(),
    })?;
    blocking(move || {
        let _permit = permit;
        operation()
    })
    .await
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    fn bad_request_error(error: anyhow::Error) -> Self {
        Self::bad_request(error.to_string())
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
        }
    }

    fn internal(error: anyhow::Error) -> Self {
        if error
            .downcast_ref::<crate::attack_rules::StaleRuleMatch>()
            .is_some()
        {
            return Self {
                status: StatusCode::CONFLICT,
                message: error.to_string(),
            };
        }
        if error
            .downcast_ref::<crate::attack_rules::RuleValidationError>()
            .is_some()
        {
            return Self::bad_request(error.to_string());
        }
        if matches!(
            error.to_string().as_str(),
            "source not found" | "at most 100 rules per user"
        ) {
            return Self::bad_request(error.to_string());
        }
        if error
            .to_string()
            .contains("classification deadline exceeded")
        {
            return Self {
                status: StatusCode::SERVICE_UNAVAILABLE,
                message: error.to_string(),
            };
        }
        tracing::error!(error = ?error, "API request failed");
        if crate::storage::chains::is_interrupted(&error) {
            return Self {
                status: StatusCode::SERVICE_UNAVAILABLE,
                message: "chain query budget exceeded; narrow filters and retry".to_owned(),
            };
        }
        let message = if is_constraint_error(&error) {
            "source port is already in use".to_owned()
        } else {
            "internal server error".to_owned()
        };
        let status = if is_constraint_error(&error) {
            StatusCode::CONFLICT
        } else {
            StatusCode::INTERNAL_SERVER_ERROR
        };
        Self { status, message }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        #[derive(Serialize)]
        struct ErrorBody {
            error: String,
        }
        (
            self.status,
            Json(ErrorBody {
                error: self.message,
            }),
        )
            .into_response()
    }
}

fn is_constraint_error(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<rusqlite::Error>()
            .is_some_and(|error| matches!(error, rusqlite::Error::SqliteFailure(code, _) if code.extended_code == 2067))
    })
}

async fn shutdown_signal() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        tracing::error!(%error, "failed to install shutdown signal handler");
    }
}
