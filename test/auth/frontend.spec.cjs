const { test, expect } = require("@playwright/test");

test("auth gate, login errors, logout, expiration and stopped polling", async ({ page }) => {
  let authenticated = false;
  let dataRequests = 0;
  let unauthorizedPayload = false;
  await page.route("**/api/**", async (route) => {
    const path = new URL(route.request().url()).pathname;
    const send = (status, body) => route.fulfill({ status, contentType: "application/json", body: JSON.stringify(body) });
    if (path === "/api/auth/me") return send(authenticated ? 200 : 401, { authenticated, username: "team" });
    if (path === "/api/payload-search/limits") return send(200, { max_terms: 10, max_term_chars: 128 });
    if (path === "/api/auth/login") {
      authenticated = route.request().postDataJSON().password === "test-only-password";
      return send(authenticated ? 200 : 401, { authenticated, username: "team" });
    }
    if (path === "/api/auth/logout") {
      authenticated = false;
      return route.fulfill({ status: 204 });
    }
    dataRequests++;
    if (!authenticated || (unauthorizedPayload && path.includes("/payload/"))) return send(401, { error: "unauthorized" });
    if (path === "/api/sources") return send(200, []);
    if (path === "/api/collectors") return send(200, { mode: "local", collectors: [] });
    if (path === "/api/sessions") return send(200, { items: [], next_cursor: null });
    return send(404, {});
  });
  await page.goto("/");
  await expect(page.getByRole("heading", { name: "Sign in" })).toBeVisible();
  expect(dataRequests).toBe(0);
  await expect(page).toHaveTitle("Нюхач");
  await page.setViewportSize({ width: 1280, height: 800 });
  expect(await page.locator(".auth-brand img").evaluate((image) => image.complete && image.naturalWidth > 0)).toBe(true);
  await page.screenshot({ path: (process.env.NEWHATCH_TEST_RESULTS || "/results") + "/login-desktop.png" });
  await page.setViewportSize({ width: 375, height: 812 });
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await page.screenshot({ path: (process.env.NEWHATCH_TEST_RESULTS || "/results") + "/login-mobile.png" });
  await page.getByLabel("Username").fill("team");
  await page.getByLabel("Password").fill("wrong");
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  await expect(page.getByRole("alert")).toHaveText("Invalid username or password.");
  expect(dataRequests).toBe(0);
  await page.getByLabel("Password").fill("test-only-password");
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Sessions", exact: true })).toBeVisible();
  await expect(page.getByText(/Collector (online|offline)/)).toHaveCount(0);
  await page.getByRole("button", { name: "Sign out", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Sign in" })).toBeVisible();
  await page.getByLabel("Password").fill("test-only-password");
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Sessions", exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: "Refresh sessions" })).toBeEnabled();
  authenticated = false;
  await page.getByRole("button", { name: "Refresh sessions" }).click();
  await expect(page.getByRole("heading", { name: "Sign in" })).toBeVisible();
  const afterExpiration = dataRequests;
  await page.waitForTimeout(5500);
  expect(dataRequests).toBe(afterExpiration);
  expect(await page.evaluate(() => localStorage.length + sessionStorage.length)).toBe(0);

  // The raw payload fetch uses the same 401 notification path as JSON requests.
  await page.getByLabel("Password").fill("test-only-password");
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Sessions", exact: true })).toBeVisible();
  unauthorizedPayload = true;
  await page.evaluate(async () => {
    const { api } = await import("/src/api.ts");
    await api.getPayload(1, "c2s").catch(() => {});
  });
  await expect(page.getByRole("heading", { name: "Sign in" })).toBeVisible();
});

test("no protected requests while me is pending; network errors allow retry", async ({ page }) => {
  const requests = [];
  let release;
  const pending = new Promise((resolve) => { release = resolve; });
  await page.route("**/api/**", async (route) => {
    requests.push(new URL(route.request().url()).pathname);
    await pending;
    return route.fulfill({ status: 503, body: "" });
  });
  await page.goto("/");
  await expect(page.getByRole("status")).toHaveText("Checking session...");
  expect(requests.length).toBeGreaterThan(0);
  expect(requests.every((path) => path === "/api/auth/me")).toBe(true);
  const initialRequests = requests.length;
  release();
  await expect(page.getByRole("alert")).toHaveText("Unable to connect.");
  await page.getByRole("button", { name: "Retry" }).click();
  await expect(page.getByRole("alert")).toHaveText("Unable to connect.");
  expect(requests.length).toBe(initialRequests + 1);
});

test("language defaults from the browser and an explicit choice persists", async ({ page }) => {
  await page.addInitScript(() => {
    if (!sessionStorage.getItem("localization_test_initialized")) {
      localStorage.removeItem("newhatch_language");
      sessionStorage.setItem("localization_test_initialized", "true");
    }
    Object.defineProperty(navigator, "language", { configurable: true, get: () => "ru-RU" });
    Object.defineProperty(navigator, "languages", { configurable: true, get: () => ["ru-RU", "ru"] });
  });
  await page.route("**/api/**", async (route) => {
    const path = new URL(route.request().url()).pathname;
    const send = (body) => route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify(body) });
    if (path === "/api/auth/me") return send({ authenticated: true, username: "team" });
    if (path === "/api/payload-search/limits") return send({ max_terms: 10, max_term_chars: 128 });
    if (path === "/api/sources") return send([]);
    if (path === "/api/collectors") return send({ mode: "local", collectors: [] });
    if (path === "/api/sessions") return send({ items: [], next_cursor: null });
    return route.fulfill({ status: 404, body: "{}" });
  });

  await page.goto("/");
  await expect(page.getByRole("heading", { name: "Сессии", exact: true })).toBeVisible();
  expect(await page.evaluate(() => localStorage.getItem("newhatch_language"))).toBeNull();
  await expect(page.locator("html")).toHaveAttribute("lang", "ru");

  const switcher = page.getByRole("button", { name: "Переключить на английский" });
  await expect(page.getByRole("button", { name: "Выйти", exact: true })).toBeVisible();
  expect(await page.locator(".language-button").evaluate((button) => button.nextElementSibling?.classList.contains("logout-button"))).toBe(true);

  await switcher.click();
  await expect(page.getByRole("heading", { name: "Sessions", exact: true })).toBeVisible();
  expect(await page.evaluate(() => localStorage.getItem("newhatch_language"))).toBe("en");
  await expect(page.locator("html")).toHaveAttribute("lang", "en");

  await page.reload();
  await expect(page.getByRole("heading", { name: "Sessions", exact: true })).toBeVisible();
});

for (const pretty of [false, true]) {
  test(`payload renderer decodes embedded Unicode with safe escaping and bounded depth; Format JSON=${pretty}`, async ({ page }) => {
    await page.route("**/api/**", (route) => route.fulfill({ status: 401, body: "{}" }));
    await page.goto("/");
    const embedded = String.raw`{"message":"\u041f\u0440\u0438\u0432\u0435\u0442","emoji":"\ud83e\udd80","quote":"\"hello\"","path":"C:\\tmp\\file","literal":"\\u041f","lines":"one\ntwo","array":"[{\"text\":\"\\u044f\"}]"}`;
    const body = JSON.stringify({ nested: embedded, list: String.raw`["\u042f",{"text":"\u043c\u0438\u0440"}]` });
    const atDepth = (depth) => {
      let value = String.raw`{"text":"\u042f"}`;
      for (let level = 1; level < depth; level++) value = JSON.stringify({ nested: value });
      return JSON.stringify({ nested: value });
    };
    const inputs = [
      body,
      `HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n${body}`,
      `HTTP/1.0 200 OK\nContent-Type: application/json\n\n${body}`,
      String.raw`{"message":"\u041f\u0440\u0438\u0432\u0435\u0442","encoded":"%D0%BC%D0%B8%D1%80"}`,
      JSON.stringify({ nested: String.raw`{"message":"\u04ZZ"}` }),
      String.raw`{"message":"\u042f"`,
      atDepth(4),
      atDepth(5),
    ];
    const results = await page.evaluate(async ({ inputs, pretty }) => {
      const { displayPayload } = await import("/src/payloadDisplay.ts");
      return inputs.map((input) => {
        const bytes = new TextEncoder().encode(input);
        const original = Array.from(bytes);
        const displayed = displayPayload(bytes, pretty);
        return { displayed, unchanged: original.every((byte, index) => byte === bytes[index]) };
      });
    }, { inputs, pretty });
    expect(results.every((result) => result.unchanged)).toBe(true);
    const standalone = results[0].displayed;
    expect(standalone).toContain("Привет");
    expect(standalone).toContain("🦀");
    expect(results[1].displayed).toBe(inputs[1].slice(0, inputs[1].indexOf("\r\n\r\n") + 4) + standalone);
    expect(results[2].displayed).toBe(inputs[2].slice(0, inputs[2].indexOf("\n\n") + 2) + standalone);
    const parsed = JSON.parse(standalone);
    const nested = pretty ? parsed.nested : JSON.parse(parsed.nested);
    expect(nested).toEqual({
      message: "Привет", emoji: "🦀", quote: '"hello"', path: "C:\\tmp\\file",
      literal: String.raw`\u041f`, lines: "one\ntwo",
      array: pretty ? [{ text: "я" }] : JSON.stringify([{ text: "я" }]),
    });
    expect(pretty ? parsed.list : JSON.parse(parsed.list)).toEqual(["Я", { text: "мир" }]);
    expect(standalone.includes("\n")).toBe(pretty);
    expect(JSON.parse(results[3].displayed)).toEqual({ message: "Привет", encoded: "мир" });
    expect(JSON.parse(results[4].displayed).nested).toBe(String.raw`{"message":"\u04ZZ"}`);
    expect(results[5].displayed).toBe('{"message":"Я"');
    expect(results[6].displayed).toContain("Я");
    expect(results[6].displayed).not.toContain(String.raw`\u042f`);
    expect(results[7].displayed).toContain(String.raw`\u042f`);
    expect(results[7].displayed).not.toContain("Я");
    for (const result of results.slice(6)) expect(() => JSON.parse(result.displayed)).not.toThrow();
  });
}

test("session detail shows embedded Unicode by default in both directions and preserves Hex", async ({ page }) => {
  const body = JSON.stringify({ nested: String.raw`{"message":"\u041f\u0440\u0438\u0432\u0435\u0442","emoji":"\ud83e\udd80"}` });
  const c2s = `POST /unicode HTTP/1.1\r\nHost: test\r\nContent-Length: ${Buffer.byteLength(body)}\r\n\r\n${body}`;
  const s2c = body;
  const session = {
    id: 21, source_id: 1, source_name: "web", collector_id: null,
    started_at: 1700000000000000, ended_at: 1700000000100000,
    first_payload_c2s_at: null, first_payload_s2c_at: null,
    client_ip: "127.0.0.2", client_port: 40021, server_ip: "127.0.0.1", server_port: 8080,
    protocol: "http", bytes_c2s: Buffer.byteLength(c2s), bytes_s2c: Buffer.byteLength(s2c),
    contains_flag: false, flag_direction: "none", flag_count: 0, suricata_alerts: 0, incomplete: false,
    http: { method: "POST", host: "test", path: "/unicode", status: 200, content_type: "application/json" },
  };
  await page.route("**/api/**", (route) => {
    const path = new URL(route.request().url()).pathname;
    const replies = {
      "/api/auth/me": { authenticated: true, username: "team" },
      "/api/payload-search/limits": { max_terms: 10, max_term_chars: 128 },
      "/api/sources": [{ id: 1, name: "web", port: 8080, enabled: true }],
      "/api/collectors": { mode: "local", collectors: [] },
      "/api/sessions": { items: [session], next_cursor: null },
      "/api/sessions/21/flag-matches": { c2s: [], s2c: [] },
    };
    if (path.endsWith("/payload/c2s") || path.endsWith("/payload/s2c")) {
      return route.fulfill({ contentType: "application/octet-stream", body: path.endsWith("/c2s") ? c2s : s2c });
    }
    return route.fulfill({ status: path in replies ? 200 : 404, contentType: "application/json", body: JSON.stringify(replies[path] ?? {}) });
  });
  await page.goto("/");
  await page.locator('[data-session-id="21"]').click();
  const toggle = page.getByRole("checkbox", { name: "Format JSON" });
  const streams = page.locator(".detail-panel .stream pre");
  await expect(toggle).not.toBeChecked();
  await expect(streams).toHaveCount(2);
  for (const stream of [streams.first(), streams.last()]) {
    await expect(stream).toContainText("Привет");
    await expect(stream).toContainText("🦀");
    await expect(stream).not.toContainText(String.raw`\u041f`);
  }
  const compact = await streams.allTextContents();
  expect(typeof JSON.parse(compact[1]).nested).toBe("string");
  await toggle.check();
  await expect(streams.last()).toContainText('"message": "Привет"');
  expect(JSON.parse(await streams.last().textContent()).nested).toEqual({ message: "Привет", emoji: "🦀" });
  await page.getByRole("button", { name: "Hex", exact: true }).click();
  await expect(toggle).toBeDisabled();
  const hex = await streams.allTextContents();
  for (const [index, raw] of [c2s, s2c].entries()) {
    const captured = hex[index].split("\n").flatMap((line) => line.slice(10, 57).trim().split(/ +/).map((byte) => Number.parseInt(byte, 16)));
    expect(captured).toEqual(Array.from(Buffer.from(raw)));
  }
  await page.getByRole("button", { name: "Text", exact: true }).click();
  await toggle.uncheck();
  await expect.poll(() => streams.allTextContents()).toEqual(compact);
});
