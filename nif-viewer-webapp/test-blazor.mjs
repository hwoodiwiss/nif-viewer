import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { resolve } from "node:path";
import puppeteer from "puppeteer-core";

const root = resolve("..");
const port = 5198;
const processHandle = spawn("dotnet", ["run", "--no-build", "-c", "Release", "--project", "nif-viewer-blazor/NavigationSmoke", "--urls", `http://127.0.0.1:${port}`], { cwd: root });
let output = "";
processHandle.stdout.on("data", (data) => { output += data; });
processHandle.stderr.on("data", (data) => { output += data; });
let browser;
try {
  const deadline = Date.now() + 60000;
  while (true) {
    try { if ((await fetch(`http://127.0.0.1:${port}`)).ok) break; } catch {}
    if (Date.now() > deadline || processHandle.exitCode !== null) throw new Error(`Smoke host failed: ${output}`);
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  browser = await puppeteer.launch({ executablePath: process.env.BROWSER_EXECUTABLE, headless: true, args: ["--enable-unsafe-webgpu", "--no-sandbox"] });
  const page = await browser.newPage(); await page.setViewport({ width: 1000, height: 900 });
  const errors = []; page.on("pageerror", (error) => errors.push(error.message));
  page.on("console", (message) => { if (message.type() === "error") console.error(message.text()); });
  await page.goto(`http://127.0.0.1:${port}`);
  await page.waitForSelector("#smoke-canvas", { timeout: 60000 });
  await page.evaluate(async () => { window.smokeInterop = await import("/_content/NifViewer.Blazor/nifviewer.interop.js"); });
  await page.waitForFunction(() => window.smokeInterop.navigationStatus().settings, { timeout: 60000 });
  const status = () => page.evaluate(() => window.smokeInterop.navigationStatus());
  for (let i = 0; i < 3; i++) {
    await page.waitForSelector("#smoke-canvas[data-navigation-ready]");
    console.log(`Blazor attachment cycle ${i + 1}`);
    await page.focus("#smoke-canvas"); await page.keyboard.press("Enter");
    await page.waitForFunction(() => window.smokeInterop.navigationStatus().active);
    const start = (await status()).eye;
    await page.keyboard.down("w");
    await page.waitForFunction((old) => window.smokeInterop.navigationStatus().eye.some((v, i) => Math.abs(v - old[i]) > 0.1), {}, start);
    await page.click("#toggle"); await page.keyboard.up("w");
    await page.waitForFunction(() => !document.querySelector("#smoke-canvas") && !window.smokeInterop.navigationStatus().active);
    await page.click("#toggle"); await page.waitForSelector("#smoke-canvas");
    // Await the component's asynchronous attach by checking status after a render tick.
    await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  }
  await page.focus("#host-field"); await page.type("#host-field", "wasd");
  assert.equal((await status()).active, false);
  await page.waitForSelector("#smoke-canvas[data-navigation-ready]");
  await page.select("select", "Orbit");
  await page.waitForFunction(() => window.smokeInterop.navigationStatus().settings.mode === "Orbit");
  await page.click("button", { count: 1 });
  assert.deepEqual(errors, []);
  console.log("Packaged Blazor smoke: settings, keyboard navigation, host-field isolation and repeated component disposal/reattach passed");
} finally {
  if (browser) await browser.close();
  if (process.platform === "win32") {
    await new Promise((resolve) => spawn("taskkill", ["/pid", String(processHandle.pid), "/T", "/F"]).once("exit", resolve));
  } else { processHandle.kill(); }
}
