const { test, expect } = require("@playwright/test");

function session(id) {
  return {
    id,
    source_id: 1,
    source_name: "web",
    started_at: 1_700_000_000_000_000 + id,
    ended_at: 1_700_000_000_000_100 + id,
    client_ip: "10.0.0.2",
    client_port: 40_000 + (id % 1_000),
    server_ip: "10.0.0.1",
    server_port: 8080,
    protocol: "http",
    bytes_c2s: 120,
    bytes_s2c: 240,
    contains_flag: false,
    flag_direction: "none",
    flag_count: 0,
    suricata_alerts: 0,
    incomplete: false,
    http: { method: "GET", host: "test", path: `/${id}`, status: 200, content_type: "text/plain" },
  };
}

function descending(from, count) {
  return Array.from({ length: count }, (_, index) => session(from - index));
}

test("live feed merges refreshes, pauses away from the top and loads history automatically", async ({ page }) => {
  let newestId = 300;
  let newestRequests = 0;
  const cursorRequests = [];

  await page.route("**/api/**", async (route) => {
    const url = new URL(route.request().url());
    const send = (body) => route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify(body),
    });

    if (url.pathname === "/api/auth/me") return send({ authenticated: true, username: "team" });
    if (url.pathname === "/api/sources") return send([]);
    if (url.pathname === "/api/collectors") return send({
      mode: "remote",
      collectors: [{
        collector_id: "vulnbox-1",
        connected: true,
        peer: "10.0.0.2:39090",
        connected_at: 1,
        last_activity: 1,
        captured_packets: 10,
        sent_packets: 10,
        dropped_packets: 0,
        queue_depth: 0,
        reconnect_count: 0,
        receiver_dropped_packets: 0,
        last_error: null,
      }],
    });
    if (url.pathname !== "/api/sessions") return route.fulfill({ status: 404, body: "{}" });

    const cursor = url.searchParams.get("cursor");
    if (cursor === null) {
      newestRequests++;
      return send({ items: descending(newestId, 100), next_cursor: newestId - 99 });
    }

    const numericCursor = Number(cursor);
    cursorRequests.push(numericCursor);
    if (numericCursor === 201) return send({ items: descending(200, 100), next_cursor: 101 });
    if (numericCursor === 101) return send({ items: descending(100, 100), next_cursor: null });
    return send({ items: [], next_cursor: null });
  });

  await page.setViewportSize({ width: 1280, height: 720 });
  await page.goto("/");
  await expect(page.getByText("100 loaded connections")).toBeVisible();
  await expect(page.getByText("Collector online")).toBeVisible();
  await expect(page.getByRole("button", { name: /load older/i })).toHaveCount(0);

  await page.evaluate(() => window.scrollTo(0, 500));
  const openedRow = page.locator('[data-session-id="290"]');
  await openedRow.click();
  await expect(openedRow).toHaveClass(/selected-row/);
  const lockedScrollPosition = await page.evaluate(() => window.scrollY);
  await page.locator(".detail-panel").hover();
  await page.mouse.wheel(0, 1_000);
  expect(await page.evaluate(() => window.scrollY)).toBe(lockedScrollPosition);

  await page.mouse.move(230, 400);
  await page.mouse.wheel(0, 500);
  await expect.poll(() => page.evaluate(() => window.scrollY)).toBeGreaterThan(lockedScrollPosition);
  expect(await page.locator(".sidebar").evaluate((sidebar) => sidebar.getBoundingClientRect().top)).toBe(0);
  const listScrollPosition = await page.evaluate(() => window.scrollY);
  await page.getByRole("button", { name: "Close session" }).click();
  await expect(openedRow).not.toHaveClass(/selected-row/);
  expect(await page.evaluate(() => window.scrollY)).toBe(listScrollPosition);

  await page.evaluate(() => window.scrollTo(0, document.body.scrollHeight));
  await expect.poll(() => cursorRequests.length).toBe(1);
  await expect(page.getByText("200 loaded connections")).toBeVisible();
  expect(cursorRequests[0]).toBe(201);

  const requestsWhileReading = newestRequests;
  await page.waitForTimeout(5_300);
  expect(newestRequests).toBe(requestsWhileReading);

  newestId = 302;
  await page.evaluate(() => window.scrollBy(0, -300));
  await expect.poll(() => newestRequests).toBe(requestsWhileReading + 1);
  await expect(page.getByText("202 loaded connections")).toBeVisible();
  await expect(page.locator('[data-session-id="101"]')).toHaveCount(1);

  const afterCatchUp = newestRequests;
  await page.waitForTimeout(5_300);
  expect(newestRequests).toBe(afterCatchUp);

  newestId = 303;
  await page.evaluate(() => window.scrollTo(0, 0));
  await expect.poll(() => newestRequests, { timeout: 6_000 }).toBe(afterCatchUp + 1);
  await expect(page.getByText("203 loaded connections")).toBeVisible();

  await page.setViewportSize({ width: 375, height: 812 });
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await page.screenshot({ path: "/results/session-feed-mobile.png" });
});
