# Commands

Run commands from the repository root unless a section says otherwise.

## Configuration

```bash
cp .env.example .env
```

At minimum, configure `NEWHATCH_USERNAME`, `NEWHATCH_PASSWORD`, `AUTH_ALLOWED_SUBNETS`, `CAPTURE_INTERFACE`, `FLAG_REGEX` and `SURICATA_BPF_FILTER`. See `docs/authentication.md`. The analyzer capture path requires Linux.

## Rust Analyzer

Cargo does not need to be installed on the host. Run formatting and workspace tests in the same Rust version used by the analyzer image:

```bash
docker run --rm -v "${PWD}:/workspace" -w /workspace rust:1.85-bookworm cargo fmt --check
docker run --rm -v "${PWD}:/workspace" -v newhatch-cargo-registry:/usr/local/cargo/registry -v newhatch-cargo-target:/workspace/target -w /workspace rust:1.85-bookworm cargo test --workspace
docker run --rm -v "${PWD}:/workspace" -v newhatch-cargo-registry:/usr/local/cargo/registry -v newhatch-cargo-target:/workspace/target -w /workspace rust:1.85-bookworm cargo clippy --workspace --all-targets --all-features -- -D warnings
```

If Cargo is available locally, the equivalent host commands are:

```bash
cargo fmt --check
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

For an auto-formatting pass, run `cargo fmt` without `--check`. Agents must only present the Docker commands for the user to run; they must not execute them.

## Frontend

Run these from `frontend/`:

```bash
npm ci
npm run lint
npm run build
npm run dev
```

Vite development mode proxies `/api` to `http://127.0.0.1:3000`.

## Docker Compose

> Agent safety rule: do not execute Docker/Docker Compose commands or control OrbStack. Present the required commands to the user for manual execution because daemon operations can disrupt the VPN tunnel.

Validate configuration:

```bash
docker compose config --quiet
docker compose --profile test config --quiet
```

Start the core analyzer and UI without Suricata:

```bash
docker compose up -d --build analyzer frontend
```

Start the complete default stack, including passive Suricata:

```bash
docker compose up -d --build
```

Useful runtime checks:

```bash
docker compose ps
docker compose logs -f analyzer
curl http://localhost:8080/api/health
```

The UI is published at `http://localhost:8080` by default.

## Flag Capture Fixture

```bash
docker compose --profile test up -d test-flag
curl "http://localhost:18080/flag?player=team01&payload=demo-exploit"
docker compose --profile test stop test-flag
```

Add an enabled Source for TCP port `18080` before sending the request. See `test/flag_test/README.md`.

## Authentication Tests

```bash
docker run --rm -v "${PWD}:/workspace" -v newhatch-cargo-registry:/usr/local/cargo/registry -v newhatch-cargo-target:/workspace/target -w /workspace rust:1.85-bookworm cargo test --test auth
python3 -m unittest discover -s test/auth -p 'test_*.py' -v
python3 test/auth/run_e2e.py
docker build -f test/auth/frontend.Dockerfile -t newhatch-auth-ui-test .
docker run --rm --shm-size=256m -v /tmp/newhatch-auth-ui:/results newhatch-auth-ui-test
```

The network suite builds an isolated stack and removes its own resources. See `test/auth/README.md`.

## Request Export Tests

Agent restriction still applies: these commands must be run by the user.
From the repository root:

```bash
docker build -f test/flag_test/replay.Dockerfile -t newhatch-replay-test .
docker run --rm --shm-size=256m newhatch-replay-test
```

The image runs frontend lint/build, then Playwright against the existing nginx
flag fixture plus a test-only request receiver behind nginx. The tests execute
generated Bash/cURL and Python/requests snippets and include the existing auth
and session-feed browser checks. No host ports or production data volumes are
used. See test/flag_test/README.md. The user reported all 20 Playwright tests
passing on 2026-09-29.

## Main Environment Variables

```text
CAPTURE_INTERFACE=eth0
ANALYZER=local
LISTEN_ADDR=127.0.0.1:3000
NEWHATCH_USERNAME=<required>
NEWHATCH_PASSWORD=<required>
AUTH_ALLOWED_SUBNETS=<team CIDRs; empty = loopback only>
SESSION_EXPIRACY=86400s
AUTH_COOKIE_SECURE=false
DATA_DIR=/data
FLAG_REGEX=FLAG\{[^}\r\n]+\}
SEGMENT_DURATION=30m
SEGMENT_RETENTION_COUNT=3
FLOW_WORKERS=<available CPUs clamped to 1..8>
PACKET_QUEUE_CAPACITY=8192
STORAGE_QUEUE_CAPACITY=2048
FLOW_IDLE_TIMEOUT=30s
MAX_ACTIVE_FLOWS_PER_WORKER=16384
MAX_STREAM_BYTES=4MiB
FRONTEND_PORT=8080
SURICATA_BPF_FILTER=tcp and (port 8080)
RUST_LOG=newhatch=info
```

Remote analyzer:

```text
ANALYZER=remote
LISTEN_CONNSTR=0.0.0.0:39090
ALLOWED_COLLECTORS=<comma-separated IPs/FQDNs; empty accepts any host>
```

Standalone collector:

```text
CAPTURE_INTERFACE=eth0
COLLECTOR_ID=vulnbox-1
ANALYZER_CONNSTR=<analyzer IP/FQDN:port>
QUEUE_CAPACITY=8192
RUST_LOG=newhatch=info
```

The root `README.md` is the operator runbook for choosing local or split deployment. `.env.example` contains the maintained defaults.

Suricata's filter is currently independent from Sources and must be kept aligned manually.
