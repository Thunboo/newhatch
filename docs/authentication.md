# Authentication and Team Access

One installation-level user is configured through the environment. This is a limited, user-approved addition to the MVP, not an account-management or RBAC system.

## Configuration

| Variable | Policy |
| --- | --- |
| `AUTH_USERNAME` | Required, nonblank, at most 128 bytes |
| `AUTH_PASSWORD_HASH` | Required Argon2id v19 PHC hash; no plaintext password |
| `AUTH_SESSION_TTL_SECONDS` | Absolute lifetime from login; default 86400, accepted range 1..604800 |
| `AUTH_ALLOWED_SUBNETS` | nginx ingress IPv4/IPv6 CIDRs separated by commas; empty/missing allows loopback only |
| `AUTH_COOKIE_SECURE` | Default `false` for the existing HTTP deployment; set `true` for HTTPS |
| `LISTEN_ADDR` | Analyzer defaults to `127.0.0.1:3000`; non-loopback addresses fail startup |
| `FRONTEND_PORT` | nginx listener, default 8080 |

Generate a hash without putting the password in shell arguments/history:

```bash
docker compose build analyzer
docker compose run --rm --no-deps --entrypoint hash-password analyzer
```

The command prompts twice with echo disabled and prints `AUTH_PASSWORD_HASH='...'`. Use that single-quoted assignment in `.env`: the quotes prevent Compose from interpreting the `$` characters. Set your own `AUTH_USERNAME` and team CIDRs. Example allowlist: `192.168.1.0/24,fd42:1234::/64`.

The analyzer rejects missing credentials, malformed PHC values and unsupported parameters. Allowed Argon2 costs are memory 19456..262144 KiB, iterations 2..10, parallelism 1..8, with at least 16 hash bytes. The supplied generator uses Argon2id defaults. nginx rejects invalid CIDRs, including non-canonical network addresses such as `192.168.1.100/24`; use `192.168.1.0/24` instead. Changing `.env` requires container recreation, not just `docker compose restart`.

## Two Independent Checks

```text
team browser -> nginx (actual client IP: loopback or explicit CIDR)
            -> 127.0.0.1:3000 (actual analyzer peer: loopback)
            -> authenticated server-side session -> protected handler
```

Production Compose uses Linux host networking for both nginx and analyzer. nginx is the only exposed application HTTP listener; analyzer binds loopback and has no published port. `AUTH_ALLOWED_SUBNETS` is enforced by nginx on every UI, asset and API request except `GET /api/health`. A valid password or session does not bypass the allowlist. Backend middleware independently checks Axum `ConnectInfo<SocketAddr>` before authentication. Only IPv4 loopback `127.0.0.0/8` and IPv6 `::1` qualify; Docker/private IPs do not.

No forwarded/client-IP header grants access, and nginx Real-IP rewriting is not enabled. Do not add Docker gateway CIDRs to make a NAT-collapsed deployment work. Docker Desktop/VM forwarding and additional reverse proxies can hide browser IPs: verify the actual ingress addresses on the target Linux host. If another TLS proxy is added, it must preserve the source address or become a separately reviewed ingress boundary; do not simply trust its forwarded headers.

`GET /api/health` returns only `{"status":"ok"}` publicly through nginx. Local `POST /api/auth/login` accepts JSON username/password and returns 200 plus a cookie on success, generic 401 on incorrect credentials. `GET /api/auth/me` returns authenticated identity; `POST /api/auth/logout` invalidates it. All data routes and unknown/future API paths require a session. Non-local analyzer peers receive 403 before credentials are considered. Overlapping password checks return 429 instead of queuing expensive Argon2 work.

## Sessions and Browser Behavior

The cookie protocol and random session IDs use [tower-sessions](https://docs.rs/tower-sessions/0.14.0/tower_sessions/); verification uses [RustCrypto Argon2](https://docs.rs/argon2/0.5.3/argon2/). A bounded in-memory store holds at most 1024 sessions, deletes expired entries on creation/load and rejects saves of removed sessions. It adds no database or service. Restarting analyzer invalidates all logins; users can log in again. Capacity exhaustion fails closed. Expired unused records may remain in bounded memory until another creation/load.

Successful login explicitly cycles the session ID and revokes the previous ID. Logout removes server state and expires the cookie. The cookie is `newhatch_session`, `HttpOnly`, `SameSite=Strict`, `Path=/`, with `Max-Age` based on the configured absolute TTL (whole-second rounding can subtract one second). Polling does not extend expiry. `AUTH_COOKIE_SECURE=true` adds `Secure`; the application does not infer HTTPS from untrusted headers.

The frontend checks `/api/auth/me` before mounting any traffic views. Loading, sign-in, authenticated and connection-error states are separate. A protected JSON or raw-payload 401 unmounts traffic views, stops polling and shows sign-in. No token goes into browser storage. API requests use same-origin cookies. Vite binds loopback and proxies `/api` to `127.0.0.1:3000`; it is not a team-facing deployment server.

Permissive CORS is removed. Mutations use POST/PUT/DELETE. When present, `Origin` must match the request authority, and Fetch Metadata rejects cross-site/same-site mutations from other origins. Strict cookies and JSON login prevent ambient cross-site login; non-browser clients without these headers remain supported. API responses are `no-store`. Do not log credentials, hashes, cookies, session IDs or complete authentication headers.

HTTP does not encrypt passwords, session cookies or captured payloads. Use the current HTTP mode only on an isolated trusted network; for untrusted transport, configure HTTPS and `AUTH_COOKIE_SECURE=true`. The subnet allowlist does not replace encryption. Local processes are inside the transport trust boundary but still need credentials.

## Verification

See [`test/auth/README.md`](../test/auth/README.md) for backend, CIDR, real-network and browser tests. The isolated Docker suite validates genuine peer preservation in a shared network namespace. Final host-network ingress still needs a check from an actual team machine on the competition network.
