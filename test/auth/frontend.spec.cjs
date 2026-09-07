const { test, expect } = require("@playwright/test");

test("auth gate, login errors, logout, expiration and stopped polling", async ({ page }) => {
  let authenticated = false;
  let dataRequests = 0;
  let unauthorizedPayload = false;
  await page.route("**/api/**", async (route) => {
    const path = new URL(route.request().url()).pathname;
    const send = (status, body) => route.fulfill({ status, contentType: "application/json", body: JSON.stringify(body) });
    if (path === "/api/auth/me") return send(authenticated ? 200 : 401, { authenticated, username: "team" });
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
    if (path === "/api/sessions") return send(200, { items: [], next_cursor: null });
    return send(404, {});
  });
  await page.goto("/");
  await expect(page.getByRole("heading", { name: "Sign in" })).toBeVisible();
  expect(dataRequests).toBe(0);
  await expect(page).toHaveTitle("Нюхач");
  await page.setViewportSize({ width: 1280, height: 800 });
  expect(await page.locator(".auth-brand img").evaluate((image) => image.complete && image.naturalWidth > 0)).toBe(true);
  await page.screenshot({ path: "/results/login-desktop.png" });
  await page.setViewportSize({ width: 375, height: 812 });
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await page.screenshot({ path: "/results/login-mobile.png" });
  await page.getByLabel("Username").fill("team");
  await page.getByLabel("Password").fill("wrong");
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  await expect(page.getByRole("alert")).toHaveText("Invalid username or password.");
  expect(dataRequests).toBe(0);
  await page.getByLabel("Password").fill("test-only-password");
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Sessions", exact: true })).toBeVisible();
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
