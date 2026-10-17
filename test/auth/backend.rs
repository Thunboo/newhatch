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
use std::net::SocketAddr;
use tower::ServiceExt;

fn app(expiration: &str, secure: &str) -> (Router, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let state = ApiState {
        catalog: Catalog::open(dir.path().join("index.sqlite")).unwrap(),
        data_dir: dir.path().to_owned(),
        source_revision: tokio::sync::watch::channel(0).0,
        flag_regex: regex::bytes::Regex::new("FLAG").unwrap(),
        collectors: Default::default(),
        analyzer_mode: newhatch_analyzer::config::AnalyzerMode::Local,
    };
    (
        router(
            state,
            AuthConfig::parse(
                "team".into(),
                "test-only-password".into(),
                expiration,
                secure,
            )
            .unwrap(),
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
    let (app, _dir) = app("86400s", "false");
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
    let (app, _dir) = app("86400s", "false");
    for (method, path) in [
        ("GET", "/api/sources"),
        ("POST", "/api/sources"),
        ("PUT", "/api/sources/1"),
        ("DELETE", "/api/sources/1"),
        ("GET", "/api/collectors"),
        ("GET", "/api/payload-search/limits"),
        ("GET", "/api/sessions"),
        ("GET", "/api/sessions/1"),
        ("GET", "/api/sessions/1/payload/c2s"),
        ("GET", "/api/sessions/1/payload/s2c"),
        ("GET", "/api/sessions/1/flag-matches"),
        ("GET", "/api/sessions/1/http-events"),
        ("GET", "/api/attack-rules"),
        ("POST", "/api/attack-rules"),
        ("PUT", "/api/attack-rules/1"),
        ("DELETE", "/api/attack-rules/1"),
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

fn payload_query(terms: &[String]) -> String {
    encode_query(&serde_json::to_string(terms).unwrap())
}

fn encode_query(text: &str) -> String {
    use std::fmt::Write;
    text.bytes().fold(String::new(), |mut value, byte| {
        write!(value, "%{byte:02X}").unwrap();
        value
    })
}

#[tokio::test]
async fn both_feeds_validate_payload_lists_using_advertised_limits() {
    use newhatch_analyzer::payload_search::{MAX_PAYLOAD_TERMS, MAX_PAYLOAD_TERM_CHARS};
    let (app, _dir) = app("86400s", "false");
    let session = login(&app, "").await;
    let session = cookie(&session);
    let response = call(
        &app,
        "127.0.0.1:1",
        "GET",
        "/api/payload-search/limits",
        session,
        "",
        &[],
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let limits: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 1024).await.unwrap()).unwrap();
    assert_eq!(limits["max_terms"], MAX_PAYLOAD_TERMS);
    assert_eq!(limits["max_term_chars"], MAX_PAYLOAD_TERM_CHARS);
    for feed in ["sessions", "chains"] {
        for terms in [
            vec![],
            vec!["same".into(); MAX_PAYLOAD_TERMS],
            vec!["🦀".repeat(MAX_PAYLOAD_TERM_CHARS)],
            vec!["  quoted,\"term\" OR .*  ".into()],
        ] {
            let path = format!("/api/{feed}?payload={}", payload_query(&terms));
            assert_eq!(
                call(&app, "127.0.0.1:1", "GET", &path, session, "", &[])
                    .await
                    .status(),
                StatusCode::OK,
                "{path}"
            );
        }
        for terms in [
            vec!["same".into(); MAX_PAYLOAD_TERMS + 1],
            vec!["🦀".repeat(MAX_PAYLOAD_TERM_CHARS + 1)],
            vec![" \n".into()],
        ] {
            let path = format!("/api/{feed}?payload={}", payload_query(&terms));
            let response = call(&app, "127.0.0.1:1", "GET", &path, session, "", &[]).await;
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            let body: serde_json::Value =
                serde_json::from_slice(&to_bytes(response.into_body(), 1024).await.unwrap())
                    .unwrap();
            assert!(body["error"].is_string());
        }
        for value in ["scalar", "%22string%22", "%5B1%5D", "%5B"] {
            let path = format!("/api/{feed}?payload={value}");
            assert_eq!(
                call(&app, "127.0.0.1:1", "GET", &path, session, "", &[])
                    .await
                    .status(),
                StatusCode::BAD_REQUEST
            );
        }
    }
}

#[tokio::test]
async fn payload_and_searches_stored_sessions_with_bounded_continuation_and_chain_context() {
    use newhatch_analyzer::{
        domain::{CompletedSession, FlagDirection, SessionProtocol, SourceInput},
        storage::{start_writer, SessionFilter},
    };
    let (app, dir) = app("86400s", "false");
    let catalog = Catalog::open(dir.path().join("index.sqlite")).unwrap();
    catalog
        .create_source(&SourceInput {
            name: "web".into(),
            port: 8080,
            enabled: true,
        })
        .unwrap();
    let writer = start_writer(
        catalog.clone(),
        dir.path().to_owned(),
        std::time::Duration::from_secs(1800),
        3,
        3000,
    )
    .unwrap();
    // The old matching session lies beyond the first 2,000 search candidates.
    // The last two sessions share a chain, but split the required terms.
    for id in 1..=2003 {
        let (c2s, s2c, flag) = match id {
            1 => (b"POST alpha".to_vec(), b"beta FLAG".to_vec(), true),
            2002 => (b"alpha".to_vec(), vec![], false),
            2003 => (vec![], b"beta FLAG".to_vec(), true),
            _ => (b"noise".to_vec(), vec![], false),
        };
        assert!(writer.try_store(CompletedSession {
            collector_id: "test".into(),
            first_payload_c2s_at: Some(id),
            first_payload_s2c_at: Some(id),
            source_id: 1,
            started_at: id * 500_000,
            ended_at: id * 500_000 + 100,
            client_ip: if id >= 2002 { "10.0.0.2" } else { "10.0.0.3" }
                .parse()
                .unwrap(),
            client_port: 40000,
            server_ip: "10.0.0.1".parse().unwrap(),
            server_port: 8080,
            protocol: SessionProtocol::Http,
            c2s,
            s2c,
            flag_direction: if flag {
                FlagDirection::S2c
            } else {
                FlagDirection::None
            },
            flag_count: usize::from(flag),
            incomplete: false,
            http: Default::default(),
        }));
    }
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            if catalog
                .list_sessions(&SessionFilter {
                    limit: 1,
                    ..Default::default()
                })
                .unwrap()
                .first()
                .is_some_and(|s| s.id == 2003)
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    drop(writer);
    let session = login(&app, "").await;
    let session = cookie(&session);
    let terms = payload_query(&["alpha".into(), "beta".into(), "alpha".into()]);
    let app = &app;
    let get = |path: String| async move {
        let response = call(app, "127.0.0.1:1", "GET", &path, session, "", &[]).await;
        assert_eq!(response.status(), StatusCode::OK);
        serde_json::from_slice::<serde_json::Value>(
            &to_bytes(response.into_body(), 100_000).await.unwrap(),
        )
        .unwrap()
    };
    let first = get(format!("/api/sessions?payload={terms}")).await;
    assert_eq!(first["items"], serde_json::json!([]));
    assert_eq!(first["next_cursor"], 4);
    let second = get(format!("/api/sessions?payload={terms}&cursor=4")).await;
    assert_eq!(second["items"][0]["id"], 1);
    assert_eq!(second["next_cursor"], serde_json::Value::Null);
    let chains = get(format!("/api/chains?payload={terms}&contains_flag=true")).await;
    assert_eq!(chains["items"].as_array().unwrap().len(), 1);
    assert_eq!(chains["items"][0]["id"], 1);
    assert_eq!(chains["items"][0]["session_count"], 2001);
    let no_match = get(format!("/api/sessions?payload={terms}&contains_flag=false")).await;
    assert_eq!(no_match["items"], serde_json::json!([]));
}

#[tokio::test]
async fn rejected_mutation_does_not_change_the_catalog() {
    let (app, dir) = app("86400s", "false");
    let response = call(
        &app,
        "127.0.0.1:1",
        "POST",
        "/api/sources",
        "",
        r#"{"name":"unauthorized","port":31337,"enabled":true}"#,
        &[],
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let catalog = Catalog::open(dir.path().join("index.sqlite")).unwrap();
    assert!(catalog.list_sources(false).unwrap().is_empty());
}

#[tokio::test]
async fn health_is_public_minimal_and_cors_is_absent() {
    let (app, _dir) = app("86400s", "false");
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
    let (app, _dir) = app("86400s", "false");
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
    let (app, _dir) = app("2s", "true");
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
    let (first, _dir1) = app("86400s", "false");
    let session = login(&first, "").await;
    let (second, _dir2) = app("86400s", "false");
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
    for (username, password, address) in [
        ("", "", "127.0.0.1:3000"),
        ("team", "", "127.0.0.1:3000"),
        ("team", "test-only-password", "0.0.0.0:3000"),
    ] {
        let output = std::process::Command::new(executable)
            .env_clear()
            .env("NEWHATCH_USERNAME", username)
            .env("NEWHATCH_PASSWORD", password)
            .env("SESSION_EXPIRACY", "86400s")
            .env("LISTEN_ADDR", address)
            .output()
            .unwrap();
        assert!(!output.status.success());
        if !password.is_empty() {
            assert!(!String::from_utf8_lossy(&output.stderr).contains(password));
        }
    }
    assert!(!std::process::Command::new(executable)
        .env_clear()
        .output()
        .unwrap()
        .status
        .success());
    let legacy = std::process::Command::new(executable)
        .env_clear()
        .env("USERNAME", "windows-account")
        .env("PASSWORD", "legacy-password")
        .output()
        .unwrap();
    assert!(!legacy.status.success());
    let stderr = String::from_utf8_lossy(&legacy.stderr);
    assert!(stderr.contains("NEWHATCH_USERNAME"));
    assert!(!stderr.contains("legacy-password"));
}

#[tokio::test]
async fn cross_origin_mutations_are_rejected() {
    let (app, _dir) = app("86400s", "false");
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
    assert!(AuthConfig::parse("".into(), "password".into(), "86400s", "false").is_err());
    for password in ["".to_owned(), "x".repeat(1025)] {
        assert!(AuthConfig::parse("team".into(), password, "86400s", "false").is_err());
    }
    for expiration in ["0s", "-1s", "604801s", "86400", "invalid"] {
        assert!(AuthConfig::parse("team".into(), "password".into(), expiration, "false").is_err());
    }
    for expiration in ["86400s", "1440m", "24h", "1d"] {
        assert!(AuthConfig::parse("team".into(), "password".into(), expiration, "false").is_ok());
    }
    assert!(AuthConfig::parse("team".into(), "password".into(), "86400s", "yes").is_err());
}

#[tokio::test]
async fn chains_share_auth_boundary_and_validate_filters_and_cursors() {
    let (app, _dir) = app("86400s", "false");
    for path in ["/api/chains", "/api/chains/1/sessions"] {
        assert_eq!(
            call(&app, "127.0.0.1:1", "GET", path, "", "", &[])
                .await
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }
    let session = login(&app, "").await;
    let session = cookie(&session);
    let response = call(
        &app,
        "127.0.0.1:1",
        "GET",
        "/api/chains?source_id=1&contains_flag=true&protocol=http&limit=10",
        session,
        "",
        &[],
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 1024).await.unwrap();
    let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(value["items"], serde_json::json!([]));
    for path in [
        "/api/chains?protocol=invalid",
        "/api/chains?chain_cursor=invalid",
        "/api/chains/1/sessions?snapshot_id=-1",
        "/api/chains/1/sessions?cursor=invalid",
    ] {
        assert_eq!(
            call(&app, "127.0.0.1:1", "GET", path, session, "", &[])
                .await
                .status(),
            StatusCode::BAD_REQUEST,
            "{path}"
        );
    }
    assert_eq!(
        call(
            &app,
            "127.0.0.1:1",
            "GET",
            "/api/chains/1/sessions",
            session,
            "",
            &[]
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn buckets_classify_more_than_2000_members_per_chain_and_per_page() {
    use newhatch_analyzer::{
        domain::{CompletedSession, FlagDirection, SessionProtocol, SourceInput},
        storage::{start_writer, SessionFilter},
    };
    let (app, dir) = app("86400s", "false");
    let catalog = Catalog::open(dir.path().join("index.sqlite")).unwrap();
    for (name, port) in [("long-chain", 8080), ("many-chains", 9000)] {
        catalog
            .create_source(&SourceInput {
                name: name.into(),
                port,
                enabled: true,
            })
            .unwrap();
    }
    let writer = start_writer(
        catalog.clone(),
        dir.path().to_owned(),
        std::time::Duration::from_secs(1800),
        3,
        5000,
    )
    .unwrap();
    for (source_id, chain_count, member_count) in [(1, 1, 2101), (2, 100, 21)] {
        for chain in 0..chain_count {
            for member in 0..member_count {
                let last = member + 1 == member_count;
                let target = if last { "/hit" } else { "/noise" };
                let started_at = chain * 4_000_000 + member * 100_000;
                assert!(writer.try_store(CompletedSession {
                    collector_id: "test".into(),
                    source_id,
                    started_at,
                    ended_at: started_at + 1,
                    first_payload_c2s_at: None,
                    first_payload_s2c_at: None,
                    client_ip: "10.0.0.2".parse().unwrap(),
                    client_port: 40000,
                    server_ip: "10.0.0.1".parse().unwrap(),
                    server_port: if source_id == 1 { 8080 } else { 9000 },
                    protocol: SessionProtocol::Http,
                    c2s: format!("GET {target} HTTP/1.1\r\nHost: test\r\n\r\n").into_bytes(),
                    s2c: b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n".to_vec(),
                    flag_direction: if last {
                        FlagDirection::C2s
                    } else {
                        FlagDirection::None
                    },
                    flag_count: usize::from(last),
                    incomplete: false,
                    http: Default::default(),
                }));
            }
        }
    }
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while catalog
            .list_sessions(&SessionFilter {
                limit: 1,
                ..Default::default()
            })
            .unwrap()
            .first()
            .is_none_or(|s| s.id != 4201)
        {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    drop(writer);
    let session = login(&app, "").await;
    for (source_id, chain_count, member_count) in [(1, 1, 2101), (2, 100, 21)] {
        let mut rule_id = None;
        for (target, matching) in [("/hit", true), ("/missing", false)] {
            let rule = catalog
                .save_rule(
                    "team",
                    rule_id,
                    serde_json::from_value(serde_json::json!({
                        "source_id": source_id,
                        "label": "test",
                        "steps": [[{"field": "path", "op": "equals", "value": target}]]
                    }))
                    .unwrap(),
                )
                .unwrap()
                .unwrap();
            rule_id = Some(rule.id);
            for feed in ["sessions", "chains"] {
                for bucket in ["known", "unknown"] {
                    // Selecting one member per chain forces standalone browsing
                    // to classify 100 distinct chains instead of reusing one result.
                    let path = format!("/api/{feed}?source_id={source_id}&contains_flag=true&bucket={bucket}&limit=100");
                    let response =
                        call(&app, "127.0.0.1:1", "GET", &path, cookie(&session), "", &[]).await;
                    assert_eq!(response.status(), StatusCode::OK, "{path}, {target}");
                    let page: serde_json::Value = serde_json::from_slice(
                        &to_bytes(response.into_body(), 1_000_000).await.unwrap(),
                    )
                    .unwrap();
                    let items = page["items"].as_array().unwrap();
                    let accepted = matching == (bucket == "known");
                    assert_eq!(items.len(), if accepted { chain_count } else { 0 });
                    for item in items {
                        if feed == "chains" {
                            assert_eq!(item["session_count"], member_count);
                        }
                        let matches = item["known_matches"].as_array().unwrap();
                        assert_eq!(matches.len(), usize::from(matching));
                        if matching {
                            assert_eq!(matches[0]["rule_id"], rule.id);
                            let offset = if feed == "chains" {
                                member_count - 1
                            } else {
                                0
                            };
                            let expected = item["id"].as_i64().unwrap() + offset;
                            assert_eq!(matches[0]["events"][0]["session_id"], expected);
                            assert_eq!(matches[0]["events"][0]["transaction"], 0);
                        }
                    }
                }
            }
        }
    }
}

#[tokio::test]
async fn source_scoped_ordered_rules_reclassify_retained_chains_and_keep_raw_payloads() {
    use newhatch_analyzer::{
        domain::{CompletedSession, FlagDirection, SessionProtocol, SourceInput},
        storage::{start_writer, SessionFilter},
    };
    let (app, dir) = app("86400s", "false");
    let catalog = Catalog::open(dir.path().join("index.sqlite")).unwrap();
    for (name, port) in [("web", 8080), ("other", 9000)] {
        catalog
            .create_source(&SourceInput {
                name: name.into(),
                port,
                enabled: true,
            })
            .unwrap();
    }
    let writer = start_writer(
        catalog.clone(),
        dir.path().to_owned(),
        std::time::Duration::from_secs(1800),
        3,
        100,
    )
    .unwrap();
    let targets = [
        "/register",
        "/users?id=%2E%2E%2F%2E%2E%2F%2E%2E%2Fetc",
        "/profile",
        "/admin",
        "/users?id=../../../etc",
        "/profile",
    ];
    for (i, target) in targets.iter().enumerate() {
        let body = if i == 2 || i == 5 {
            r#"{"result":"root: %46%4C%41%47"}"#
        } else {
            "ok"
        };
        let request = format!("GET {target} HTTP/1.1\r\nHost: test\r\n\r\n").into_bytes();
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
        .into_bytes();
        assert!(writer.try_store(CompletedSession {
            collector_id: "test".into(),
            source_id: if i < 4 { 1 } else { 2 },
            started_at: i as i64 * 100_000,
            ended_at: i as i64 * 100_000 + 1,
            first_payload_c2s_at: None,
            first_payload_s2c_at: None,
            client_ip: "10.0.0.2".parse().unwrap(),
            client_port: 40000,
            server_ip: "10.0.0.1".parse().unwrap(),
            server_port: if i < 4 { 8080 } else { 9000 },
            protocol: SessionProtocol::Http,
            c2s: request,
            s2c: response,
            flag_direction: FlagDirection::None,
            flag_count: 0,
            incomplete: false,
            http: Default::default(),
        }));
    }
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while catalog
            .list_sessions(&SessionFilter {
                limit: 1,
                ..Default::default()
            })
            .unwrap()
            .first()
            .is_none_or(|s| s.id != 6)
        {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    drop(writer);
    catalog
        .refresh_flag_metadata(dir.path(), &regex::bytes::Regex::new("FLAG").unwrap())
        .unwrap();
    catalog
        .refresh_flag_metadata(dir.path(), &regex::bytes::Regex::new("FLAG").unwrap())
        .unwrap();
    let auth = login(&app, "").await;
    let auth = cookie(&auth);
    let get = |path: String| {
        let app = &app;
        async move {
            let response = call(app, "127.0.0.1:1", "GET", &path, auth, "", &[]).await;
            assert_eq!(response.status(), 200, "{path}");
            serde_json::from_slice::<serde_json::Value>(
                &to_bytes(response.into_body(), 100_000).await.unwrap(),
            )
            .unwrap()
        }
    };
    for path in ["/api/attack-rules", "/api/sessions/1/http-events"] {
        assert_eq!(
            call(&app, "127.0.0.1:1", "GET", path, "", "", &[])
                .await
                .status(),
            401
        );
    }
    assert_eq!(
        get("/api/sessions?bucket=known".into()).await["items"],
        serde_json::json!([])
    );
    assert_eq!(
        get("/api/chains?bucket=known&limit=1".into()).await["next_cursor"],
        serde_json::Value::Null
    );
    let definition = serde_json::json!({"source_id":1,"label":"Traversal","steps":[[
        {"field":"method","op":"equals","value":"GET"},
        {"field":"path","op":"equals","value":"/users"},
        {"field":"query","name":"id","op":"contains","value":"../../../"}
    ],[
        {"field":"path","op":"equals","value":"/profile"},
        {"field":"status","op":"equals","value":"200"},
        {"field":"response_body","op":"contains","value":"root:"}
    ]]});
    let response = call(
        &app,
        "127.0.0.1:1",
        "POST",
        "/api/attack-rules",
        auth,
        &definition.to_string(),
        &[],
    )
    .await;
    assert_eq!(response.status(), 201);
    let rule: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 10_000).await.unwrap()).unwrap();
    assert_eq!(rule["owner"], "team");
    let id = rule["id"].as_i64().unwrap();
    let empty_page = get("/api/sessions?bucket=known&limit=1".into()).await;
    assert_eq!(empty_page["items"], serde_json::json!([]));
    assert_eq!(empty_page["next_cursor"], 6);
    let resumed = get("/api/sessions?bucket=known&limit=1&cursor=5".into()).await;
    assert_eq!(resumed["items"][0]["id"], 4);
    let reopened = Catalog::open(dir.path().join("index.sqlite")).unwrap();
    assert_eq!(
        reopened.list_rules("team").unwrap()[0].input.label,
        "Traversal"
    );
    let known = get("/api/chains?bucket=known&source_id=1&contains_flag=true".into()).await;
    let version = known["items"][0]["known_matches"][0]["version"]
        .as_str()
        .unwrap();
    let selection = encode_query(&serde_json::json!([[id, version, [[0, 0]]]]).to_string());
    let highlight_url = format!("/api/sessions/2/rule-matches?selection={selection}");
    let explanation = get(highlight_url.clone()).await;
    assert_eq!(explanation["representation"], "readable-v1");
    assert_eq!(explanation["c2s"][0]["label"], "Traversal");
    assert!(!explanation["c2s"][0]["ranges"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(explanation["s2c"], serde_json::json!([]));
    assert_eq!(
        call(&app, "127.0.0.1:1", "GET", &highlight_url, "", "", &[])
            .await
            .status(),
        401
    );
    assert_eq!(
        call(
            &app,
            "127.0.0.1:1",
            "GET",
            "/api/sessions/2/rule-matches?selection=invalid",
            auth,
            "",
            &[]
        )
        .await
        .status(),
        400
    );
    assert_eq!(known["items"][0]["session_count"], 4);
    assert_eq!(
        known["items"][0]["known_matches"][0]["events"][0]["session_id"],
        2
    );
    assert_eq!(
        known["items"][0]["known_matches"][0]["events"][1]["session_id"],
        3
    );
    assert_eq!(
        get("/api/sessions?bucket=known&source_id=1".into()).await["items"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    assert_eq!(
        get("/api/sessions?bucket=unknown".into()).await["items"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let raw = call(
        &app,
        "127.0.0.1:1",
        "GET",
        "/api/sessions/3/payload/s2c",
        auth,
        "",
        &[],
    )
    .await;
    assert!(
        String::from_utf8_lossy(&to_bytes(raw.into_body(), 10_000).await.unwrap())
            .contains("%46%4C%41%47")
    );
    let flags = get("/api/sessions/3/flag-matches".into()).await;
    assert_eq!(flags["representation"], "readable-v1");
    assert_eq!(flags["s2c"][0]["value"], "FLAG");
    let search = payload_query(&["FLAG".into()]);
    assert_eq!(
        get(format!("/api/sessions?contains_flag=true&payload={search}")).await["items"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let events = get("/api/sessions/2/http-events".into()).await;
    assert_eq!(events[0]["path"], "/users");
    assert_eq!(events[0]["query_params"][0][1], "../../../etc");
    let mut invalid = definition.clone();
    invalid["steps"][0][0]["op"] = "regex".into();
    invalid["steps"][0][0]["value"] = "[".into();
    assert_eq!(
        call(
            &app,
            "127.0.0.1:1",
            "PUT",
            &format!("/api/attack-rules/{id}"),
            auth,
            &invalid.to_string(),
            &[]
        )
        .await
        .status(),
        400
    );
    let mut changed = definition.clone();
    // A different identity's rule is neither listed nor editable through this login.
    let other_rule = catalog
        .save_rule(
            "other-user",
            None,
            serde_json::from_value(definition.clone()).unwrap(),
        )
        .unwrap()
        .unwrap();
    let other_version = newhatch_analyzer::attack_rules::CompiledRule::new(other_rule.clone())
        .unwrap()
        .version();
    let selection =
        encode_query(&serde_json::json!([[other_rule.id, other_version, [[0, 0]]]]).to_string());
    assert_eq!(
        call(
            &app,
            "127.0.0.1:1",
            "GET",
            &format!("/api/sessions/2/rule-matches?selection={selection}"),
            auth,
            "",
            &[]
        )
        .await
        .status(),
        409
    );
    assert_eq!(
        get("/api/attack-rules".into())
            .await
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        call(
            &app,
            "127.0.0.1:1",
            "DELETE",
            &format!("/api/attack-rules/{}", other_rule.id),
            auth,
            "",
            &[]
        )
        .await
        .status(),
        404
    );
    let duplicate = catalog
        .save_rule(
            "team",
            None,
            serde_json::from_value(definition.clone()).unwrap(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        get("/api/chains?bucket=known&source_id=1".into()).await["items"][0]["known_matches"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    changed["enabled"] = false.into();
    assert_eq!(
        call(
            &app,
            "127.0.0.1:1",
            "PUT",
            &format!("/api/attack-rules/{id}"),
            auth,
            &changed.to_string(),
            &[]
        )
        .await
        .status(),
        200
    );
    // Explanations from the previous definition must not be reused.
    assert_eq!(
        call(&app, "127.0.0.1:1", "GET", &highlight_url, auth, "", &[])
            .await
            .status(),
        409
    );
    // Disabling one match leaves the other label active; removing the final
    // active rule restores the retained chain to Unknown.
    assert_eq!(
        get("/api/sessions?bucket=known&source_id=1".into()).await["items"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    assert!(catalog.delete_rule("team", duplicate.id).unwrap());
    assert_eq!(
        get("/api/sessions?bucket=unknown".into()).await["items"]
            .as_array()
            .unwrap()
            .len(),
        6
    );
    assert_eq!(
        call(
            &app,
            "127.0.0.1:1",
            "DELETE",
            &format!("/api/attack-rules/{id}"),
            auth,
            "",
            &[]
        )
        .await
        .status(),
        204
    );
    assert!(catalog.list_rules("team").unwrap().is_empty());
    assert_eq!(
        get("/api/sessions?bucket=known".into()).await["items"],
        serde_json::json!([])
    );
}
