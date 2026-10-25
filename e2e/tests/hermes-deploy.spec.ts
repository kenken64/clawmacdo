import { test, expect, Page } from "@playwright/test";

test.beforeEach(async ({ page }, testInfo) => {
  // Each isolated browser gets its own rate-limit bucket on the shared test server.
  await page.setExtraHTTPHeaders({ "x-forwarded-for": `192.0.2.${testInfo.line % 254 + 1}` });
});

async function openHermesForm(page: Page) {
  await page.goto("/login");
  const pin = page.locator('input[name="pin"]');
  if (await pin.isVisible()) {
    await pin.fill(process.env.CLAWMACDO_E2E_PIN || "111111");
    await page.locator('button[type="submit"]').click();
  }
  await page.goto("/");
  const card = page.locator("#deploy-card-1");
  await card.locator('input[name="target"][value="hermes"]').check();
  await card.locator('[name="customer_name"]').fill("Hermes Test");
  await card.locator('[name="customer_email"]').fill("hermes@example.com");
  await card.locator('[name="aws_access_key_id"]').fill("test-access");
  await card.locator('[name="aws_secret_access_key"]').fill("test-secret");
  return card;
}

test("Hermes OpenRouter deployment configures OpenCode and Telegram access without pairing", async ({ page }) => {
  const card = await openHermesForm(page);
  await expect(card.locator('[name="hermes_ai_provider"]')).toHaveValue("openrouter");
  await expect(card.locator('[name="hermes_image"]')).toHaveCount(0);
  await expect(card.locator('[name="hermes_telegram_pair_code"]')).toHaveCount(0);
  await expect(card.locator('[name="hermes_mount_docker_socket"]')).toHaveCount(0);
  await expect(card).toContainText("Hermes runs directly on the Lightsail host");
  await expect(card.locator('[name="hermes_bedrock_api_key"]')).toBeHidden();
  await expect(card.locator('[name="hermes_install_opencode"]')).toBeChecked();
  await card.locator('[name="hermes_openrouter_api_key"]').fill("sk-or-web-test");
  await card.locator('[name="hermes_openrouter_model"]').fill("openai/custom-test-model");
  await card.locator('[name="hermes_telegram_bot_token"]').fill("123456:web-test-token");
  await card.locator('[name="hermes_telegram_allowed_users"]').fill("111,222");
  await card.locator('[name="hermes_telegram_home_channel"]').fill("111");

  const requestPromise = page.waitForRequest(req => req.url().endsWith("/api/deploy") && req.method() === "POST");
  await card.locator('.deploy-submit-btn').click();
  const body = (await requestPromise).postDataJSON();
  expect(body).not.toHaveProperty("hermes_telegram_pair_code");
  expect(body).toMatchObject({
    target: "hermes",
    provider: "hermes-lightsail",
    hermes_ai_provider: "openrouter",
    hermes_openrouter_api_key: "sk-or-web-test",
    hermes_openrouter_model: "openai/custom-test-model",
    hermes_install_opencode: true,
    hermes_bedrock_api_key: "",
    hermes_telegram_bot_token: "123456:web-test-token",
    hermes_telegram_allowed_users: "111,222",
    hermes_telegram_home_channel: "111",
  });
  await expect(card.locator('.deploy-summary')).toBeVisible({ timeout: 30_000 });
  await expect(card.locator('.deploy-summary')).toContainText("Hermes Agent on AWS Lightsail");
  await expect(card.locator('.deploy-summary')).toContainText("journalctl -u hermes-gateway");
  await expect(card.locator('.deploy-summary')).not.toContainText("docker");
  await expect(card.locator('.deploy-log-area')).toContainText("OpenRouter openai/custom-test-model");
  await expect(card.locator('.deploy-log-area')).toContainText("OpenCode: CLI v2 will be installed");
  await expect(card.locator('.deploy-log-area')).toContainText("prebuilt Hermes bundle alongside OS prerequisites");
  await expect(card.locator('.hermes-os-upgrade')).toContainText("OS upgrade not run (dry-run)");

  await expect(card.locator('.telegram-pairing-btn')).toHaveCount(0);
  await expect(card.locator('.telegram-pairing-code')).toHaveCount(0);

  const stored = await page.evaluate(() => JSON.stringify(localStorage));
  expect(stored).not.toContain("sk-or-web-test");
  expect(stored).not.toContain("123456:web-test-token");
  await page.reload();
  await expect(page.locator('.deploy-summary:visible')).toBeVisible();
  await expect(page.locator('.deploy-summary:visible .telegram-pairing-btn')).toHaveCount(0);
  await expect(page.locator('.deploy-summary:visible .hermes-os-upgrade')).toContainText("OS upgrade not run (dry-run)");
});

for (const osUpgrade of ['queued', 'launch_failed']) {
  test(`Hermes remains ready with OS upgrade ${osUpgrade} and preserves the notice after reload`, async ({ page }) => {
    const card = await openHermesForm(page);
    await card.locator('[name="hermes_openrouter_api_key"]').fill('sk-or-web-test');
    await page.route('**/api/deploy/*/events', route => route.fulfill({
      contentType: 'text/event-stream',
      body: 'data: DEPLOY_COMPLETE_JSON:' + JSON.stringify({
        ip: '192.0.2.10', ssh_key_path: '/tmp/test-key', hostname: 'hermes-background-test',
        dashboard_enabled: false, os_upgrade: osUpgrade,
      }) + '\n\n',
    }));
    await card.locator('.deploy-submit-btn').click();
    await expect(card.locator('.deploy-summary')).toBeVisible();
    await expect(card.locator('.hermes-os-upgrade')).toContainText('Hermes is ready to use');
    await expect(card.locator('.hermes-os-upgrade')).toContainText('/hermes-os-upgrade/status');
    const message = osUpgrade === 'queued' ? 'queued after deployment' : 'launch could not be confirmed';
    await expect(card.locator('.hermes-os-upgrade')).toContainText(message);
    if (osUpgrade === 'launch_failed') {
      await expect(card.locator('.hermes-os-upgrade')).toContainText('systemctl start --no-block');
    }
    await page.reload();
    await expect(page.locator('.deploy-summary:visible .hermes-os-upgrade')).toContainText(message);
  });
}

test("Hermes provider switching validates the selected key and retains Bedrock support", async ({ page }) => {
  const card = await openHermesForm(page);
  await card.locator('.deploy-submit-btn').click();
  await expect(card.locator('.field-error')).toHaveText("OpenRouter API Key is required");
  await card.locator('[name="hermes_openrouter_api_key"]').fill("sk-or-unused");
  await card.locator('[name="hermes_ai_provider"]').selectOption("bedrock");
  await expect(card.locator('[name="hermes_openrouter_api_key"]')).toBeHidden();
  await expect(card.locator('[name="hermes_install_opencode"]')).toBeHidden();
  await card.locator('.deploy-submit-btn').click();
  await expect(card.locator('.field-error')).toHaveText("AWS Bedrock API Key is required");
  await card.locator('[name="hermes_bedrock_api_key"]').fill("bedrock-test");
  const requestPromise = page.waitForRequest(req => req.url().endsWith("/api/deploy") && req.method() === "POST");
  await card.locator('.deploy-submit-btn').click();
  expect((await requestPromise).postDataJSON()).toMatchObject({
    hermes_ai_provider: "bedrock",
    hermes_bedrock_api_key: "bedrock-test",
    hermes_openrouter_api_key: "",
    hermes_install_opencode: false,
  });
  await expect(card.locator('.deploy-summary')).toBeVisible({ timeout: 30_000 });
});

test("Hermes permits skipping OpenCode without requesting Telegram pairing", async ({ page }) => {
  const card = await openHermesForm(page);
  await card.locator('[name="hermes_openrouter_api_key"]').fill("sk-or-web-test");
  await card.locator('[name="hermes_install_opencode"]').uncheck();
  await card.locator('[name="hermes_telegram_bot_token"]').fill("123456:web-test-token");
  await card.locator('[name="hermes_telegram_allowed_users"]').fill("111");
  const requestPromise = page.waitForRequest(req => req.url().endsWith("/api/deploy") && req.method() === "POST");
  await card.locator('.deploy-submit-btn').click();
  expect((await requestPromise).postDataJSON()).toMatchObject({
    hermes_install_opencode: false,
    hermes_telegram_allowed_users: "111",
  });
  await expect(card.locator('.deploy-summary')).toBeVisible({ timeout: 30_000 });
  await expect(card.locator('.deploy-log-area')).not.toContainText("OpenCode: CLI v2 will be installed");
});

test("Hermes Quick Tunnel needs no Cloudflare credentials and preserves the generated link", async ({ page }) => {
  const card = await openHermesForm(page);
  await card.locator('[name="hermes_openrouter_api_key"]').fill("sk-or-web-test");
  await expect(card.locator('.hermes-tunnel-config')).toBeHidden();
  await card.locator('[name="hermes_dashboard"]').uncheck();
  await card.locator('[name="hermes_cloudflare_tunnel"]').check();
  await expect(card.locator('[name="hermes_dashboard"]')).toBeChecked();
  await expect(card.locator('[name="hermes_dashboard"]')).toBeDisabled();
  await card.locator('.deploy-submit-btn').click();
  await expect(card.locator('.field-error')).toHaveCount(1);
  await expect(card.locator('[name="hermes_cloudflare_tunnel_token"]')).toHaveCount(0);
  await expect(card.locator('[name="hermes_dashboard_hostname"]')).toHaveCount(0);
  await card.locator('[name="hermes_dashboard_password"]').fill("test-private-dashboard-password");
  // No real tunnel exists in dry-run mode: simulate the URL returned over SSH after cloud-init.
  await page.route("**/api/deploy/*/events", route => route.fulfill({
    contentType: "text/event-stream",
    body: 'data: DEPLOY_COMPLETE_JSON:' + JSON.stringify({
      ip: '0.0.0.0', ssh_key_path: '(dry-run)', hostname: 'hermes-test',
      dashboard_enabled: true, dashboard_url: 'https://hermes-web-test.trycloudflare.com',
    }) + '\n\n',
  }));
  const requestPromise = page.waitForRequest(req => req.url().endsWith("/api/deploy") && req.method() === "POST");
  await card.locator('.deploy-submit-btn').click();
  const body = (await requestPromise).postDataJSON();
  expect(body).not.toHaveProperty("hermes_cloudflare_tunnel_token");
  expect(body).not.toHaveProperty("hermes_dashboard_hostname");
  expect(body).toMatchObject({
    hermes_dashboard: true,
    hermes_cloudflare_tunnel: true,
    hermes_dashboard_password: "test-private-dashboard-password",
  });
  const summary = card.locator('.deploy-summary');
  await expect(summary).toBeVisible({ timeout: 30_000 });
  await expect(summary.getByRole('link', { name: 'https://hermes-web-test.trycloudflare.com' })).toHaveAttribute('href', 'https://hermes-web-test.trycloudflare.com');
  await expect(summary).toContainText('Dashboard login: admin');
  const stored = await page.evaluate(() => JSON.stringify(localStorage));
  expect(stored).toContain('https://hermes-web-test.trycloudflare.com');
  expect(stored).not.toContain('test-private-tunnel-token');
  expect(stored).not.toContain('test-private-dashboard-password');
  await expect(card.locator('.deploy-log-area')).not.toContainText('test-private-tunnel-token');
  await expect(card.locator('.deploy-log-area')).not.toContainText('test-private-dashboard-password');
  await page.reload();
  await expect(page.locator('.deploy-summary:visible').getByRole('link', { name: 'https://hermes-web-test.trycloudflare.com' })).toBeVisible();
});

test("Hermes omits disabled tunnel credentials and dashboard instructions", async ({ page }) => {
  const card = await openHermesForm(page);
  await card.locator('[name="hermes_openrouter_api_key"]').fill("sk-or-web-test");
  await card.locator('[name="hermes_cloudflare_tunnel"]').check();
  await card.locator('[name="hermes_dashboard_password"]').fill("unused-password");
  await card.locator('[name="hermes_cloudflare_tunnel"]').uncheck();
  await expect(card.locator('[name="hermes_dashboard"]')).toBeEnabled();
  await card.locator('[name="hermes_dashboard"]').uncheck();
  const requestPromise = page.waitForRequest(req => req.url().endsWith("/api/deploy") && req.method() === "POST");
  await card.locator('.deploy-submit-btn').click();
  expect((await requestPromise).postDataJSON()).toMatchObject({
    hermes_dashboard: false,
    hermes_cloudflare_tunnel: false,
    hermes_dashboard_password: '',
  });
  await expect(card.locator('.deploy-summary')).toBeVisible({ timeout: 30_000 });
  await expect(card.locator('.deploy-summary')).not.toContainText('9119');
  await page.reload();
  await expect(page.locator('.deploy-summary:visible')).not.toContainText('9119');
});
