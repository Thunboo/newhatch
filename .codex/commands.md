# Commands

Run commands from the repository root unless a section says otherwise.

## Configuration

```bash
cp .env.example .env
```

At minimum, verify `CAPTURE_INTERFACE`, `FLAG_REGEX` and `SURICATA_BPF_FILTER`. The analyzer capture path requires Linux.

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

Vite development mode proxies `/api` to `http://localhost:3000`.

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

Add an enabled Source for TCP port `18080` before sending the request. See `test/README.md`.

## Main Environment Variables

```text
CAPTURE_INTERFACE=eth0
LISTEN_ADDR=0.0.0.0:3000
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
