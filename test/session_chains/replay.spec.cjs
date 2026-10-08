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

async function generate(page, steps, port, pretty = false) {
  return page.evaluate(async ({ steps, port, pretty }) => {
    const { parseReplayRequests, PythonChainExport } = await import("/src/requestExport.ts");
    const script = new PythonChainExport();
    steps.forEach((step) => {
      const result = parseReplayRequests(new TextEncoder().encode(step.payload), pretty, { host: "127.0.0.1", port });
      if (!result.ok) throw new Error(JSON.stringify(result));
      script.append(step.id, result.requests);
    });
    return script.finish();
  }, { steps, port, pretty });
}

function executePython(code) {
  return new Promise((resolve, reject) => {
    const child = spawn(process.env.NEWHATCH_TEST_PYTHON || "python3", ["-"], { stdio: ["pipe", "pipe", "pipe"] });
    let stdout = "", stderr = "";
    const timeout = setTimeout(() => { child.kill(); reject(new Error("Python replay timed out")); }, 15_000);
    child.stdout.on("data", (data) => { stdout += data; });
    child.stderr.on("data", (data) => { stderr += data; });
    child.on("error", (error) => { clearTimeout(timeout); reject(error); });
    child.on("exit", (status) => { clearTimeout(timeout); status === 0 ? resolve(stdout) : reject(new Error(stderr || `Python exited ${status}`)); });
    child.stdin.end(code);
  });
}

for (const host of ["fixture.test", "localhost", "127.0.0.1", "[::1]"]) {
  test(`generated Python replays every request, updates/deletes cookies and keeps redirects explicit (${host})`, async ({ page }) => {
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
      const output = await executePython(code);
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
      expect(received.every((item) => item.headers.host === `${host}:${port}`)).toBe(true);
      expect(output).toContain("Session #41 · request 2 500");
      expect(output).toContain("Session #42 · request 1 302");
      expect(output).toContain("Session #43 · request 1 200");
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
      { name: "different host", first: [["Cookie", "sid=initial"]], second: [["Cookie", "sid=initial"]], secondHost: [["Host", "other.test"]], expectedFirst: "sid=initial", expectedSecond: undefined },
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
