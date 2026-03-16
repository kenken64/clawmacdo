import { Page, Locator } from "@playwright/test";
import { DeployScenario } from "./csv-reader";

export class DeployFormFiller {
  private card: Locator;
  private n: number;

  constructor(
    private page: Page,
    cardNumber: number = 1
  ) {
    this.n = cardNumber;
    this.card = page.locator(`#deploy-card-${cardNumber}`);
  }

  async fillCustomerInfo(s: DeployScenario) {
    await this.card
      .locator('input[name="customer_name"]')
      .fill(s.customer_name);
    await this.card
      .locator('input[name="customer_email"]')
      .fill(s.customer_email);
  }

  async selectProvider(s: DeployScenario) {
    await this.card
      .locator('select[name="provider"]')
      .selectOption(s.provider);
    // Wait for toggleProvider() JS to finish re-rendering
    await this.page.waitForTimeout(300);
  }

  async fillProviderCredentials(s: DeployScenario) {
    switch (s.provider) {
      case "digitalocean":
        await this.fillVisible('input[name="do_token"]', s.do_token);
        break;
      case "tencent":
        await this.fillVisible(
          'input[name="tencent_secret_id"]',
          s.tencent_secret_id
        );
        await this.fillVisible(
          'input[name="tencent_secret_key"]',
          s.tencent_secret_key
        );
        break;
      case "lightsail":
        await this.fillVisible(
          'input[name="aws_access_key_id"]',
          s.aws_access_key_id
        );
        await this.fillVisible(
          'input[name="aws_secret_access_key"]',
          s.aws_secret_access_key
        );
        break;
      case "azure":
        await this.fillVisible(
          'input[name="azure_tenant_id"]',
          s.azure_tenant_id
        );
        await this.fillVisible(
          'input[name="azure_subscription_id"]',
          s.azure_subscription_id
        );
        await this.fillVisible(
          'input[name="azure_client_id"]',
          s.azure_client_id
        );
        await this.fillVisible(
          'input[name="azure_client_secret"]',
          s.azure_client_secret
        );
        break;
      case "byteplus":
        await this.fillVisible(
          'input[name="byteplus_access_key"]',
          s.byteplus_access_key
        );
        await this.fillVisible(
          'input[name="byteplus_secret_key"]',
          s.byteplus_secret_key
        );
        break;
    }
  }

  async selectRegionAndSize(s: DeployScenario) {
    if (s.region) {
      await this.card.locator('select[name="region"]').selectOption(s.region);
    }
    if (s.size) {
      await this.card.locator('select[name="size"]').selectOption(s.size);
    }
  }

  async configureModels(s: DeployScenario) {
    const modelContainer = this.card.locator(
      `#model-selectors-${this.n}`
    );

    // Select primary model (for BytePlus, it's auto-selected — override if different)
    const primarySelect = modelContainer.locator(
      'select[data-model-slot="primary"]'
    );
    const currentPrimary = await primarySelect.inputValue();
    if (currentPrimary !== s.primary_model) {
      await primarySelect.selectOption(s.primary_model);
      await this.page.waitForTimeout(200); // wait for syncModelSelectors re-render
    }

    // Fill primary model's API key
    await this.fillModelKey(s, s.primary_model);

    // Failover 1
    if (s.failover_1) {
      const fo1Select = modelContainer.locator(
        'select[data-model-slot="failover_1"]'
      );
      await fo1Select.selectOption(s.failover_1);
      await this.page.waitForTimeout(200);
      await this.fillModelKey(s, s.failover_1);
    }

    // Failover 2
    if (s.failover_2) {
      const fo2Select = modelContainer.locator(
        'select[data-model-slot="failover_2"]'
      );
      await fo2Select.selectOption(s.failover_2);
      await this.page.waitForTimeout(200);
      await this.fillModelKey(s, s.failover_2);
    }
  }

  private async fillModelKey(s: DeployScenario, model: string) {
    const keyFieldMap: Record<string, { field: string; value: string }> = {
      anthropic: { field: "anthropic_key", value: s.anthropic_key },
      openai: { field: "openai_key", value: s.openai_key },
      gemini: { field: "gemini_key", value: s.gemini_key },
      byteplus: {
        field: "byteplus_ark_api_key",
        value: s.byteplus_ark_api_key,
      },
    };

    const mapping = keyFieldMap[model];
    if (mapping && mapping.value) {
      const input = this.card.locator(`input[name="${mapping.field}"]`);
      if ((await input.count()) > 0 && (await input.isVisible())) {
        await input.fill(mapping.value);
      }
    }
  }

  async fillMessaging(s: DeployScenario) {
    if (s.telegram_bot_token) {
      const tgInput = this.card.locator(
        'input[name="telegram_bot_token"]'
      );
      if ((await tgInput.count()) > 0) {
        await tgInput.fill(s.telegram_bot_token);
      }
    }
    if (s.whatsapp_phone_number) {
      const waInput = this.card.locator(
        'input[name="whatsapp_phone_number"]'
      );
      if ((await waInput.count()) > 0) {
        await waInput.fill(s.whatsapp_phone_number);
      }
    }
  }

  async configureOptions(s: DeployScenario) {
    // Tailscale
    const tailscaleToggle = this.card.locator('input[name="tailscale"]');
    if ((await tailscaleToggle.count()) > 0) {
      const isChecked = await tailscaleToggle.isChecked();
      const wantChecked = s.tailscale === "true";
      if (isChecked !== wantChecked) {
        await tailscaleToggle.setChecked(wantChecked);
        await this.page.waitForTimeout(100);
      }
      if (wantChecked && s.tailscale_auth_key) {
        await this.fillVisible(
          'input[name="tailscale_auth_key"]',
          s.tailscale_auth_key
        );
      }
    }
  }

  async submit() {
    const deployBtn = this.card.locator(
      'button[type="submit"], .deploy-submit-btn, button:has-text("Deploy")'
    );
    await deployBtn.click();
  }

  async waitForDryRunComplete(timeoutMs: number = 30_000) {
    // Wait for SSE progress to show "Completed" or dry-run indication
    await this.page.waitForSelector(
      'text=Completed, text=dry-run, text=0.0.0.0',
      { timeout: timeoutMs }
    );
  }

  private async fillVisible(selector: string, value: string) {
    if (!value) return;
    const input = this.card.locator(selector);
    if ((await input.count()) > 0 && (await input.isVisible())) {
      await input.fill(value);
    }
  }
}
