import { test, expect, Page } from "@playwright/test";
import { loadScenarios, DeployScenario } from "../helpers/csv-reader";
import { DeployFormFiller } from "../helpers/form-filler";

const scenarios = loadScenarios();
const e2ePin = process.env.CLAWMACDO_E2E_PIN || "111111";

test.describe("Deploy Form — All Providers & Permutations", () => {
  for (const scenario of scenarios) {
    test(`[${scenario.provider}] ${scenario.scenario_name}`, async ({
      page,
    }) => {
      // Fresh page — creates one deploy card automatically
      await openDeployPage(page);

      const filler = new DeployFormFiller(page, 1);

      // Step 1: Fill customer info
      await filler.fillCustomerInfo(scenario);

      // Step 2: Select provider (triggers toggleProvider JS)
      await filler.selectProvider(scenario);

      // Step 3: Fill provider-specific credentials
      await filler.fillProviderCredentials(scenario);

      // Step 4: Select region & size
      await filler.selectRegionAndSize(scenario);

      // Step 5: Configure AI models (primary + failovers + API keys)
      await filler.configureModels(scenario);

      // Step 6: Fill messaging fields
      await filler.fillMessaging(scenario);

      // Step 7: Configure options (tailscale, etc.)
      await filler.configureOptions(scenario);

      // Step 8: Verify form state before submit
      await verifyFormState(page, scenario);

      // Step 9: Submit the form (dry-run mode)
      await filler.submit();

      // Step 10: Wait for dry-run deploy to complete
      await filler.waitForDryRunComplete();
    });
  }
});

test("Tailscale auth key stays detected when entered before enabling Tailscale", async ({
  page,
}) => {
  await openDeployPage(page);

  const card = page.locator("#deploy-card-1");
  const tailscaleKey = card.locator('input[name="tailscale_auth_key"]');
  const tailscaleToggle = card.locator('input[name="tailscale"]');

  await tailscaleKey.fill("tskey-auth-test-before-checkbox");
  await expect(tailscaleToggle).toBeChecked();
  await expect(tailscaleKey).toHaveValue("tskey-auth-test-before-checkbox");

  await tailscaleToggle.setChecked(false);
  await expect(tailscaleKey).toHaveValue("tskey-auth-test-before-checkbox");
  await tailscaleToggle.setChecked(true);
  await expect(tailscaleKey).toHaveValue("tskey-auth-test-before-checkbox");

  const state = await card.locator("form").evaluate((form: HTMLFormElement) => {
    const input = form.querySelector<HTMLInputElement>(
      'input[name="tailscale_auth_key"]'
    );
    const toggle = form.querySelector<HTMLInputElement>(
      'input[name="tailscale"]'
    );
    return {
      required: input?.hasAttribute("data-required") ?? false,
      tailscale: toggle?.checked ?? false,
      value: input?.value ?? "",
    };
  });

  expect(state).toEqual({
    required: true,
    tailscale: true,
    value: "tskey-auth-test-before-checkbox",
  });
});

async function openDeployPage(page: Page) {
  await page.goto("/login");
  const pinInput = page.locator('input[name="pin"]');
  if (await pinInput.isVisible()) {
    await pinInput.fill(e2ePin);
    await page.locator('button[type="submit"]').click();
  }
  await page.goto("/");
  await page.waitForSelector('[id^="deploy-card-"]', { timeout: 10_000 });
}

async function verifyFormState(page: any, s: DeployScenario) {
  const card = page.locator("#deploy-card-1");

  // Verify provider is selected
  const providerVal = await card
    .locator('select[name="provider"]')
    .inputValue();
  expect(providerVal).toBe(s.provider);

  // Verify primary model is selected
  const modelContainer = card.locator("#model-selectors-1");
  const primaryVal = await modelContainer
    .locator('select[data-model-slot="primary"]')
    .inputValue();
  expect(primaryVal).toBe(s.primary_model);

  // Verify customer name
  const customerName = await card
    .locator('input[name="customer_name"]')
    .inputValue();
  expect(customerName).toBe(s.customer_name);

  // Verify tailscale toggle
  if (s.tailscale === "true") {
    const tailscaleChecked = await card
      .locator('input[name="tailscale"]')
      .isChecked();
    expect(tailscaleChecked).toBe(true);
  }

  // Verify failover 1 if set
  if (s.failover_1) {
    const fo1Val = await modelContainer
      .locator('select[data-model-slot="failover_1"]')
      .inputValue();
    expect(fo1Val).toBe(s.failover_1);
  }
}
