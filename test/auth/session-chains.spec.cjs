const { test, expect } = require("@playwright/test");

function session(id) {
  return {
    id, collector_id: "vulnbox-1", source_id: 1, source_name: "web",
    started_at: 1700000000000000 + (id - 41) * 800000,
    ended_at: 1700000000100000 + (id - 41) * 800000,
    first_payload_c2s_at: 1700000000100000 + (id - 41) * 800000,
    first_payload_s2c_at: 1700000000140000 + (id - 41) * 800000,
    client_ip: "10.0.4.17", client_port: 40000 + id, server_ip: "10.0.0.1", server_port: 8080,
    protocol: "http", bytes_c2s: 3000, bytes_s2c: 3000,
    contains_flag: id === 42, flag_direction: id === 42 ? "s2c" : "none", flag_count: id === 42 ? 1 : 0,
    suricata_alerts: 0, incomplete: false,
    http: { method: "POST", host: "test", path: `/${id}`, status: 200, content_type: "text/plain" },
  };
}

function chain(id = 41) {
  return { id, collector_id: "vulnbox-1", source_id: 1, source_name: "web", client_ip: "10.0.4.17",
    started_at: session(41).started_at, ended_at: session(70).ended_at, session_count: 30,
    bytes_c2s: 90000, bytes_s2c: 90000, contains_flag: true, flag_count: 1, suricata_alerts: 0,
    incomplete: false, snapshot_id: 100 };
}

async function routes(page) {
  const state = { memberRequests: [], payloadRequests: [], flagMatchRequests: [], chainRequests: [], rows: [chain()], c2s: new Map(), s2c: new Map(), flagMatches: new Map(), waitPayload: null, memberSummary: null };
  await page.route("**/api/**", async (route) => {
    const url = new URL(route.request().url());
    const send = (value) => route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify(value) });
    if (url.pathname === "/api/auth/me") return send({ authenticated: true, username: "team" });
    if (url.pathname === "/api/payload-search/limits") return send({ max_terms: 10, max_term_chars: 128 });
    if (url.pathname === "/api/sources") return send([{ id: 1, name: "web", port: 8080, enabled: true }]);
    if (url.pathname === "/api/collectors") return send({ mode: "local", collectors: [] });
    if (url.pathname === "/api/sessions") return send({ items: [session(41)], next_cursor: null });
    if (url.pathname === "/api/chains") {
      state.chainRequests.push(url.searchParams);
      return send({ items: state.rows, next_cursor: null });
    }
    if (url.pathname === "/api/chains/41/sessions") {
      state.memberRequests.push(url.searchParams);
      const older = url.searchParams.has("cursor");
      return send({ chain: state.memberSummary ?? chain(), items: Array.from({ length: older ? 10 : 20 }, (_, i) => session((older ? 61 : 41) + i)), next_cursor: older ? null : "members-older" });
    }
    const found = url.pathname.match(/^\/api\/sessions\/(\d+)\/(payload\/(c2s|s2c)|flag-matches)$/);
    if (!found) return route.fulfill({ status: 404, body: "{}" });
    const id = Number(found[1]);
    if (found[2] === "flag-matches") {
      state.flagMatchRequests.push(id);
      return send(state.flagMatches.get(id) ?? { c2s: [], s2c: [] });
    }
    state.payloadRequests.push({ id, direction: found[3] });
    if (state.waitPayload) await state.waitPayload(id, found[3]);
    const body = Array.from({ length: 200 }, (_, i) => `line ${i}: contents for ${id}`).join("\n");
    const payload = found[3] === "c2s" ? `POST /${id} HTTP/1.1\r\nHost: test\r\nContent-Type: text/plain\r\nContent-Length: ${Buffer.byteLength(body)}\r\n\r\n${body}` : `HTTP/1.1 200 OK\r\n\r\n${body}`;
    return route.fulfill({ status: 200, contentType: "application/octet-stream", body: state[found[3]].get(id) ?? payload });
  });
  return state;
}

function flagRanges(payload, flags) {
  const bytes = Buffer.from(payload);
  let offset = 0;
  return flags.map((flag) => {
    const token = Buffer.from(flag);
    const start = bytes.indexOf(token, offset);
    expect(start).toBeGreaterThanOrEqual(0);
    offset = start + token.length;
    return { start, end: offset };
  });
}

for (const grouped of [false, true]) {
  test(`${grouped ? "chain" : "standalone"} S2C header counts unique flags and keeps the badge across display modes`, async ({ page }, testInfo) => {
    const state = await routes(page);
    const c2s = "GET /?flag=FLAG{A} HTTP/1.1\r\nHost: test\r\n\r\n";
    const body = JSON.stringify({ message: "Привет", flags: ["FLAG{A}", "FLAG{A}", "FLAG{B}"] });
    const s2c = `HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n${body}`;
    state.c2s.set(41, c2s);
    state.s2c.set(41, s2c);
    state.flagMatches.set(41, { c2s: flagRanges(c2s, ["FLAG{A}"]), s2c: flagRanges(s2c, ["FLAG{A}", "FLAG{A}", "FLAG{B}"]) });
    state.s2c.set(42, "FLAG{A}");
    state.flagMatches.set(42, { c2s: [], s2c: flagRanges("FLAG{A}", ["FLAG{A}"]) });
    await page.goto("/");
    if (grouped) await page.getByRole("checkbox", { name: "Group into chains" }).check();
    await page.locator(grouped ? '[data-chain-id="41"]' : '[data-session-id="41"]').click();
    const streams = page.locator(grouped ? '.chain-pair[data-session-id="41"] .stream' : ".detail-panel .stream");
    const request = streams.first();
    const response = streams.last();
    const badge = response.getByRole("img", { name: "2 unique flags, 3 occurrences" });
    await expect(badge).toHaveText("2");
    await expect(badge).toHaveAttribute("title", "2 unique flags, 3 occurrences");
    await expect(response).toHaveClass(/response-flagged/);
    await expect(request).not.toHaveClass(/response-flagged/);
    await expect(request.locator(".stream-flag-count")).toHaveCount(0);
    await expect(request.locator("mark")).toHaveText("FLAG{A}");
    expect(await badge.evaluate((node) => node.parentElement.className)).toBe("stream-title-line");
    const payloadReads = state.payloadRequests.length;
    const matchReads = state.flagMatchRequests.length;
    await page.getByRole("checkbox", { name: "Format JSON" }).check();
    await expect(badge).toHaveText("2");
    await page.getByRole("button", { name: "Hex", exact: true }).click();
    await expect(badge).toHaveText("2");
    await expect(response.locator("mark")).toHaveCount(0);
    await page.getByRole("button", { name: "Text", exact: true }).click();
    await expect(badge).toHaveText("2");
    expect(state.payloadRequests).toHaveLength(payloadReads);
    expect(state.flagMatchRequests).toHaveLength(matchReads);
    if (grouped) {
      await page.locator('.chain-pair[data-session-id="42"]').evaluate((node) => node.scrollIntoView({ block: "center" }));
      await expect(page.locator('.chain-pair[data-session-id="42"] .stream').last().getByRole("img", { name: "1 unique flag, 1 occurrence" })).toHaveText("1");
      await page.locator(".chain-scroll").evaluate((node) => { node.scrollTop = 0; });
      await expect(badge).toHaveText("2");
    }
    await page.screenshot({ path: testInfo.outputPath(`s2c-flags-${grouped ? "chain" : "standalone"}.png`) });
  });
}

test("S2C uniqueness uses canonical flag values and handles long matches", async ({ page }) => {
  const state = await routes(page);
  const long = `FLAG{${"x".repeat(12_000)}}`;
  const cases = [
    { id: 41, payload: "FLAG{A} FLAG{%41}", flags: ["FLAG{A}", "FLAG{%41}"], values: ["FLAG{A}", "FLAG{A}"], unique: 1 },
    { id: 42, payload: Buffer.from([0xff, 0x20, 0xfe, 0x20, 0xff]), flags: [Buffer.from([0xff]), Buffer.from([0xfe]), Buffer.from([0xff])], values: ["�", "�", "�"], unique: 1 },
    { id: 43, payload: `${long} ${long}`, flags: [long, long], unique: 1 },
    { id: 44, payload: "no flag in this response", flags: [], unique: 0 },
  ];
  for (const item of cases) {
    state.c2s.set(item.id, "FLAG{request-only}");
    state.s2c.set(item.id, item.payload);
    state.flagMatches.set(item.id, { c2s: flagRanges("FLAG{request-only}", ["FLAG{request-only}"]), s2c: flagRanges(item.payload, item.flags).map((range, index) => ({ ...range, value: item.values?.[index] ?? item.flags[index] })) });
  }
  await page.goto("/");
  await page.getByRole("checkbox", { name: "Group into chains" }).check();
  await page.locator('[data-chain-id="41"]').click();
  for (const item of cases) {
    const pair = page.locator(`.chain-pair[data-session-id="${item.id}"]`);
    await pair.evaluate((node) => node.scrollIntoView({ block: "center" }));
    await expect(pair.locator("pre")).toHaveCount(2);
    await expect(pair.locator(".stream").first().locator(".stream-flag-count")).toHaveCount(0);
    const response = pair.locator(".stream").last();
    if (item.unique) {
      await expect(response.locator(".stream-flag-count")).toHaveText(String(item.unique));
    } else {
      await expect(response.locator(".stream-flag-count")).toHaveCount(0);
      await expect(response).not.toHaveClass(/response-flagged/);
    }
  }
});

test("Russian S2C badge plurals and placement fit mobile headers", async ({ page }, testInfo) => {
  const state = await routes(page);
  const cases = [
    { id: 41, unique: 1, occurrences: 1, title: "1 уникальный флаг, 1 вхождение" },
    { id: 42, unique: 2, occurrences: 3, title: "2 уникальных флага, 3 вхождения" },
    { id: 43, unique: 5, occurrences: 11, title: "5 уникальных флагов, 11 вхождений" },
    { id: 44, unique: 21, occurrences: 22, title: "21 уникальный флаг, 22 вхождения" },
  ];
  for (const item of cases) {
    const flags = Array.from({ length: item.occurrences }, (_, index) => `FLAG{${index % item.unique}}`);
    const payload = flags.join(" ");
    state.s2c.set(item.id, payload);
    state.flagMatches.set(item.id, { c2s: [], s2c: flagRanges(payload, flags) });
  }
  await page.addInitScript(() => localStorage.setItem("newhatch_language", "ru"));
  await page.setViewportSize({ width: 375, height: 812 });
  await page.goto("/");
  await page.getByRole("checkbox", { name: "Группировать в цепочки" }).check();
  await page.locator('[data-chain-id="41"]').click();
  for (const item of cases) {
    const response = page.locator(`.chain-pair[data-session-id="${item.id}"] .stream`).last();
    await response.evaluate((node) => node.scrollIntoView({ block: "center" }));
    const badge = response.getByRole("img", { name: item.title });
    await expect(badge).toHaveText(String(item.unique));
    await expect(badge).toHaveAttribute("title", item.title);
    const bounds = await badge.boundingBox();
    const title = await response.locator(".stream-title-line").boundingBox();
    const timestamp = await response.locator("header small").boundingBox();
    const actions = await response.locator(".stream-actions").boundingBox();
    expect(bounds.y).toBeGreaterThanOrEqual(title.y);
    expect(bounds.y + bounds.height).toBeLessThanOrEqual(timestamp.y);
    expect(bounds.x + bounds.width).toBeLessThan(actions.x);
  }
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await page.locator(".chain-scroll").evaluate((node) => { node.scrollTop = 0; });
  await page.screenshot({ path: testInfo.outputPath("s2c-flags-mobile-ru.png") });
});

test("chain detail shows embedded Unicode by default in C2S and S2C", async ({ page }) => {
  const state = await routes(page);
  const body = JSON.stringify({ nested: String.raw`{"message":"\u041f\u0440\u0438\u0432\u0435\u0442","emoji":"\ud83e\udd80"}` });
  state.c2s.set(41, `POST /unicode HTTP/1.1\r\nHost: test\r\nContent-Length: ${Buffer.byteLength(body)}\r\n\r\n${body}`);
  state.s2c.set(41, `HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n${body}`);
  await page.goto("/");
  await page.getByRole("checkbox", { name: "Group into chains" }).check();
  await page.locator('[data-chain-id="41"]').click();
  const toggle = page.getByRole("checkbox", { name: "Format JSON" });
  const streams = page.locator('.chain-pair[data-session-id="41"] .stream pre');
  await expect(toggle).not.toBeChecked();
  await expect(streams).toHaveCount(2);
  for (const stream of [streams.first(), streams.last()]) {
    await expect(stream).toContainText("Привет");
    await expect(stream).toContainText("🦀");
    await expect(stream).not.toContainText(String.raw`\u041f`);
    const visible = await stream.textContent();
    expect(typeof JSON.parse(visible.slice(visible.indexOf("\r\n\r\n") + 4)).nested).toBe("string");
  }
  const compact = await streams.allTextContents();
  await toggle.check();
  for (const stream of [streams.first(), streams.last()]) await expect(stream).toContainText('"message": "Привет"');
  await toggle.uncheck();
  await expect.poll(() => streams.allTextContents()).toEqual(compact);
});

test("chain card keeps session pairs, C2S exports and two independent fixed-size scroll levels", async ({ page, context }, testInfo) => {
  const state = await routes(page);
  await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto("/");
  const toggle = page.getByRole("checkbox", { name: "Group into chains" });
  await expect(toggle).not.toBeChecked();
  await expect(page.locator('[data-session-id="41"]')).toHaveCount(1);
  await page.locator('[data-session-id="41"]').click();
  await expect(page.locator(".detail-panel")).toBeVisible();
  await expect(page.getByText("Scroll sessions here", { exact: true })).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Copy chain as Python (requests)", exact: true })).toHaveCount(0);
  await page.keyboard.press("Escape");
  await toggle.check();
  await expect(page.getByText("1 loaded chains")).toBeVisible();
  await page.getByRole("checkbox", { name: "Flags only" }).check();
  await expect.poll(() => state.chainRequests.some((params) => params.get("contains_flag") === "true")).toBe(true);
  await page.screenshot({ path: testInfo.outputPath("chain-list.png") });
  await page.locator('[data-chain-id="41"]').click();
  await expect(page.getByRole("complementary", { name: "Chain details" })).toBeVisible();
  await expect(page.getByText("Scroll sessions here", { exact: true })).toBeVisible();
  const scroll = page.locator(".chain-scroll");
  const pair = page.locator('.chain-pair[data-session-id="41"]');
  const c2s = pair.locator(".stream").nth(0);
  const s2c = pair.locator(".stream").nth(1);
  await expect(c2s.locator("pre")).toContainText("POST /41");
  await expect(s2c.locator("pre")).toContainText("HTTP/1.1 200 OK");
  await expect(c2s.locator("header small")).toHaveText(/\d{2}:\d{2}:\d{2}\.100/);
  await expect(s2c.locator("header small")).toHaveText(/\d{2}:\d{2}:\d{2}\.140/);
  await expect(c2s.getByRole("button", { name: "Copy as Bash (cURL)" })).toHaveCount(1);
  await expect(c2s.getByRole("button", { name: "Copy as Python (requests)" })).toHaveCount(1);
  await expect(s2c.getByRole("button", { name: /Copy as/ })).toHaveCount(0);
  expect((await c2s.boundingBox()).y).toBeLessThan((await s2c.boundingBox()).y);
  expect(state.payloadRequests.length).toBeLessThan(40);
  const height = await c2s.evaluate((node) => node.clientHeight);
  const background = await page.evaluate(() => scrollY);
  const outer = await scroll.evaluate((node) => node.scrollTop);
  await c2s.locator("pre").hover();
  await page.mouse.wheel(0, 400);
  await expect.poll(() => c2s.locator("pre").evaluate((node) => node.scrollTop)).toBeGreaterThan(0);
  expect(await scroll.evaluate((node) => node.scrollTop)).toBe(outer);
  expect(await page.evaluate(() => scrollY)).toBe(background);
  await c2s.getByRole("button", { name: "Copy as Bash (cURL)" }).click();
  await expect.poll(() => page.evaluate(() => navigator.clipboard.readText())).toContain("curl");
  await c2s.locator("pre").evaluate((node) => { node.scrollTop = node.scrollHeight; });
  await c2s.locator("pre").hover();
  await page.mouse.wheel(0, 600);
  expect(await scroll.evaluate((node) => node.scrollTop)).toBe(outer);
  expect(await page.evaluate(() => scrollY)).toBe(background);
  const bounds = await scroll.boundingBox();
  await page.mouse.move(bounds.x + 5, bounds.y + 30);
  await page.mouse.wheel(0, 900);
  await expect.poll(() => scroll.evaluate((node) => node.scrollTop)).toBeGreaterThan(outer);
  expect(await page.evaluate(() => scrollY)).toBe(background);
  expect(await c2s.evaluate((node) => node.clientHeight)).toBe(height);
  await scroll.evaluate((node) => { node.scrollTop = node.scrollHeight; });
  await expect.poll(() => state.memberRequests.length).toBe(2);
  expect(await c2s.evaluate((node) => node.clientHeight)).toBe(height);
  await scroll.evaluate((node) => { node.scrollTop = 0; });
  await expect(c2s.locator("pre")).toBeVisible();
  await page.getByRole("button", { name: "Hex", exact: true }).click();
  await expect(c2s.getByRole("button", { name: "Copy as Bash (cURL)" })).toBeDisabled();
  await page.getByRole("button", { name: "Text", exact: true }).click();
  await page.screenshot({ path: testInfo.outputPath("chain-desktop.png") });
  await page.keyboard.press("Escape");
  await expect(page.locator(".chain-panel")).toHaveCount(0);
  await toggle.uncheck();
  await expect(page.getByText("1 loaded connections")).toBeVisible();
});

test("chain windows remain bounded and independently scrollable on mobile", async ({ page }, testInfo) => {
  await routes(page);
  await page.setViewportSize({ width: 375, height: 812 });
  await page.goto("/");
  await page.getByRole("checkbox", { name: "Group into chains" }).check();
  await page.locator('[data-chain-id="41"]').click();
  const payload = page.locator('.chain-pair[data-session-id="41"] .stream pre').first();
  await expect(payload).toBeVisible();
  await expect(page.locator('.chain-pair[data-session-id="41"] .stream-title').first()).toContainText("#41 · C2S");
  await expect(page.locator('.chain-pair[data-session-id="41"] header small').first()).toHaveText(/\d{2}:\d{2}:\d{2}\.100/);
  expect(await page.locator(".chain-panel").evaluate((node) => node.getBoundingClientRect().bottom)).toBeLessThanOrEqual(812 - 58);
  expect(await payload.evaluate((node) => node.scrollHeight > node.clientHeight)).toBe(true);
  expect(await page.locator(".chain-scroll").evaluate((node) => node.scrollHeight > node.clientHeight)).toBe(true);
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  const toolbar = await page.locator(".chain-panel .payload-toolbar").boundingBox();
  const button = await page.getByRole("button", { name: "Copy chain as Python (requests)" }).boundingBox();
  expect(button.x + button.width / 2).toBeCloseTo(toolbar.x + toolbar.width / 2, 1);
  await page.screenshot({ path: testInfo.outputPath("chain-mobile.png") });
});

test("loaded offscreen pairs stay searchable by native browser find without refetching", async ({ page }) => {
  const state = await routes(page);
  state.c2s.set(41, "GET /old-c2s-search-marker HTTP/1.1\r\nHost: test\r\n\r\n");
  state.s2c.set(41, "HTTP/1.1 200 OK\r\n\r\nold-s2c-search-marker");
  state.s2c.set(70, "HTTP/1.1 200 OK\r\n\r\nnew-s2c-search-marker");
  await page.goto("/");
  await page.getByRole("checkbox", { name: "Group into chains" }).check();
  await page.locator('[data-chain-id="41"]').click();
  const scroll = page.locator(".chain-scroll");
  const first = page.locator('.chain-pair[data-session-id="41"]');
  await expect(first.locator("pre").first()).toContainText("old-c2s-search-marker");
  expect(state.payloadRequests.some((request) => request.id === 55)).toBe(false);
  await scroll.evaluate((node) => { node.scrollTop = node.scrollHeight; });
  await expect(page.locator(".chain-pair")).toHaveCount(30);
  await scroll.evaluate((node) => { node.scrollTop = node.scrollHeight; });
  await expect(page.locator('.chain-pair[data-session-id="70"] pre').last()).toContainText("new-s2c-search-marker");
  expect(await first.evaluate((node) => node.getBoundingClientRect().bottom)).toBeLessThan((await scroll.boundingBox()).y - 200);
  await expect(first.locator("pre")).toHaveCount(2);
  for (const marker of ["old-c2s-search-marker", "old-s2c-search-marker", "new-s2c-search-marker"]) {
    const found = await page.evaluate((text) => {
      window.getSelection().removeAllRanges();
      return window.find(text, false, false, true) && window.getSelection().toString();
    }, marker);
    expect(found).toBe(marker);
  }
  await scroll.evaluate((node) => { node.scrollTop = 0; });
  await expect(first.locator("pre").first()).toBeVisible();
  expect(state.payloadRequests.filter((request) => request.id === 41)).toHaveLength(2);
});

test("quick reopening reuses cached pages and pairs but resets all scroll positions", async ({ page }) => {
  const state = await routes(page);
  await page.clock.install();
  await page.clock.pauseAt(new Date());
  await page.goto("/");
  await page.getByRole("checkbox", { name: "Group into chains" }).check();
  await page.locator('[data-chain-id="41"]').click();
  const scroll = page.locator(".chain-scroll");
  const first = page.locator('.chain-pair[data-session-id="41"] pre');
  await expect(first.first()).toContainText("POST /41");
  await first.evaluateAll((nodes) => nodes.forEach((node) => { node.scrollTop = 120; }));
  await scroll.evaluate((node) => { node.scrollTop = node.scrollHeight; });
  await expect(page.locator(".chain-pair")).toHaveCount(30);
  await scroll.evaluate((node) => { node.scrollTop = node.scrollHeight; });
  await expect(page.locator('.chain-pair[data-session-id="70"] pre').last()).toContainText("HTTP/1.1 200 OK");
  const reads = state.payloadRequests.length;
  const memberReads = state.memberRequests.length;
  await page.getByRole("button", { name: "Close session", exact: true }).click();
  await expect(page.locator(".chain-panel")).toHaveCount(0);
  expect(await page.evaluate(() => window.find("POST /41", false, false, true))).toBe(false);
  await page.clock.fastForward(2_999);
  await page.locator('[data-chain-id="41"]').click();
  await expect(first.first()).toContainText("POST /41");
  await expect(page.locator(".chain-pair")).toHaveCount(30);
  expect(await scroll.evaluate((node) => node.scrollTop)).toBe(0);
  expect(await page.locator(".chain-pair pre").evaluateAll((nodes) => nodes.every((node) => node.scrollTop === 0))).toBe(true);
  expect(state.memberRequests).toHaveLength(memberReads);
  expect(state.payloadRequests).toHaveLength(reads);
  // Reopening cancels expiry; an open card must never lose its accumulated data.
  await page.clock.fastForward(4_000);
  await expect(first.first()).toContainText("POST /41");
  await page.keyboard.press("Escape");
  await page.clock.fastForward(2_999);
  await page.locator('[data-chain-id="41"]').click();
  await expect(first.first()).toContainText("POST /41");
  expect(state.memberRequests).toHaveLength(memberReads);
  expect(state.payloadRequests).toHaveLength(reads);
});

test("closed chain data expires after three seconds", async ({ page }) => {
  const state = await routes(page);
  await page.clock.install();
  await page.clock.pauseAt(new Date());
  await page.goto("/");
  await page.getByRole("checkbox", { name: "Group into chains" }).check();
  await page.locator('[data-chain-id="41"]').click();
  const first = page.locator('.chain-pair[data-session-id="41"] pre').first();
  await expect(first).toContainText("POST /41");
  const memberReads = state.memberRequests.length;
  const payloadReads = state.payloadRequests.filter((request) => request.id === 41).length;
  await page.keyboard.press("Escape");
  await expect(page.locator(".chain-panel")).toHaveCount(0);
  await page.clock.fastForward(3_001);
  state.c2s.set(41, "GET /after-cache-expiry HTTP/1.1\r\nHost: test\r\n\r\n");
  await page.locator('[data-chain-id="41"]').click();
  await expect(first).toContainText("/after-cache-expiry");
  expect(state.memberRequests.length).toBeGreaterThan(memberReads);
  expect(state.memberRequests.slice(memberReads).every((params) => !params.has("cursor"))).toBe(true);
  expect(state.payloadRequests.filter((request) => request.id === 41)).toHaveLength(payloadReads + 2);
  expect(await page.locator(".chain-scroll").evaluate((node) => node.scrollTop)).toBe(0);
});

test("scrolling away lets an unfinished pair complete and keeps its text", async ({ page }) => {
  const state = await routes(page);
  let release;
  const waiting = new Promise((resolve) => { release = resolve; });
  state.waitPayload = (id) => id === 41 ? waiting : Promise.resolve();
  await page.goto("/");
  await page.getByRole("checkbox", { name: "Group into chains" }).check();
  await page.locator('[data-chain-id="41"]').click();
  await expect.poll(() => state.payloadRequests.filter((request) => request.id === 41).length).toBe(2);
  const scroll = page.locator(".chain-scroll");
  await scroll.evaluate((node) => { node.scrollTop = node.scrollHeight; });
  await expect(page.locator(".chain-pair")).toHaveCount(30);
  await scroll.evaluate((node) => { node.scrollTop = node.scrollHeight; });
  await expect(page.locator('.chain-pair[data-session-id="70"] pre').last()).toContainText("HTTP/1.1 200 OK");
  release();
  const first = page.locator('.chain-pair[data-session-id="41"] pre');
  await expect(first.first()).toContainText("POST /41");
  await expect(first.last()).toContainText("HTTP/1.1 200 OK");
  expect(state.payloadRequests.filter((request) => request.id === 41)).toHaveLength(2);
});

test("closing aborts unfinished payload reads and reopening retries the pair", async ({ page }) => {
  const state = await routes(page);
  let release;
  const waiting = new Promise((resolve) => { release = resolve; });
  state.waitPayload = (id, direction) => id === 41 && direction === "c2s" ? waiting : Promise.resolve();
  await page.goto("/");
  await page.getByRole("checkbox", { name: "Group into chains" }).check();
  await page.locator('[data-chain-id="41"]').click();
  await expect.poll(() => state.payloadRequests.filter((request) => request.id === 41).length).toBe(2);
  const aborted = page.waitForEvent("requestfailed", (request) => request.url().endsWith("/sessions/41/payload/c2s"));
  await page.keyboard.press("Escape");
  await aborted;
  state.waitPayload = null;
  release();
  await page.locator('[data-chain-id="41"]').click();
  await expect(page.locator('.chain-pair[data-session-id="41"] pre').first()).toContainText("POST /41");
  expect(state.payloadRequests.filter((request) => request.id === 41)).toHaveLength(4);
});

test("cached members and pairs are isolated by snapshot and cleared on filter changes", async ({ page }) => {
  const state = await routes(page);
  await page.goto("/");
  await page.getByRole("checkbox", { name: "Group into chains" }).check();
  await page.locator('[data-chain-id="41"]').click();
  const first = page.locator('.chain-pair[data-session-id="41"] pre').first();
  await expect(first).toContainText("POST /41");
  await page.keyboard.press("Escape");
  state.rows = [{ ...chain(), snapshot_id: 101 }];
  state.c2s.set(41, "GET /new-snapshot HTTP/1.1\r\nHost: test\r\n\r\n");
  const beforeRefresh = state.chainRequests.length;
  await page.getByRole("button", { name: "Refresh sessions", exact: true }).click();
  await expect.poll(() => state.chainRequests.length).toBe(beforeRefresh + 1);
  await page.locator('[data-chain-id="41"]').click();
  await expect(first).toContainText("/new-snapshot");
  expect(state.memberRequests.at(-1).get("snapshot_id")).toBe("101");
  await page.keyboard.press("Escape");
  state.c2s.set(41, "GET /after-filter-change HTTP/1.1\r\nHost: test\r\n\r\n");
  const beforeFilter = state.chainRequests.length;
  await page.getByRole("checkbox", { name: "Flags only" }).check();
  await expect.poll(() => state.chainRequests.length).toBe(beforeFilter + 1);
  await page.locator('[data-chain-id="41"]').click();
  await expect(first).toContainText("/after-filter-change");
  expect(state.payloadRequests.filter((request) => request.id === 41)).toHaveLength(6);
});

test("chain Python export loads offscreen sessions and all requests within each C2S", async ({ page, context }, testInfo) => {
  const state = await routes(page);
  const body = '{"message":"\\u041f\\u0440\\u0438\\u0432\\u0435\\u0442","nested":"{\\"ok\\":true}"}';
  state.c2s.set(41, `POST /login HTTP/1.1\r\nHost: test\r\nCookie: sid=initial\r\nContent-Length: ${Buffer.byteLength(body)}\r\n\r\n${body}GET /next HTTP/1.1\r\nHost: test\r\nCookie: sid=captured-old\r\n\r\n`);
  await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  await page.goto("/");
  await page.getByRole("checkbox", { name: "Group into chains" }).check();
  await page.locator('[data-chain-id="41"]').click();
  await expect(page.locator(".chain-pair")).toHaveCount(20);
  expect(state.payloadRequests.some((request) => request.id === 70)).toBe(false);
  await page.getByRole("checkbox", { name: "Format JSON" }).check();
  const button = page.getByRole("button", { name: "Copy chain as Python (requests)", exact: true });
  await page.getByRole("button", { name: "Hex", exact: true }).click();
  await expect(button).toBeDisabled();
  await page.getByRole("button", { name: "Text", exact: true }).click();
  await button.click();
  await expect.poll(() => page.evaluate(() => navigator.clipboard.readText())).toContain("Session #70 · request 1/1");
  const script = await page.evaluate(() => navigator.clipboard.readText());
  expect(script.match(/response = session\.request\(/g)).toHaveLength(31);
  expect(script).toContain("with requests.Session() as session:");
  expect(script).toContain("Привет");
  expect(script).toContain('  "nested": {');
  expect(script).toContain('seed_cookies(session, url, headers["Host"], "sid=initial")');
  expect(script).not.toContain("captured-old");
  const headings = Array.from(script.matchAll(/# Session #(\d+) · request (\d+)\//g), (match) => [Number(match[1]), Number(match[2])]);
  expect(headings).toEqual([[41, 1], [41, 2], ...Array.from({ length: 29 }, (_, i) => [42 + i, 1])]);
  expect(state.memberRequests.every((params) => params.get("snapshot_id") === "100")).toBe(true);
  expect(state.memberRequests.some((params) => params.get("cursor") === "members-older")).toBe(true);
  // Exporting does not expand the card's lazy metadata/payload windows.
  await expect(page.locator(".chain-pair")).toHaveCount(20);
  await page.screenshot({ path: testInfo.outputPath("chain-export-desktop.png") });
});

test("chain export reports an unsupported offscreen request and never copies a partial script", async ({ page, context }) => {
  const state = await routes(page);
  state.c2s.set(70, "POST /last HTTP/1.1\r\nContent-Length: 20\r\n\r\nx");
  await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  await page.goto("/");
  await page.evaluate(() => navigator.clipboard.writeText("keep clipboard"));
  await page.getByRole("checkbox", { name: "Group into chains" }).check();
  await page.locator('[data-chain-id="41"]').click();
  const button = page.getByRole("button", { name: "Copy chain as Python (requests)" });
  await button.click();
  await expect(page.getByRole("alert")).toHaveText("Session #70, request 1: The HTTP request is incomplete");
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe("keep clipboard");
  state.c2s.delete(70);
  await button.click();
  await expect.poll(() => page.evaluate(() => navigator.clipboard.readText())).toContain("Session #70 · request 1/1");
  const script = await page.evaluate(() => navigator.clipboard.readText());
  expect(script).not.toContain("seed_cookies");
  expect(script).not.toContain("SimpleCookie");
  expect(script).not.toContain("urlsplit");
  await expect(page.getByRole("alert")).toHaveCount(0);
});

test("closing a chain aborts script preparation before clipboard changes", async ({ page, context }) => {
  const state = await routes(page);
  let release;
  const waiting = new Promise((resolve) => { release = resolve; });
  state.waitPayload = (id, direction) => id === 70 && direction === "c2s" ? waiting : Promise.resolve();
  await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  await page.goto("/");
  await page.evaluate(() => navigator.clipboard.writeText("keep clipboard"));
  await page.getByRole("checkbox", { name: "Group into chains" }).check();
  await page.locator('[data-chain-id="41"]').click();
  const button = page.getByRole("button", { name: "Copy chain as Python (requests)" });
  await button.click();
  await expect(button).toHaveAttribute("title", "Preparing script: 29/30 sessions");
  await expect(button).toBeDisabled();
  const aborted = page.waitForEvent("requestfailed", (request) => request.url().endsWith("/sessions/70/payload/c2s"));
  await page.keyboard.press("Escape");
  await aborted;
  release();
  await expect(page.locator(".chain-panel")).toHaveCount(0);
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe("keep clipboard");
});

test("chain export detects retention changes and supports Russian mobile clipboard fallback", async ({ page }, testInfo) => {
  const state = await routes(page);
  await page.addInitScript(() => {
    localStorage.setItem("newhatch_language", "ru");
    window.copiedScripts = [];
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: undefined });
    document.execCommand = (command) => {
      if (command !== "copy") return false;
      window.copiedScripts.push(document.querySelector("textarea").value);
      return true;
    };
  });
  await page.setViewportSize({ width: 375, height: 812 });
  await page.goto("/");
  await page.getByRole("checkbox", { name: "Группировать в цепочки" }).check();
  await page.locator('[data-chain-id="41"]').click();
  state.memberSummary = { ...chain(), session_count: 29 };
  const button = page.getByRole("button", { name: "Копировать цепочку как Python (requests)" });
  await button.click();
  await expect(page.getByRole("alert")).toContainText("Состав цепочки изменился");
  expect(await page.evaluate(() => window.copiedScripts)).toHaveLength(0);
  state.memberSummary = null;
  await button.click();
  await expect.poll(() => page.evaluate(() => window.copiedScripts.length)).toBe(1);
  expect(await page.evaluate(() => window.copiedScripts[0])).toContain("Session #70 · request 1/1");
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await page.screenshot({ path: testInfo.outputPath("chain-export-mobile-ru.png") });
});

test("refresh replaces stale chain rows after a late session bridges them", async ({ page }) => {
  const state = await routes(page);
  state.rows = [chain(), { ...chain(71), started_at: session(41).started_at - 2000000, session_count: 1 }];
  await page.goto("/");
  await page.getByRole("checkbox", { name: "Group into chains" }).check();
  await expect(page.locator("[data-chain-id]")).toHaveCount(2);
  state.rows = [{ ...chain(), started_at: session(41).started_at - 2000000, session_count: 32 }];
  await page.getByRole("button", { name: "Refresh sessions", exact: true }).click();
  await expect(page.locator("[data-chain-id]")).toHaveCount(1);
  await expect(page.locator('[data-chain-id="41"]')).toContainText("32");
  await expect(page.locator('[data-chain-id="71"]')).toHaveCount(0);
});

test("responsive lanes reach the right edge and scroll the chain while copy buttons remain accessible", async ({ page, context }) => {
  await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  const state = await routes(page);
  for (const viewport of [{ width: 1280, height: 900 }, { width: 375, height: 812 }]) {
    state.memberRequests.length = 0;
    await page.setViewportSize(viewport);
    await page.goto("/");
    await page.getByRole("checkbox", { name: "Group into chains" }).check();
    await page.locator('[data-chain-id="41"]').click();
    // Make the background genuinely scrollable to expose accidental chaining.
    await page.locator("main").evaluate((node) => { node.style.minHeight = "2400px"; });
    const scroll = page.locator(".chain-scroll");
    const streams = page.locator('.chain-pair[data-session-id="41"] .stream');
    await expect(streams.first().locator("pre")).toContainText("POST /41");
    const background = await page.evaluate(() => scrollY);
    for (const stream of [streams.first(), streams.last()]) {
      const payload = stream.locator("pre");
      await payload.evaluate((node) => { node.scrollTop = 120; });
      const inner = await payload.evaluate((node) => node.scrollTop);
      const lane = await stream.locator(".chain-scroll-lane").boundingBox();
      const windowWidth = await stream.evaluate((node) => node.clientWidth);
      expect(lane.width).toBeCloseTo(Math.max(100, windowWidth * 0.26), 1);
      const bounds = await payload.boundingBox();
      expect(lane.x + lane.width).toBeCloseTo(bounds.x + bounds.width, 1);
      await page.mouse.move(lane.x + lane.width / 2, bounds.y + 40);
      await page.mouse.wheel(0, 100);
      await expect.poll(() => scroll.evaluate((node) => node.scrollTop)).toBeGreaterThan(0);
      expect(await payload.evaluate((node) => node.scrollTop)).toBe(inner);
      expect(await page.evaluate(() => scrollY)).toBe(background);
      await scroll.evaluate((node) => { node.scrollTop = 0; });

      // The strip also routes scrolling at the rightmost edge to the chain.
      const track = await payload.boundingBox();
      const edge = { x: track.x + track.width - 4, y: track.y + 40 };
      expect(await page.evaluate(({ x, y }) => !!document.elementFromPoint(x, y)?.closest(".chain-scroll-lane"), edge)).toBe(true);
      await page.mouse.move(edge.x, edge.y);
      await page.mouse.wheel(0, 100);
      await expect.poll(() => scroll.evaluate((node) => node.scrollTop)).toBeGreaterThan(0);
      expect(await payload.evaluate((node) => node.scrollTop)).toBe(inner);
      expect(await page.evaluate(() => scrollY)).toBe(background);
      await scroll.evaluate((node) => { node.scrollTop = 0; });
    }
    const c2s = streams.first();
    const python = c2s.getByRole("button", { name: "Copy as Python (requests)" });
    await python.click();
    await expect.poll(() => page.evaluate(() => navigator.clipboard.readText())).toContain("import requests");
    await python.hover();
    await page.mouse.wheel(0, 100);
    await expect.poll(() => scroll.evaluate((node) => node.scrollTop)).toBeGreaterThan(0);
    expect(await page.evaluate(() => scrollY)).toBe(background);
    await scroll.evaluate((node) => { node.scrollTop = 0; });
    await c2s.locator(".stream-actions button").last().click();
    await expect.poll(() => page.evaluate(() => navigator.clipboard.readText())).toContain("POST /41");
    await streams.last().locator(".stream-actions button").last().click();
    await expect.poll(() => page.evaluate(() => navigator.clipboard.readText())).toContain("HTTP/1.1 200 OK");

    await scroll.evaluate((node) => { node.scrollTop = node.scrollHeight; });
    await expect(page.locator(".chain-pair")).toHaveCount(30);
    await scroll.evaluate((node) => { node.scrollTop = node.scrollHeight; });
    const last = page.locator('.chain-pair[data-session-id="70"] .stream').last();
    await expect(last.locator("pre")).toContainText("HTTP/1.1 200 OK");
    const lane = await last.locator(".chain-scroll-lane").boundingBox();
    await page.mouse.move(lane.x + lane.width / 2, lane.y + lane.height - 30);
    const end = await scroll.evaluate((node) => node.scrollTop);
    await page.mouse.wheel(0, 500);
    expect(await scroll.evaluate((node) => node.scrollTop)).toBe(end);
    expect(await page.evaluate(() => scrollY)).toBe(background);
  }
});
