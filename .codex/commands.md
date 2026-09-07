# Commands

Run commands from the repository root unless a section says otherwise.

## Configuration

```bash
cp .env.example .env
```

At minimum, configure `AUTH_USERNAME`, `AUTH_PASSWORD_HASH`, `AUTH_ALLOWED_SUBNETS`, `CAPTURE_INTERFACE`, `FLAG_REGEX` and `SURICATA_BPF_FILTER`. See `docs/authentication.md`. The analyzer capture path requires Linux.

## Rust Analyzer

```bash
cargo fmt --check
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

For an auto-formatting pass, run `cargo fmt` without `--check`.

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
cargo test --test auth
python3 -m unittest discover -s test/auth -p 'test_*.py' -v
python3 test/auth/run_e2e.py
docker build -f test/auth/frontend.Dockerfile -t newhatch-auth-ui-test .
docker run --rm --shm-size=256m -v /tmp/newhatch-auth-ui:/results newhatch-auth-ui-test
```

The network suite builds an isolated stack and removes its own resources. See `test/auth/README.md`.

## Main Environment Variables

```text
CAPTURE_INTERFACE=eth0
LISTEN_ADDR=127.0.0.1:3000
AUTH_USERNAME=<required>
AUTH_PASSWORD_HASH=<required Argon2id PHC hash>
AUTH_ALLOWED_SUBNETS=<team CIDRs; empty = loopback only>
AUTH_SESSION_TTL_SECONDS=86400
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

Suricata's filter is currently independent from Sources and must be kept aligned manually.
