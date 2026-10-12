# newhatchctl

Local, keyboard-only terminal control panel for newhatch. The standalone Rust
package lives in `newhatchctl-src/` so the executable can occupy `newhatchctl` in
the repository root. Its dependency lockfile is independent of the capture
workspace; building analyzer/collector images does not require this package.

## Install and launch

Install the latest stable release from any directory:

```bash
curl -fsSL https://raw.githubusercontent.com/Thunboo/newhatch/main/install/unix.sh | bash
newhatchctl
```

The installer supports Linux and macOS on amd64 and arm64. It resolves the latest
release once, downloads the platform archive and `SHA256SUMS` from that same tag,
verifies the checksum and executable version, then installs into `/usr/local/bin`.
It asks for sudo only when the destination requires elevated permissions. Bash
3.2 or newer, curl, tar, standard Unix utilities and either `sha256sum` (Linux)
or `shasum` (macOS) are required. Rust/Cargo, Python and jq are not required.

Run the same command to update. An already installed latest version is left in
place. Downloads and verification finish before replacing an older binary, and
replacement uses an atomic rename. The installer prints launch instructions;
it does not launch the interactive panel. No installable version exists until
the first release is published and the script is available on `main`.

The default destination is `/usr/local/bin`. Set `NEWHATCHCTL_INSTALL_DIR` to an
absolute directory to choose another destination, and add that directory to PATH.

At each launch, the current directory wins if it contains the newhatch
`compose.yaml`. Otherwise the tool checks the host environment variable
`NEWHATCH_DIR`. A candidate is recognized by the three service names `collector`,
`analyzer` and `frontend` in its Compose file. Image names, registry locations,
build configuration and Dockerfile paths are not checked: prebuilt images,
source builds and mixed deployments are supported. No valid candidate produces
exactly `Unknown Newhatch location`.

To launch from any directory, add this to your shell configuration
(`~/.bashrc` or `~/.zshrc`) and open a new terminal:

```bash
export NEWHATCH_DIR=/absolute/path/to/newhatch
```

Runtime requirements are Docker with Compose v2 (including `config --format
json`, `ps --format json`, `--progress` and `up --pull`), access to its daemon,
and a terminal at least 80 × 24. The panel uses the installation's Compose
configuration and normal Docker CLI environment/context. It provides no SSH
or remote-host management.

## Build from source

From the repository root, with Rust 1.85 or newer:

```bash
cargo build --locked --release --manifest-path newhatchctl-src/Cargo.toml
cp newhatchctl-src/target/release/newhatchctl ./newhatchctl
./newhatchctl
```

Build for the target host's OS/architecture. Capture containers require Linux;
the tool itself also builds on macOS. The same project discovery rules apply:

```bash
NEWHATCH_DIR=/path/to/newhatch /path/to/newhatchctl
```

## Release workflow

The [release workflow](../.github/workflows/newhatchctl-release.yml) runs only
when a `newhatchctl-v*` Git tag is pushed. Ordinary branch pushes and pull requests
do not trigger it. Only stable version tags are accepted, and the version must
match this package's `Cargo.toml` (for example `0.1.0` → `newhatchctl-v0.1.0`).

Merge the release workflow into the default branch before publishing the first
release. Commit the version, sources, lockfile and installer before creating
the tag on the release commit:

```bash
git tag newhatchctl-v0.1.0
git push origin newhatchctl-v0.1.0
```

Native GitHub-hosted runners build and test these archives. Each contains one
executable named `newhatchctl`, with executable permissions preserved:

| Archive | Rust target |
| --- | --- |
| `newhatchctl-linux-amd64.tar.gz` | `x86_64-unknown-linux-musl` |
| `newhatchctl-linux-arm64.tar.gz` | `aarch64-unknown-linux-musl` |
| `newhatchctl-macos-amd64.tar.gz` | `x86_64-apple-darwin` |
| `newhatchctl-macos-arm64.tar.gz` | `aarch64-apple-darwin` |

The toolchain is pinned to Rust 1.85.1 and dependencies are built with `--locked`.
Every target runs formatting checks, tests, Clippy, installer fixture tests,
binary version/help checks and the PTY smoke test with a fake Docker CLI. Linux
binaries use static musl; the workflow verifies they have no dynamic loader or
shared-library dependency.

Publication waits for all four builds. It generates `SHA256SUMS`, uploads every
asset to a draft release, then publishes it as the latest stable release. Failed
uploads leave the release as a draft; rerunning can resume a draft. An already
published version is not overwritten: increment the package version and push a
new tag. The publishing job uses the repository's `GITHUB_TOKEN` with
`contents: write`; no additional token is required.

## Controls

| Key | Action |
| --- | --- |
| Up / Down | Select a component or bulk action; scroll the focused settings/status view |
| Tab / Shift+Tab | Move between components, actions, bulk actions, settings and status |
| Left / Right | Select a per-component action; scroll status/logs horizontally |
| Enter | Open the selected component's actions or execute the selected action |
| `s` / `r` / `x` / `l` | Start / recreate / stop / logs for the selected component |
| F5 | Refresh status and settings |
| Escape | Cancel a confirmation or return from logs/completed output |
| `q` / Ctrl+C | Exit; an in-progress CLI command is terminated |

No mouse events are enabled. Confirmation dialogs default to **No**; use Tab or
Left/Right to select **Yes**, then Enter. Repeated Enter does not accept a dialog.

## Lifecycle and status

Only `collector`, `analyzer` and `frontend` are managed. Analyzer is displayed as
Receiver / Analyzer when configured with `ANALYZER=remote`. Suricata, test
services, orphan services and other Compose projects are excluded from actions
and status. Scaled services are rejected before mutation.

Start creates/starts the selected service, building its image if absent. Existing
images are reused. Restart is `up --force-recreate` with the current configuration,
not `compose restart`; it preserves mounted data and does not request a rebuild.
All starts explicitly disable automatic dependency startup.

Starting/recreating frontend without a running analyzer asks whether to start
both. Analyzer starts first and must reach Docker's `running` state before
frontend is started. API readiness is not checked. Stopping analyzer while
frontend runs asks whether to stop both, stopping frontend first.

Bulk Restart/Stop resolves current container state before mutation and fixes the
set of running managed services for the operation. Stopped or absent services
are not added. Empty sets show `No running containers`. If frontend alone is
running, bulk recreation reports `Start Analyzer first` rather than starting a
stopped analyzer. Operations stop at the first failure and show their output;
already completed steps are not rolled back.

Status is the unmodified Compose `ps --all` table with its normal headings and
explicit service selection. The dashboard refreshes every two seconds. Actions
re-read state; stale status cannot bypass dependency confirmation.

## Settings and logs

The read-only settings panel displays `CAPTURE_INTERFACE`, `FLAG_REGEX`,
`FRONTEND_PORT`, `ANALYZER` and `AUTH_ALLOWED_SUBNETS`. Desired values come from
resolved Compose configuration, including `.env`, defaults and host environment
overrides. A single container `docker inspect` reads applied values for all
existing managed containers on each refresh. Only these five environment fields
are retained in the panel's snapshot.

Differences add `ACTIVE (service): "applied value" [Needs Restart]`. Equal values
and absent containers have no annotation. Capture interface is compared for
collector and for analyzer only when its applied mode is `local`; a remote
analyzer has no interface drift annotation. Flag regex/mode belong to analyzer,
and frontend port/allowed subnets belong to frontend.

Logs open with the last 200 lines and follow new output. Up/Down or PageUp/PageDown
enters scrollback; End resumes following, Home visits the oldest retained lines.
Escape stops the log-reading CLI and returns to the dashboard. The display keeps
at most 5,000 lines, with individual lines capped at 16 KiB. ANSI formatting and
terminal escape sequences are removed from CLI output before display, including
colors already written by containers (`logs --no-color` only controls Compose's
own formatting). Tabs become spaces. Logs are displayed as plain text.

## Verification

These checks do not start Docker or contact its daemon:

```bash
cargo fmt --manifest-path newhatchctl-src/Cargo.toml --check
cargo test --locked --manifest-path newhatchctl-src/Cargo.toml
cargo clippy --locked --manifest-path newhatchctl-src/Cargo.toml --all-targets --all-features -- -D warnings
python3 newhatchctl-src/tests/tui_smoke.py newhatchctl-src/target/release/newhatchctl
bash -n install/unix.sh
python3 -m unittest discover -s newhatchctl-src/tests -p test_installer.py -v
```

Unit tests cover project discovery, planning, dependency dialogs, configuration
drift and terminal rendering. Integration tests inject a stateful Python CLI
replacement to check command selection, single-request inspection, recreation,
backend failure/early exit, shutdown order and colored-log cleanup. The PTY
smoke test uses the same replacement to exercise keyboard navigation, dialogs,
restart, logs and terminal restoration. Python 3 is required for these tests.

Installer tests use local archives and command stubs; downloads, sudo and Docker
are never invoked for real. They cover all platforms, Bash stdin execution,
updates and unchanged versions, SHA256 validation, archive validation, privilege
selection, cleanup and preservation of an existing binary on failures. They
install only into temporary directories. Set `NEWHATCHCTL_TEST_BASH` to another
Bash executable to test compatibility with that version. These tests require
Python 3.9 or newer.

Real Docker/Compose behavior, first-image builds and the target Linux terminal
must be checked by the user under the repository's Docker/VPN restriction. On a
disposable newhatch installation, start only collector, use Restart running,
verify analyzer/frontend stayed stopped, then test both dependency confirmations,
edit a displayed setting externally and verify its annotation disappears after
recreation. Inspect logs and confirm Escape returns to the dashboard.

For release workflow changes, run `actionlint .github/workflows/newhatchctl-release.yml`.
The four-runner matrix and GitHub release permissions are verified by the actual
tag-triggered run; local syntax checks do not publish releases.
