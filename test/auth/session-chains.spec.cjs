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
  const state = { memberRequests: [], payloadRequests: [], chainRequests: [], rows: [chain()] };
  await page.route("**/api/**", async (route) => {
    const url = new URL(route.request().url());
    const send = (value) => route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify(value) });
    if (url.pathname === "/api/auth/me") return send({ authenticated: true, username: "team" });
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
      return send({ chain: chain(), items: Array.from({ length: older ? 10 : 20 }, (_, i) => session((older ? 61 : 41) + i)), next_cursor: older ? null : "members-older" });
    }
    const found = url.pathname.match(/^\/api\/sessions\/(\d+)\/(payload\/(c2s|s2c)|flag-matches)$/);
    if (!found) return route.fulfill({ status: 404, body: "{}" });
    const id = Number(found[1]);
    if (found[2] === "flag-matches") return send({ c2s: [], s2c: [] });
    state.payloadRequests.push({ id, direction: found[3] });
    const body = Array.from({ length: 200 }, (_, i) => `line ${i}: contents for ${id}`).join("\n");
    const payload = found[3] === "c2s" ? `POST /${id} HTTP/1.1\r\nHost: test\r\nContent-Type: text/plain\r\nContent-Length: ${Buffer.byteLength(body)}\r\n\r\n${body}` : `HTTP/1.1 200 OK\r\n\r\n${body}`;
    return route.fulfill({ status: 200, contentType: "application/octet-stream", body: payload });
  });
  return state;
}

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
  await page.screenshot({ path: testInfo.outputPath("chain-mobile.png") });
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
