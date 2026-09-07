# Active Tasks

Work currently identified as the next implementation tranche.

## Collector / Analyzer Split

Status: implemented and automatically verified on 2026-09-07. Actual two-host Linux/VPN deployment verification remains developer-owned Phase 5. The complete user specification remains in `newhatch-collector-analyzer-split.md`.

### Fixed Scope

- [x] Split immediately after `ClassifiedPacket`: collector captures, parses, classifies and forwards; analyzer owns worker routing, reassembly, flag/protocol analysis, storage, API and UI.
- [x] Add `crates/protocol` for versioned protobuf wire messages and shared domain types. Do not duplicate packet/flow/source DTOs.
- [x] Add lightweight `crates/collector` with cooked `AF_PACKET`, kernel BPF, in-memory Sources, bounded queues, persistent bidirectional transport, reconnect/backoff and counters.
- [x] Add analyzer collector ingress. Route by `CollectorId + FlowKey` so different collectors cannot merge, while retaining existing `FlowWorker` and session processing.
- [x] Keep analyzer SQLite as the Sources source of truth and push complete active source snapshots after handshake and after CRUD changes.
- [x] Support one expected collector operationally while keeping message identity and analyzer flow isolation multi-collector capable.
- [x] Add a sidebar Collectors view showing identity, connection state, peer, last activity, counters, queue depth and reconnect information available from the protocol.
- [x] Preserve local/monolithic ingress for migration and tests.
- [x] Keep collector memory bounded and diskless; drop with observable counters when disconnected/slow instead of blocking capture or growing without bound.
- [x] Add short split-deployment instructions to root README. Actual cross-host rollout remains developer-owned Phase 5.

### Planned Delivery

1. Packet sink abstraction and shared protocol crate.
2. Collector binary plus persistent framed protobuf transport.
3. Analyzer ingress, source synchronization and collector-aware worker/flow identity.
4. Collector status API and frontend view.
5. Unit/integration tests for serialization, routing, reassembly parity, source updates, disconnect/reconnect, overflow, approval and collector isolation.
6. Compose/Dockerfiles, environment examples and brief deployment documentation.

### Collector Admission Contract

- [x] Collector is configured with the receiver IP and port it connects to.
- [x] Receiver is configured with the expected collector source IP and rejects other peer IPs.
- [x] Do not add application-layer authentication in this tranche. Pre-shared-key authentication is deferred to backlog hardening.

## Authentication and API Access Control

Status: implemented with automated backend, CIDR, Docker-network and browser coverage (2026-09-07). Final reruns after the last small changes are blocked by the tool approval usage limit. Final ingress verification from an actual team machine on the target Linux host also remains pending. Configuration and implementation choices are in `docs/authentication.md`; test commands are in `test/auth/README.md`.

Updated network requirement: team clients may log in and use the protected API through nginx from explicitly configured CIDR subnets. Loopback remains allowed. The client allowlist is enforced at nginx ingress; the analyzer separately enforces local proxy-to-backend transport and authentication.

### Goal and Scope

Require login before the UI or protected API can be used. Keep the existing Rust/Axum/Tower, React/Vite, nginx and SQLite architecture. Use one installation-level user, Argon2id password verification and server-side sessions, preferably `tower-sessions`.

No external identity provider, registration, password reset, email flow, account management, roles or OAuth. Avoid JWT unless a concrete compatibility reason requires it. Do not store authentication tokens in localStorage/sessionStorage or rely on frontend state as authorization.

### Backend and Configuration

- [x] Load `AUTH_USERNAME`, `AUTH_PASSWORD_HASH` (Argon2id PHC hash) and configurable `AUTH_SESSION_TTL_SECONDS` (suggested default 86400; 12-24 hours is acceptable).
- [x] Add `AUTH_ALLOWED_SUBNETS` as a comma-separated IPv4/IPv6 CIDR allowlist, for example `AUTH_ALLOWED_SUBNETS=192.168.1.0/24,fd42:1234::/64`. Missing/empty means loopback clients only, never unrestricted access. Reject malformed CIDRs at startup; do not infer trust from private address ranges.
- [x] Fail fast when required auth configuration is missing or malformed. Never silently disable auth or provide default credentials.
- [x] Verify passwords using a cryptographically secure Argon2id implementation such as the `argon2` crate. Never persist plaintext passwords in source, frontend assets, images or SQLite.
- [x] Implement server-side sessions, preferably with `tower-sessions`; choose a compatible session store during implementation.
- [x] Set cookies with `HttpOnly`, `SameSite=Strict`, `Path=/`, configured `Max-Age`, and `Secure` when served over HTTPS. Frontend JavaScript must not need to read the cookie.
- [x] Create/regenerate session identity on successful login to prevent session fixation. Logout must invalidate server-side state and expire the cookie. Missing, expired or invalid sessions return 401.

### Route Contract

For browser-facing nginx, every application route except `GET /api/health` requires a client socket peer in loopback or `AUTH_ALLOWED_SUBNETS`, including the login page, assets and `/api/auth/login`. An allowed subnet permits attempting login; it never replaces credentials or a valid session. The table below describes the separate analyzer-side checks, where the actual peer is nginx, not the browser.

| Route | Actual loopback peer required | Authenticated session required | Result |
| --- | --- | --- | --- |
| `GET /api/health` | No | No | Minimal `{"status":"ok"}` only |
| `POST /api/auth/login` | Yes | No | Accept `{"username":"...","password":"..."}`; 200 plus session cookie on success; generic 401 for incorrect credentials |
| `GET /api/auth/me` | Yes | Yes | `{"authenticated":true,"username":"..."}`; otherwise 401 |
| `POST /api/auth/logout` | Yes | Yes | Invalidate session and expire/remove cookie |
| All other `/api/*` routes | Yes | Yes | Existing handler after both checks |

- [x] Define explicit public-health and local-only-login exceptions; apply locality/session middleware after all routers are merged (including method and path fallbacks). Future API routes must be protected by default unless an exception is explicitly documented.
- [x] Guard Sources CRUD, session listing/detail, directional payload retrieval and flag-match ranges in backend middleware, including direct API requests.
- [x] Check locality before authentication: non-local peers receive 403 even with valid credentials or a valid session. Login is the sole session-check exception among local-only routes.
- [x] Keep health output free of configuration, usernames, source/session counts, IPs, filesystem paths, internal errors, auth state and payload metadata. Public health does not require a public analyzer listener.

### Local Transport and Deployment

- [x] Generate nginx ingress allow/deny rules from validated `AUTH_ALLOWED_SUBNETS`, allowing loopback as well and denying all other clients. Use nginx's actual accepted TCP peer, without Real-IP/header rewriting or header-based authorization. Check the allowlist on every protected request, including requests with an already valid session.
- [ ] Preserve the client source IP at nginx ingress. Verify the deployed Docker/network topology actually exposes the team client's address there; do not allow a bridge/gateway subnet as a workaround if source addresses are collapsed by NAT. Keep nginx-to-analyzer communication local as described below.

- [x] Obtain the accepted connection's real `SocketAddr` through Axum `ConnectInfo<SocketAddr>` or equivalent connection metadata. Authorize only loopback IPs (`127.0.0.0/8` and `::1`), using standard-library helpers such as `is_loopback()`.
- [x] Reject unspecified addresses such as `0.0.0.0`, LAN/public IPs, Docker bridge/container addresses and other private ranges. Private networks are not loopback.
- [x] Never derive locality from HTTP headers, including `X-Forwarded-For`, `X-Real-IP`, `Forwarded`, `CF-Connecting-IP`, `True-Client-IP`, `Client-IP`, `X-Client-IP`, `X-Cluster-Client-IP`, `Forwarded-For`, `Forwarded-For-IP` or `Via`.
- [x] Restructure Compose/nginx so nginx actually connects to analyzer through loopback in a shared network namespace, preferably `127.0.0.1:<analyzer-port>`. A Unix domain socket is acceptable if supported cleanly with equivalent local-transport enforcement.
- [x] Bind analyzer HTTP to loopback/local transport and keep browser-facing nginx as the only externally exposed application HTTP service. Do not publish analyzer port `3000:3000`.
- [x] Do not allow Docker subnets or trust forwarded headers to compensate for the current bridge topology. A request passing through nginx is insufficient unless its actual analyzer-side transport is local.

### Frontend and Request Handling

- [x] Check `GET /api/auth/me` before loading Sources, Sessions or starting protected polling. Model loading, authenticated and unauthenticated states explicitly.
- [x] Show a minimal login page matching the existing UI with username, password and a login button. Conditional rendering is sufficient; do not add React Router solely for auth.
- [x] On successful login, let the browser store the cookie, set authenticated state and load normal data. Show a generic invalid-credentials error on failure without backend details.
- [x] Keep requests same-origin through `/api/*` and nginx; use `credentials: "same-origin"` where explicit configuration is useful. Do not manually attach bearer tokens.
- [x] Handle 401 consistently across all protected requests: clear authenticated state, stop polling and show login when a session expires.
- [x] Remove `CorsLayer::permissive()`. Production should need no CORS layer; Vite development should use its local API proxy. Any necessary development CORS must use a narrow explicit allowlist and credentials policy, never wildcard origins.
- [x] Keep state changes on POST/PUT/DELETE, never destructive GET routes. Use `SameSite=Strict`; reevaluate CSRF protection if transport/origin requirements change. Locality is measured on the proxy-to-analyzer connection and is not a substitute for browser-origin protection.

### Errors and Logging

- [x] Return generic 401 for missing/expired/invalid authentication or incorrect credentials, and 403 for a non-local TCP peer. Never disclose whether username or password was incorrect or reveal the configured username in errors.
- [x] Allow useful login success/failure, logout, expired-session and rejected-peer events. Use the actual peer IP for locality logs.
- [x] Never log plaintext passwords, password hashes, session cookies/IDs, `AUTH_SESSION_SECRET`, or complete Authorization/Cookie headers.

### Acceptance Tests

#### Team Access Through nginx

- [x] With `AUTH_ALLOWED_SUBNETS=192.168.1.0/24`, client `192.168.1.100` can reach login; correct credentials return 200 plus cookie, incorrect credentials return 401, and protected Sessions returns 200 only with a valid session (otherwise 401).
- [x] With that allowlist, clients `192.168.2.100` and `172.18.0.5` receive 403 at nginx even with correct credentials or a valid session. Forged forwarded/client-IP headers containing an allowed address do not change the result.
- [x] Verify multiple CIDRs, IPv6 CIDRs, addresses at subnet boundaries, and loopback. Missing/empty configuration permits only loopback; malformed configuration fails startup.
- [x] Verify team-subnet access end to end through deployed nginx with the actual client source address preserved, including login, me, logout and protected API calls. Public minimal health remains available outside the allowlist.

#### Direct Analyzer Peer Checks

The following remote-peer rejection cases apply to direct analyzer connections, not to authorized team clients using nginx. A loopback-only listener may reject external connections before HTTP; test middleware 403 behavior separately with connection metadata.

- [x] Loopback peers `127.0.0.1` and `::1` pass locality checks; the entire IPv4 loopback range is recognized.
- [x] Peers `192.168.1.100` and `172.18.0.5` receive 403.
- [x] A remote peer remains rejected with forged `X-Forwarded-For: 127.0.0.1`, `X-Real-IP: 127.0.0.1` or `Forwarded: for=127.0.0.1`; no forwarded/client-IP header affects authorization.
- [x] Local `GET /api/sessions` without a session returns 401, with a valid session returns 200, and from a remote peer with that same valid session returns 403.
- [x] Remote login returns 403; local incorrect credentials return generic 401; local correct credentials return 200 and the configured session cookie.
- [x] Remote unauthenticated `GET /api/health` passes the router and returns only minimal health data.
- [x] Verify cookie attributes, fixation prevention, logout invalidation, expiration, configuration failure behavior and frontend startup/401 handling.
- [x] Verify actual deployment connectivity: nginx reaches analyzer over local transport, direct external analyzer access is unavailable, and permissive CORS is removed.
- [x] Update canonical docs, environment examples, runbook and agent memory with implemented auth behavior and verified commands when implementation is complete.

### Verification Record

- Rust: 12 existing tests plus 3 session-store tests and 9 auth integration tests.
- Python: 4 CIDR/config-generation tests.
- Docker: real IPv4/IPv6 team login; forbidden LAN/bridge clients with valid cookies and forged headers; minimal public health; direct backend unreachable. The subsequently added invalid-CIDR container-startup test still needs a successful rerun: its original Compose-run version conflicted with the running gateway's static IP; it now uses an independent network-less container.
- Browser: 2 Playwright scenarios covering startup gating, login errors, logout, JSON/payload 401, stopped polling, retry and desktop/mobile screenshots.
- Frontend production build and TypeScript checks passed; Rust formatting and Clippy checked.
- Final competition-network/host-network source-IP verification remains open. The automated network fixture uses a shared nginx/analyzer namespace and direct client addresses, not host-port NAT.

### Resume Checklist

- [ ] Rerun Rust tests, formatting and Clippy after the last `AuthConfig::from_env` error-redaction change and explicit `session.save()` before the successful login response. Earlier version passed 24 Rust tests; these last changes are not yet verified.
- [ ] Rerun `python3 test/auth/run_e2e.py` with the independent invalid-CIDR startup check. Earlier real-network login/logout and peer-isolation scenarios passed.
- [ ] Stop/remove the task-owned `newhatch-auth-rust-check` container after verification. It may be stopped or running after repeated OrbStack shutdowns; do not touch unrelated containers.
- [ ] Start and verify a local preview if desired. The attempted `newhatch-auth-preview-ui` start failed because Docker was unavailable; no preview URL was verified. Production `.env` and data were not changed.

Docker execution was blocked by the automatic approval review's usage limit, not by a finding that the tests are unsafe. Do not bypass that rejection. Resume Docker operations only after approval/access is restored.

### Completion Invariants

Health is the only API route exempt from network/authentication checks. At nginx ingress, loopback and explicitly configured team CIDRs may access the application; all other clients are rejected even with valid credentials/sessions. At analyzer ingress, local transport is still required. Login requires the applicable network checks but no preexisting session; every other API route also requires an authenticated server-side session. Headers and frontend state cannot bypass these checks. Missing required credentials fail startup; missing subnet configuration means loopback-only access.

## Other Active Work

- [ ] Add crash-tail scan/recovery for the latest segment.
- [ ] Add WebSocket frame extraction after HTTP upgrade.
- [ ] Add API integration tests and broaden catalog/storage integration coverage.
- [ ] Exercise live capture on the target Linux/vulnbox network layout.
