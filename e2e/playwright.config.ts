import { defineConfig } from "@playwright/test";
import * as path from "path";
import * as fs from "fs";

// Load .env.e2e if it exists
const envFile = path.resolve(__dirname, ".env.e2e");
if (fs.existsSync(envFile)) {
  const lines = fs.readFileSync(envFile, "utf-8").split("\n");
  for (const line of lines) {
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith("#")) continue;
    const eq = trimmed.indexOf("=");
    if (eq > 0) {
      const key = trimmed.slice(0, eq).trim();
      const val = trimmed.slice(eq + 1).trim();
      if (!process.env[key]) process.env[key] = val;
    }
  }
}

export default defineConfig({
  testDir: "./tests",
  timeout: 120_000,
  retries: 0,
  workers: 1, // sequential — single server
  use: {
    baseURL: "http://localhost:3456",
    trace: "on-first-retry",
    screenshot: "only-on-failure",
  },
  webServer: {
    command:
      "CLAWMACDO_DRY_RUN=true cargo run --bin clawmacdo -- serve --port 3456",
    url: "http://localhost:3456",
    timeout: 300_000, // 5 min for Rust compile + startup
    reuseExistingServer: !process.env.CI,
    cwd: path.resolve(__dirname, ".."),
  },
});
