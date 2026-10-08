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

```bash
CHAIN_TEST_TOOLS="$(mktemp -d)"
npm install --prefix "$CHAIN_TEST_TOOLS" --no-audit --no-fund @playwright/test@1.51.1
"$CHAIN_TEST_TOOLS/node_modules/.bin/playwright" install chromium
NODE_PATH="$CHAIN_TEST_TOOLS/node_modules" "$CHAIN_TEST_TOOLS/node_modules/.bin/playwright" test --config=test/session_chains/playwright.config.cjs
```

The configuration starts and stops a temporary Vite server on port 4173 and runs
the existing auth/localization/feed regressions plus four chain scenarios.
Screenshots/results default to the OS temporary directory. Optional overrides:
`NEWHATCH_TEST_RESULTS`, `NEWHATCH_TEST_NODE`, `NEWHATCH_TEST_CHROMIUM`.

Chain browser coverage verifies the default checkbox, active filters, whole-pair
order and timestamps, C2S-only cURL/Python actions, clipboard/Hex behavior,
progressive metadata/payload loading, fixed window heights, independent inner
and outer scroll with no background movement, mobile layout, Escape and removal
of stale rows after a late bridge merges chains.

The additional responsive strip scenario checks 26% width with a 100 px minimum and outer-scroll routing over C2S and S2C
without moving payload offsets, Python/raw copy controls above the layer,
right-edge scroll routing and background isolation at the chain end
on both desktop and mobile viewports.

On 2026-10-08, 45 Rust tests, strict clippy/fmt, TypeScript checking, Vite production
build and all seven local Chromium scenarios passed on macOS. Live AF_PACKET
capture and the production Linux deployment were not exercised in that run.

The strip follow-up passed TypeScript/build and all four chain browser scenarios
on the same date; it adds no backend behavior.

The later `right: 0px` adjustment passed production build and the focused
desktop/mobile right-edge scroll/copy scenario.

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
