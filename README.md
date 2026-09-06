

## Quick Start

The capture path is Linux-only. On the target vulnbox:

```bash
cp .env.example .env
# Edit CAPTURE_INTERFACE, FLAG_REGEX
docker compose up -d --build
```

Open `http://localhost:8080`, add monitored services on the Sources screen, and the analyzer will rebuild its kernel BPF filter from the enabled TCP ports.

For a local end-to-end flag capture check, use the nginx fixture described in [`test/README.md`](test/README.md).

Useful checks:

```bash
docker compose ps
docker compose logs -f analyzer
curl http://localhost:8080/api/health
```