const { test, expect } = require("@playwright/test");
const http = require("node:http");
const { spawn } = require("node:child_process");

function request(path, headers = [], body) {
  return `GET ${path} HTTP/1.1\r\n${headers.map(([name, value]) => `${name}: ${value}\r\n`).join("")}${body === undefined ? "" : `Content-Length: ${Buffer.byteLength(body)}\r\n`}\r\n${body ?? ""}`;
}

test.beforeEach(async ({ page }) => {
  await page.route("**/api/**", (route) => {
    const pathname = new URL(route.request().url()).pathname;
    const value = pathname === "/api/auth/me" ? { authenticated: true, username: "team" }
      : pathname === "/api/collectors" ? { mode: "local", collectors: [] }
      : pathname === "/api/sources" ? [] : { items: [], next_cursor: null };
    return route.fulfill({ contentType: "application/json", body: JSON.stringify(value) });
  });
  await page.goto("/");
});

async function generate(page, steps, port, pretty = false, kind = "chain") {
  return page.evaluate(async ({ steps, port, pretty, kind }) => {
    const { parseReplayRequest, parseReplayRequests, exportPython, PythonChainExport } = await import("/src/requestExport.ts");
    const { displayPayload } = await import("/src/payloadDisplay.ts");
    if (kind === "single") {
      const step = steps[0];
      const bytes = new TextEncoder().encode(step.payload);
      const result = parseReplayRequest(bytes, displayPayload(bytes, pretty), { host: step.host ?? "127.0.0.1", port: step.port ?? port });
      if (!result.ok) throw new Error(JSON.stringify(result));
      return exportPython(result.request);
    }
    const script = new PythonChainExport();
    steps.forEach((step) => {
      const result = parseReplayRequests(new TextEncoder().encode(step.payload), pretty, { host: step.host ?? "127.0.0.1", port: step.port ?? port });
      if (!result.ok) throw new Error(JSON.stringify(result));
      script.append(step.id, result.requests);
    });
    return script.finish();
  }, { steps, port, pretty, kind });
}

function runPython(code, args = [], onOutput = () => {}) {
  return new Promise((resolve, reject) => {
    const child = spawn(process.env.NEWHATCH_TEST_PYTHON || "python3", ["-", ...args], {
      stdio: ["pipe", "pipe", "pipe"],
      env: { ...process.env, PYTHONUNBUFFERED: "", NO_PROXY: "127.0.0.1,localhost,::1,service.example", no_proxy: "127.0.0.1,localhost,::1,service.example" },
    });
    let stdout = "", stderr = "";
    const timeout = setTimeout(() => { child.kill(); reject(new Error("Python replay timed out")); }, 15_000);
    child.stdout.on("data", (data) => { stdout += data; onOutput(stdout, child); });
    child.stderr.on("data", (data) => { stderr += data; });
    child.on("error", (error) => { clearTimeout(timeout); reject(error); });
    child.on("close", (status) => { clearTimeout(timeout); resolve({ status, stdout, stderr }); });
    child.stdin.end(code);
  });
}

async function executePython(code, args = [], onOutput) {
  const result = await runPython(code, args, onOutput);
  expect(result.status, result.stderr).toBe(0);
  return result.stdout;
}

function listen(server, host = "127.0.0.1", port = 0) {
  return new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen({ port, host, ipv6Only: true }, () => { server.removeListener("error", reject); resolve(server.address().port); });
  });
}

function close(server) {
  return new Promise((resolve) => { server.closeAllConnections(); server.close(resolve); });
}

const cookieHosts = ["fixture.test", "localhost", "127.0.0.1", "[::1]"];
for (const [host, destination] of cookieHosts.flatMap((host) => [undefined, "localhost"].map((destination) => [host, destination]))) {
  test(`generated Python replays every request, updates/deletes cookies and keeps redirects explicit (${host}, argv=${destination ?? "default"})`, async ({ page }) => {
    const received = [];
    const server = http.createServer(async (incoming, outgoing) => {
      const parts = [];
      for await (const part of incoming) parts.push(part);
      received.push({ path: incoming.url, method: incoming.method, headers: incoming.headers, body: Buffer.concat(parts).toString("utf8") });
      if (incoming.url === "/start") outgoing.setHeader("Set-Cookie", "sid=fresh; Path=/");
      if (incoming.url === "/work") outgoing.statusCode = 500;
      if (incoming.url === "/redirect") {
        outgoing.statusCode = 302;
        outgoing.setHeader("Location", "/unexpected");
        outgoing.setHeader("Set-Cookie", "sid=; Max-Age=0; Path=/");
      }
      outgoing.end(incoming.url);
    });
    await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
    try {
      const port = server.address().port;
      const base = [["Host", `${host}:${port}`]];
      const body = "Привет\nGET /inside-body HTTP/1.1\r\n\r\n''' literal \\ end\n";
      const start = request("/start", [...base, ["Cookie", 'sid=initial; theme=dark; quoted="a=b"']]);
      const work = request("/work", [...base, ["Cookie", "sid=captured-old"], ["Content-Type", "text/plain"]], body).replace(/^GET/, "POST");
      const code = await generate(page, [
        { id: 41, payload: start + work },
        { id: 42, payload: request("/redirect", [...base, ["Cookie", "sid=stale"]]) },
        { id: 43, payload: request("/last", [...base, ["Cookie", "sid=stale"]]) },
      ], port);
      expect(code).toContain("def seed_cookies(");
      const output = await executePython(code, destination ? [destination] : []);
      expect(received.map((item) => item.path)).toEqual(["/start", "/work", "/redirect", "/last"]);
      expect(received[0].headers.cookie).toContain("sid=initial");
      expect(received[1].headers.cookie).toContain("sid=fresh");
      expect(received[2].headers.cookie).toContain("sid=fresh");
      expect(received[3].headers.cookie).not.toMatch(/sid=/);
      expect(received.every((item) => item.headers.cookie.includes("theme=dark"))).toBe(true);
      expect(received[0].headers.cookie).toContain('quoted="a=b"');
      expect(received[1].method).toBe("POST");
      expect(received[1].body).toBe(body);
      expect(received[1].headers["content-length"]).toBe(String(Buffer.byteLength(body)));
      expect(received.every((item) => item.headers.host === `${destination ?? host}:${port}`)).toBe(true);
      expect(output).toContain("Session #41 · request 2 500");
      expect(output).toContain("Session #42 · request 1 302");
      expect(output).toContain("Session #43 · request 1 200");
      expect(output).toContain("/work");
      expect(output).toContain("/redirect");
    } finally { await new Promise((resolve) => server.close(resolve)); }
  });
}

test("unused initial-cookie helpers are omitted while live response cookies still work", async ({ page }) => {
  const received = [];
  const server = http.createServer((incoming, outgoing) => {
    received.push({ path: incoming.url, cookie: incoming.headers.cookie });
    if (incoming.url === "/start") outgoing.setHeader("Set-Cookie", "sid=fresh; Path=/");
    outgoing.end("ok");
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  try {
    const port = server.address().port;
    const base = [["Host", "fixture.test"]];
    const cases = [
      { name: "no initial cookie", first: [], second: [["Cookie", "sid=captured-old"]], expectedFirst: undefined, expectedSecond: "sid=fresh" },
      { name: "only one request", first: [["Cookie", "sid=initial"]], second: null, expectedFirst: "sid=initial" },
      { name: "cookie only in first request", first: [["Cookie", "sid=initial"]], second: [], expectedFirst: "sid=initial", expectedSecond: "sid=fresh" },
      { name: "different cookie name", first: [["Cookie", "sid=initial"]], second: [["Cookie", "other=old"]], expectedFirst: "sid=initial", expectedSecond: "sid=fresh" },
    ];
    for (const item of cases) {
      received.length = 0;
      const steps = [{ id: 41, payload: request("/start", [...base, ...item.first]) }];
      if (item.second !== null) steps.push({ id: 42, payload: request("/next", [...item.secondHost ?? base, ...item.second]) });
      const code = await generate(page, steps, port);
      expect(code, item.name).not.toMatch(/seed_cookies|SimpleCookie|urlsplit|first request seed/);
      await executePython(code);
      expect(received[0].cookie, item.name).toBe(item.expectedFirst);
      expect(received.length, item.name).toBe(steps.length);
      if (item.second !== null) expect(received[1].cookie, item.name).toBe(item.expectedSecond);
    }
  } finally { await new Promise((resolve) => server.close(resolve)); }
});

test("chain replay formats each JSON body before Unicode byte lengths are recalculated", async ({ page }) => {
  let received;
  const server = http.createServer(async (incoming, outgoing) => {
    const parts = [];
    for await (const part of incoming) parts.push(part);
    received = { body: Buffer.concat(parts).toString("utf8"), length: incoming.headers["content-length"] };
    outgoing.end("ok");
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  try {
    const body = '{"message":"\\u041f\\u0440\\u0438\\u0432\\u0435\\u0442","nested":"{\\"ok\\":true}"}';
    const code = await generate(page, [{ id: 41, payload: request("/json", [["Content-Type", "application/json"]], body).replace(/^GET/, "POST") }], server.address().port, true);
    await executePython(code);
    expect(received.body).toBe(JSON.stringify({ message: "Привет", nested: { ok: true } }, null, 2));
    expect(received.length).toBe(String(Buffer.byteLength(received.body)));
  } finally { await new Promise((resolve) => server.close(resolve)); }
});

for (const kind of ["single", "chain"]) {
  test(kind + " argv changes the destination and existing Host while preserving other headers/query/body", async ({ page }) => {
    const received = [];
    const server = http.createServer(async (incoming, outgoing) => {
      const parts = [];
      for await (const part of incoming) parts.push(part);
      received.push({ url: incoming.url, method: incoming.method, headers: incoming.headers, body: Buffer.concat(parts).toString("utf8") });
      outgoing.end("FLAG{argv-target}");
    });
    const port = await listen(server);
    try {
      const body = "Привет\n''' literal \\ end\n";
      for (const host of ["fixture.test:8080", "192.0.2.9:8080", undefined]) {
        for (const destination of [undefined, "127.0.0.1", "localhost", "service.example"]) {
          received.length = 0;
          const payload = request("/path%2Fpart?tag=one&tag=two&value=A%26B", [
            ...(host ? [["hOsT", host]] : []), ["Content-Type", "text/plain"], ["X-Replay", "captured"],
          ], body).replace(/^GET/, "POST");
          const code = await generate(page, [{ id: 41, host: destination ? "192.0.2.9" : "127.0.0.1", payload }], port, false, kind);
          if (!host) expect(code).not.toMatch(/["']host["']\s*[:\]]/i);
          // Resolve the fixture FQDN locally, asserting that argv reaches DNS.
          const input = destination === "service.example" ? [
            "import socket",
            "test_getaddrinfo = socket.getaddrinfo",
            "def test_resolve(host, port, *args, **kwargs):",
            '    assert host == "service.example", host',
            '    return test_getaddrinfo("127.0.0.1", port, *args, **kwargs)',
            "socket.getaddrinfo = test_resolve", "", code,
          ].join("\n") : code;
          const output = await executePython(input, destination ? [destination] : []);
          expect(received).toHaveLength(1);
          expect(received[0].method).toBe("POST");
          expect(received[0].url).toBe("/path%2Fpart?tag=one&tag=two&value=A%26B");
          expect(received[0].body).toBe(body);
          expect(received[0].headers["content-length"]).toBe(String(Buffer.byteLength(body)));
          expect(received[0].headers["x-replay"]).toBe("captured");
          expect(received[0].headers.host).toBe(destination ? destination + ":" + port : host ?? "127.0.0.1:" + port);
          expect(output).toContain("FLAG{argv-target}");
        }
      }
    } finally { await close(server); }
  });

  test(kind + " defaults and argv support real IPv6 connections with or without brackets", async ({ page }) => {
    const received = [];
    const server = http.createServer((incoming, outgoing) => { received.push(incoming.headers.host); outgoing.end("FLAG{ipv6}"); });
    const port = await listen(server, "::1");
    try {
      for (const host of [undefined, "fixture.test:8080", "[2001:db8::9]:8080"]) {
        for (const destination of [undefined, "::1", "[::1]"]) {
          received.length = 0;
          const code = await generate(page, [{ id: 41, host: destination ? "192.0.2.9" : "::1", payload: request("/flag", host ? [["Host", host]] : []) }], port, false, kind);
          expect(await executePython(code, destination ? [destination] : [])).toContain("FLAG{ipv6}");
          expect(received).toEqual([destination ? "[::1]:" + port : host ?? "[::1]:" + port]);
        }
      }
    } finally { await close(server); }
  });

  test(kind + " invalid destinations and extra arguments fail before any request", async ({ page }) => {
    const received = [];
    const server = http.createServer((incoming, outgoing) => { received.push(incoming.url); outgoing.end("unexpected"); });
    const port = await listen(server);
    try {
      const code = await generate(page, [{ id: 41, payload: request("/must-not-send") }], port, false, kind);
      for (const args of [
        [""], [" "], ["http://localhost"], ["localhost:8080"], ["localhost/path"], ["localhost?query"],
        ["user@localhost"], ["localhost#fragment"], ["127.0.0.999"], ["[127.0.0.1]"], ["[::1"], ["::gg"],
        ["[::1]:8080"], ["bad_host"], ["-bad.test"], ["bad..test"], ["a".repeat(64) + ".test"],
        ["localhost\n"], ["fe80::1%lo0"], ["localhost", "extra"],
      ]) {
        const result = await runPython(code, args);
        expect(result.status, JSON.stringify(args)).toBe(2);
        expect(result.stderr, JSON.stringify(args)).toContain("usage:");
        expect(result.stdout).toBe("");
      }
      expect(received).toEqual([]);
    } finally { await close(server); }
  });

  test(kind + " prints a flag through piped stdout while a later request is still waiting", async ({ page }) => {
    let waitingResponse, python, sawFlag = false, execution;
    const server = http.createServer((incoming, outgoing) => {
      if (incoming.url === "/wait") waitingResponse = outgoing;
      else outgoing.end("FLAG{flushed-before-exit}");
    });
    const port = await listen(server);
    try {
      let code = await generate(page, [{ id: 41, payload: request("/flag") + (kind === "chain" ? request("/wait") : "") }], port, false, kind);
      // Hold the single-request script alive too; stdout must not wait for exit.
      if (kind === "single") code += '\nrequests.get("http://127.0.0.1:' + port + '/wait", timeout=10)\n';
      execution = runPython(code, [], (output, child) => { python = child; sawFlag = output.includes("FLAG{flushed-before-exit}"); });
      await expect.poll(() => !!waitingResponse && sawFlag, { timeout: 3000 }).toBe(true);
      expect(python.exitCode).toBeNull();
      expect(waitingResponse.writableEnded).toBe(false);
      waitingResponse.end("done");
      expect((await execution).status).toBe(0);
    } finally { await close(server); if (execution) await execution; }
  });
}

test("chain defaults retain each session destination and argv overrides all requests without changing ports", async ({ page }) => {
  const received = [], servers = [];
  const make = (name) => {
    const server = http.createServer((incoming, outgoing) => { received.push(name + incoming.url); outgoing.end("ok"); });
    servers.push(server);
    return server;
  };
  try {
    const firstPort = await listen(make("v4-first"), "127.0.0.1");
    await listen(make("v6-first"), "::1", firstPort);
    const secondPort = await listen(make("v6-second"), "::1");
    await listen(make("v4-second"), "127.0.0.1", secondPort);
    const code = await generate(page, [
      { id: 41, host: "127.0.0.1", port: firstPort, payload: request("/start") + request("/next") },
      { id: 42, host: "::1", port: secondPort, payload: request("/last") },
    ], firstPort);
    for (const item of [
      { args: [], expected: ["v4-first/start", "v4-first/next", "v6-second/last"] },
      { args: ["127.0.0.1"], expected: ["v4-first/start", "v4-first/next", "v4-second/last"] },
      { args: ["::1"], expected: ["v6-first/start", "v6-first/next", "v6-second/last"] },
    ]) {
      received.length = 0;
      await executePython(code, item.args);
      expect(received).toEqual(item.expected);
    }
  } finally { await Promise.all(servers.map(close)); }
});

test("argv without Host seeds cookies for the new destination and preserves response replacement/deletion", async ({ page }) => {
  const received = [];
  const server = http.createServer((incoming, outgoing) => {
    received.push(incoming.headers.cookie);
    if (incoming.url === "/start") outgoing.setHeader("Set-Cookie", "sid=fresh; Path=/");
    if (incoming.url === "/delete") outgoing.setHeader("Set-Cookie", "sid=; Max-Age=0; Path=/");
    outgoing.end("ok");
  });
  const port = await listen(server);
  try {
    const code = await generate(page, [
      { id: 41, host: "192.0.2.9", payload: request("/start", [["Cookie", "sid=initial; theme=dark"]]) },
      { id: 42, host: "198.51.100.9", payload: request("/delete", [["Cookie", "sid=stale"]]) + request("/last", [["Cookie", "sid=stale"]]) },
    ], port);
    await executePython(code, ["localhost"]);
    expect(received).toHaveLength(3);
    expect(received[0]).toContain("sid=initial");
    expect(received[1]).toContain("sid=fresh");
    expect(received[2]).not.toContain("sid=");
    expect(received.every((cookie) => cookie.includes("theme=dark"))).toBe(true);
  } finally { await close(server); }
});

test("argv unifies different captured Host cookie scopes across ports, including IPv6", async ({ page }) => {
  const received = [], servers = [];
  const make = () => {
    const server = http.createServer((incoming, outgoing) => {
      received.push({ path: incoming.url, host: incoming.headers.host, cookie: incoming.headers.cookie });
      if (incoming.url === "/next") outgoing.setHeader("Set-Cookie", "sid=fresh; Path=/");
      if (incoming.url === "/delete") outgoing.setHeader("Set-Cookie", "sid=; Max-Age=0; Path=/");
      outgoing.end("ok");
    });
    servers.push(server);
    return server;
  };
  try {
    const firstPort = await listen(make());
    await listen(make(), "::1", firstPort);
    const laterPort = await listen(make());
    await listen(make(), "::1", laterPort);
    const hosts = ["First.test", "192.0.2.9:8080", "[2001:db8::9]:8080", "last.test"];
    const steps = ["/start", "/next", "/delete", "/last"].map((path, index) => ({
      id: 41 + index, port: index ? laterPort : firstPort,
      payload: request(path, [["hOsT", hosts[index]], ["Cookie", index ? "sid=stale" : "sid=initial; theme=dark"]]),
    }));
    const code = await generate(page, steps, firstPort);
    expect(code).toContain("seed_initial_cookies = target_host is not None");
    for (const destination of [undefined, "localhost", "::1"]) {
      received.length = 0;
      await executePython(code, destination ? [destination] : []);
      expect(received.map((item) => item.path)).toEqual(["/start", "/next", "/delete", "/last"]);
      const effective = destination === "::1" ? "[::1]" : destination;
      expect(received.map((item) => item.host)).toEqual(effective ? [firstPort, laterPort, laterPort, laterPort].map((port) => effective + ":" + port) : hosts);
      expect(received[0].cookie).toContain("sid=initial");
      if (destination) {
        // The first response sets no cookie: the second must receive the seed.
        expect(received[1].cookie).toContain("sid=initial");
        expect(received[2].cookie).toContain("sid=fresh");
        expect(received[3].cookie).not.toContain("sid=");
        expect(received.every((item) => item.cookie.includes("theme=dark"))).toBe(true);
      } else expect(received.slice(1).map((item) => item.cookie)).toEqual([undefined, undefined, undefined]);
    }
  } finally { await Promise.all(servers.map(close)); }
});

test("mixed captured/absent Host cookies follow the effective host with and without argv", async ({ page }) => {
  const received = [];
  const server = http.createServer((incoming, outgoing) => {
    received.push({ host: incoming.headers.host, cookie: incoming.headers.cookie });
    if (incoming.url === "/start") outgoing.setHeader("Set-Cookie", "sid=fresh; Path=/");
    outgoing.end("ok");
  });
  const port = await listen(server);
  try {
    for (const [firstHost, laterHost] of [
      [undefined, "localhost"], ["localhost", undefined],
      [undefined, "127.0.0.1"], ["127.0.0.1", undefined],
    ]) {
      const code = await generate(page, [
        { id: 41, payload: request("/start", [...(firstHost ? [["Host", firstHost]] : []), ["Cookie", "sid=initial; theme=dark"]]) },
        { id: 42, payload: request("/next", [...(laterHost ? [["Host", laterHost]] : []), ["Cookie", "sid=stale"]]) },
      ], port);
      for (const destination of [undefined, "localhost"]) {
        received.length = 0;
        await executePython(code, destination ? [destination] : []);
        expect(received[0].host).toBe(destination ? destination + ":" + port : firstHost ?? "127.0.0.1:" + port);
        expect(received[1].host).toBe(destination ? destination + ":" + port : laterHost ?? "127.0.0.1:" + port);
        expect(received[0].cookie).toContain("sid=initial");
        const same = !!destination || (firstHost ?? "127.0.0.1") === (laterHost ?? "127.0.0.1");
        if (same) {
          expect(received[1].cookie).toContain("sid=fresh");
          expect(received[1].cookie).toContain("theme=dark");
        } else expect(received[1].cookie).toBeUndefined();
      }
    }
  } finally { await close(server); }
});

test("a transport failure stops the chain after printing earlier responses", async ({ page }) => {
  const received = [];
  const server = http.createServer((incoming, outgoing) => { received.push(incoming.url); outgoing.end("FLAG{before-failure}"); });
  const port = await listen(server);
  try {
    const unavailable = http.createServer();
    const unavailablePort = await listen(unavailable);
    await close(unavailable);
    const code = await generate(page, [
      { id: 41, payload: request("/flag") },
      { id: 42, port: unavailablePort, payload: request("/unavailable") },
      { id: 43, payload: request("/must-not-send") },
    ], port);
    const result = await runPython(code);
    expect(result.status).not.toBe(0);
    expect(result.stdout).toContain("FLAG{before-failure}");
    expect(received).toEqual(["/flag"]);
  } finally { await close(server); }
});

const invalid = [
  ["truncated body", "POST /bad HTTP/1.1\r\nContent-Length: 9\r\n\r\nx", "incomplete"],
  ["truncated headers", "GET /bad HTTP/1.1\r\nHost: test", "incomplete"],
  ["chunked", "POST /bad HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n", "encoding"],
  ["compressed", request("/bad", [["Content-Encoding", "gzip"]]), "encoding"],
  ["Upgrade", request("/bad", [["Upgrade", "websocket"]]), "encoding"],
  ["duplicate length", request("/bad", [["Content-Length", "0"], ["Content-Length", "0"]]), "headers"],
  ["invalid length", request("/bad", [["Content-Length", "-1"]]), "headers"],
  ["duplicate header", request("/bad", [["X-Test", "a"], ["x-test", "b"]]), "headers"],
  ["non HTTP", "HELLO /bad TCP/1.0\r\n\r\n", "http"],
  ["binary", Buffer.concat([Buffer.from("POST /bad HTTP/1.1\r\nContent-Length: 1\r\n\r\n"), Buffer.from([255])]), "encoding"],
];
test("an unsupported later request fails the complete stream with its request index", async ({ page }) => {
  for (const [name, bad, problem] of invalid) {
    const bytes = Array.from(Buffer.concat([Buffer.from(request("/good")), Buffer.from(bad)]));
    const result = await page.evaluate(async (bytes) => {
      const { parseReplayRequests } = await import("/src/requestExport.ts");
      return parseReplayRequests(new Uint8Array(bytes), false, { host: "127.0.0.1", port: 1 });
    }, bytes);
    expect(result, name).toEqual({ ok: false, problem, requestIndex: 2 });
  }
});
