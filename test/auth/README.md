# Authentication Tests

These fixtures are isolated from production data and do not change your `.env`.

## Backend and CIDR Checks

From the repository root:

```bash
docker run --rm -v "${PWD}:/workspace" -v newhatch-cargo-registry:/usr/local/cargo/registry -v newhatch-cargo-target:/workspace/target -w /workspace rust:1.85-bookworm cargo test --workspace
python3 -m unittest discover -s test/auth -p 'test_*.py' -v
```

`backend.rs` is a Cargo integration target. It uses temporary SQLite data and injected real-peer metadata to check local/remote decisions, forged headers, every protected route, future-path fallback, rejected mutation side effects, generic login errors, cookie attributes, ID rotation, logout, absolute expiration and rejection of legacy unprefixed credential variables. Python tests validate nginx generation, IPv4/IPv6 boundaries, multiple networks, sensitive artifact deny rules, frontend/data-volume isolation, fail-closed configuration and Compose credential interpolation when the host defines conflicting `USERNAME`/`PASSWORD` values.

Chain list/member routes share the same protected boundary and have invalid
filter/cursor regression coverage. The browser image also picks up chain-card
tests from `session-chains.spec.cjs`; host-only execution is documented in
[session-chain verification](../session_chains/README.md).

Known/Unsorted coverage checks owned Source-scoped rule CRUD, regex validation,
ordered transaction matches inside full chains, overlapping rules, disable/delete,
retained-data reclassification, classification beyond 2,000 members in one chain
and across a 100-chain page in both feeds, empty pages with continuation,
canonical flag metadata and unchanged raw bytes. The rule editor and separate
persisted bucket filters are exercised by session-buckets.spec.cjs. Shared
text/export fixtures and focused commands are in [test/payload_text](../payload_text/README.md).
Rule explanation coverage includes authenticated/owner-scoped reads, stale rule
definitions, field-local UTF-8 ranges, named query parameters, informational
responses, legacy transaction ordinals, chunk boundaries, JSON expansion and
empty/zero-width matches. Browser checks cover adaptive labels at notebook and
ultrawide widths, popup hover/focus/click/Escape, direction-local marks and labels,
unmarked context members, Text/Hex/JSON stability and failed explanation retry
while payload remains inspectable.
The enabled/disabled explanation modes share bucket, ownership and stale-rule
checks. Disabled-mode coverage removes access to a payload segment and still
expects direction-local labels with empty ranges. Browser cases in both feeds
check that labels, search marks and flag marks remain usable without rule marks.
nginx generation checks keep login/ordinary API bodies at 4 KiB while rule routes
allow bounded 2 MiB definitions under the same CIDR/authentication boundary.

Run the bucket classification volume regression without Docker when host Cargo
is available:

```bash
cargo test -p newhatch-analyzer --test auth buckets_classify_more_than_2000_members_per_chain_and_per_page -- --exact
```

Run the rule-highlighting configuration and API checks in the existing Rust test
container (no host Cargo is required):

```bash
docker run --rm -v "${PWD}:/workspace" -v newhatch-cargo-registry:/usr/local/cargo/registry -v newhatch-cargo-target:/workspace/target -w /workspace rust:1.85-bookworm cargo test --locked -p newhatch-analyzer rule_highlighting -- --test-threads=1
docker run --rm -v "${PWD}:/workspace" -v newhatch-cargo-registry:/usr/local/cargo/registry -v newhatch-cargo-target:/workspace/target -w /workspace rust:1.85-bookworm cargo test --locked -p newhatch-analyzer --test auth source_scoped_ordered_rules -- --test-threads=1
```

Session filtering coverage verifies the authenticated limits endpoint and both
feeds' payload-list validation, stored AND matches, bounded continuation and
full chain context. `session-filtering.spec.cjs` checks chips, pending/failed
application, stale replies, empty-submit suppression and backend-provided limits
in both modes. Delayed responses and DOM mutation tracking catch even transient
Refresh notices during Enter/Add; failure, retry and stale failure are covered.
nginx generation checks also confirm that a full-length Unicode
filter list fits one configured request-line buffer.

## Real Network Test

```bash
python3 test/auth/run_e2e.py
```

Requires Docker Compose and unused subnets `192.168.1.0/24`, `192.168.2.0/24`, `172.18.0.0/24`, `fd42:1234::/64`. It builds production images and creates a shared nginx/analyzer network namespace. Clients connect directly to nginx container addresses, so their real source IPs are preserved without header spoofing or host-port NAT.

`192.168.1.100` and IPv6 team clients must log in successfully. Before login, data routes and mutations return 401; requests for `.env`, `.git`, SQLite/database files, `data`, `segments` and `.seg` paths do not fall back to the SPA and return 404. `192.168.2.100` and `172.18.0.5` must receive 403 even with a valid cookie or forged IP headers. Health remains public, and analyzer port 3000 must be unreachable externally. The runner passes cookies through stdin, never prints them, and removes its containers, networks and temporary data in `finally`.

The fixture uses public test-only credentials `team` / `test-only-password`. Never deploy them in a real installation. Port `127.0.0.1:18081` is reserved while the fixture runs; browser access through that host mapping is not a source-IP preservation test and may correctly receive 403. The real production host-network topology still needs validation from an actual team machine on Linux.

## Browser Tests

```bash
docker build -f test/auth/frontend.Dockerfile -t newhatch-auth-ui-test .
docker run --rm --shm-size=256m -v /tmp/newhatch-auth-ui:/results newhatch-auth-ui-test
```

Playwright uses Chromium and mocked API replies. It checks startup gating, errors/retry, valid and invalid login, logout, expired sessions, raw-payload 401 handling, stopped polling, the merged live Sessions feed, automatic history pagination, polling pause away from the live edge, empty browser token storage, English/Russian browser defaults and persisted language switching, and desktop/mobile screenshots. Images are written to `/tmp/newhatch-auth-ui/`. Real cookies and server authorization are covered by the backend and network suites, not by browser mocks.
