# Development Commands

Run from the repository root unless stated otherwise.
Agents must not run Docker/Docker Compose, access its daemon or control OrbStack;
Docker commands below are for the user to execute.

## Configuration and runtime

Copy .env.example to .env and configure credentials, team CIDRs, capture interface,
flag regex and Suricata's independent filter. Maintained defaults are in
.env.example; local/split deployment and lifecycle commands are in
[README](../README.md). Capture requires Linux.

## Host checks

```bash
cargo fmt --check
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
python3 -m unittest discover -s test/auth -p 'test_*.py' -v
```

Use cargo fmt without --check to format.
Frontend commands run from frontend/:

```bash
npm ci
npm run lint
npm run build
npm run dev
```

From the repository root, check the shared decoded text/highlight/export fixtures:

```bash
node test/payload_text/frontend.cjs
```

Contract and coverage: [test/payload_text](../test/payload_text/README.md).

Vite binds loopback and proxies /api to 127.0.0.1:3000.

## Rust checks without host Cargo — user only

Use the Rust image version from the analyzer Dockerfile:

```bash
docker run --rm -v "${PWD}:/workspace" -w /workspace rust:1.85-bookworm cargo fmt --check
docker run --rm -v "${PWD}:/workspace" -v newhatch-cargo-registry:/usr/local/cargo/registry -v newhatch-cargo-target:/workspace/target -w /workspace rust:1.85-bookworm cargo test --workspace
docker run --rm -v "${PWD}:/workspace" -v newhatch-cargo-registry:/usr/local/cargo/registry -v newhatch-cargo-target:/workspace/target -w /workspace rust:1.85-bookworm cargo clippy --workspace --all-targets --all-features -- -D warnings
```

## Focused suites

| Area | Runbook |
| --- | --- |
| Auth, nginx/Compose configuration, real ingress, browser auth/feed | [test/auth](../test/auth/README.md) |
| Chains, filters, scroll, export and actual Python replay | [test/session_chains](../test/session_chains/README.md) |
| nginx flag fixture and individual HTTP export | [test/flag_test](../test/flag_test/README.md) |
| Collector/analyzer split | [test/collector_split](../test/collector_split/README.md) |

The host browser runbook starts a temporary Vite server and mocked APIs;
replay uses a temporary loopback HTTP receiver. The full replay suite needs
Python 3 with requests. Set NEWHATCH_TEST_RESULTS to a temporary directory for
shared screenshots; NEWHATCH_TEST_NODE, NEWHATCH_TEST_CHROMIUM and
NEWHATCH_TEST_PYTHON select runtimes. To run only filter cases, append
session-filtering.spec.cjs to the runbook's Playwright command.

Choose checks for the changed behavior. Keep runnable commands and coverage in
these runbooks; do not append dated pass reports or duplicate environment defaults.
