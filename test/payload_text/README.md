# Canonical Payload Text Checks

The [readable-v1 contract](../../docs/session-filtering-v2.md#canonical-payload-text)
uses cases.json as shared Rust/TypeScript golden input. Run from the repository root:

```bash
npm --prefix frontend ci
node test/payload_text/frontend.cjs
cargo test --workspace --locked
```

The Node runner transpiles the existing TypeScript with the frontend's installed
TypeScript package. It creates no generated source files and needs no running API.
Fixtures cover Cyrillic, surrogate pairs, nested JSON, percent encoding, malformed
input, invalid UTF-8, BOM, browser whitespace, JavaScript number/key ordering and
compact/pretty forms. Highlight checks preserve canonical matches across JSON
formatting without a second decoding pass. Individual cURL/Python and multi-request
chain export checks use byte-framed HTTP requests, readable Unicode and UTF-8 bodies.

Backend integration coverage in test/auth/backend.rs persists real segment records,
rebuilds flag metadata, searches decoded flags, extracts HTTP events and checks
source/owner-scoped ordered rules, retained chain context, edits, overlapping rules,
disable/delete, authentication and bounded pagination. Parser/matcher unit tests
cover adjacency, max_gap, repeated starts, named/repeated query values, response
conditions, chunked/pipelined messages and validation.

Browser checks for both feeds, copy, Hex and exports use
[the host Playwright configuration](../session_chains/README.md).
Append `session-buckets.spec.cjs payload-highlighting.spec.cjs` to select focused
UI tests. Replay tests execute generated Python against a loopback receiver and
need Python 3 with requests; stdout is read as UTF-8 with normalized test line endings.
Live Linux AF_PACKET, actual nginx ingress and container packaging require the
separate target-host checks in the linked runbooks. Agents must not run Docker.
