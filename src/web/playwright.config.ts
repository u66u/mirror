import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./tests",
  testMatch: "**/*.e2e.ts",
  outputDir: "./test-results",
  reporter: "line",
  use: {
    baseURL: "http://127.0.0.1:4173",
    browserName: "chromium",
    headless: true,
    launchOptions: {
      executablePath: process.env.MIRROR_CHROMIUM_PATH ?? "/bin/chromium"
    }
  },
  webServer: {
    command: "npm run dev -- --port 4173 --strictPort",
    reuseExistingServer: true,
    url: "http://127.0.0.1:4173"
  }
});
