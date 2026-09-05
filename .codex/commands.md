# Commands

Команды актуальны для текущего Rust/TypeScript/Docker scaffold.

## Expected Runtime

```bash
docker compose up -d
```

Expected services:

```text
analyzer
frontend
suricata
```

## Rust Analyzer / Backend

Use after Rust workspace is created:

```bash
cargo fmt
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

## TypeScript Frontend

Use after frontend scaffold is created:

```bash
npm install
npm run dev
npm run build
npm run lint
```

Package manager: npm. Run frontend commands from `frontend/`.

## Docker Compose

Use after compose scaffold is created:

```bash
docker compose config
docker compose up -d --build
docker compose logs -f analyzer
docker compose down
```

## Configuration

Expected environment/config values:

```bash
CAPTURE_INTERFACE=<interface>
FLAG_REGEX=<regex>
SEGMENT_DURATION=30m
SEGMENT_RETENTION_COUNT=3
DATA_DIR=/data
FLOW_IDLE_TIMEOUT=30s
MAX_STREAM_BYTES=4MiB
```

The UI is published at `http://localhost:8080` by default.
