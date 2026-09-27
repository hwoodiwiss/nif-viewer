// Run from either webapp with BROWSER_EXECUTABLE set. Tests the real WASM adapter.
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { resolve, extname, sep } from "node:path";
import { createRequire } from "node:module";
import { navigationNif } from "./fixture.mjs";

const require = createRequire(resolve(process.cwd(), "package.json"));
const puppeteer = require("puppeteer-core");
const root = resolve(process.cwd(), "..");
const nif = root.endsWith("nif-viewer");
const name = nif ? "nif-viewer" : "wgpu-testbed";
const moduleName = name.replaceAll("-", "_") + "_lib";
const format = process.env.WASM_HOST || "web";
const html = `<!doctype html><html><body><input id="field"><canvas id="scene" width="800" height="600" style="width:800px;height:600px"></canvas><script type="module">
import init, * as wasm from '/${name}-lib/pkg-web/${moduleName}.js';
await init(); window.viewer = wasm;
${nif ? 'wasm.attach("scene");' : 'document.querySelector("canvas").remove(); wasm.run_wasm();'}
</script></body></html>`;
const server = createServer(async (request, response) => {
  try {
    const path = decodeURIComponent(new URL(request.url, "http://localhost").pathname);
    if (path === "/interop.js" && nif) {
      const interop = (await readFile(resolve(root, "nif-viewer-blazor/NifViewer.Blazor/wwwroot/nifviewer.interop.js"), "utf8"))
        .replace('"./nif_viewer_lib.js"', `"/${name}-lib/pkg-web/${moduleName}.js"`);
      response.setHeader("Content-Type", "text/javascript"); response.end(interop); return;
    }
    if (path === "/" && format === "web") { response.setHeader("Content-Type", "text/html"); response.end(html); return; }
    const file = format === "bundler" ? resolve(root, `${name}-webapp/dist`, "." + (path === "/" ? "/index.html" : path)) : resolve(root, "." + path);
    if (!file.startsWith(root + sep)) { response.writeHead(403).end(); return; }
    response.setHeader("Content-Type", ({ ".wasm": "application/wasm", ".js": "text/javascript", ".html": "text/html", ".png": "image/png" })[extname(file)] || "application/octet-stream");
    response.end(await readFile(file));
  } catch { response.writeHead(404).end(); }
});
await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
const browser = await puppeteer.launch({ executablePath: process.env.BROWSER_EXECUTABLE,
  headless: process.env.HEADED !== "1", args: ["--enable-unsafe-webgpu", "--disable-background-timer-throttling", "--no-sandbox"] });
try {
  const page = await browser.newPage();
  await page.setViewport({ width: 1200, height: 1000 });
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  if (process.env.NAV_TEST_VERBOSE) page.on("console", (message) => console.log(message.type(), message.text()));
  await page.evaluateOnNewDocument(() => {
    window.testPads = [];
    const platformObject = (prototype, values) => Object.create(prototype,
      Object.fromEntries(Object.entries(values).map(([key, value]) => [key, { value, enumerable: true }])));
    Object.defineProperty(navigator, "getGamepads", { value: () => window.testPads.map((p) => p ? platformObject(Gamepad.prototype, {
      ...p, buttons: p.buttons.map((value) => platformObject(GamepadButton.prototype, { value, pressed: value > 0.5, touched: value > 0 }))
    }) : null) });
  });
  await page.goto(`http://127.0.0.1:${server.address().port}/`);
  const gpu = await page.evaluate(async () => !!(navigator.gpu && await navigator.gpu.requestAdapter()));
  assert.ok(gpu, "WebGPU adapter unavailable: run on a GPU-capable headed browser; this is not a rendering pass");
  await page.waitForFunction(() => {
    window.viewer ||= window.nifViewer || window.sceneViewer;
    return window.viewer && JSON.parse(window.viewer.navigation_status()).settings;
  }, { timeout: 60000 });
  console.log(`Browser: ${await browser.version()}`);
  const status = () => page.evaluate(() => JSON.parse(window.viewer.navigation_status()));
  const waitFrames = () => page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  const canvas = await page.$("canvas");
  if (!nif) {
    const before = (await status()).eye;
    await page.evaluate(() => window.viewer.navigation_command("Frame"));
    await page.waitForFunction((old) => JSON.parse(window.viewer.navigation_status()).eye.some((v, i) => v !== old[i]), {}, before);
  }
  if (nif) {
    const before = (await status()).eye;
    await page.evaluate((bytes) => {
      window.viewer.set_light(1, 1, 1, -0.4, -0.8, -0.45, 8, 0.25, false);
      window.viewer.load_nif("generated-navigation.nif", new Uint8Array(bytes), {});
    }, navigationNif());
    await page.waitForFunction((old) => JSON.parse(window.viewer.navigation_status()).eye.some((v, i) => v !== old[i]), {}, before);
  }
  // Copy the final displayed canvas to a 2D canvas immediately after render.
  const pixels = () => page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => {
    const source = document.querySelector("canvas");
    const copy = document.createElement("canvas"); copy.width = 160; copy.height = 90;
    const context = copy.getContext("2d"); context.drawImage(source, 0, 0, 160, 90);
    resolve(Array.from(context.getImageData(0, 0, 160, 90).data));
  })));
  await waitFrames();
  const rendered = await pixels();
  const colors = new Set();
  for (let i = 0; i < rendered.length; i += 4) colors.add(`${rendered[i] >> 4},${rendered[i + 1] >> 4},${rendered[i + 2] >> 4}`);
  assert.ok(colors.size >= 4, `Expected rendered landmarks plus background, saw only ${colors.size} coarse colors`);
  await canvas.focus(); await page.keyboard.press("Enter");
  await page.waitForFunction(() => JSON.parse(window.viewer.navigation_status()).active);
  const start = await status();
  await page.keyboard.down("w");
  await page.waitForFunction((start) => {
    const eye = JSON.parse(window.viewer.navigation_status()).eye;
    return Math.hypot(...eye.map((v, i) => v - start[i])) > 0.2;
  }, {}, start.eye);
  await page.keyboard.up("w");
  await page.keyboard.press("Escape"); await waitFrames();
  assert.equal((await status()).active, false, "Escape releases without terminating WASM");
  await page.keyboard.press("Enter"); await page.keyboard.down("ArrowRight");
  const beforeLook = await status();
  await page.waitForFunction((old) => JSON.parse(window.viewer.navigation_status()).target.some((v, i) => Math.abs(v - old[i]) > 0.05), {}, beforeLook.target);
  await page.keyboard.up("ArrowRight");
  const turned = await pixels();
  assert.ok(turned.some((v, i) => Math.abs(v - rendered[i]) > 30), "Displayed scene must change after moving/turning");
  // Uncaptured right-button drag rotates once, without multiplying by dt.
  const rectangle = await canvas.boundingBox();
  const dragStart = await status();
  await page.mouse.move(rectangle.x + 100, rectangle.y + 100); await page.mouse.down({ button: "right" });
  await page.mouse.move(rectangle.x + 200, rectangle.y + 130, { steps: 8 }); await page.mouse.up({ button: "right" }); await waitFrames();
  assert.notDeepEqual((await status()).target, dragStart.target, "Right drag must look");
  await page.keyboard.press("o"); await waitFrames(); assert.equal((await status()).settings.mode, "Orbit");
  await page.keyboard.press("o"); await waitFrames(); assert.equal((await status()).settings.mode, "Fly");
  // Focus outside the canvas while a key remains held, then release outside.
  await page.evaluate(() => {
    if (!document.querySelector("#field")) { const input = document.createElement("input"); input.id = "field"; document.body.prepend(input); }
  });
  await page.keyboard.down("w"); await page.focus("#field"); await page.keyboard.up("w"); await waitFrames();
  assert.equal((await status()).active, false);
  const stopped = (await status()).eye;
  await page.type("#field", "wasd"); await waitFrames(); assert.deepEqual((await status()).eye, stopped);
  await canvas.focus(); await page.keyboard.press("Enter"); await waitFrames(); assert.deepEqual((await status()).eye, stopped, "No stuck key on return");
  await page.keyboard.press("Tab"); await waitFrames(); assert.equal((await status()).active, false, "Canvas must not trap Tab");
  // Read fresh objects from a sparse standard-mapped Gamepad API array.
  await page.evaluate(() => {
    document.activeElement.blur();
    window.testPads = [null, { id: "Synthetic Xbox", index: 1, connected: true, mapping: "standard", axes: [0, 0, 0, 0], buttons: Array(16).fill(0) }];
  });
  await waitFrames();
  await page.evaluate(() => { window.testPads[1].buttons[0] = 1; });
  await page.waitForFunction(() => JSON.parse(window.viewer.navigation_status()).active);
  assert.equal((await status()).device, "Synthetic Xbox");
  const padStart = (await status()).eye;
  await page.evaluate(() => { window.testPads[1].axes[1] = -1; });
  await page.waitForFunction((old) => JSON.parse(window.viewer.navigation_status()).eye.some((v, i) => Math.abs(v - old[i]) > 0.2), {}, padStart);
  await page.evaluate(() => { window.testPads = []; }); await waitFrames();
  assert.equal((await status()).active, false, "Disconnect clears gamepad movement");
  await page.evaluate(() => {
    window.viewer.set_navigation_bindings(JSON.stringify({ Enter: "Activate", Escape: "Release", KeyI: "Forward", KeyK: "Backward" }));
  });
  await waitFrames(); await canvas.focus(); await page.keyboard.press("Enter"); await waitFrames();
  const rebound = (await status()).eye;
  await page.keyboard.down("i");
  await page.waitForFunction((old) => JSON.parse(window.viewer.navigation_status()).eye.some((v, i) => Math.abs(v - old[i]) > 0.05), {}, rebound);
  await page.keyboard.up("i"); await page.keyboard.press("Escape"); await waitFrames();
  assert.equal((await status()).active, false);
  await page.evaluate(() => {
    window.testPads = [{ id: "Unknown", index: 0, connected: true, mapping: "", axes: [], buttons: [] }];
  }); await waitFrames();
  assert.match((await status()).gamepadStatus, /Unsupported/);
  await page.evaluate(() => { window.testPads = []; });
  if (nif) {
    await page.evaluate(() => {
      const old = window.viewer.navigation_attach();
      const current = window.viewer.navigation_attach();
      window.viewer.navigation_detach(old);
      window.attachmentGeneration = current;
    });
    await waitFrames(); await canvas.focus(); await page.keyboard.press("Enter"); await waitFrames();
    assert.equal((await status()).active, true, "Stale detach must not detach a newer attachment");
    await page.evaluate(() => window.viewer.navigation_detach(window.attachmentGeneration)); await waitFrames();
    assert.equal((await status()).active, false);
    await page.evaluate(() => window.viewer.navigation_attach());
    if (format === "web") {
      // Exercise the same ES module that ships in the NuGet package.
      await page.evaluate(async () => {
        window.interop = await import("/interop.js");
        const canvas = document.querySelector("canvas"); canvas.id = "interop-canvas";
        const old = await window.interop.attach(canvas.id);
        const current = await window.interop.attach(canvas.id);
        window.interop.detachNavigation(old);
        window.currentInterop = current;
        window.interop.setNavigationSettings({ ...window.interop.navigationStatus().settings, speed: 7, automatic_speed: false });
      });
      await page.waitForFunction(() => JSON.parse(window.viewer.navigation_status()).settings.speed === 7);
      await page.evaluate(() => window.interop.detachNavigation(window.currentInterop)); await waitFrames();
      assert.equal((await status()).active, false);
    }
  }
  assert.deepEqual(errors, [], "Browser runtime errors");
  console.log(`${name} ${format}: rendered geometry, keyboard/mouse, focus, Tab, simulated gamepad, and lifecycle checks passed`);
} finally {
  await browser.close(); server.close();
}
