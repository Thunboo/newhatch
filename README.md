# Нюхач

`newhatch` is a lightweight live-traffic analyzer for Attack/Defence CTF competitions. The user-facing name is **Нюхач**.

See [PROJECT.md](PROJECT.md) for the product brief, current implementation and architectural constraints. Detailed design documents live in [`docs/`](docs/).

## Login Setup

Create `.env` and set the login credentials near the top:

```bash
cp .env.example .env
```

```env
USERNAME=admin
PASSWORD=admin123
SESSION_EXPIRACY=86400s
AUTH_ALLOWED_SUBNETS=100.0.0.0/8
```

Use your own password and actual team CIDR. An empty `AUTH_ALLOWED_SUBNETS` allows only clients that nginx sees as loopback. The analyzer hashes the password with Argon2id during startup. See [authentication details](docs/authentication.md).

## Quick Start

The capture path is Linux-only. On the target vulnbox:

```bash
# Configure CAPTURE_INTERFACE, FLAG_REGEX and authentication in .env first.
docker compose up -d --build analyzer frontend
```

Open `http://localhost:8080`, sign in, then add monitored services on the Sources screen. The analyzer rebuilds its kernel BPF filter from enabled TCP ports.

While the Sessions view is at the live edge, the frontend checks for new sessions every five seconds. Polling pauses while older traffic is being inspected and resumes near the top; see [`LIVE_REFRESH_INTERVAL_MS` and the feed polling effect](frontend/src/App.tsx).

nginx and analyzer use Linux host networking; API port 3000 is loopback-only. Team access requires both an allowed client subnet and valid credentials. The frontend has no data volume, and nginx explicitly rejects paths resembling dotfiles, SQLite databases or stored payload segments. Sessions expire after 24 hours by default and are invalidated on analyzer restart. Missing credentials stop startup. See [authentication and deployment details](docs/authentication.md), including HTTPS, cookie settings and configuration changes.

## Adding Suricata (not ready)

Suricata is optional passive IDS enrichment and is not required for capture or flag detection. Start the complete default stack, including Suricata, with:

```bash
docker compose up -d --build
```

Its current filter is configured separately through `SURICATA_BPF_FILTER`; EVE ingestion and session correlation are not implemented yet.

## Split Collector Deployment

The default `ANALYZER=local` keeps capture and analysis on one host in a single `analyzer` container. For a split deployment, use the following configuration.

### Receiver (not the Vulnbox)

Prepare `.env`:
```env
ANALYZER=remote
LISTEN_CONNSTR=0.0.0.0:39090
# Optional IPs or FQDNs. Empty accepts collector traffic from any host.
ALLOWED_COLLECTORS=VULNBOX_IP_OR_FQDN
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
ANALYZER_CONNSTR=ANALYZER_IP_OR_FQDN:39090
# Bounded packet buffer; full queues drop new packets. Default: 8192.
QUEUE_CAPACITY=8192
```

```bash
docker compose --profile collector up -d --build collector
```

### Security annotation

Open TCP port `39090` between the collector and analyzer. `ANALYZER_CONNSTR` accepts either `IP:port` or `FQDN:port` and resolves DNS again on reconnect. `LISTEN_CONNSTR` also accepts either form. A non-empty `ALLOWED_COLLECTORS` accepts comma-separated IPs and FQDNs; names are resolved when analyzer starts, so restart it after their DNS records change. An empty value accepts any host. This initial transport is unencrypted and has no PSK (task is in Backlog), so use it only on the trusted players/VPN network. Sources remain managed in the analyzer UI and are pushed to connected collectors automatically.

## Useful Checks

```bash
docker compose ps
docker compose logs -f analyzer
curl http://localhost:8080/api/health
```

Authentication tests and an isolated multi-subnet Docker fixture are in [`test/auth/`](test/auth/README.md).

## Flag Capture Test

The opt-in nginx fixture is documented in [`test/flag_test/README.md`](test/flag_test/README.md):
