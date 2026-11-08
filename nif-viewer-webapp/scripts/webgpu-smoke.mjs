// Portable WebGPU smoke test using a generated NIF (no game assets).
// Serves dist/ under a nested URL, waits for viewer readiness, loads a
// red-textured quad through load_nif, waits for the "rendered" milestone and
// samples the presented canvas.
// Usage: node scripts/webgpu-smoke.mjs <browser.exe>
import http from "node:http";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import puppeteer from "puppeteer-core";

const [browserExe] = process.argv.slice(2);
const dist = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "dist");
const prefix = "/nested/app/";

const mime = (p) =>
  p.endsWith(".html") ? "text/html"
  : p.endsWith(".js") ? "text/javascript"
  : p.endsWith(".wasm") ? "application/wasm"
  : p.endsWith(".json") ? "application/json"
  : "text/plain";

const server = http.createServer((req, res) => {
  const url = decodeURIComponent(req.url.split("?")[0]);
  if (!url.startsWith(prefix)) return res.writeHead(404).end();
  const file = path.join(dist, url.slice(prefix.length) || "index.html");
  fs.readFile(file, (err, data) =>
    err ? res.writeHead(404).end() : res.writeHead(200, { "content-type": mime(file) }).end(data));
});
await new Promise((r) => server.listen(0, r));
const base = `http://127.0.0.1:${server.address().port}${prefix}`;

const browser = await puppeteer.launch({
  executablePath: browserExe,
  headless: "new",
  args: ["--enable-unsafe-webgpu", "--enable-features=Vulkan", "--use-angle=d3d11"],
});
let failed = false;
try {
  const page = await browser.newPage();
  await page.setViewport({ width: 800, height: 600 });
  page.on("console", (m) => console.log(`[page] ${m.text()}`));
  await page.goto(base, { waitUntil: "load" });

  const status = await page.evaluate(async () => {
    const wait = async (pred, ms) => {
      const end = performance.now() + ms;
      while (performance.now() < end) {
        const s = window.nifViewer?.viewer_status();
        if (s && pred(s)) return s;
        await new Promise((r) => setTimeout(r, 50));
      }
      return window.nifViewer?.viewer_status();
    };
    const ready = await wait((s) => s.state !== "initializing", 20000);
    if (ready?.state !== "ready") return ready;

    // Generated FO3/NV NiTriStrips quad (mirrors nif-viewer-lib/tests/regression.rs).
    const out = [];
    const u8 = (v) => out.push(v & 255);
    const u16 = (v) => { u8(v); u8(v >> 8); };
    const u32 = (v) => { u16(v & 0xffff); u16(v >>> 16); };
    const f32 = (v) => { const b = new Uint8Array(new Float32Array([v]).buffer); out.push(...b); };
    const str = (s) => { u32(s.length); for (const c of s) u8(c.charCodeAt(0)); };
    const block = (fn) => { const start = out.length; fn(); return out.splice(start); };
    const shape = block(() => {
      u32(0); u32(0); u32(0xffffffff); u32(14);
      [10, 20, 30].forEach(f32); [1, 0, 0, 0, 1, 0, 0, 0, 1].forEach(f32); f32(2);
      u32(1); u32(2); u32(0xffffffff); u32(1); u32(0xffffffff); u32(0); u32(0xffffffff); u8(0);
    });
    const data = block(() => {
      u32(0); u16(4); u8(0); u8(0); u8(1);
      [[0, 0, 0], [1, 0, 0], [0, 1, 0], [1, 1, 0]].flat().forEach(f32);
      u16(1); u8(1); for (let i = 0; i < 4; i++) [0, 0, 1].forEach(f32);
      for (let i = 0; i < 16; i++) u8(0); u8(1); for (let i = 0; i < 16; i++) f32(1);
      [[0, 0], [1, 0], [0, 1], [1, 1]].flat().forEach(f32);
      u16(0); u32(0xffffffff); u16(2); u16(1); u16(4); u8(1); [0, 1, 2, 3].forEach(u16);
    });
    const shader = block(() => { u32(0xffffffff); u32(0); u32(0xffffffff); for (let i = 0; i < 22; i++) u8(0); u32(3); });
    const textures = block(() => { u32(2); str("textures\\test.dds"); str("textures\\test_n.dds"); });
    const blocks = [shape, data, shader, textures];
    for (const c of "Gamebryo File Format, Version 20.2.0.7\n") u8(c.charCodeAt(0));
    u32(0x14020007); u8(1); u32(11); u32(4); u32(34); [1, 0, 1, 0, 1, 0].forEach(u8);
    u16(4); ["NiTriStrips", "NiTriStripsData", "BSShaderPPLightingProperty", "BSShaderTextureSet"].forEach(str);
    [0, 1, 2, 3].forEach(u16); blocks.forEach((b) => u32(b.length));
    u32(1); u32(4); str("quad"); u32(0); blocks.forEach((b) => out.push(...b)); u32(1); u32(0);
    const nif = new Uint8Array(out);

    const dds = block(() => {
      "DDS ".split("").forEach((c) => u8(c.charCodeAt(0)));
      u32(124); u32(0x100f); u32(4); u32(4); u32(16); u32(0); u32(1);
      for (let i = 0; i < 11; i++) u32(0);
      u32(32); u32(0x41); u32(0); u32(32); [0xff, 0xff00, 0xff0000, 0xff000000].forEach(u32);
      u32(0x1000); for (let i = 0; i < 4; i++) u32(0);
      for (let i = 0; i < 16; i++) [200, 40, 40, 255].forEach(u8);
    });
    window.nifViewer.set_light(1, 1, 1, -0.4, -0.8, -0.45, 8, 0.25, false);
    window.nifViewer.load_nif("quad.nif", nif, { "textures/test.dds": new Uint8Array(dds) });
    return wait((s) => ["rendered", "failed"].includes(s.load.state), 10000);
  });
  console.log("status:", JSON.stringify(status));
  if (status?.load?.state !== "rendered") throw new Error("model did not reach the rendered milestone");

  await new Promise((r) => setTimeout(r, 500));
  const canvas = await page.$("#nif-canvas");
  const png = await canvas.screenshot({ encoding: "base64" });
  const sample = await page.evaluate(async (png) => {
    const img = new Image();
    img.src = `data:image/png;base64,${png}`;
    await img.decode();
    const c = new OffscreenCanvas(img.width, img.height);
    const ctx = c.getContext("2d");
    ctx.drawImage(img, 0, 0);
    const px = (x, y) => Array.from(ctx.getImageData(x, y, 1, 1).data);
    return { size: [img.width, img.height], centre: px(img.width >> 1, img.height >> 1), corner: px(2, 2) };
  }, png);
  console.log("canvas:", JSON.stringify(sample));
  const [r, g, b] = sample.centre;
  if (!(r > g + 40 && r > b + 40)) throw new Error("textured quad not visible at canvas centre");
  if (sample.corner.slice(0, 3).some((v) => v > 10)) throw new Error("background is not clear");
  console.log("PASS: WebGPU canvas shows the generated textured quad");
} catch (e) {
  failed = true;
  console.error(`FAIL: ${e.message}`);
} finally {
  await browser.close();
  server.close();
}
process.exit(failed ? 1 : 0);
