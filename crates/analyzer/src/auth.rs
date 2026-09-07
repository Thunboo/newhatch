use std::{collections::HashMap, env, net::SocketAddr, sync::Arc};

use anyhow::{bail, Context};
use argon2::{Argon2, Params, PasswordHash, PasswordVerifier};
use axum::{
    extract::{ConnectInfo, DefaultBodyLimit, Request, State},
    http::{header, Method, StatusCode, Uri},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
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
            env::var("AUTH_USERNAME").map_err(|_| {
                anyhow::anyhow!("AUTH_USERNAME is required and must be valid text")
            })?,
            env::var("AUTH_PASSWORD_HASH").map_err(|_| {
                anyhow::anyhow!("AUTH_PASSWORD_HASH is required and must be valid text")
            })?,
            &optional_env("AUTH_SESSION_TTL_SECONDS", "86400")?,
            &optional_env("AUTH_COOKIE_SECURE", "false")?,
        )
    }

    pub fn parse(
        username: String,
        password_hash: String,
        ttl: &str,
        secure: &str,
    ) -> anyhow::Result<Self> {
        if username.trim().is_empty() || username.len() > 128 {
            bail!("AUTH_USERNAME must contain 1..128 bytes");
        }
        let parsed = PasswordHash::new(&password_hash)
            .map_err(|_| anyhow::anyhow!("AUTH_PASSWORD_HASH must be an Argon2id PHC hash"))?;
        if parsed.algorithm.as_str() != "argon2id"
            || parsed.version != Some(19)
            || parsed.salt.is_none()
            || parsed.hash.is_none()
        {
            bail!("AUTH_PASSWORD_HASH must be a complete Argon2id v19 PHC hash");
        }
        let params = Params::try_from(&parsed)
            .map_err(|_| anyhow::anyhow!("invalid AUTH_PASSWORD_HASH parameters"))?;
        let mut salt = [0u8; 64];
        if parsed
            .salt
            .unwrap()
            .decode_b64(&mut salt)
            .map_or(true, |salt| salt.len() < 8)
            || ["m", "t", "p"]
                .iter()
                .any(|name| parsed.params.get(*name).is_none())
        {
            bail!("AUTH_PASSWORD_HASH requires >=8 salt bytes and explicit m,t,p parameters");
        }
        // Bound verification work to protect the capture process from costly configuration.
        if !(19_456..=262_144).contains(&params.m_cost())
            || !(2..=10).contains(&params.t_cost())
            || !(1..=8).contains(&params.p_cost())
            || parsed.hash.as_ref().is_some_and(|hash| hash.len() < 16)
        {
            bail!("AUTH_PASSWORD_HASH requires m=19456..262144, t=2..10, p=1..8 and >=16 output bytes");
        }
        let ttl: i64 = ttl.parse().context("invalid AUTH_SESSION_TTL_SECONDS")?;
        if !(1..=604_800).contains(&ttl) {
            bail!("AUTH_SESSION_TTL_SECONDS must be 1..604800");
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
