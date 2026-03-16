import { parse } from "csv-parse/sync";
import * as fs from "fs";
import * as path from "path";

export interface DeployScenario {
  scenario_name: string;
  customer_name: string;
  customer_email: string;
  provider: string;
  do_token: string;
  tencent_secret_id: string;
  tencent_secret_key: string;
  aws_access_key_id: string;
  aws_secret_access_key: string;
  azure_tenant_id: string;
  azure_subscription_id: string;
  azure_client_id: string;
  azure_client_secret: string;
  byteplus_access_key: string;
  byteplus_secret_key: string;
  region: string;
  size: string;
  primary_model: string;
  anthropic_key: string;
  openai_key: string;
  gemini_key: string;
  byteplus_ark_api_key: string;
  failover_1: string;
  failover_2: string;
  tailscale: string;
  tailscale_auth_key: string;
  telegram_bot_token: string;
  whatsapp_phone_number: string;
}

function resolveEnvVar(raw: string): string {
  if (!raw) return "";
  const match = raw.match(/^\$\{(.+)\}$/);
  if (match) {
    const envVar = match[1];
    const value = process.env[envVar];
    if (!value) {
      console.warn(`Warning: env var ${envVar} not set, using placeholder`);
      return `PLACEHOLDER_${envVar}`;
    }
    return value;
  }
  return raw;
}

export function loadScenarios(): DeployScenario[] {
  const csvPath = path.resolve(__dirname, "../fixtures/deploy-scenarios.csv");
  const content = fs.readFileSync(csvPath, "utf-8");
  const records: Record<string, string>[] = parse(content, {
    columns: true,
    skip_empty_lines: true,
    trim: true,
  });

  return records.map((row) => {
    const resolved: Record<string, string> = {};
    for (const [key, val] of Object.entries(row)) {
      resolved[key] = resolveEnvVar(val);
    }
    return resolved as unknown as DeployScenario;
  });
}
