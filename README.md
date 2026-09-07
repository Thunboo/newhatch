# Нюхач

`newhatch` is a lightweight live-traffic analyzer for Attack/Defence CTF competitions. The user-facing name is **Нюхач**.

See [PROJECT.md](PROJECT.md) for the product brief, current implementation and architectural constraints. Detailed design documents live in [`docs/`](docs/).

## Password Setup

Create `.env`, generate an Argon2id password hash, and add the displayed value without removing its single quotes to `.env`:

```bash
cp .env.example .env
docker compose build analyzer
docker compose run --rm --no-deps --entrypoint hash-password analyzer
```

```env
AUTH_USERNAME=team
AUTH_PASSWORD_HASH='$argon2id$v=19$...'
AUTH_ALLOWED_SUBNETS=100.97.0.0/16
```

Use your actual team CIDR. An empty `AUTH_ALLOWED_SUBNETS` allows only clients that nginx sees as loopback. See [authentication details](docs/authentication.md).

## Quick Start

The capture path is Linux-only. On the target vulnbox:

```bash
# Configure CAPTURE_INTERFACE, FLAG_REGEX and authentication in .env first.
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
