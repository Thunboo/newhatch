# Python Replay Export

Current contract for [#20](https://github.com/Thunboo/newhatch/issues/20),
used by individual C2S and whole-chain Python actions.
Supported input/display rules are in [MVP UX](mvp.md#individual-http-export);
chain loading, raw multi-request framing and cancellation are in
[chain export](session-chains.md#whole-chain-python-export).

## Destination and headers

Generated scripts accept one optional IP/FQDN argument:

```text
python3 replay.py
python3 replay.py 10.60.2.5
python3 replay.py service.example
```

Without an argument, use each request's own captured server_ip/server_port and
preserve its existing Host value and header-name casing. A chain must not substitute
the first member's destination for all requests.

With an argument, replace every connection host and existing Host value using
each request's captured server port. For port 8080, Host: old.example becomes
Host: 10.60.2.5:8080. Apply equally to IP-valued and virtual-host captures.
Support IPv4/FQDN/IPv6, normalizing/bracketing IPv6 authorities appropriately.

The argument is a host, not a URL or host:port; it cannot replace scheme, port,
path, query or body. Parse once using standard-library code and requests.
Invalid input or excess arguments exit nonzero with a concise usage/error
before any network request.

Do not synthesize an absent Host or other destination-related explicit headers.
Normal HTTP-client transport headers remain its responsibility; this is not
byte-for-byte raw replay or a requirement to suppress automatic Host.
Existing header validation/omissions still apply.

Individual and chain Python exports omit captured Authorization headers,
regardless of header-name casing. Configure authentication manually in the script:
Session automatically persists response cookies, but does not extract or retain
Authorization from responses. For a chain, set session.auth or session.headers
explicitly; response-derived Bearer tokens require manual extraction. Individual
scripts use requests.request and require explicit auth or headers there.
cURL exports retain captured Authorization.

## Execution and output

Every generated Python script starts with #!/usr/bin/env python3.

Individual exports use requests.request; a chain uses one requests.Session,
sequentially in (session started_at, id) order and then C2S byte order.
Wait for each response without reproducing capture timing gaps. A chain always
pauses before each request except the first, after the preceding response is
printed. Each pause independently samples random.uniform(DELAY_MIN, DELAY_MAX),
then calls time.sleep with that duration. DELAY_MIN = 0.5 and DELAY_MAX = 1.5
seconds are editable constants near the script's beginning. Apply pauses within
multi-request C2S payloads and across chain members; do not pause after the last
request. Individual exports have no delay or random/time imports. Keep a
10-second timeout per request and disable automatic redirects.

After every completed request, keep the variable response and emit
`print(response.text, flush=True)`. Print the actual replay body, including
HTTP-error bodies and flags, rather than captured S2C or extracted flag matches.
Output must be visible through a pipe before later requests finish.
Labels may supplement but cannot replace bodies. Flushing applies to complete
received bodies, not partial-response streaming.

HTTP error statuses do not skip subsequent captured requests. Transport failures
stop replay. Blocks remain readable/editable; displayed UTF-8 bodies and query
semantics follow the export contract and Content-Length is recalculated.

## Chain cookies

Cookie scope follows effective Host when present, otherwise effective destination.
Host comparison ignores case and port; cookie names are case-sensitive.
An override can unify different captured hosts/ports.

Emit seed_cookies, SimpleCookie/urlsplit imports and its explanatory comment only
when a cookie name from the first request is reused later for the same effective
host. Check later requests within the first TCP session and later members;
values may change. When argv determines host reuse, guard cookie imports/helper/
initialization with a runtime host condition. Omit scaffolding entirely if reuse
cannot occur.

When seeding is needed, initialize first-request cookies for the effective host
at path /. Captured Cookie does not reveal original paths.
Without reuse, send the first Cookie directly. In both cases omit later captured
Cookie headers so actual Set-Cookie replacement, expiry and deletion govern
following requests through Session. Response-dependent tokens/IDs need manual edits.

## Implementation and checks

frontend/src/requestExport.ts owns shared generation and command-line behavior;
frontend/src/chainExport.ts owns paginated, one-payload-at-a-time preparation.
No backend/storage change is required.

See [test/session_chains](../test/session_chains/README.md) and
[test/flag_test](../test/flag_test/README.md). Actual script execution must cover
captured defaults, IPv4/FQDN/IPv6 override, mixed/absent Host, ports, invalid argv,
cookie replacement/deletion, HTTP/transport failure and piped output before exit.
Retain unsupported-input, formatting, cancellation and no-partial-export coverage.
