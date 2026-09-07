use std::{net::SocketAddr, sync::OnceLock};

use argon2::{password_hash::SaltString, Argon2, PasswordHasher};
use axum::{
    body::{to_bytes, Body},
    extract::ConnectInfo,
    http::{Request, StatusCode},
    response::Response,
    Router,
};
use newhatch_analyzer::{
    api::{router, ApiState},
    auth::AuthConfig,
    storage::Catalog,
};
use tower::ServiceExt;

fn hash() -> String {
    static HASH: OnceLock<String> = OnceLock::new();
    HASH.get_or_init(|| {
        Argon2::default()
            .hash_password(
                b"test-only-password",
                &SaltString::encode_b64(b"test-only-salt!").unwrap(),
            )
            .unwrap()
            .to_string()
    })
    .clone()
}

fn app(ttl: &str, secure: &str) -> (Router, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let state = ApiState {
        catalog: Catalog::open(dir.path().join("index.sqlite")).unwrap(),
        data_dir: dir.path().to_owned(),
        source_revision: tokio::sync::watch::channel(0).0,
        flag_regex: regex::bytes::Regex::new("FLAG").unwrap(),
        collectors: Default::default(),
        ingress_mode: newhatch_analyzer::config::IngressMode::Local,
    };
    (
        router(
            state,
            AuthConfig::parse("team".into(), hash(), ttl, secure).unwrap(),
        ),
        dir,
    )
}

async fn call(
    app: &Router,
    peer: &str,
    method: &str,
    path: &str,
    cookie: &str,
    body: &str,
    headers: &[(&str, &str)],
) -> Response {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("host", "localhost:8080")
        .header("content-type", "application/json");
    if !peer.is_empty() {
        request = request.extension(ConnectInfo(peer.parse::<SocketAddr>().unwrap()));
    }
    if !cookie.is_empty() {
        request = request.header("cookie", cookie);
    }
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    app.clone()
        .oneshot(request.body(Body::from(body.to_owned())).unwrap())
        .await
        .unwrap()
}

const LOGIN: &str = r#"{"username":"team","password":"test-only-password"}"#;

async fn login(app: &Router, cookie: &str) -> String {
    let response = call(
        app,
        "127.0.0.1:1234",
        "POST",
        "/api/auth/login",
        cookie,
        LOGIN,
        &[],
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    response.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .to_owned()
}

fn cookie(header: &str) -> &str {
    header.split(';').next().unwrap()
}

#[tokio::test]
async fn locality_precedes_auth_and_headers_cannot_override_it() {
    let (app, _dir) = app("86400", "false");
    let session = login(&app, "").await;
    for peer in ["127.0.0.1:1", "127.10.20.30:1", "[::1]:1"] {
        assert_eq!(
            call(&app, peer, "GET", "/api/sessions", "", "", &[])
                .await
                .status(),
            401
        );
        assert_eq!(
            call(
                &app,
                peer,
                "GET",
                "/api/sessions",
                cookie(&session),
                "",
                &[]
            )
            .await
            .status(),
            200
        );
    }
    let forged = [
        ("x-forwarded-for", "127.0.0.1"),
        ("x-real-ip", "127.0.0.1"),
        ("forwarded", "for=127.0.0.1"),
        ("cf-connecting-ip", "127.0.0.1"),
        ("true-client-ip", "127.0.0.1"),
        ("client-ip", "127.0.0.1"),
        ("x-client-ip", "127.0.0.1"),
        ("x-cluster-client-ip", "127.0.0.1"),
        ("forwarded-for", "127.0.0.1"),
        ("forwarded-for-ip", "127.0.0.1"),
        ("via", "127.0.0.1"),
    ];
    for peer in [
        "192.168.1.100:1",
        "172.18.0.5:1",
        "0.0.0.0:1",
        "[::]:1",
        "[::ffff:127.0.0.1]:1",
        "",
    ] {
        for headers in [&[][..], &forged[..]] {
            assert_eq!(
                call(
                    &app,
                    peer,
                    "GET",
                    "/api/sessions",
                    cookie(&session),
                    "",
                    headers
                )
                .await
                .status(),
                403
            );
            assert_eq!(
                call(&app, peer, "POST", "/api/auth/login", "", LOGIN, headers)
                    .await
                    .status(),
                403
            );
        }
    }
}

#[tokio::test]
async fn every_data_route_and_future_route_is_protected() {
    let (app, _dir) = app("86400", "false");
    for (method, path) in [
        ("GET", "/api/sources"),
        ("POST", "/api/sources"),
        ("PUT", "/api/sources/1"),
        ("DELETE", "/api/sources/1"),
        ("GET", "/api/sessions"),
        ("GET", "/api/sessions/1"),
        ("GET", "/api/sessions/1/payload/c2s"),
        ("GET", "/api/sessions/1/payload/s2c"),
        ("GET", "/api/sessions/1/flag-matches"),
        ("GET", "/api/auth/me"),
        ("POST", "/api/auth/logout"),
        ("GET", "/api/future-route"),
        ("HEAD", "/api/health"),
        ("POST", "/api/health"),
        ("GET", "/api/auth/login"),
    ] {
        assert_eq!(
            call(&app, "127.0.0.1:1", method, path, "", "{}", &[])
                .await
                .status(),
            401,
            "{method} {path}"
        );
        assert_eq!(
            call(&app, "192.168.1.100:1", method, path, "", "{}", &[])
                .await
                .status(),
            403,
            "{method} {path}"
        );
    }
}

#[tokio::test]
async fn health_is_public_minimal_and_cors_is_absent() {
    let (app, _dir) = app("86400", "false");
    let response = call(
        &app,
        "192.168.1.100:1",
        "GET",
        "/api/health",
        "",
        "",
        &[("origin", "https://evil.example")],
    )
    .await;
    assert_eq!(response.status(), 200);
    assert!(!response
        .headers()
        .contains_key("access-control-allow-origin"));
    assert!(!response.headers().contains_key("set-cookie"));
    assert_eq!(
        to_bytes(response.into_body(), 1024).await.unwrap(),
        r#"{"status":"ok"}"#
    );
}

#[tokio::test]
async fn login_errors_are_generic_and_cookies_rotate_and_logout_revokes() {
    let (app, _dir) = app("86400", "false");
    for body in [
        r#"{"username":"wrong","password":"test-only-password"}"#,
        r#"{"username":"team","password":"wrong"}"#,
    ] {
        let response = call(
            &app,
            "127.0.0.1:1",
            "POST",
            "/api/auth/login",
            "",
            body,
            &[],
        )
        .await;
        assert_eq!(response.status(), 401);
        assert_eq!(
            to_bytes(response.into_body(), 1024).await.unwrap(),
            r#"{"error":"unauthorized"}"#
        );
    }
    let first = login(&app, "newhatch_session=attacker-chosen").await;
    for attribute in ["HttpOnly", "SameSite=Strict", "Path=/", "Max-Age="] {
        assert!(first.contains(attribute));
    }
    assert!(!first.contains("Secure"));
    let second = login(&app, cookie(&first)).await;
    assert_ne!(cookie(&first), cookie(&second));
    assert_eq!(
        call(
            &app,
            "127.0.0.1:1",
            "GET",
            "/api/auth/me",
            cookie(&first),
            "",
            &[]
        )
        .await
        .status(),
        401
    );
    let me = call(
        &app,
        "127.0.0.1:1",
        "GET",
        "/api/auth/me",
        cookie(&second),
        "",
        &[],
    )
    .await;
    assert_eq!(me.status(), 200);
    assert_eq!(
        to_bytes(me.into_body(), 1024).await.unwrap(),
        r#"{"authenticated":true,"username":"team"}"#
    );
    let response = call(
        &app,
        "127.0.0.1:1",
        "POST",
        "/api/auth/logout",
        cookie(&second),
        "",
        &[],
    )
    .await;
    assert_eq!(response.status(), 204);
    assert!(response.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .contains("Max-Age=0"));
    assert_eq!(
        call(
            &app,
            "127.0.0.1:1",
            "GET",
            "/api/sessions",
            cookie(&second),
            "",
            &[]
        )
        .await
        .status(),
        401
    );
}

#[tokio::test]
async fn absolute_ttl_and_secure_cookie() {
    let (app, _dir) = app("2", "true");
    let session = login(&app, "").await;
    assert!(session.contains("Secure"));
    assert!(session.contains("Max-Age=1") || session.contains("Max-Age=2"));
    assert_eq!(
        call(
            &app,
            "127.0.0.1:1",
            "GET",
            "/api/auth/me",
            cookie(&session),
            "",
            &[]
        )
        .await
        .status(),
        200
    );
    tokio::time::sleep(std::time::Duration::from_millis(2100)).await;
    assert_eq!(
        call(
            &app,
            "127.0.0.1:1",
            "GET",
            "/api/auth/me",
            cookie(&session),
            "",
            &[]
        )
        .await
        .status(),
        401
    );
}

#[tokio::test]
async fn restart_and_unknown_sessions_require_login() {
    let (first, _dir1) = app("86400", "false");
    let session = login(&first, "").await;
    let (second, _dir2) = app("86400", "false");
    for session in [
        cookie(&session),
        "newhatch_session=invalid",
        "newhatch_session=AAAAAAAAAAAAAAAAAAAAAA",
    ] {
        assert_eq!(
            call(
                &second,
                "127.0.0.1:1",
                "GET",
                "/api/auth/me",
                session,
                "",
                &[]
            )
            .await
            .status(),
            401
        );
    }
}

#[test]
fn process_fails_closed_without_credentials_or_with_nonlocal_listener() {
    let executable = env!("CARGO_BIN_EXE_newhatch-analyzer");
    for (username, password_hash, address) in [
        ("", "", "127.0.0.1:3000"),
        ("team", "plaintext", "127.0.0.1:3000"),
        ("team", hash().as_str(), "0.0.0.0:3000"),
    ] {
        let output = std::process::Command::new(executable)
            .env_clear()
            .env("AUTH_USERNAME", username)
            .env("AUTH_PASSWORD_HASH", password_hash)
            .env("LISTEN_ADDR", address)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(!String::from_utf8_lossy(&output.stderr).contains("$argon2"));
    }
    assert!(!std::process::Command::new(executable)
        .env_clear()
        .output()
        .unwrap()
        .status
        .success());
}

#[tokio::test]
async fn cross_origin_mutations_are_rejected() {
    let (app, _dir) = app("86400", "false");
    for headers in [
        vec![("origin", "http://evil.example")],
        vec![("origin", "null")],
        vec![("sec-fetch-site", "cross-site")],
        vec![("sec-fetch-site", "same-site")],
    ] {
        assert_eq!(
            call(
                &app,
                "127.0.0.1:1",
                "POST",
                "/api/auth/login",
                "",
                LOGIN,
                &headers
            )
            .await
            .status(),
            403
        );
    }
    assert_eq!(
        call(
            &app,
            "127.0.0.1:1",
            "POST",
            "/api/auth/login",
            "",
            LOGIN,
            &[
                ("origin", "http://localhost:8080"),
                ("sec-fetch-site", "same-origin")
            ]
        )
        .await
        .status(),
        200
    );
}

#[test]
fn config_rejects_missing_malformed_or_unsafe_values() {
    assert!(AuthConfig::parse("".into(), hash(), "86400", "false").is_err());
    for value in [
        "".to_owned(),
        "plaintext".into(),
        hash().replace("argon2id", "argon2i"),
        hash().replace("v=19", "v=16"),
        hash().replace("m=19456", "m=8"),
        "$argon2id$v=19$m=19456,t=2,p=1".into(),
    ] {
        assert!(AuthConfig::parse("team".into(), value, "86400", "false").is_err());
    }
    for ttl in ["0", "-1", "604801", "invalid"] {
        assert!(AuthConfig::parse("team".into(), hash(), ttl, "false").is_err());
    }
    assert!(AuthConfig::parse("team".into(), hash(), "86400", "yes").is_err());
}
