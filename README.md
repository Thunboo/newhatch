# Нюхач

`newhatch` is a lightweight live-traffic analyzer for Attack/Defence CTF competitions. The user-facing name is **Нюхач**.

See [PROJECT.md](PROJECT.md) for the product brief, current implementation and architectural constraints. Detailed design documents live in [`docs/`](docs/).

## Quick Start

The capture path is Linux-only. On the target vulnbox:

```bash
cp .env.example .env
# Edit CAPTURE_INTERFACE and FLAG_REGEX.
docker compose build analyzer
docker compose run --rm --no-deps --entrypoint hash-password analyzer
# Set AUTH_USERNAME and the printed single-quoted AUTH_PASSWORD_HASH in .env.
# Set AUTH_ALLOWED_SUBNETS to your team's CIDRs (empty = loopback only).
docker compose up -d --build analyzer frontend
```

Open `http://localhost:8080`, sign in, then add monitored services on the Sources screen. The analyzer rebuilds its kernel BPF filter from enabled TCP ports.

nginx and analyzer use Linux host networking; API port 3000 is loopback-only. Team access requires both an allowed client subnet and valid credentials. Sessions expire after 24 hours by default and are invalidated on analyzer restart. Missing credentials stop startup. See [authentication and deployment details](docs/authentication.md), including HTTPS, cookie settings and configuration changes.

Suricata is optional passive IDS enrichment and is not required for capture or flag detection. Start the complete default stack, including Suricata, with:

```bash
docker compose up -d --build
```

Its current filter is configured separately through `SURICATA_BPF_FILTER`; EVE ingestion and session correlation are not implemented yet.

## Flag Capture Test

The opt-in nginx fixture is documented in [`test/flag_test/README.md`](test/flag_test/README.md):

```bash
docker compose --profile test up -d test-flag
```

## Useful Checks

```bash
docker compose ps
docker compose logs -f analyzer
curl http://localhost:8080/api/health
```

Authentication tests and an isolated multi-subnet Docker fixture are in [`test/auth/`](test/auth/README.md).
