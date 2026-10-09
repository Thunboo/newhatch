# Session-Chain Verification

The agreed behavior and API limits are in
[session chains](../../docs/session-chains.md). Tests use temporary data and
mocked browser API responses; production `.env` and traffic data are untouched.

## Rust And Frontend Checks

With Rust 1.85 and Node/npm available, run from the repository root:

```bash
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
npm --prefix frontend ci
npm --prefix frontend run lint
npm --prefix frontend run build
```

Rust tests cover analyzer-owned capture timestamps, retransmissions/empty
directions, collector identity, additive legacy migration, payload-pointer
preservation, exact one-second boundaries, sliding adjacency, all key components,
equal-time ordering, filtered full context, list/member pagination, late merging,
retention, continuation after 2,000 payload candidates and SQL interruption.
Authentication integration tests cover both new routes and invalid cursors/filters.

## Browser Checks Without Docker

Install the browser test runner outside the repository, then run the dedicated
configuration from the repository root:

The complete suite also requires Python 3 with `requests` to execute generated
scripts against a temporary loopback-only HTTP receiver. A temporary virtual
environment keeps these test dependencies outside the repository.

```bash
CHAIN_TEST_TOOLS="$(mktemp -d)"
npm install --prefix "$CHAIN_TEST_TOOLS" --no-audit --no-fund @playwright/test@1.51.1
"$CHAIN_TEST_TOOLS/node_modules/.bin/playwright" install chromium
python3 -m venv "$CHAIN_TEST_TOOLS/python"
"$CHAIN_TEST_TOOLS/python/bin/pip" install requests
NEWHATCH_TEST_RESULTS="$CHAIN_TEST_TOOLS/results" NEWHATCH_TEST_PYTHON="$CHAIN_TEST_TOOLS/python/bin/python3" NODE_PATH="$CHAIN_TEST_TOOLS/node_modules" "$CHAIN_TEST_TOOLS/node_modules/.bin/playwright" test --config=test/session_chains/playwright.config.cjs
```

The configuration starts and stops a temporary Vite server on port 4173 and runs
the existing auth/localization/feed regressions plus eight chain UI scenarios
and 24 framing/Python replay scenarios.
The command sets a temporary results directory for all screenshots, including
the shared auth/feed tests whose standalone fallback is `/results`. Overrides:
`NEWHATCH_TEST_RESULTS`, `NEWHATCH_TEST_NODE`, `NEWHATCH_TEST_CHROMIUM`,
`NEWHATCH_TEST_PYTHON`.

Chain browser coverage verifies the default checkbox, active filters, whole-pair
order and timestamps, C2S-only per-window cURL/Python actions, clipboard/Hex behavior,
progressive metadata/payload loading, fixed window heights, independent inner
and outer scroll with no background movement, mobile layout, Escape and removal
of stale rows after a late bridge merges chains.

The chain-card scenario also verifies that the small scroll hint appears below
Text/Hex in chain detail and is absent in ordinary session detail.

The additional responsive strip scenario checks 26% width with a 100 px minimum and outer-scroll routing over C2S and S2C
without moving payload offsets, Python/raw copy controls above the layer,
right-edge scroll routing and background isolation at the chain end
on both desktop and mobile viewports.

On 2026-10-08, 45 Rust tests, strict clippy/fmt, TypeScript checking, Vite production
build and all seven local Chromium scenarios passed on macOS. Live AF_PACKET
capture and the production Linux deployment were not exercised in that run.

The current chain UI follow-ups passed TypeScript/build and all four chain browser
scenarios on the same date, including right-edge scroll routing and chain-only
hint visibility. They add no backend behavior.

Whole-chain export coverage includes all 30 paginated members without scrolling,
multiple requests inside one C2S, Unicode/JSON display transforms, contextual
offscreen failures without partial copy, retry, abort on close, retention changes
and Russian mobile clipboard fallback. The toolbar action is absent from ordinary
session detail and stays centered on mobile.

`replay.spec.cjs` executes generated Python against a temporary HTTP receiver,
checking request order, byte-accurate Unicode bodies, cookie replacement/deletion
and initial cookies under DNS, single-label, IPv4 and IPv6 Host headers. HTTP 500
does not skip later requests and 302 does not introduce an uncaptured redirect.
Framing tests reject incomplete, binary, chunked, compressed, Upgrade and invalid
header inputs with the failing request index. TypeScript/build and all 18 unique
local browser/replay scenarios passed on 2026-10-08; the final targeted rerun also
checked IPv6 Host and readable button feedback. No Docker was run by the agent.

The conditional-helper follow-up passed TypeScript/build and nine focused
browser/replay scenarios on the same date. Executed scripts verify helper/import
omission without initial cookies, for a single request, for first-only cookies,
and when later cookie names or hosts differ. The first captured Cookie header
still reaches the receiver, and Session continues to process response cookies
without the helper. Reused-cookie initialization still passes all four Host cases.

## Python Replay Destination And Flush (Issue #20)

The [Python replay contract](../../docs/python-replay.md) is verified through
executed individual and chain scripts: captured defaults, IPv4/FQDN/IPv6 argv,
unchanged captured ports, original Host without argv and adaptive existing Host
with argv, absent Host in explicit headers, query/body
preservation and argument rejection before any request. Fixture FQDN resolution
is redirected to loopback in the test process, with an assertion on the hostname
passed to DNS. Actual IPv6 servers listen on `::1`.

Cookie checks cover replacement/deletion with argv, no Host and mixed
captured/absent Host, including runtime-dependent reuse and different captured
Host values converging under argv across ports with IPv4/IPv6. Buffered stdout checks
hold a later HTTP response open until the preceding flag is visible in the pipe;
they run without Python's unbuffered mode. Transport failure stops later requests
while preserving prior printed bodies; HTTP 500/302 bodies remain visible.

After the Host clarification on 2026-10-10, TypeScript lint, Vite production
build and all 36 unique local browser/replay cases passed. The full run passed
35; the chain clipboard test passed a focused rerun after its expected seeding
call was updated to use the effective Host. All runs used the explicit temporary
results path above. No Docker was run and no production data was used.

## User-Run Docker Checks

Agents must not invoke Docker or OrbStack. If verification is performed through
the existing isolated images, the user can run these exact commands:

```bash
docker run --rm -v "${PWD}:/workspace" -v newhatch-cargo-registry:/usr/local/cargo/registry -v newhatch-cargo-target:/workspace/target -w /workspace rust:1.85-bookworm cargo test --workspace
docker build -f test/auth/frontend.Dockerfile -t newhatch-auth-ui-test .
docker run --rm --shm-size=256m -v /tmp/newhatch-auth-ui:/results newhatch-auth-ui-test
```

The browser image already includes `test/auth/*.spec.cjs`, including chain tests.
For live target-host validation, use local capture and then the collector split:
send successive connections to one Source within one second, a connection after
a gap over one second, and traffic to a different Source. Check grouping and
full-context flag filtering, then inspect both scroll levels with long streams.
Legacy data should remain separately inspectable with unknown times. These
checks require the user's actual Linux capture topology.
