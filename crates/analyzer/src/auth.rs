use std::{collections::HashMap, env, net::SocketAddr, sync::Arc};

use anyhow::{bail, Context};
use argon2::{password_hash::SaltString, Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use axum::{
    extract::{ConnectInfo, DefaultBodyLimit, Request, State},
    http::{header, Method, StatusCode, Uri},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use rand_core::OsRng;
use serde::{Deserialize, Serialize};
use time::{Duration, OffsetDateTime};
use tokio::sync::{Mutex, Semaphore};
use tower_sessions::{
    cookie::SameSite,
    session::{Id, Record},
    session_store, Expiry, Session, SessionManagerLayer, SessionStore,
};

#[derive(Clone)]
pub struct AuthConfig {
    username: String,
    password_hash: String,
    ttl: i64,
    secure: bool,
}

impl AuthConfig {
    pub fn from_env() -> anyhow::Result<Self> {
        Self::parse(
            env::var("USERNAME")
                .map_err(|_| anyhow::anyhow!("USERNAME is required and must be valid text"))?,
            env::var("PASSWORD")
                .map_err(|_| anyhow::anyhow!("PASSWORD is required and must be valid text"))?,
            &optional_env("SESSION_EXPIRACY", "86400s")?,
            &optional_env("AUTH_COOKIE_SECURE", "false")?,
        )
    }

    pub fn parse(
        username: String,
        password: String,
        expiration: &str,
        secure: &str,
    ) -> anyhow::Result<Self> {
        if username.trim().is_empty() || username.len() > 128 {
            bail!("USERNAME must contain 1..128 bytes");
        }
        if password.is_empty() || password.len() > 1024 {
            bail!("PASSWORD must contain 1..1024 bytes");
        }
        let password_hash = Argon2::default()
            .hash_password(password.as_bytes(), &SaltString::generate(&mut OsRng))
            .map_err(|error| anyhow::anyhow!("failed to initialize password verifier: {error}"))?
            .to_string();
        let ttl = parse_expiration(expiration)?;
        if !(1..=604_800).contains(&ttl) {
            bail!("SESSION_EXPIRACY must be between 1s and 168h");
        }
        let secure = secure
            .parse()
            .context("AUTH_COOKIE_SECURE must be true or false")?;
        Ok(Self {
            username,
            password_hash,
            ttl,
            secure,
        })
    }
}

fn parse_expiration(raw: &str) -> anyhow::Result<i64> {
    let split = raw
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(raw.len());
    let (value, unit) = raw.split_at(split);
    let value: i64 = value.parse().context("invalid SESSION_EXPIRACY")?;
    let multiplier = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 3_600,
        "d" => 86_400,
        _ => bail!("SESSION_EXPIRACY supports s, m, h and d units"),
    };
    value
        .checked_mul(multiplier)
        .context("SESSION_EXPIRACY is too large")
}

fn optional_env(name: &str, default: &str) -> anyhow::Result<String> {
    match env::var(name) {
        Ok(value) => Ok(value),
        Err(env::VarError::NotPresent) => Ok(default.into()),
        Err(_) => bail!("invalid {name}"),
    }
}

#[derive(Clone)]
struct AuthState {
    config: AuthConfig,
    verifier: Arc<Semaphore>,
}

// The session protocol/IDs/cookies come from tower-sessions. This store only
// bounds memory, expires records and prevents a concurrent save reviving logout.
#[derive(Clone, Default)]
struct BoundedStore(Arc<Mutex<HashMap<Id, Record>>>);

impl std::fmt::Debug for BoundedStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BoundedStore([redacted])")
    }
}

#[async_trait::async_trait]
impl SessionStore for BoundedStore {
    async fn create(&self, record: &mut Record) -> session_store::Result<()> {
        let mut records = self.0.lock().await;
        records.retain(|_, record| record.expiry_date > OffsetDateTime::now_utc());
        if records.len() >= 1024 {
            return Err(session_store::Error::Backend(
                "session capacity reached".into(),
            ));
        }
        while records.contains_key(&record.id) {
            record.id = Id::default();
        }
        records.insert(record.id, record.clone());
        Ok(())
    }

    async fn save(&self, record: &Record) -> session_store::Result<()> {
        let mut records = self.0.lock().await;
        if let Some(existing) = records.get_mut(&record.id) {
            if existing.expiry_date > OffsetDateTime::now_utc() {
                *existing = record.clone();
                return Ok(());
            }
        }
        Err(session_store::Error::Backend(
            "session no longer exists".into(),
        ))
    }

    async fn load(&self, id: &Id) -> session_store::Result<Option<Record>> {
        let mut records = self.0.lock().await;
        if records
            .get(id)
            .is_some_and(|record| record.expiry_date <= OffsetDateTime::now_utc())
        {
            records.remove(id);
        }
        Ok(records.get(id).cloned())
    }

    async fn delete(&self, id: &Id) -> session_store::Result<()> {
        self.0.lock().await.remove(id);
        Ok(())
    }
}

pub fn protect(api: Router, config: AuthConfig) -> Router {
    let sessions = SessionManagerLayer::new(BoundedStore::default())
        .with_name("newhatch_session")
        .with_http_only(true)
        .with_same_site(SameSite::Strict)
        .with_path("/")
        .with_secure(config.secure)
        .with_expiry(Expiry::OnInactivity(Duration::seconds(config.ttl)));
    let state = AuthState {
        config,
        verifier: Arc::new(Semaphore::new(1)),
    };
    api.fallback(|| async { StatusCode::NOT_FOUND })
        .merge(
            Router::new()
                .route("/api/auth/me", get(me))
                .route("/api/auth/logout", post(logout))
                .route(
                    "/api/auth/login",
                    post(login).layer(DefaultBodyLimit::max(4096)),
                )
                .with_state(state),
        )
        .route(
            "/api/health",
            get(|| async { Json(serde_json::json!({"status": "ok"})) }),
        )
        // Apply checks after merging all routes, including method/path fallbacks.
        .layer(middleware::from_fn(require_session))
        .layer(sessions)
        .layer(middleware::from_fn(require_local_and_origin))
        .layer(middleware::from_fn(no_store))
}

async fn no_store(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    response
}

async fn require_local_and_origin(request: Request, next: Next) -> Response {
    if public_health(&request) {
        return next.run(request).await;
    }
    let peer = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|peer| peer.0);
    if !peer.is_some_and(|peer| peer.ip().is_loopback()) {
        tracing::warn!(?peer, "rejected non-local API peer");
        return error(StatusCode::FORBIDDEN);
    }
    if !matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    ) {
        let headers = request.headers();
        let cross_site = headers
            .get("sec-fetch-site")
            .is_some_and(|value| value != "same-origin" && value != "none");
        let invalid_origin = headers.get(header::ORIGIN).is_some_and(|origin| {
            let origin = origin
                .to_str()
                .ok()
                .and_then(|origin| origin.parse::<Uri>().ok());
            !origin.is_some_and(|origin| {
                matches!(origin.scheme_str(), Some("http" | "https"))
                    && origin.authority().map(|authority| authority.as_str())
                        == headers
                            .get(header::HOST)
                            .and_then(|host| host.to_str().ok())
                    && origin.path() == "/"
                    && origin.query().is_none()
            })
        });
        if cross_site || invalid_origin {
            return error(StatusCode::FORBIDDEN);
        }
    }
    next.run(request).await
}

async fn require_session(session: Session, request: Request, next: Next) -> Response {
    if public_health(&request)
        || (request.method() == Method::POST && request.uri().path() == "/api/auth/login")
    {
        return next.run(request).await;
    }
    match session.get::<bool>("authenticated").await {
        Ok(Some(true)) => next.run(request).await,
        Ok(_) => error(StatusCode::UNAUTHORIZED),
        Err(_) => error(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

fn public_health(request: &Request) -> bool {
    request.method() == Method::GET && request.uri().path() == "/api/health"
}

#[derive(Deserialize)]
struct Credentials {
    username: String,
    password: String,
}

#[derive(Serialize)]
struct Identity {
    authenticated: bool,
    username: String,
}

async fn login(
    State(state): State<AuthState>,
    session: Session,
    Json(input): Json<Credentials>,
) -> Response {
    if input.username.len() > 128 || input.password.is_empty() || input.password.len() > 1024 {
        return error(StatusCode::UNAUTHORIZED);
    }
    let Ok(permit) = state.verifier.clone().try_acquire_owned() else {
        return error(StatusCode::TOO_MANY_REQUESTS);
    };
    let config = state.config.clone();
    let verified = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let hash = PasswordHash::new(&config.password_hash).expect("validated at startup");
        let password_ok = Argon2::default()
            .verify_password(input.password.as_bytes(), &hash)
            .is_ok();
        password_ok && input.username == config.username
    })
    .await
    .unwrap_or(false);
    if !verified {
        tracing::info!("login rejected");
        return error(StatusCode::UNAUTHORIZED);
    }
    if session.flush().await.is_err() {
        return error(StatusCode::INTERNAL_SERVER_ERROR);
    }
    if session.cycle_id().await.is_err() {
        return error(StatusCode::INTERNAL_SERVER_ERROR);
    }
    // Absolute expiry: polling cannot keep an unattended browser authenticated forever.
    session.set_expiry(Some(Expiry::AtDateTime(
        OffsetDateTime::now_utc() + Duration::seconds(state.config.ttl),
    )));
    if session.insert("authenticated", true).await.is_err() {
        return error(StatusCode::INTERNAL_SERVER_ERROR);
    }
    if session.save().await.is_err() {
        return error(StatusCode::INTERNAL_SERVER_ERROR);
    }
    tracing::info!("login succeeded");
    Json(Identity {
        authenticated: true,
        username: state.config.username,
    })
    .into_response()
}

async fn me(State(state): State<AuthState>) -> Json<Identity> {
    Json(Identity {
        authenticated: true,
        username: state.config.username,
    })
}

async fn logout(session: Session) -> Response {
    if session.flush().await.is_err() {
        return error(StatusCode::INTERNAL_SERVER_ERROR);
    }
    tracing::info!("logout succeeded");
    StatusCode::NO_CONTENT.into_response()
}

fn error(status: StatusCode) -> Response {
    let message = match status {
        StatusCode::UNAUTHORIZED => "unauthorized",
        StatusCode::FORBIDDEN => "forbidden",
        StatusCode::TOO_MANY_REQUESTS => "try again shortly",
        _ => "internal server error",
    };
    (status, Json(serde_json::json!({"error": message}))).into_response()
}

#[cfg(test)]
#[path = "../../../test/auth/store.rs"]
mod store_tests;
