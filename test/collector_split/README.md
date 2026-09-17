# Collector / Analyzer Split Test

This test verifies the remote capture path on one Linux host before deploying it across two machines.

1. Configure `.env` with `ANALYZER=remote`, `LISTEN_CONNSTR=0.0.0.0:39090`, `ANALYZER_CONNSTR=127.0.0.1:39090`, and the desired `CAPTURE_INTERFACE`. Connection strings accept `IP:port` or `FQDN:port`. Optionally set `ALLOWED_COLLECTORS` to comma-separated IPs or FQDNs; an empty value accepts any source IP.
2. Start analyzer, frontend, collector, and the flag fixture:

```bash
docker compose --profile collector --profile test up -d --build analyzer frontend collector test-flag
```

3. Add TCP port `18080` as an enabled Source in the UI. The analyzer pushes it to collector, which rebuilds its kernel BPF filter.
4. Generate traffic and inspect the resulting session:

```bash
curl "http://localhost:18080/flag?player=team01&payload=collector-test"
```

The Collectors screen should show `vulnbox-1` online with increasing captured/sent counters. Stop analyzer briefly to verify collector reconnects and its bounded `QUEUE_CAPACITY` buffer (8192 packets by default) reports drops instead of growing indefinitely.

The Rust test suites cover protobuf round trips, collector-aware worker routing, IP admission helpers, bounded queue behavior, and collector status tracking.
