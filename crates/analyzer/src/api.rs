use std::{net::SocketAddr, path::PathBuf, str::FromStr, sync::Arc};

use anyhow::{anyhow, Context};
use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use tokio::sync::watch;

use crate::{
    auth::{self, AuthConfig},
    collector::{CollectorRegistry, CollectorStatus},
    config::IngressMode,
    domain::{SessionProtocol, SessionSummary, Source, SourceInput},
    storage::{parse_optional_ip, read_payload, Catalog, SessionFilter},
};

#[derive(Clone)]
pub struct ApiState {
    pub catalog: Catalog,
    pub data_dir: PathBuf,
    pub source_revision: watch::Sender<u64>,
    pub flag_regex: regex::bytes::Regex,
    pub collectors: CollectorRegistry,
    pub ingress_mode: IngressMode,
}

pub fn router(state: ApiState, config: AuthConfig) -> Router {
    let app = Router::new()
        .route("/api/sources", get(list_sources).post(create_source))
        .route("/api/collectors", get(list_collectors))
        .route(
            "/api/sources/{id}",
            put(update_source).delete(delete_source),
        )
        .route("/api/sessions", get(list_sessions))
        .route("/api/sessions/{id}", get(get_session))
        .route("/api/sessions/{id}/payload/{direction}", get(get_payload))
        .route("/api/sessions/{id}/flag-matches", get(get_flag_matches))
        .with_state(Arc::new(state));
    auth::protect(app, config)
}

#[derive(Serialize)]
struct CollectorsResponse {
    mode: IngressMode,
    collectors: Vec<CollectorStatus>,
}

async fn list_collectors(State(state): State<Arc<ApiState>>) -> Json<CollectorsResponse> {
    Json(CollectorsResponse {
        mode: state.ingress_mode,
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
    payload: Option<String>,
}

#[derive(Serialize)]
struct SessionPage {
    items: Vec<SessionSummary>,
    next_cursor: Option<i64>,
}

async fn list_sessions(
    State(state): State<Arc<ApiState>>,
    Query(query): Query<SessionQuery>,
) -> Result<Json<SessionPage>, ApiError> {
    let protocol = query
        .protocol
        .as_deref()
        .map(SessionProtocol::from_str)
        .transpose()
        .map_err(|_| ApiError::bad_request("invalid protocol filter"))?;
    let base_filter = SessionFilter {
        source_id: query.source_id,
        contains_flag: query.contains_flag,
        protocol,
        client_ip: parse_optional_ip(query.client_ip.as_deref())
            .map_err(ApiError::bad_request_error)?,
        server_ip: parse_optional_ip(query.server_ip.as_deref())
            .map_err(ApiError::bad_request_error)?,
        client_port: query.client_port,
        server_port: query.server_port,
        started_after: query.started_after,
        cursor: query.cursor,
        limit: query.limit.unwrap_or(100).clamp(1, 200),
    };

    let page = if let Some(payload) = query.payload.filter(|value| !value.is_empty()) {
        search_payload(state, base_filter, payload.into_bytes()).await?
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
    Ok(Json(page))
}

async fn search_payload(
    state: Arc<ApiState>,
    filter: SessionFilter,
    needle: Vec<u8>,
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
                if contains_bytes(&payload.c2s, &needle) || contains_bytes(&payload.s2c, &needle) {
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
    c2s: Vec<ByteRange>,
    s2c: Vec<ByteRange>,
}

#[derive(Serialize)]
struct ByteRange {
    start: usize,
    end: usize,
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
            regex
                .find_iter(bytes)
                .map(|found| ByteRange {
                    start: found.start(),
                    end: found.end(),
                })
                .collect()
        };
        Ok(FlagMatches {
            c2s: ranges(&payload.c2s),
            s2c: ranges(&payload.s2c),
        })
    })
    .await?;
    Ok(Json(matches))
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    needle.is_empty()
        || haystack
            .windows(needle.len())
            .any(|window| window == needle)
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
        tracing::error!(error = ?error, "API request failed");
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
