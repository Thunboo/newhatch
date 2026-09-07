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
AUTH_ALLOWED_SUBNETS=100.0.0.0/8
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

## Adding Suricata (not ready)

Suricata is optional passive IDS enrichment and is not required for capture or flag detection. Start the complete default stack, including Suricata, with:

```bash
docker compose up -d --build
```

Its current filter is configured separately through `SURICATA_BPF_FILTER`; EVE ingestion and session correlation are not implemented yet.

## Split Collector Deployment

The default `PACKET_INGRESS_MODE=local` keeps capture and analysis on one host in a single `analyzer` container. For a split deployment, follow this configuration steps:

### Receiver (not the Vulnbox)

Prepare `.env`:
```env
PACKET_INGRESS_MODE=receiver
COLLECTOR_LISTEN_ADDR=0.0.0.0:39090
COLLECTOR_ALLOWED_IPS=VULNBOX_IP
```

Start the receiver-side analyzer and frontend:
```bash
docker compose up -d --build analyzer frontend
```

### Vulnbox

On the vulnbox, configure and start only the lightweight collector:
```env
CAPTURE_INTERFACE=eth0
COLLECTOR_ID=vulnbox-1
RECEIVER_ADDR=RECEIVER_IP
RECEIVER_PORT=39090
```

```bash
docker compose --profile collector up -d --build collector
```

### Security annotation

Open TCP port `39090` only between those hosts. The collector pins the configured receiver endpoint, and the receiver accepts only exact IPs in `COLLECTOR_ALLOWED_IPS`. This initial transport is unencrypted and has no PSK (task is in Backlog), so use it only on the trusted players/VPN network. Sources remain managed in the analyzer UI and are pushed to connected collectors automatically.

## Useful Checks

```bash
docker compose ps
docker compose logs -f analyzer
curl http://localhost:8080/api/health
```

Authentication tests and an isolated multi-subnet Docker fixture are in [`test/auth/`](test/auth/README.md).

## Flag Capture Test

The opt-in nginx fixture is documented in [`test/flag_test/README.md`](test/flag_test/README.md):
