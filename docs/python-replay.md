# Python Replay Export (Issue #20)

Status: agreed on 2026-10-09; Host override clarified, implemented and locally verified on 2026-10-10.
Issue: [#20 — Change the way of generating scripts](https://github.com/Thunboo/newhatch/issues/20).

## Goal And Scope

Make exported Python/requests scripts usable against another team's service
without editing every captured URL, and make each replayed response immediately
visible on stdout after it has been received.

Apply this contract to both the per-window Python action for one supported C2S
HTTP request and the whole-chain Python action for all supported C2S requests.
The existing HTTP input restrictions, readable body representation and Format
JSON behavior remain those in [MVP UX](mvp.md) and
[session chains](session-chains.md#whole-chain-python-export-2026-10-08).

This is a frontend export change. It does not expand protocol support, change
TCP-session persistence/grouping or add processing to the collector/analyzer.
Bash/cURL and ordinary payload-copy behavior are outside this change.

## Command-Line Destination

The generated script accepts one optional positional destination argument:

```text
python3 replay.py
python3 replay.py 10.60.2.5
python3 replay.py service.example
```

- With no argument, use the captured `server_ip` and `server_port` for each
  request's own session. A chain must not replace all member destinations with
  the first member's address: its grouping key does not include server IP.
- With an argument, replace the connection host for every exported request,
  including every request inside a multi-request C2S stream. Keep each request's
  captured server port.
- Accept an IP literal or FQDN. Support IPv4 and IPv6; normalize IPv6 brackets
  when constructing URLs without changing the address itself.
- The argument is a host, not a URL or `host:port`. It does not replace the
  captured port, HTTP scheme, path, query parameters or body.
- Invalid arguments or extra positional arguments must fail before sending a
  request, with a concise usage/error message and a nonzero exit status.
- Parse the argument once per script. Use Python's standard library and the
  existing `requests` dependency; keep generated request blocks readable and
  editable.

## Captured Host And Other Headers

Without a destination argument, preserve an existing captured `Host` header
exactly, including its value and original header-name casing. With an argument,
replace its value with the supplied destination and the request's captured
`server_port`, matching the new URL authority. Apply this to IP-valued and
virtual-host values alike; bracket IPv6 addresses. For example, a request captured
on port 8080 with `Host: old.example` becomes `Host: 10.60.2.5:8080` when run
with `10.60.2.5`.

If the capture has no `Host`, do not insert one into the generated `headers`
dictionary. Do not invent other explicit headers for destination replacement.
Existing export validation and connection-specific header omission still apply.

This is a rule for generated explicit headers, not a byte-for-byte raw HTTP
replay requirement. The normal HTTP client's automatic transport headers remain
its responsibility. Python's HTTP client, for example, automatically sends
Host unless that behavior is explicitly disabled; see
[HTTPConnection.putrequest](https://docs.python.org/3/library/http.client.html#http.client.HTTPConnection.putrequest).
No custom transport or header-suppression mechanism is required.

## Response Output

- Keep the existing `response` variable name.
- After every completed request, print the received response body using
  `print(response.text, flush=True)`.
- Print the actual replay response, including any flag it contains. Do not
  substitute captured S2C bytes, extract only regex matches or wait until the
  whole chain finishes before printing bodies.
- Keep HTTP error response bodies visible. An HTTP error status must not skip
  later captured requests; transport failures still stop the script.
- Existing chain session/request/status labels may remain. They must not replace
  response-body output.

The flush guarantee applies after receiving a response body; it does not require
streaming a partially received response.

## Chain Execution And Cookies

Keep one `requests.Session()` per exported chain and sequential execution in
session-start/ID order, then C2S byte order within each session. Wait for each
response before sending the next request. Preserve the current 10-second timeout
per request and disabled automatic redirects.

Retain first-request cookie initialization and actual response Set-Cookie
updates, expiry and deletion. Later captured Cookie headers must not overwrite
fresh cookies. Keep the existing conditional cookie-helper rule: include cookie
scaffolding only when an initial cookie name is reused later for the same host.
Cookie scope must follow the effective Host when present, or the effective
connection host when Host is absent. With an override, all requests use the same
cookie host, including captures with different Host values or server ports.
If cookie reuse depends on the supplied argument, generated code
guards cookie imports, helper definition and initialization with a runtime host
condition. If reuse cannot occur, omit the scaffolding entirely.
Response-dependent tokens/IDs remain manually editable.

Preserve cancellable, paginated chain preparation and complete-export failure
handling. Do not skip unsupported requests or copy a partial script.

## Implementation Location

- `frontend/src/requestExport.ts`: shared Python command-line setup, destination
  construction, individual/chain generation and flushed response output.
- `frontend/src/chainExport.ts`: already supplies each member's captured server
  address to the parser; preserve one-page/one-payload-at-a-time loading.
- Existing `Stream` and chain-toolbar actions consume the revised generators.
- Maintain the single-request and chain replay coverage in `test/flag_test` and
  `test/session_chains`; no backend API or storage migration is needed.

## Acceptance Checks

1. Both Python export actions produce runnable scripts without a destination
   argument, using the appropriate captured IP/port.
2. IPv4/FQDN/IPv6 overrides change connection hosts and preserve ports, method,
   path escaping, repeated query parameters and readable UTF-8 bodies.
3. A chain with different captured server IPs keeps per-member defaults and
   applies one supplied host to every request, including multi-request streams.
4. Captured Host values remain unchanged without an override, including
   IP-valued Host. An override replaces existing Host with the new destination
   and captured server port. A missing Host stays absent from generated explicit headers;
   wire tests may observe the HTTP client's automatically generated Host.
5. Invalid destinations and excess arguments cause no network requests.
6. First/fresh/deleted cookies still work with and without Host and with a
   destination override. Cookie helpers remain conditional.
7. A first response containing a test flag becomes observable through piped
   stdout while the script is still running and a later request is waiting.
   Checking only the final output or the generated print text is insufficient.
8. HTTP error bodies print, later requests continue after HTTP error statuses,
   redirects stay disabled and transport errors stop replay.
9. Existing formatting, unsupported-input, cancellation and no-partial-export
   checks continue to pass.
10. Run frontend lint/build and the relevant browser/replay suites. Follow the
    repository's Docker restriction: agents provide Docker-backed commands for
    the user to execute and never invoke Docker or OrbStack themselves.

## Verification

On 2026-10-10, frontend TypeScript lint and the Vite production build passed.
After the Host clarification, all 36 unique tests in the local session-chain
browser/replay configuration passed: 35 in the full run and the chain clipboard
test on a focused rerun after updating its expected cookie-seeding call to use
the effective Host. `NEWHATCH_TEST_RESULTS` was explicitly set for shared screenshots.

Generated individual and chain Python scripts executed against temporary
loopback HTTP servers. Coverage includes defaults, IPv4/FQDN/IPv6 overrides,
multiple requests and destination ports, original/adaptive/absent Host, invalid argv,
cookie replacement/deletion and mixed Host cases, transport failure, and actual
flag output through buffered pipes before script exit. Different captured Host
values also converge under argv with cookie seeding, response replacement and
deletion across ports on real IPv4/IPv6 connections. FQDN tests use a
controlled local DNS resolver; they do not contact an external service.
No Docker or production capture was run. See
[local verification commands](../test/session_chains/README.md).
