# Authentication Tests

These fixtures are isolated from production data and do not change your `.env`.

## Backend and CIDR Checks

From the repository root:

```bash
cargo test --workspace
python3 -m unittest discover -s test/auth -p 'test_*.py' -v
```

`backend.rs` is a Cargo integration target. It uses temporary SQLite data and injected real-peer metadata to check local/remote decisions, forged headers, every protected route, future-path fallback, generic login errors, cookie attributes, ID rotation, logout and absolute expiration. Python tests validate nginx generation, IPv4/IPv6 boundaries, multiple networks and fail-closed configuration.

## Real Network Test

```bash
python3 test/auth/run_e2e.py
```

Requires Docker Compose and unused subnets `192.168.1.0/24`, `192.168.2.0/24`, `172.18.0.0/24`, `fd42:1234::/64`. It builds production images and creates a shared nginx/analyzer network namespace. Clients connect directly to nginx container addresses, so their real source IPs are preserved without header spoofing or host-port NAT.

`192.168.1.100` and IPv6 team clients must log in successfully. `192.168.2.100` and `172.18.0.5` must receive 403 even with a valid cookie or forged IP headers. Health remains public, and analyzer port 3000 must be unreachable externally. The runner passes cookies through stdin, never prints them, and removes its containers, networks and temporary data in `finally`.

The fixture uses public test-only credentials `team` / `test-only-password`. Never deploy them in a real installation. Port `127.0.0.1:18081` is reserved while the fixture runs; browser access through that host mapping is not a source-IP preservation test and may correctly receive 403. The real production host-network topology still needs validation from an actual team machine on Linux.

## Browser Tests

```bash
docker build -f test/auth/frontend.Dockerfile -t newhatch-auth-ui-test .
docker run --rm --shm-size=256m -v /tmp/newhatch-auth-ui:/results newhatch-auth-ui-test
```

Playwright uses Chromium and mocked API replies. It checks startup gating, errors/retry, valid and invalid login, logout, expired sessions, raw-payload 401 handling, stopped polling, empty browser token storage and desktop/mobile screenshots. Images are written to `/tmp/newhatch-auth-ui/`. Real cookies and server authorization are covered by the backend and network suites, not by browser mocks.
