import { test, expect } from "@playwright/test";
import { loadScenarios, DeployScenario } from "../helpers/csv-reader";
import { DeployFormFiller } from "../helpers/form-filler";

const scenarios = loadScenarios();

test.describe("Deploy Form — All Providers & Permutations", () => {
  for (const scenario of scenarios) {
    test(`[${scenario.provider}] ${scenario.scenario_name}`, async ({
      page,
    }) => {
      // Fresh page — creates one deploy card automatically
      await page.goto("/");
      await page.waitForSelector('[id^="deploy-card-"]', { timeout: 10_000 });

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
