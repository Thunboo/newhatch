const { test, expect } = require("@playwright/test");

function byteRanges(text, tokens) {
  const bytes = Buffer.from(text);
  let offset = 0;
  return tokens.map((token) => {
    const start = bytes.indexOf(Buffer.from(token), offset);
    expect(start).toBeGreaterThanOrEqual(0);
    offset = start + Buffer.byteLength(token);
    return { start, end: offset };
  });
}

async function setup(page, grouped) {
  const body = JSON.stringify({ value: "needle needle", flag: "FLAG{needle}", encoded: "%6E%65%65%64%6C%65" });
  const requestBody = "needle <b>literal</b> needle";
  const c2s = `POST /needle HTTP/1.1\r\nHost: test\r\nContent-Length: ${Buffer.byteLength(requestBody)}\r\n\r\n${requestBody}`;
  const s2c = `HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n${body}`;
  const metadata = (id) => ({
    id, collector_id: "test", source_id: 1, source_name: "web", started_at: id * 1_000_000,
    ended_at: id * 1_000_000 + 100, first_payload_c2s_at: id * 1_000_000, first_payload_s2c_at: id * 1_000_000 + 50,
    client_ip: "10.0.0.2", client_port: 40000 + id, server_ip: "10.0.0.1", server_port: 8080,
    protocol: "http", bytes_c2s: Buffer.byteLength(c2s), bytes_s2c: Buffer.byteLength(s2c),
    contains_flag: id === 41, flag_direction: id === 41 ? "s2c" : "none", flag_count: id === 41 ? 1 : 0,
    suricata_alerts: 0, incomplete: false, http: {},
  });
  const members = [metadata(41), metadata(42)];
  const chain = { ...metadata(41), session_count: 2, snapshot_id: 100 };
  const state = { requests: [] };
  await page.route("**/api/**", (route) => {
    const url = new URL(route.request().url());
    state.requests.push({ path: url.pathname, terms: JSON.parse(url.searchParams.get("payload") || "[]") });
    const send = (value) => route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify(value) });
    if (url.pathname === "/api/auth/me") return send({ authenticated: true, username: "team" });
    if (url.pathname === "/api/payload-search/limits") return send({ max_terms: 10, max_term_chars: 128 });
    if (url.pathname === "/api/sources") return send([{ id: 1, name: "web", port: 8080, enabled: true }]);
    if (url.pathname === "/api/collectors") return send({ mode: "local", collectors: [] });
    if (url.pathname === "/api/sessions" || url.pathname === "/api/chains") return send({ items: [url.pathname === "/api/chains" ? chain : members[0]], next_cursor: null });
    if (url.pathname === "/api/chains/41/sessions") return send({ chain, items: members, next_cursor: null });
    if (url.pathname.endsWith("/flag-matches")) return send({ c2s: [], s2c: url.pathname.includes("/41/") ? byteRanges(s2c, ["FLAG{needle}"]) : [] });
    const payload = url.pathname.match(/\/sessions\/(\d+)\/payload\/(c2s|s2c)$/);
    if (payload) return route.fulfill({ status: 200, contentType: "application/octet-stream", body: payload[1] === "42" ? "needle context" : payload[2] === "c2s" ? c2s : s2c });
    return route.fulfill({ status: 404, body: "{}" });
  });
  await page.goto("/");
  if (grouped) await page.getByRole("checkbox", { name: "Group into chains" }).check();
  const input = page.getByRole("textbox", { name: "Search reconstructed payload" });
  const add = async (term) => {
    await input.fill(term);
    await input.press("Enter");
    await expect(page.getByRole("button", { name: "Refresh sessions", exact: true })).toBeEnabled();
  };
  const open = () => page.locator(grouped ? '[data-chain-id="41"]' : '[data-session-id="41"]').click();
  const streams = page.locator(grouped ? '.chain-pair[data-session-id="41"] .stream' : ".detail-panel .stream");
  const reads = () => state.requests.filter(({ path }) => /\/sessions\/41\/(payload\/|flag-matches)/.test(path)).length;
  return { state, input, add, open, streams, reads, body, c2s, s2c };
}

for (const grouped of [false, true]) {
  test(`${grouped ? "chains" : "sessions"}: all applied matches are green and flag overlaps stay yellow with green underline`, async ({ page, context }, testInfo) => {
    await context.grantPermissions(["clipboard-read", "clipboard-write"]);
    const fixture = await setup(page, grouped);
    await fixture.add("needle");
    await fixture.open();
    const request = fixture.streams.first();
    const response = fixture.streams.last();
    await expect(request.locator("mark.payload-match")).toHaveCount(3);
    await expect(response.locator("mark.payload-match")).toHaveCount(3);
    await expect(response.locator("mark.payload-match:not(.flag-match)")).toHaveCount(2);
    const overlap = response.locator("mark.flag-match.payload-match");
    await expect(overlap).toHaveText("needle");
    const colors = await overlap.evaluate((node) => {
      const style = getComputedStyle(node);
      return { background: style.backgroundColor, underline: style.textDecorationColor, line: style.textDecorationLine };
    });
    expect(colors).toEqual({ background: "rgb(243, 200, 64)", underline: "rgb(134, 217, 160)", line: "underline" });
    expect(await request.locator("mark.payload-match").first().evaluate((node) => getComputedStyle(node).backgroundColor)).toBe("rgb(134, 217, 160)");
    await expect(request.locator("pre b")).toHaveCount(0);
    const compact = await fixture.streams.locator("pre").allTextContents();
    const reads = fixture.reads();
    await page.getByRole("checkbox", { name: "Format JSON" }).check();
    await expect(response.locator("mark.payload-match")).toHaveCount(3);
    expect(await response.locator("pre").textContent()).toBe(fixture.s2c.slice(0, fixture.s2c.indexOf("\r\n\r\n") + 4) + JSON.stringify({ value: "needle needle", flag: "FLAG{needle}", encoded: "needle" }, null, 2));
    await response.locator(".stream-actions button").last().click();
    expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(await response.locator("pre").textContent());
    await page.getByRole("button", { name: "Hex", exact: true }).click();
    await expect(fixture.streams.locator("mark")).toHaveCount(0);
    await page.getByRole("button", { name: "Text", exact: true }).click();
    await page.getByRole("checkbox", { name: "Format JSON" }).uncheck();
    await expect.poll(() => fixture.streams.locator("pre").allTextContents()).toEqual(compact);
    expect(fixture.reads()).toBe(reads);
    if (grouped) {
      const pair = page.locator('.chain-pair[data-session-id="42"]');
      await pair.evaluate((node) => node.scrollIntoView({ block: "center" }));
      await expect(pair.locator("mark.payload-match")).toHaveCount(2);
      await page.locator(".chain-scroll").evaluate((node) => { node.scrollTop = 0; });
    }
    await page.screenshot({ path: testInfo.outputPath(`payload-highlights-${grouped ? "chains" : "sessions"}.png`) });
  });

  test(`${grouped ? "chains" : "sessions"}: drafts and pending chip removal do not change highlighting`, async ({ page }) => {
    const fixture = await setup(page, grouped);
    await fixture.open();
    await expect(fixture.streams.last().locator("mark.flag-match")).toHaveCount(1);
    await expect(fixture.streams.locator("mark.payload-match")).toHaveCount(0);
    await fixture.input.fill("needle");
    await expect(fixture.streams.locator("mark.payload-match")).toHaveCount(0);
    await fixture.input.press("Enter");
    if (grouped) await fixture.open();
    await expect(fixture.streams.last().locator("mark.payload-match")).toHaveCount(3);
    const reads = fixture.reads();
    await fixture.input.fill("unfinished");
    await page.getByRole("button", { name: "Remove filter: needle", exact: true }).click();
    await expect(page.locator(".filter-notice")).toBeVisible();
    await expect(fixture.streams.last().locator("mark.payload-match")).toHaveCount(3);
    expect(fixture.reads()).toBe(reads);
    await page.keyboard.press("Escape");
    await page.getByRole("button", { name: "Refresh sessions", exact: true }).click();
    await fixture.open();
    await expect(fixture.streams.locator("mark.payload-match")).toHaveCount(0);
    await expect(fixture.streams.last().locator("mark.flag-match")).toHaveCount(1);
  });
}

for (const pretty of [false, true]) {
  test(`raw match projection preserves payload formatting and source identity; Format JSON=${pretty}`, async ({ page }) => {
    await page.route("**/api/**", (route) => route.fulfill({ status: 401, body: "{}" }));
    await page.goto("/");
    const results = await page.evaluate(async (pretty) => {
      const { displayPayload } = await import("/src/payloadDisplay.ts");
      const { payloadHighlights } = await import("/src/payloadHighlights.ts");
      const embedded = String.raw`{"message":"\u041f\u0440\u0438\u0432\u0435\u0442","emoji":"\ud83e\udd80","literal":"\\u041f","path":"C:\\tmp","lines":"one\ntwo"}`;
      let deep = String.raw`{"text":"\u042f"}`;
      for (let index = 0; index < 4; index++) deep = JSON.stringify({ nested: deep });
      const cases = [
        { text: "%41 A %41", terms: ["A"] },
        { text: "%41 A %41", terms: ["%41"] },
        { text: String.raw`{"a":"\u041f","b":"П","c":"\ud83e\udd80","d":"🦀"}`, terms: [String.raw`\u041f`, String.raw`\ud83e`] },
        { text: String.raw`{"a":"\u041f","b":"П","c":"\ud83e\udd80","d":"🦀"}`, terms: ["П", "🦀"] },
        { text: '{"a":1}', terms: ['"a":1'] },
        { text: '{"2":"b","1":"a","dup":"old","dup":"new","__proto__":"value"}', terms: ["b", "new"] },
        { text: "ABABA aba A.*B a.*b", terms: ["ABA", "ABA", "A.*B"] },
        { text: "😀 П needle \uFEFF %EF%BB%BF", terms: ["needle"] },
        { text: "\uFEFFneedle", terms: ["needle"] },
        { text: JSON.stringify({ nested: embedded, array: String.raw`["\u042f",{"text":"%D0%BC%D0%B8%D1%80"}]` }), terms: [String.raw`\u041f`, "%D0%BC", "tmp", "one"] },
        { text: `HTTP/1.1 200 OK\r\nX-Value: %41\r\n\r\n${JSON.stringify({ nested: embedded })}`, terms: ["%41", "message", "tmp"] },
        { text: `HTTP/1.0 200 OK\nX-Value: %41\n\n${JSON.stringify({ nested: embedded })}`, terms: ["%41", "message", "tmp"] },
        { text: '{"a": "\\u04ZZ"', terms: ["04ZZ"] },
        { text: deep, terms: [String.raw`\u042f`] },
        { text: '{"numbers":[-0,1e3,1e400,1.00],"bool":true,"null":null}', terms: ["1.00", "true"] },
      ];
      return cases.map(({ text, terms }) => {
        const bytes = new TextEncoder().encode(text);
        const result = payloadHighlights(bytes, terms, [], pretty);
        return { text: result.text, expected: displayPayload(bytes, pretty), marked: result.ranges.map((range) => ({ text: result.text.slice(range.start, range.end), start: range.start, kind: range.kind })) };
      });
    }, pretty);
    for (const result of results) expect(result.text).toBe(result.expected);
    expect(results[0].marked).toEqual([{ text: "A", start: 2, kind: "payload" }]);
    expect(results[1].marked).toEqual([{ text: "A", start: 0, kind: "payload" }, { text: "A", start: 4, kind: "payload" }]);
    expect(results[2].marked.map((range) => range.text)).toEqual(["П", "🦀"]);
    expect(results[3].marked.map((range) => range.text)).toEqual(["П", "🦀"]);
    expect(results[4].marked.map((range) => range.text)).toEqual([pretty ? '"a": 1' : '"a":1']);
    expect(results[6].marked.map((range) => range.text)).toEqual(["ABABA", "A.*B"]);
  });
}

test("raw UTF-8 matching does not highlight decoder replacements and scales to a large loaded stream", async ({ page }) => {
  await page.route("**/api/**", (route) => route.fulfill({ status: 401, body: "{}" }));
  await page.goto("/");
  const result = await page.evaluate(async () => {
    const { displayPayload } = await import("/src/payloadDisplay.ts");
    const { payloadHighlights } = await import("/src/payloadHighlights.ts");
    const malformed = new Uint8Array([0xe1, 0x80, 0x41, 0xff, 0x41, 0xef, 0xbf, 0xbd]);
    const replacement = payloadHighlights(malformed, ["�"], [], false);
    const data = new TextEncoder().encode("x".repeat(3 * 1024 * 1024) + "needle");
    const large = payloadHighlights(data, ["needle"], [], false);
    return { replacement, expected: displayPayload(malformed, false), largeText: large.text === displayPayload(data, false), large: large.ranges };
  });
  expect(result.replacement.text).toBe(result.expected);
  expect(result.replacement.ranges).toEqual([{ start: 4, end: 5, kind: "payload" }]);
  expect(result.largeText).toBe(true);
  expect(result.large).toEqual([{ start: 3 * 1024 * 1024, end: 3 * 1024 * 1024 + 6, kind: "payload" }]);
});
