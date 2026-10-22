import { test, expect, Page } from "@playwright/test";

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

test("Hermes OpenRouter deployment configures OpenCode and supports Telegram pairing", async ({ page }) => {
  const card = await openHermesForm(page);
  await expect(card.locator('[name="hermes_ai_provider"]')).toHaveValue("openrouter");
  await expect(card.locator('[name="hermes_image"]')).toHaveCount(0);
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

  // Dry-run deployments have no SSH server: exercise the UI against simulated approval responses.
  let attempts = 0;
  await page.route("**/api/telegram/pairing/approve", async route => {
    expect(route.request().postDataJSON()).toMatchObject({
      provider: "hermes-lightsail", pairing_code: "abcd1234", ip: "0.0.0.0",
    });
    attempts++;
    await route.fulfill({
      status: attempts === 1 ? 502 : 200,
      json: { ok: attempts > 1, message: attempts === 1 ? "Pairing code expired" : "Telegram pairing approved" },
    });
  });
  await card.locator('.telegram-pairing-code').fill("abcd1234");
  await card.locator('.telegram-pairing-btn').click();
  await expect(card.locator('.telegram-pairing-result')).toHaveText("Pairing code expired");
  await expect(card.locator('.telegram-pairing-btn')).toBeEnabled();
  await card.locator('.telegram-pairing-btn').click();
  await expect(card.locator('.telegram-pairing-result')).toHaveText("Telegram pairing approved");
  await expect(card.locator('.telegram-pairing-code')).toHaveValue("");

  const stored = await page.evaluate(() => JSON.stringify(localStorage));
  expect(stored).not.toContain("sk-or-web-test");
  expect(stored).not.toContain("123456:web-test-token");
  await page.reload();
  await expect(page.locator('.deploy-summary:visible .telegram-pairing-btn')).toBeVisible();
});

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

test("Hermes permits skipping OpenCode and submitting a pending Telegram code", async ({ page }) => {
  const card = await openHermesForm(page);
  await card.locator('[name="hermes_openrouter_api_key"]').fill("sk-or-web-test");
  await card.locator('[name="hermes_install_opencode"]').uncheck();
  await card.locator('[name="hermes_telegram_bot_token"]').fill("123456:web-test-token");
  await card.locator('[name="hermes_telegram_pair_code"]').fill("ABCD1234");
  const requestPromise = page.waitForRequest(req => req.url().endsWith("/api/deploy") && req.method() === "POST");
  await card.locator('.deploy-submit-btn').click();
  expect((await requestPromise).postDataJSON()).toMatchObject({
    hermes_install_opencode: false,
    hermes_telegram_pair_code: "ABCD1234",
  });
  await expect(card.locator('.deploy-summary')).toBeVisible({ timeout: 30_000 });
  await expect(card.locator('.deploy-log-area')).not.toContainText("OpenCode: CLI v2 will be installed");
});
