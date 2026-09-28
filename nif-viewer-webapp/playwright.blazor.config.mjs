import { defineConfig } from "@playwright/test";
import { browserUse } from "./playwright.config.mjs";

export default defineConfig({
  testDir: "./tests",
  testMatch: "**/blazor.spec.mjs",
  timeout: 90_000,
  expect: { timeout: 20_000 },
  workers: 1,
  forbidOnly: !!process.env.CI,
  retries: 0,
  reporter: [["list"], ["html", { open: "never" }]],
  use: { ...browserUse, baseURL: "http://127.0.0.1:5198" },
  webServer: {
    command:
      "dotnet run --no-build --no-restore -c Release --project ../nif-viewer-blazor/NavigationSmoke --urls http://127.0.0.1:5198",
    url: "http://127.0.0.1:5198",
    reuseExistingServer: false,
    timeout: 60_000,
  },
});
