const { test, expect } = require('@playwright/test');

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
  await expect(page.locator('[data-session-id="41"] .known-label')).toHaveText('Known elsewhere');
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
  await expect(page.locator('[data-session-id="41"] .known-label')).toHaveText('Traversal');
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
