# Нюхач / newhatch

`newhatch` is a lightweight live-traffic analyzer for Attack/Defence CTF competitions.

[RU Readme](README.RU.md)

![Reconstructed session details](docs/images/session-detail.png)

## Setup

Create `.env` and set the login credentials:

```bash
cp .env.example .env
```

```py
NEWHATCH_USERNAME=admin
NEWHATCH_PASSWORD=admin123
```

## Quick Start (All-in-One)

For keyboard-only local control, install [newhatchctl](newhatchctl-src/README.md) from the latest release:

```bash
curl -fsSL https://raw.githubusercontent.com/Thunboo/newhatch/main/install/unix.sh | bash
newhatchctl
```

To start service directly on the vulnbox:

```bash
# (!!!) Configure CAPTURE_INTERFACE, FLAG_REGEX and creds in .env first
docker compose up -d --build analyzer frontend
```

Open `http://localhost:8080`, sign in, then add monitored services on the Sources screen:

![Capture sources](docs/images/sources.png)

## Split Collector Deployment

The default `ANALYZER=local` keeps capture and analysis on one host in a single `analyzer` container. For a split deployment (lowers the load on vulnbox), use the following configuration:

### Receiver (not the Vulnbox)

Prepare `.env`:
```python
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
```python
CAPTURE_INTERFACE=eth0
COLLECTOR_ID=vulnbox-1 # Optional
ANALYZER_CONNSTR=ANALYZER_IP_OR_FQDN:39090
```

```bash
docker compose --profile collector up -d --build collector
```

### Security annotation

- TCP port `39090` (Or other reconfigured one) between the collector and analyzer must be open;
- `ANALYZER_CONNSTR` accepts either `IP:port` or `FQDN:port` and resolves DNS again on reconnect. `LISTEN_CONNSTR` also accepts either form;
- A non-empty `ALLOWED_COLLECTORS` accepts comma-separated IPs and FQDNs; 
- Names are resolved when analyzer starts, so restart it after their DNS records change. An empty value accepts any host;
- Collector-Receiver transport is unencrypted (Backlog task), so use it only on the trusted players/VPN network.;
- Sources remain managed in the analyzer UI and are pushed to connected collectors automatically.

## Useful Checks

```bash
docker compose ps
docker compose logs -f analyzer
curl http://analyzer-host:8080/api/health
```

### Flag Capture Test

The opt-in nginx fixture is documented in [`test/flag_test/README.md`](test/flag_test/README.md)

## Project info

See [PROJECT.md](PROJECT.md) for the product brief, current implementation and architectural constraints. Detailed design documents live in [`docs/`](docs/).

### Main menu - Sessions and Functions:

- **Sessions viewer** - click on desired sessions / sessions chains to check its contents.
- **Group into chains** - toggle related traffic grouping (kill-chains). In preview - scroll between sessions using the card or the right side of a payload window.
- **Filtering** - Add text filters via search bar. Filter match is colored green in Session preview window.

C2S windows offer cURL/Python export; the chain-toolbar Python action exports all supported requests. 

Run `python3 replay.py` for captured destinations or `python3 replay.py <IP/FQDN>` to replace hosts while preserving ports.
See [Python replay](docs/python-replay.md) for Host, response output and cookies.
