# Нюхач

`newhatch` is a lightweight live-traffic analyzer for Attack/Defence CTF competitions. The user-facing name is **Нюхач**.

See [PROJECT.md](PROJECT.md) for the product brief, current implementation and architectural constraints. Detailed design documents live in [`docs/`](docs/).

## Quick Start

The capture path is Linux-only. On the target vulnbox:

```bash
cp .env.example .env
# Edit CAPTURE_INTERFACE and FLAG_REGEX.
docker compose up -d --build analyzer frontend
```

Open `http://localhost:8080`, then add monitored services on the Sources screen. The analyzer rebuilds its kernel BPF filter from enabled TCP ports.

Suricata is optional passive IDS enrichment and is not required for capture or flag detection. Start the complete default stack, including Suricata, with:

```bash
docker compose up -d --build
```

Its current filter is configured separately through `SURICATA_BPF_FILTER`; EVE ingestion and session correlation are not implemented yet.

## Flag Capture Test

The opt-in nginx fixture is documented in [`test/README.md`](test/README.md):

```bash
docker compose --profile test up -d test-flag
```

## Useful Checks

```bash
docker compose ps
docker compose logs -f analyzer
curl http://localhost:8080/api/health
```
