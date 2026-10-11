const { test, expect } = require("@playwright/test");

function session(id) {
  return { id, source_id: 1, source_name: "web", started_at: id * 1_000_000,
    ended_at: id * 1_000_000 + 100, client_ip: "10.0.0.2", client_port: 40000,
    server_ip: "10.0.0.1", server_port: 8080, protocol: "http", bytes_c2s: 10,
    bytes_s2c: 10, contains_flag: false, flag_direction: "none", flag_count: 0,
    suricata_alerts: 0, incomplete: false, http: {}, collector_id: "test",
    first_payload_c2s_at: null, first_payload_s2c_at: null };
}
function chain(id) {
  return { ...session(id), session_count: 1, snapshot_id: 100 };
}
function deferred() {
  let resolve;
  const promise = new Promise((done) => { resolve = done; });
  return { promise, resolve };
}

async function setup(page, grouped, limits = { max_terms: 10, max_term_chars: 128 }) {
  await page.clock.install();
  const state = { requests: [], handler: null };
  await page.route("**/api/**", async (route) => {
    const url = new URL(route.request().url());
    const send = (body, status = 200) => route.fulfill({ status, contentType: "application/json", body: JSON.stringify(body) });
    if (url.pathname === "/api/auth/me") return send({ authenticated: true, username: "team" });
    if (url.pathname === "/api/payload-search/limits") return send(limits);
    if (url.pathname === "/api/sources") return send([{ id: 1, name: "web", port: 8080, enabled: true }]);
    if (url.pathname === "/api/collectors") return send({ mode: "local", collectors: [] });
    if (!["/api/sessions", "/api/chains"].includes(url.pathname)) return send({}, 404);
    const entry = { path: url.pathname, terms: JSON.parse(url.searchParams.get("payload") || "[]"), params: url.searchParams };
    state.requests.push(entry);
    const response = state.handler ? await state.handler(entry) : null;
    return send(response?.body ?? { items: [url.pathname === "/api/chains" ? chain(10) : session(10)], next_cursor: null }, response?.status ?? 200);
  });
  await page.goto("/");
  await expect(page.getByRole("button", { name: "Refresh sessions" })).toBeEnabled();
  if (grouped) {
    await page.getByRole("checkbox", { name: "Group into chains" }).check();
    await expect(page.locator('[data-chain-id="10"]')).toBeVisible();
  }
  const path = grouped ? "/api/chains" : "/api/sessions";
  const requests = () => state.requests.filter((entry) => entry.path === path);
  const input = page.getByRole("textbox", { name: "Search reconstructed payload" });
  const add = async (term) => {
    await input.fill(term);
    await input.press("Enter");
    await expect(input).toHaveValue("");
    await expect(page.getByRole("button", { name: "Refresh sessions" })).toBeEnabled();
  };
  return { state, requests, input, add };
}

for (const grouped of [false, true]) {
  const mode = grouped ? "chains" : "sessions";
  test(`${mode}: edits apply atomically, duplicates stay and empty submits do no work`, async ({ page }) => {
    const { state, requests, input, add } = await setup(page, grouped);
    await add(" A ");
    await input.fill("B");
    await page.getByRole("button", { name: "Add to filter", exact: true }).click();
    await expect.poll(() => requests().at(-1).terms).toEqual(["A", "B"]);
    await expect(page.locator(".filter-notice")).toHaveCount(0);
    let count = requests().length;
    await input.fill("unfinished");
    await page.getByRole("button", { name: "Clear search" }).click();
    await page.getByRole("button", { name: "Remove filter: B", exact: true }).click();
    await expect(page.locator(".filter-notice")).toHaveText("Filters changed. Press Refresh to apply.");
    await page.clock.fastForward(10_000);
    expect(requests()).toHaveLength(count); // Active payload search retains its polling gate.
    await add("C");
    expect(requests().at(-1).terms).toEqual(["A", "C"]);
    await add("A");
    await expect(page.getByRole("button", { name: "Remove filter: A", exact: true })).toHaveCount(2);
    expect(requests().at(-1).terms).toEqual(["A", "C", "A"]);
    await page.getByRole("button", { name: "Remove filter: A", exact: true }).first().click();
    await page.getByRole("checkbox", { name: "Flags only", exact: true }).check();
    await expect.poll(() => requests().at(-1).terms).toEqual(["C", "A"]);
    expect(requests().at(-1).params.get("contains_flag")).toBe("true");
    await expect(page.locator(".filter-notice")).toHaveCount(0);
    await page.getByRole("button", { name: "Remove filter: C", exact: true }).click();
    await page.getByRole("combobox", { name: "Source filter" }).selectOption("1");
    await expect.poll(() => requests().at(-1).terms).toEqual(["A"]);
    expect(requests().at(-1).params.get("source_id")).toBe("1");
    await page.getByRole("button", { name: "Remove filter: A", exact: true }).click();
    await page.getByRole("button", { name: "HTTP", exact: true }).click();
    await expect.poll(() => requests().at(-1).terms).toEqual([]);
    expect(requests().at(-1).params.get("protocol")).toBe("http");
    await expect(page.locator(".filter-notice")).toHaveCount(0);
    await input.fill("draft is ignored by Refresh");
    await page.getByRole("button", { name: "Refresh sessions" }).click();
    await expect(input).toHaveValue("draft is ignored by Refresh");
    expect(requests().at(-1).terms).toEqual([]);
    await add("Z");
    await page.getByRole("button", { name: "Remove filter: Z", exact: true }).click();
    const held = deferred();
    state.handler = () => held.promise;
    count = requests().length;
    await input.press("Enter");
    await expect.poll(() => requests().length).toBe(count + 1);
    for (let i = 0; i < 5; i++) await input.press("Enter");
    expect(requests()).toHaveLength(count + 1);
    held.resolve(null);
    state.handler = null;
    await expect(page.locator(".filter-notice")).toHaveCount(0);
    for (let i = 0; i < 5; i++) await input.press("Enter");
    await page.getByRole("button", { name: "Add to filter", exact: true }).click();
    expect(requests()).toHaveLength(count + 1);
  });

  test(`${mode}: automatic application never flashes the Refresh notice`, async ({ page }) => {
    const { state, requests, input } = await setup(page, grouped);
    await page.evaluate(() => {
      window.noticeInsertions = 0;
      new MutationObserver((records) => {
        for (const record of records) for (const node of record.addedNodes) {
          if (node instanceof Element && (node.matches(".filter-notice") || node.querySelector(".filter-notice"))) {
            window.noticeInsertions++;
          }
        }
      }).observe(document.body, { childList: true, subtree: true });
    });
    const refresh = page.getByRole("button", { name: "Refresh sessions" });
    let held = deferred();
    state.handler = () => held.promise;
    let count = requests().length;
    await input.fill("A");
    await input.press("Enter");
    await expect.poll(() => requests().length).toBe(count + 1);
    await expect(refresh).toBeDisabled();
    await expect(page.locator(".filter-notice")).toHaveCount(0);
    expect(await page.evaluate(() => window.noticeInsertions)).toBe(0);
    held.resolve(null);
    await expect(refresh).toBeEnabled();
    expect(await page.evaluate(() => window.noticeInsertions)).toBe(0);

    await page.getByRole("button", { name: "Remove filter: A", exact: true }).click();
    await expect(page.locator(".filter-notice")).toBeVisible();
    await page.evaluate(() => { window.noticeInsertions = 0; });
    held = deferred();
    count = requests().length;
    await input.fill("B");
    await page.getByRole("button", { name: "Add to filter", exact: true }).click();
    await expect.poll(() => requests().length).toBe(count + 1);
    expect(requests().at(-1).terms).toEqual(["B"]);
    await expect(refresh).toBeDisabled();
    await expect(page.locator(".filter-notice")).toHaveCount(0);
    expect(await page.evaluate(() => window.noticeInsertions)).toBe(0);
    held.resolve({ status: 500, body: { error: "automatic search failed" } });
    await expect(page.locator(".error-strip")).toContainText("automatic search failed");
    await expect(page.locator(".filter-notice")).toBeVisible();
    state.handler = null;
    await refresh.click();
    await expect(page.locator(".filter-notice")).toHaveCount(0);
    await expect(page.locator(".error-strip")).toHaveCount(0);

    // A superseded request must not restore a notice after newer filters load.
    held = deferred();
    state.handler = (entry) => entry.terms.includes("OLD") ? held.promise : null;
    await input.fill("OLD");
    await input.press("Enter");
    await expect.poll(() => requests().at(-1).terms).toEqual(["B", "OLD"]);
    await page.getByRole("button", { name: "Remove filter: OLD", exact: true }).click();
    await input.fill("NEW");
    await input.press("Enter");
    await expect.poll(() => requests().at(-1).terms).toEqual(["B", "NEW"]);
    await expect(refresh).toBeEnabled();
    held.resolve({ status: 500, body: { error: "superseded search failed" } });
    await page.waitForTimeout(100);
    await expect(page.locator(".filter-notice")).toHaveCount(0);
    await expect(page.locator(".error-strip")).toHaveCount(0);
  });

  test(`${mode}: failed application keeps notice and stale first-page response is ignored`, async ({ page }) => {
    const { state, requests, add } = await setup(page, grouped);
    await add("A");
    await page.getByRole("button", { name: "Remove filter: A", exact: true }).click();
    state.handler = () => ({ status: 500, body: { error: "test search failure" } });
    await page.getByRole("button", { name: "Refresh sessions" }).click();
    await expect(page.locator(".error-strip")).toContainText("test search failure");
    await expect(page.locator(".filter-notice")).toBeVisible();
    state.handler = null;
    await page.getByRole("button", { name: "Refresh sessions" }).click();
    await expect(page.locator(".filter-notice")).toHaveCount(0);
    // Returning to the last successful terms still needs a successful new load.
    state.handler = () => ({ status: 500, body: { error: "test search failure" } });
    const failedInput = page.getByRole("textbox", { name: "Search reconstructed payload" });
    await failedInput.fill("never loaded");
    await failedInput.press("Enter");
    await expect(page.locator(".error-strip")).toContainText("test search failure");
    await page.getByRole("button", { name: "Remove filter: never loaded", exact: true }).click();
    await page.getByRole("button", { name: "Refresh sessions" }).click();
    await expect(page.locator(".error-strip")).toContainText("test search failure");
    await expect(page.locator(".filter-notice")).toBeVisible();
    state.handler = null;
    await page.getByRole("button", { name: "Refresh sessions" }).click();
    await expect(page.locator(".filter-notice")).toHaveCount(0);
    const held = deferred();
    state.handler = (entry) => entry.terms.includes("OLD") ? held.promise
      : { body: { items: [grouped ? chain(99) : session(99)], next_cursor: null } };
    const input = page.getByRole("textbox", { name: "Search reconstructed payload" });
    await input.fill("OLD");
    await input.press("Enter");
    await expect.poll(() => requests().at(-1).terms).toEqual(["OLD"]);
    await page.getByRole("button", { name: "Remove filter: OLD", exact: true }).click();
    await expect(page.locator(".filter-notice")).toBeVisible();
    await add("NEW");
    const attr = grouped ? "data-chain-id" : "data-session-id";
    await expect(page.locator(`[${attr}="99"]`)).toBeVisible();
    held.resolve({ body: { items: [grouped ? chain(88) : session(88)], next_cursor: null } });
    await page.waitForTimeout(100);
    await expect(page.locator(`[${attr}="88"]`)).toHaveCount(0);
    await expect(page.locator(`[${attr}="99"]`)).toHaveCount(1);
  });

  test(`${mode}: backend-provided count and code-point length limits are enforced`, async ({ page }) => {
    const { requests, input, add } = await setup(page, grouped, { max_terms: 2, max_term_chars: 128 });
    await add("🦀".repeat(128));
    await input.fill("🦀".repeat(129));
    const count = requests().length;
    await input.press("Enter");
    await expect(page.getByRole("alert")).toHaveText("A filter must not exceed 128 characters.");
    expect(requests()).toHaveLength(count);
    await add("second");
    await input.fill("third");
    await input.press("Enter");
    await expect(page.getByRole("alert")).toHaveText("At most 2 filters are allowed.");
    await expect(page.locator(".payload-filter")).toHaveCount(2);
    expect(requests()).toHaveLength(count + 1);
    await page.getByRole("button", { name: "Remove filter: second", exact: true }).click();
    await add("third");
    expect(requests().at(-1).terms).toEqual(["🦀".repeat(128), "third"]);
  });

  test(`${mode}: a new filter discards old history and its late pagination response`, async ({ page }) => {
    const { state, requests, add } = await setup(page, grouped);
    const held = deferred();
    const cursorKey = grouped ? "chain_cursor" : "cursor";
    state.handler = (entry) => {
      if (entry.params.has(cursorKey)) return held.promise;
      if (entry.terms.length) return { body: { items: [grouped ? chain(999) : session(999)], next_cursor: null } };
      return { body: { items: Array.from({ length: 100 }, (_, i) => grouped ? chain(200 - i) : session(200 - i)), next_cursor: grouped ? "older" : 101 } };
    };
    // A metadata change resets the feed, giving it a fresh history cursor.
    await page.getByRole("checkbox", { name: "Flags only", exact: true }).check();
    const attr = grouped ? "data-chain-id" : "data-session-id";
    await expect(page.locator(`[${attr}="200"]`)).toBeVisible();
    await page.evaluate(() => window.scrollTo(0, document.body.scrollHeight));
    await expect.poll(() => requests().some((entry) => entry.params.has(cursorKey))).toBe(true);
    await page.evaluate(() => window.scrollTo(0, 0));
    await add("fresh");
    await expect(page.locator(`[${attr}="999"]`)).toBeVisible();
    held.resolve({ body: { items: [grouped ? chain(100) : session(100)], next_cursor: null } });
    await page.waitForTimeout(100);
    await expect(page.locator(`[${attr}="100"]`)).toHaveCount(0);
    await expect(page.locator(`[${attr}="200"]`)).toHaveCount(0);
    await expect(page.locator(`[${attr}="999"]`)).toHaveCount(1);
  });
}

test("raising only the advertised backend limit allows eleven duplicate chips", async ({ page }, testInfo) => {
  const { requests, add } = await setup(page, false, { max_terms: 12, max_term_chars: 128 });
  for (let i = 0; i < 11; i++) await add("duplicate");
  await expect(page.locator(".payload-filter")).toHaveCount(11);
  expect(requests().at(-1).terms).toHaveLength(11);
  await page.setViewportSize({ width: 375, height: 812 });
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await page.getByRole("button", { name: "Remove filter: duplicate", exact: true }).first().click();
  await page.getByRole("button", { name: "Switch to Russian" }).click();
  await expect(page.locator(".filter-notice")).toHaveText("Фильтры изменены. Для применения нажмите Refresh.");
  await page.screenshot({ path: testInfo.outputPath("payload-filters-mobile.png") });
});
