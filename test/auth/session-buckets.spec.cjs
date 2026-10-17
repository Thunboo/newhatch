const { test, expect } = require('@playwright/test');

async function expectRowLabel(page, label) {
  await page.locator('[data-session-id="41"] .rule-label-trigger').hover();
  await expect(page.getByRole('dialog', { name: 'Matched rules', exact: true }).getByText(label, { exact: true })).toBeVisible();
  await page.keyboard.press('Escape');
}

async function setup(page) {
  const state = { rules: [], requests: [], nextId: 1 };
  await page.route('**/api/**', async (route) => {
    const req = route.request();
    const url = new URL(req.url());
    const send = (body, status = 200) => route.fulfill({ status, contentType: 'application/json', body: JSON.stringify(body) });
    if (url.pathname === '/api/auth/me') return send({ authenticated: true, username: 'team' });
    if (url.pathname === '/api/payload-search/limits') return send({ max_terms: 10, max_term_chars: 128 });
    if (url.pathname === '/api/sources') return send([{ id: 1, name: 'web', port: 8080, enabled: true }]);
    if (url.pathname === '/api/collectors') return send({ mode: 'local', collectors: [] });
    if (url.pathname.startsWith('/api/attack-rules')) {
      if (req.method() === 'GET') return send(state.rules);
      if (req.method() === 'DELETE') { state.rules = []; return route.fulfill({ status: 204 }); }
      const input = req.postDataJSON();
      expect(input.id).toBeUndefined(); expect(input.owner).toBeUndefined();
      if (input.steps.some((step) => step.some((c) => c.op === 'regex' && c.value === '['))) return send({ error: 'invalid regex: unclosed character class' }, 400);
      const id = req.method() === 'POST' ? state.nextId++ : Number(url.pathname.split('/').pop());
      const rule = { ...input, id, owner: 'team' };
      state.rules = [...state.rules.filter((r) => r.id !== id), rule]; return send(rule, req.method() === 'POST' ? 201 : 200);
    }
    if (url.pathname === '/api/sessions' || url.pathname === '/api/chains') {
      state.requests.push({ feed: url.pathname, bucket: url.searchParams.get('bucket'), payload: JSON.parse(url.searchParams.get('payload') || '[]'), source: url.searchParams.get('source_id') });
      const rule = state.rules.find((r) => r.enabled);
      const known = url.searchParams.get('bucket') === 'known';
      const item = { id: 41, source_id: 1, source_name: 'web', client_ip: '10.0.0.2', client_port: 40000, server_ip: '10.0.0.1', server_port: 8080, protocol: 'http',
        started_at: 1700000000000000, ended_at: 1700000000000100, bytes_c2s: 10, bytes_s2c: 20, contains_flag: false, flag_count: 0, suricata_alerts: 0,
        http: { method: 'GET', path: '/users' }, session_count: 4, snapshot_id: 41, collector_id: 'test', known_matches: rule ? [{ rule_id: rule.id, label: rule.label, events: [{ session_id: 42, transaction: 0 }, { session_id: 43, transaction: 0 }] }] : [] };
      return send({ items: known === !!rule ? [item] : [], next_cursor: null });
    }
    return send({}, 404);
  });
  await page.goto('/');
  await expect(page.getByRole('heading', { name: 'Sessions', exact: true })).toBeVisible();
  return state;
}

test('bucket filters are independent, survive reload and remain active in chains', async ({ page }) => {
  const state = await setup(page);
  await expect(page.getByRole('tab', { name: 'Unsorted', exact: true })).toHaveAttribute('aria-selected', 'true');
  const input = page.getByRole('textbox', { name: 'Search reconstructed payload' });
  await input.fill('Привет'); await input.press('Enter');
  await expect(page.getByRole('button', { name: 'Remove filter: Привет' })).toBeVisible();
  await page.getByRole('tab', { name: 'Known', exact: true }).click();
  await expect(page.getByRole('button', { name: 'Remove filter: Привет' })).toHaveCount(0);
  await input.fill('root:'); await input.press('Enter');
  await page.getByRole('checkbox', { name: 'Group into chains' }).check();
  await expect.poll(() => state.requests.at(-1)).toMatchObject({ feed: '/api/chains', bucket: 'known', payload: ['root:'] });
  await page.getByRole('tab', { name: 'Unsorted', exact: true }).click();
  await expect(page.getByRole('button', { name: 'Remove filter: Привет' })).toBeVisible();
  await expect.poll(() => state.requests.at(-1)).toMatchObject({ feed: '/api/chains', bucket: 'unknown', payload: ['Привет'] });
  await page.reload();
  await expect(page.getByRole('button', { name: 'Remove filter: Привет' })).toBeVisible();
  await page.getByRole('tab', { name: 'Known', exact: true }).click();
  await expect(page.getByRole('button', { name: 'Remove filter: root:' })).toBeVisible();
});

test('refresh removes rows that moved to another bucket since the previous read', async ({ page }) => {
  const state = await setup(page);
  await expect(page.locator('[data-session-id="41"]')).toBeVisible();
  state.rules.push({ id: 1, enabled: true, label: 'Known elsewhere' });
  await page.getByRole('button', { name: 'Refresh sessions', exact: true }).click();
  await expect(page.locator('[data-session-id]')).toHaveCount(0);
  await page.getByRole('tab', { name: 'Known', exact: true }).click();
  await expectRowLabel(page, 'Known elsewhere');
});

test('Russian rule editor stays usable on a narrow screen and closes with Escape', async ({ page }, testInfo) => {
  await page.setViewportSize({ width: 375, height: 812 });
  await setup(page);
  await page.getByRole('button', { name: 'Switch to Russian', exact: true }).click();
  await page.getByRole('tab', { name: 'Известные', exact: true }).click();
  await expect(page.getByRole('tab', { name: 'Известные', exact: true })).toHaveAttribute('aria-selected', 'true');
  await page.getByRole('button', { name: 'Правила известных атак', exact: true }).click();
  const dialog = page.locator('dialog');
  await expect(dialog.getByRole('heading', { name: 'Правила известных атак' })).toBeVisible();
  await dialog.getByLabel('Метка', { exact: true }).fill('Проверка');
  await dialog.getByRole('button', { name: '+ ЗАТЕМ шаг', exact: true }).click();
  await dialog.evaluate((node) => { node.scrollTop = node.scrollHeight; });
  const close = dialog.getByRole('button', { name: 'Закрыть правила' });
  await expect(close).toBeInViewport();
  expect(await dialog.evaluate((node) => node.scrollWidth <= node.clientWidth)).toBe(true);
  await page.screenshot({ path: testInfo.outputPath('rules-mobile-ru.png') });
  await page.keyboard.press('Escape');
  await expect(dialog).toHaveCount(0);
});

test('create, edit, validate, disable and delete a rule without losing traffic', async ({ page }, testInfo) => {
  const state = await setup(page);
  await page.getByRole('button', { name: 'Known attack rules', exact: true }).click();
  const dialog = page.locator('dialog');
  await dialog.getByLabel('Label', { exact: true }).fill('Traversal');
  const first = dialog.locator('.rule-step').first();
  await first.getByRole('textbox', { name: 'Value', exact: true }).fill('/users');
  await first.getByRole('button', { name: '+ AND condition' }).click();
  const query = first.locator('.rule-condition').last();
  await query.getByLabel('Field', { exact: true }).selectOption('query');
  await query.getByLabel('Operator').selectOption('contains');
  await query.getByLabel('Parameter name (optional)').fill('id');
  await query.getByRole('textbox', { name: 'Value', exact: true }).fill('../../../');
  await dialog.getByRole('button', { name: '+ THEN step' }).click();
  const second = dialog.locator('.rule-step').last();
  await second.getByRole('textbox', { name: 'Value', exact: true }).fill('/profile');
  await second.getByRole('button', { name: '+ AND condition' }).click();
  const response = second.locator('.rule-condition').last();
  await response.getByLabel('Field', { exact: true }).selectOption('response_body');
  await response.getByLabel('Operator').selectOption('contains');
  await response.getByRole('textbox', { name: 'Value', exact: true }).fill('root:');
  await dialog.getByRole('button', { name: 'Save rule', exact: true }).click();
  await expect.poll(() => state.rules.length).toBe(1);
  expect(state.rules[0]).toMatchObject({ source_id: 1, owner: 'team', adjacent: true, max_gap: 0 });
  await dialog.getByRole('button', { name: 'Close rules' }).click();
  await expect(page.locator('[data-session-id]')).toHaveCount(0);
  await page.getByRole('tab', { name: 'Known', exact: true }).click();
  await expectRowLabel(page, 'Traversal');
  await page.getByRole('button', { name: 'Known attack rules', exact: true }).click();
  await dialog.getByRole('button', { name: /Traversal.*team/ }).click();
  await dialog.getByLabel('Steps must be adjacent').uncheck();
  await dialog.getByLabel('Maximum intervening events between steps').fill('5');
  await first.getByLabel('Operator').first().selectOption('regex');
  await first.getByRole('textbox', { name: 'Value', exact: true }).first().fill('[');
  await dialog.getByRole('button', { name: 'Save rule', exact: true }).click();
  await expect(dialog.getByRole('alert')).toContainText('invalid regex');
  await first.getByRole('textbox', { name: 'Value', exact: true }).first().fill('^/users$');
  await dialog.getByRole('button', { name: 'Save rule', exact: true }).click();
  await expect.poll(() => state.rules[0].max_gap).toBe(5);
  await page.screenshot({ path: testInfo.outputPath('known-attack-rule-editor.png') });
  await dialog.getByLabel('Enabled', { exact: true }).uncheck();
  await dialog.getByRole('button', { name: 'Save rule', exact: true }).click();
  await expect.poll(() => state.rules[0].enabled).toBe(false);
  await dialog.getByRole('button', { name: 'Delete rule Traversal' }).click();
  await expect.poll(() => state.rules.length).toBe(0);
  await dialog.getByRole('button', { name: 'Close rules' }).click();
  await page.getByRole('tab', { name: 'Unsorted', exact: true }).click();
  await expect(page.locator('[data-session-id="41"]')).toBeVisible();
});

async function highlightSetup(page, grouped) {
  const definitions = [
    { rule_id: 7, label: 'Traversal', version: '0123456789abcdef', events: [{ session_id: 41, transaction: 0 }, { session_id: 42, transaction: 0 }] },
    { rule_id: 8, label: 'Second rule', version: '1123456789abcdef', events: [{ session_id: 41, transaction: 0 }] },
    { rule_id: 9, label: 'Long label '.repeat(12), version: '2123456789abcdef', events: [{ session_id: 41, transaction: 0 }] },
  ];
  const c2s = 'GET /needle?echo=needle HTTP/1.1\r\nHost: needle\r\n\r\n';
  const headers = 'HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n';
  const body = { prefix: 'А😀', value: 'root: FLAG{root:}' };
  const s2c = headers + JSON.stringify({ ...body, prefix: '%D0%90%F0%9F%98%80' });
  const compact = headers + JSON.stringify(body);
  const pretty = headers + JSON.stringify(body, null, 2);
  const ranges = (text, term) => {
    const found = [];
    for (let at = text.indexOf(term); at >= 0; at = text.indexOf(term, at + term.length)) {
      found.push({ start: Buffer.byteLength(text.slice(0, at)), end: Buffer.byteLength(text.slice(0, at + term.length)) });
    }
    return found;
  };
  const metadata = (id) => ({ id, source_id: 1, source_name: 'web', collector_id: 'test', client_ip: '10.0.0.2', client_port: 40000,
    server_ip: '10.0.0.1', server_port: 8080, protocol: 'http', started_at: id * 100000, ended_at: id * 100000 + 1,
    first_payload_c2s_at: id * 100000, first_payload_s2c_at: id * 100000 + 1, bytes_c2s: c2s.length, bytes_s2c: s2c.length,
    contains_flag: id === 42, flag_direction: id === 42 ? 's2c' : 'none', flag_count: id === 42 ? 1 : 0, suricata_alerts: 0,
    known_matches: definitions, http: { method: 'GET', path: '/needle' } });
  const members = [41, 42, 43].map(metadata);
  const chain = { ...members[0], session_count: 3, snapshot_id: 43 };
  const state = { reads: [], fail: 0 };
  await page.route('**/api/**', (route) => {
    const url = new URL(route.request().url());
    const send = (body, status = 200) => route.fulfill({ status, contentType: 'application/json', body: JSON.stringify(body) });
    if (url.pathname === '/api/auth/me') return send({ authenticated: true, username: 'team' });
    if (url.pathname === '/api/payload-search/limits') return send({ max_terms: 10, max_term_chars: 128 });
    if (url.pathname === '/api/sources') return send([{ id: 1, name: 'web', port: 8080, enabled: true }]);
    if (url.pathname === '/api/collectors') return send({ mode: 'local', collectors: [] });
    if (url.pathname === '/api/sessions') return send({ items: members, next_cursor: null });
    if (url.pathname === '/api/chains') return send({ items: [chain], next_cursor: null });
    if (url.pathname === '/api/chains/41/sessions') return send({ chain, items: members.map((session) => ({ ...session, known_matches: [] })), next_cursor: null });
    const match = url.pathname.match(/\/sessions\/(\d+)\/(payload\/(c2s|s2c)|flag-matches|rule-matches)$/);
    if (match) {
      const id = Number(match[1]);
      state.reads.push({ id, path: match[2], selection: url.searchParams.get('selection') });
      if (match[2] === 'flag-matches') return send({ representation: 'readable-v1', c2s: [], s2c: id === 42 ? ranges(compact, 'FLAG{root:}').map((range) => ({ ...range, value: 'FLAG{root:}' })) : [] });
      if (match[2] === 'rule-matches') {
        if (state.fail) return send({ error: 'rule highlights unavailable' }, state.fail);
        const selected = JSON.parse(url.searchParams.get('selection'));
        expect(selected.map(([rule]) => rule)).toEqual(id === 41 ? [7, 8, 9] : [7]);
        expect(selected[0][2]).toEqual(id === 41 ? [[0, 0]] : [[1, 0]]);
        return send({ representation: 'readable-v1', c2s: id === 41 ? definitions.map((rule) => ({ ...rule, ranges: ranges(c2s, '/needle').map((r) => ({ start: r.start + 1, end: r.end })), pretty_ranges: ranges(c2s, '/needle').map((r) => ({ start: r.start + 1, end: r.end })) })) : [],
          s2c: id === 42 ? [{ ...definitions[0], ranges: ranges(compact, 'root:'), pretty_ranges: ranges(pretty, 'root:') }] : [] });
      }
      return route.fulfill({ status: 200, contentType: 'application/octet-stream', body: match[3] === 'c2s' ? c2s : s2c });
    }
    return send({}, 404);
  });
  await page.goto('/');
  if (grouped) await page.getByRole('checkbox', { name: 'Group into chains' }).check();
  const row = page.locator(grouped ? '[data-chain-id="41"]' : 'tr[data-session-id="41"]');
  const pair = (id) => grouped ? page.locator(`.chain-pair[data-session-id="${id}"]`) : page.locator('.detail-panel');
  return { state, row, pair, compact, pretty, c2s };
}

for (const grouped of [false, true]) {
  test(`${grouped ? 'chains' : 'sessions'}: adaptive labels and popup survive narrow and ultrawide layouts`, async ({ page }, testInfo) => {
    await page.setViewportSize({ width: 1280, height: 800 });
    const { row } = await highlightSetup(page, grouped);
    const trigger = row.locator('.rule-label-trigger');
    await expect(trigger.locator('.known-label')).toHaveText('+3');
    const count = await trigger.locator('.rule-label-count').boundingBox();
    const cell = await row.locator('td').nth(1).boundingBox();
    expect(count.x + count.width).toBeLessThanOrEqual(cell.x + cell.width);
    await trigger.hover();
    const popup = page.getByRole('dialog', { name: 'Matched rules', exact: true });
    await expect(popup.locator('li')).toHaveCount(3);
    await expect(popup.getByText('Long label '.repeat(12).trim(), { exact: true })).toBeVisible();
    expect(await popup.evaluate((node) => node.scrollWidth <= node.clientWidth)).toBe(true);
    await popup.hover(); await page.waitForTimeout(180); await expect(popup).toBeVisible();
    await popup.focus(); await page.keyboard.press('Escape'); await expect(popup).toHaveCount(0);
    await expect(page.locator('.detail-panel')).toHaveCount(0);
    await trigger.click(); await expect(popup).toBeVisible(); await expect(page.locator('.detail-panel')).toHaveCount(0);
    await page.keyboard.press('Escape');
    await page.setViewportSize({ width: 3440, height: 1440 });
    await expect(trigger.locator('.known-label')).toHaveText(['Traversal', '+2']);
    await page.screenshot({ path: testInfo.outputPath(`rule-labels-${grouped ? 'chains' : 'sessions'}-wide.png`) });
    await page.setViewportSize({ width: 375, height: 812 });
    await row.locator('td').first().click();
    const header = page.locator('.detail-panel .stream > header').first();
    const headerTrigger = header.locator('.rule-label-trigger');
    await expect(headerTrigger.locator('.rule-label-count')).toBeVisible();
    const badge = await headerTrigger.locator('.rule-label-count').boundingBox();
    const headerBox = await header.boundingBox();
    expect(badge.x + badge.width).toBeLessThanOrEqual(headerBox.x + headerBox.width);
    await headerTrigger.click(); await expect(popup).toBeVisible();
    await popup.focus(); await page.keyboard.press('Escape');
    await expect(popup).toHaveCount(0); await expect(page.locator('.detail-panel')).toBeVisible();
    await page.screenshot({ path: testInfo.outputPath(`rule-labels-${grouped ? 'chains' : 'sessions'}-mobile.png`) });
  });

  test(`${grouped ? 'chains' : 'sessions'}: only selected fields are green, directions and JSON/Hex remain correct`, async ({ page }, testInfo) => {
    const fixture = await highlightSetup(page, grouped);
    await fixture.row.locator('td').first().click();
    const request = fixture.pair(41).locator('.stream').first();
    await expect(request.locator('mark.payload-match')).toHaveText('needle');
    await expect(request.locator('.rule-label-trigger')).toHaveAttribute('aria-label', 'Matched rules: 3');
    await expect(fixture.pair(41).locator('.stream').last().locator('mark')).toHaveCount(0);
    await expect(fixture.pair(41).locator('.stream').last().locator('.rule-label-trigger')).toHaveCount(0);
    if (grouped) await fixture.pair(42).scrollIntoViewIfNeeded();
    else { await page.keyboard.press('Escape'); await page.locator('tr[data-session-id="42"] td').first().click(); }
    const response = fixture.pair(42).locator('.stream').last();
    await expect(response.locator('mark.payload-match')).toHaveText(['root:', 'root:']);
    await expect(response.locator('mark.flag-match.payload-match')).toHaveText('root:');
    await expect(response.locator('.rule-label-trigger')).toHaveAttribute('aria-label', 'Matched rules: 1');
    const reads = fixture.state.reads.filter((read) => read.id === 42).length;
    await page.getByRole('checkbox', { name: 'Format JSON' }).check();
    await expect(response.locator('pre')).toHaveText(fixture.pretty);
    await expect(response.locator('mark.payload-match')).toHaveText(['root:', 'root:']);
    await page.getByRole('button', { name: 'Hex', exact: true }).click();
    await expect(response.locator('mark')).toHaveCount(0);
    await expect(response.locator('.rule-label-trigger')).toBeVisible();
    await page.getByRole('button', { name: 'Text', exact: true }).click();
    await expect(response.locator('mark.payload-match')).toHaveText(['root:', 'root:']);
    expect(fixture.state.reads.filter((read) => read.id === 42).length).toBe(reads);
    if (grouped) {
      await fixture.pair(43).scrollIntoViewIfNeeded();
      await expect(fixture.pair(43).locator('pre')).toHaveCount(2);
      await expect(fixture.pair(43).locator('mark, .rule-label-trigger')).toHaveCount(0);
      expect(fixture.state.reads.filter((read) => read.id === 43 && read.path === 'rule-matches')).toHaveLength(0);
    }
    await page.screenshot({ path: testInfo.outputPath(`rule-highlights-${grouped ? 'chains' : 'sessions'}.png`) });
  });
}

test('failed rule explanations keep payload inspectable and support retry', async ({ page }) => {
  const fixture = await highlightSetup(page, false);
  fixture.state.fail = 503;
  await fixture.row.locator('td').first().click();
  await expect(page.locator('.detail-panel pre').first()).toHaveText(fixture.c2s);
  await expect(page.getByText('Could not load rule highlights', { exact: false })).toBeVisible();
  await expect(page.locator('.detail-panel mark')).toHaveCount(0);
  fixture.state.fail = 0;
  await page.getByRole('button', { name: 'Retry', exact: true }).click();
  await expect(page.locator('.detail-panel mark.payload-match')).toHaveText('needle');
});

test('chain rule explanations reuse the card cache on quick reopening', async ({ page }) => {
  const fixture = await highlightSetup(page, true);
  await fixture.row.locator('td').first().click();
  await expect(fixture.pair(41).locator('mark.payload-match')).toHaveText('needle');
  await fixture.pair(42).scrollIntoViewIfNeeded();
  await expect(fixture.pair(42).locator('mark.payload-match')).toHaveText(['root:', 'root:']);
  await page.keyboard.press('Escape');
  await expect(page.locator('.detail-panel')).toHaveCount(0);
  const reads = fixture.state.reads.filter((read) => read.id === 41 || read.id === 42).length;
  await fixture.row.locator('td').first().click();
  await expect(fixture.pair(41).locator('mark.payload-match')).toHaveText('needle');
  await fixture.pair(42).scrollIntoViewIfNeeded();
  await expect(fixture.pair(42).locator('mark.payload-match')).toHaveText(['root:', 'root:']);
  expect(fixture.state.reads.filter((read) => read.id === 41 || read.id === 42).length).toBe(reads);
});

test('stale rule explanations request a refresh instead of showing old marks', async ({ page }) => {
  const fixture = await highlightSetup(page, false);
  fixture.state.fail = 409;
  await fixture.row.locator('td').first().click();
  await expect(page.getByText('Rule matches changed. Refresh the list and reopen the session.', { exact: false })).toBeVisible();
  await expect(page.locator('.detail-panel mark')).toHaveCount(0);
  await expect(page.locator('.detail-panel pre').first()).toHaveText(fixture.c2s);
});
