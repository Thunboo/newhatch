const { test, expect } = require("@playwright/test");
const { spawnSync } = require("node:child_process");
const flag = require("./html/flag.json");

function capturedRequest({ method = "GET", target = "/flag", port = 18080, headers = [], body }) {
  const lines = [method + " " + target + " HTTP/1.1", "Host: fixture.test:" + port, ...headers.map(([name, value]) => name + ": " + value)];
  if (body !== undefined) lines.push("Content-Length: " + Buffer.byteLength(body));
  return lines.join("\r\n") + "\r\n\r\n" + (body ?? "");
}

async function openDetail(page, payload, { port = 18080, language = "en" } = {}) {
  await page.addInitScript((language) => {
    localStorage.setItem("newhatch_language", language);
    window.copiedPayloads = [];
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText: async (text) => { window.copiedPayloads.push(text); } },
    });
  }, language);
  const session = {
    id: 12, source_id: 1, source_name: "flag-test",
    started_at: 1789900000000000, ended_at: 1789900000100000,
    client_ip: "127.0.0.2", client_port: 50000,
    server_ip: "127.0.0.1", server_port: port,
    protocol: "http", bytes_c2s: Buffer.byteLength(payload), bytes_s2c: 60,
    contains_flag: true, flag_direction: "s2c", flag_count: 1, suricata_alerts: 0,
    // A timeout can mark a TCP session incomplete even when this request is complete.
    incomplete: true,
    http: { method: "GET", host: "fixture.test", path: "/flag", status: 200, content_type: "application/json" },
  };
  await page.route("**/api/**", async (route) => {
    const path = new URL(route.request().url()).pathname;
    const json = (body) => route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify(body) });
    if (path === "/api/auth/me") return json({ authenticated: true, username: "team" });
    if (path === "/api/health") return json({ status: "ok" });
    if (path === "/api/sources") return json([{ id: 1, name: "flag-test", port, enabled: true }]);
    if (path === "/api/collectors") return json({ mode: "local", collectors: [] });
    if (path === "/api/sessions") return json({ items: [session], next_cursor: null });
    if (path.endsWith("/flag-matches")) return json({ c2s: [], s2c: [] });
    if (path.endsWith("/payload/c2s")) return route.fulfill({ contentType: "application/octet-stream", body: Buffer.from(payload) });
    if (path.endsWith("/payload/s2c")) return route.fulfill({ contentType: "application/octet-stream", body: "HTTP/1.1 200 OK\r\n\r\n" + JSON.stringify(flag) });
    return route.fulfill({ status: 404, body: "" });
  });
  await page.goto("/");
  await page.locator(".session-table tbody tr").click();
  await expect(page.locator(".stream")).toHaveCount(2);
  return page.locator(".stream").first();
}

async function copyExport(page, stream, label) {
  const count = await page.evaluate(() => window.copiedPayloads.length);
  await stream.getByRole("button", { name: label, exact: true }).click();
  await expect.poll(() => page.evaluate(() => window.copiedPayloads.length)).toBe(count + 1);
  return page.evaluate(() => window.copiedPayloads.at(-1));
}

function executeExport(format, code) {
  // Only fixed, repository-owned test requests are executed, against loopback
  // nginx endpoints inside this disposable test container.
  const command = format === "bash" ? "bash" : "python3";
  const result = spawnSync(command, ["-"], {
    input: code, encoding: "utf8", timeout: 15_000, maxBuffer: 2 * 1024 * 1024,
  });
  expect(result.error, result.stderr).toBeUndefined();
  expect(result.status, result.stderr).toBe(0);
  return JSON.parse(result.stdout);
}

const formats = [
  { format: "bash", label: "Copy as Bash (cURL)" },
  { format: "python", label: "Copy as Python (requests)" },
];

test("C2S export buttons precede ordinary copy and replay GET against the nginx flag fixture", async ({ page }) => {
  const stream = await openDetail(page, capturedRequest({ target: "/flag?player=team01&payload=demo-exploit" }));
  const buttons = stream.getByRole("button");
  await expect(buttons).toHaveCount(3);
  await expect(buttons.nth(0)).toHaveAttribute("aria-label", formats[0].label);
  await expect(buttons.nth(1)).toHaveAttribute("aria-label", formats[1].label);
  await expect(buttons.nth(2)).toHaveAttribute("aria-label", "Copy Client -> server");
  await expect(buttons.nth(0).locator("img.export-logo")).toHaveAttribute("src", /Curl-logo.*\.svg/);
  await expect(buttons.nth(1).locator("img.export-logo")).toHaveAttribute("src", /python-logo/);
  await expect(buttons.nth(0)).toHaveCSS("width", "64px");
  await expect(buttons.nth(1)).toHaveCSS("width", "76px");
  await expect(buttons.nth(0).locator("img.export-logo")).toHaveCSS("width", "56px");
  await expect(buttons.nth(0).locator("img.export-logo")).toHaveCSS("height", "18px");
  await expect(buttons.nth(1).locator("img.export-logo")).toHaveCSS("width", "68px");
  await expect(buttons.nth(1).locator("img.export-logo")).toHaveCSS("height", "20px");
  await expect(buttons.nth(0)).toHaveCSS("background-color", "rgb(232, 235, 238)");
  await expect(buttons.nth(1)).toHaveCSS("background-color", "rgb(232, 235, 238)");
  await expect(buttons.nth(0)).toHaveAttribute("title", formats[0].label);
  await expect(page.locator(".stream").nth(1).getByRole("button")).toHaveCount(1);
  const boxes = await Promise.all([0, 1, 2].map((index) => buttons.nth(index).boundingBox()));
  expect(boxes[0].x).toBeLessThan(boxes[1].x);
  expect(boxes[1].x).toBeLessThan(boxes[2].x);
  await buttons.nth(0).locator("img.export-logo").dispatchEvent("error");
  await buttons.nth(1).locator("img.export-logo").dispatchEvent("error");
  await expect(buttons.nth(0)).toHaveText("cUrl");
  await expect(buttons.nth(1)).toHaveText("python");
  for (const { format, label } of formats) {
    expect(executeExport(format, await copyExport(page, stream, label))).toEqual(flag);
  }
  expect(await copyExport(page, stream, "Copy Client -> server")).toBe(await stream.locator("pre").textContent());
});

for (const pretty of [false, true]) {
  test("exports the visible Unicode JSON body through nginx; Format JSON=" + pretty, async ({ page }) => {
    const body = JSON.stringify({
      message: "Привет",
      escaped: "%D0%BC%D0%B8%D1%80",
      nested: JSON.stringify({ text: "вложенный JSON" }),
      literal: "@file $HOME $(printf injected) 'single' \"double\"",
    }).replace("Привет", "\\u041f\\u0440\\u0438\\u0432\\u0435\\u0442");
    const target = "/echo?tag=%D1%82%D0%B5%D1%81%D1%82&tag=second&value=A%26B%3D1&literal=%24%28printf%20injected%29";
    const stream = await openDetail(page, capturedRequest({
      method: "POST", target, port: 18081, body,
      headers: [
        ["Content-Type", "application/json"],
        ["Cookie", "session=fixture"],
        ["Authorization", "Bearer fixture-only"],
        ["X-Replay", "literal $(printf wrong) 'quote'"],
        ["Connection", "keep-alive, X-Connection-Only"],
        ["X-Connection-Only", "discard-me"],
      ],
    }), { port: 18081 });
    if (pretty) await page.getByRole("checkbox", { name: "Format JSON" }).check();
    const visible = await stream.locator("pre").textContent();
    const expectedBody = visible.slice(visible.indexOf("\r\n\r\n") + 4);
    expect(expectedBody).toContain("Привет");
    expect(expectedBody).toContain("мир");
    if (pretty) expect(expectedBody).toContain("\n");
    for (const { format, label } of formats) {
      const code = await copyExport(page, stream, label);
      expect(code).toContain("Привет");
      expect(code).not.toContain("\\u041f");
      expect(code).not.toContain("\\x");
      const received = executeExport(format, code);
      expect(received.method).toBe("POST");
      expect(received.path).toBe("/echo");
      expect(received.body).toBe(expectedBody);
      expect(received.bodyBytes).toBe(Buffer.byteLength(expectedBody));
      expect(received.headers["content-length"]).toBe(String(Buffer.byteLength(expectedBody)));
      expect(received.headers["content-type"]).toBe("application/json");
      expect(received.headers.host).toBe("fixture.test:18081");
      expect(received.headers.cookie).toBe("session=fixture");
      expect(received.headers.authorization).toBe("Bearer fixture-only");
      expect(received.headers["x-replay"]).toBe("literal $(printf wrong) 'quote'");
      expect(received.headers["x-connection-only"]).toBeUndefined();
      expect(received.params).toEqual([
        ["tag", "тест"], ["tag", "second"], ["value", "A&B=1"], ["literal", "$(printf injected)"],
      ]);
    }
  });
}

for (const body of ["@not-a-file\n''' $HOME $(printf injected)\nlast line\n\n", ""]) {
  test("preserves a literal text body including empty/trailing newlines: " + JSON.stringify(body), async ({ page }) => {
    const stream = await openDetail(page, capturedRequest({ method: "PUT", target: "/echo", port: 18081, body }), { port: 18081 });
    const visible = await stream.locator("pre").textContent();
    const expectedBody = visible.slice(visible.indexOf("\r\n\r\n") + 4);
    for (const { format, label } of formats) {
      const received = executeExport(format, await copyExport(page, stream, label));
      expect(received.method).toBe("PUT");
      expect(received.body).toBe(expectedBody);
      expect(received.headers["content-type"]).toBeUndefined();
    }
  });
}

const unsupported = [
  ["truncated body", "POST /flag HTTP/1.1\r\nContent-Length: 9\r\n\r\nx", "incomplete"],
  ["multiple requests", capturedRequest({}) + capturedRequest({}), "one complete"],
  ["chunked", "POST /flag HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n", "uncompressed"],
  ["compressed", capturedRequest({ headers: [["Content-Encoding", "gzip"]] }), "uncompressed"],
  ["duplicate headers", capturedRequest({ headers: [["X-Test", "a"], ["x-test", "b"]] }), "repeated"],
  ["displayed header newline", capturedRequest({ headers: [["X-Test", "one\\ntwo"]] }), "repeated"],
  ["non-HTTP", "HELLO\r\n\r\n", "HTTP/1.x"],
  ["binary body", Buffer.from([0xff, 0x00, 0x01]), "uncompressed"],
];
for (const [name, payload, tooltip] of unsupported) {
  test("unsupported " + name + " leaves ordinary copy available", async ({ page }) => {
    const stream = await openDetail(page, payload);
    for (const { label } of formats) {
      await expect(stream.getByRole("button", { name: label, exact: true })).toBeDisabled();
      await expect(stream.getByRole("button", { name: label, exact: true })).toHaveAttribute("title", new RegExp(tooltip));
    }
    await expect(stream.getByRole("button", { name: "Copy Client -> server", exact: true })).toBeEnabled();
  });
}

test("Hex preserves ordinary copy and disables readable exports; Russian tooltips fit on mobile", async ({ page }) => {
  await page.setViewportSize({ width: 375, height: 812 });
  const stream = await openDetail(page, capturedRequest({}), { language: "ru" });
  const bash = stream.getByRole("button", { name: "Копировать как Bash (cURL)", exact: true });
  const python = stream.getByRole("button", { name: "Копировать как Python (requests)", exact: true });
  await expect(bash).toBeEnabled();
  await expect(python).toBeEnabled();
  await expect(bash).toHaveAttribute("title", "Копировать как Bash (cURL)");
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await page.getByRole("button", { name: "Hex", exact: true }).click();
  await expect(bash).toBeDisabled();
  await expect(python).toBeDisabled();
  expect(await copyExport(page, stream, "Копировать: Клиент -> сервер")).toBe(await stream.locator("pre").textContent());
  await page.getByRole("button", { name: "Текст", exact: true }).click();
  await expect(bash).toBeEnabled();
  await expect(python).toBeEnabled();
});

test("clipboard fallback copies a readable Python script", async ({ page }) => {
  const stream = await openDetail(page, capturedRequest({}));
  await page.evaluate(() => {
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: undefined });
    document.execCommand = (command) => {
      if (command !== "copy") return false;
      window.copiedPayloads.push(document.querySelector("textarea").value);
      return true;
    };
  });
  expect(executeExport("python", await copyExport(page, stream, formats[1].label))).toEqual(flag);
});

test("HEAD with Content-Length zero exports without waiting for a response body", async ({ page }) => {
  const stream = await openDetail(page, capturedRequest({ method: "HEAD", body: "" }));
  for (const { format, label } of formats) {
    const code = await copyExport(page, stream, label);
    const input = format === "python" ? code + "\nprint(response.status_code)\n" : code;
    const result = spawnSync(format === "bash" ? "bash" : "python3", ["-"], {
      input, encoding: "utf8", timeout: 15_000,
    });
    expect(result.error, result.stderr).toBeUndefined();
    expect(result.status, result.stderr).toBe(0);
    if (format === "bash") expect(result.stdout).toMatch(/^HTTP\/1\.[01] 200/m);
    else expect(result.stdout.trim()).toBe("200");
  }
});
