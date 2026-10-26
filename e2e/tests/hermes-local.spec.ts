import { test, expect, Page } from "@playwright/test";

test.beforeEach(async ({ page }, testInfo) => {
  // Each isolated browser gets its own rate-limit bucket on the shared test server.
  await page.setExtraHTTPHeaders({ "x-forwarded-for": `198.51.100.${testInfo.line % 254 + 1}` });
});

async function openLocalHermesForm(page: Page) {
  await page.goto("/login");
  const pin = page.locator('input[name="pin"]');
  if (await pin.isVisible()) {
    await pin.fill(process.env.CLAWMACDO_E2E_PIN || "111111");
    await page.locator('button[type="submit"]').click();
  }
  await page.goto("/");
  const card = page.locator("#deploy-card-1");
  await card.locator('input[name="target"][value="hermes-local"]').check();
  await card.locator('[name="customer_name"]').fill("Local Test");
  await card.locator('[name="customer_email"]').fill("local@example.com");
  return card;
}

const installedStatus = {
  supported: true,
  reason: null,
  installed: true,
  record: {
    platform: "macos",
    hermes_home: "/Users/test/.hermes",
    launcher: "/Users/test/.local/bin/hermes",
    hermes_version: "0.22.5",
    hermes_commit: "bf0203a794a07aac3ff764972ab9087baa4e4f4e",
    ai_model: "OpenRouter openai/gpt-6-sol",
    terminal_backend: "local",
    service_installed: true,
    opencode_binary_installed: true,
    written_files: [],
    backup_dir: null,
    installed_at: "2026-10-26T08:00:00Z",
  },
};

test("Hermes on this computer hides cloud settings and runs a dry-run install", async ({ page }) => {
  await page.route("**/api/hermes-local/status", route =>
    route.fulfill({ json: { supported: true, reason: null, installed: false, record: null } }));
  const card = await openLocalHermesForm(page);

  await expect(card.locator('[name="aws_access_key_id"]')).toBeHidden();
  await expect(card.locator('[name="region"]')).toBeHidden();
  await expect(card.locator('[name="hermes_cloudflare_tunnel"]')).toBeHidden();
  await expect(card.locator('[name="hermes_dashboard"]')).toBeHidden();
  await expect(card.getByText("Hermes runs directly on the Lightsail host")).toBeHidden();
  await expect(card.locator('.deploy-submit-btn')).toHaveText("Install on this computer");
  await expect(card.locator('.deploy-step-total')).toHaveText("6");

  const warning = card.locator('.hermes-local-terminal-warning');
  await expect(warning).toBeVisible();
  await card.locator('[name="hermes_terminal_backend"]').selectOption("docker");
  await expect(warning).toBeHidden();
  await card.locator('[name="hermes_terminal_backend"]').selectOption("local");
  await expect(warning).toBeVisible();

  await card.locator('[name="hermes_openrouter_api_key"]').fill("sk-or-local-test");
  const requestPromise = page.waitForRequest(req => req.url().endsWith("/api/deploy") && req.method() === "POST");
  await card.locator('.deploy-submit-btn').click();
  const body = (await requestPromise).postDataJSON();
  expect(body).toMatchObject({
    target: "hermes-local",
    provider: "hermes-local",
    hermes_openrouter_api_key: "sk-or-local-test",
    hermes_terminal_backend: "local",
    hermes_local_force: false,
    hermes_cloudflare_tunnel: false,
    hermes_dashboard: false,
  });

  // The test server runs with CLAWMACDO_DRY_RUN, so nothing is installed.
  await expect(card.locator('.deploy-summary')).toBeVisible({ timeout: 30_000 });
  await expect(card.locator('.deploy-summary')).toContainText("Hermes Agent on this computer");
  await expect(card.locator('.deploy-summary')).toContainText("nothing was installed");
  await expect(card.locator('.deploy-log-area')).toContainText("[Step 1/6] Checking this computer");
  await expect(card.locator('.deploy-log-area')).toContainText("[Dry-run] No changes will be made");
  await expect(card.locator('.deploy-badge')).not.toContainText(/fail/i);
});

test("an existing clawmacdo install shows status and can be uninstalled", async ({ page }) => {
  await page.route("**/api/hermes-local/status", route => route.fulfill({ json: installedStatus }));
  let uninstallBody: unknown = null;
  await page.route("**/api/hermes-local/uninstall", async route => {
    uninstallBody = route.request().postDataJSON();
    await route.fulfill({
      json: {
        ok: true,
        message: "Hermes and its data were removed from this computer.",
        dry_run: false,
        data_removed: true,
        warnings: ["example leftover warning"],
        log: ["Hermes local uninstall plan:", "    ✓ Removed gateway"],
      },
    });
  });
  const card = await openLocalHermesForm(page);

  const banner = card.locator('.hermes-local-status');
  await expect(banner).toBeVisible();
  await expect(banner).toContainText("Hermes 0.22.5 is installed on this computer");
  await expect(banner).toContainText("OpenRouter openai/gpt-6-sol");

  await banner.locator('.hermes-local-uninstall-btn[data-keep-data="false"]').click();
  const modal = page.locator('#local-uninstall-modal');
  await expect(modal).toContainText("permanently deletes the Hermes data folder");
  await modal.locator('.local-uninstall-confirm').click();
  await expect(modal).toContainText("Hermes and its data were removed from this computer.");
  await expect(modal).toContainText("Warning: example leftover warning");
  await expect(modal).toContainText("Removed gateway");
  expect(uninstallBody).toEqual({ keep_data: false });
  await modal.locator('.local-uninstall-cancel').click();
  await expect(modal).toHaveCount(0);

  await banner.locator('.hermes-local-uninstall-btn[data-keep-data="true"]').click();
  await expect(modal).toContainText("The Hermes data folder (configuration, .env API keys, sessions and memory) is kept.");
});

test("local installs are disabled when the server cannot install here", async ({ page }) => {
  const reason = "Installing on this computer is only available when the web UI listens on localhost";
  await page.route("**/api/hermes-local/status", route =>
    route.fulfill({ json: { supported: false, reason, installed: false, record: null } }));
  await page.goto("/login");
  const pin = page.locator('input[name="pin"]');
  if (await pin.isVisible()) {
    await pin.fill(process.env.CLAWMACDO_E2E_PIN || "111111");
    await page.locator('button[type="submit"]').click();
  }
  await page.goto("/");
  const card = page.locator("#deploy-card-1");
  const option = card.locator('[data-target-option="hermes-local"]');
  await expect(option.locator('input')).toBeDisabled();
  await expect(option).toContainText("Unavailable: " + reason);
  await expect(card.locator('input[name="target"][value="openclaw"]')).toBeChecked();
});
