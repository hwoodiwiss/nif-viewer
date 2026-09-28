import { defineConfig } from "@playwright/test";

export const browserUse = {
  browserName: "chromium",
  // Use full Chromium, including its compositor, rather than headless-shell.
  channel: "chromium",
  viewport: { width: 1200, height: 1000 },
  trace: "retain-on-failure",
  screenshot: "only-on-failure",
  launchOptions: {
    executablePath: process.env.BROWSER_EXECUTABLE || undefined,
    args: ["--enable-unsafe-webgpu", "--enable-unsafe-swiftshader"],
  },
};

export default defineConfig({
  testDir: "./tests",
  testIgnore: "**/blazor.spec.mjs",
  timeout: 60_000,
  expect: { timeout: 15_000 },
  // Each viewer owns a continuous GPU loop. Serialize tests, isolate contexts.
  workers: 1,
  forbidOnly: !!process.env.CI,
  retries: 0,
  reporter: [["list"], ["html", { open: "never" }]],
  use: { ...browserUse, baseURL: "http://127.0.0.1:5197" },
  projects: [
    { name: "web", metadata: { host: "/web/" } },
    {
      name: "bundler",
      metadata: { host: "/bundler/" },
      testIgnore: ["**/blazor.spec.mjs", "**/*.web.spec.mjs"],
    },
  ],
  webServer: {
    command: "node tests/support/server.mjs",
    url: "http://127.0.0.1:5197/health",
    reuseExistingServer: false,
    timeout: 30_000,
  },
});
