// Headless WebGPU smoke test: loads the webapp in Edge/Chrome, drives the
// session API against a locally served Starfield data root, and screenshots
// the canvas so rendering regressions are visible.
// Usage: node scripts/webgpu-test.mjs <browser.exe> <dataRoot> <nifRelPath>
import http from "node:http";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import puppeteer from "puppeteer-core";

const [browserExe, dataRoot, nifRel] = process.argv.slice(2);
const dist = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "dist");

const mime = (p) =>
  p.endsWith(".html") ? "text/html"
  : p.endsWith(".js") ? "text/javascript"
  : p.endsWith(".wasm") ? "application/wasm"
  : "application/octet-stream";

const server = http.createServer((req, res) => {
  const url = decodeURIComponent(req.url.split("?")[0]);
  if (url === "/texture-index") {
    // Stem index of <dataRoot>/textures *_color.dds / *_normal.dds, mirroring
    // what index.js builds from directory handles.
    const index = {};
    let seen = 0;
    const stack = [path.join(dataRoot, "textures")];
    while (stack.length > 0) {
      const dir = stack.pop();
      let entries = [];
      try {
        entries = fs.readdirSync(dir, { withFileTypes: true });
      } catch {}
      for (const e of entries) {
        const full = path.join(dir, e.name);
        if (e.isDirectory()) {
          stack.push(full);
          continue;
        }
        if (++seen > 30000) break;
        const lower = e.name.toLowerCase();
        for (const kind of ["color", "normal", "rough", "metal", "ao"]) {
          const suffix = `_${kind}.dds`;
          if (lower.endsWith(suffix)) {
            const stem = lower.slice(0, -suffix.length);
            (index[stem] ??= {})[kind] = path
              .relative(dataRoot, full)
              .replaceAll("\\", "/")
              .toLowerCase();
          }
        }
      }
    }
    res
      .writeHead(200, { "content-type": "application/json" })
      .end(JSON.stringify(index));
    return;
  }
  const file = url.startsWith("/data/")
    ? path.join(dataRoot, url.slice(6))
    : path.join(dist, url === "/" ? "index.html" : url);
  fs.readFile(file, (err, buf) => {
    if (err) {
      res.writeHead(404).end();
    } else {
      res.writeHead(200, { "content-type": mime(file) }).end(buf);
    }
  });
});
await new Promise((r) => server.listen(8123, r));

const browser = await puppeteer.launch({
  executablePath: browserExe,
  headless: "new",
  args: [
    "--enable-unsafe-webgpu",
    "--enable-features=WebGPU",
    "--no-sandbox",
    "--window-size=1400,900",
  ],
});
const page = await browser.newPage();
await page.setViewport({ width: 1400, height: 900 });
page.on("console", (m) => console.log(`[console:${m.type()}]`, m.text()));
page.on("pageerror", (e) => console.log("[pageerror]", e.message));

await page.goto("http://localhost:8123/", { waitUntil: "networkidle0" });
await new Promise((r) => setTimeout(r, 3000)); // let wgpu init + first frames
await page.screenshot({ path: "test-before-load.png" });

const result = await page.evaluate(async (nifRel) => {
  const wasm = window.nifViewer;
  const nifBuf = new Uint8Array(
    await (await fetch(`/data/${nifRel}`)).arrayBuffer()
  );
  let required = wasm.load_session_begin(nifRel.split("/").pop(), nifBuf);
  const fetched = [];
  const missing = [];
  for (let round = 0; round < 4; round++) {
    const pending = wasm
      .load_session_pending()
      .filter((p) => !missing.includes(p));
    if (pending.length === 0) break;
    for (const p of pending) {
      const resp = await fetch(`/data/${p}`);
      if (!resp.ok) {
        missing.push(p);
        continue;
      }
      const buf = new Uint8Array(await resp.arrayBuffer());
      wasm.load_session_file_begin(p, buf.length);
      for (let i = 0; i < buf.length; i += 65536) {
        wasm.load_session_file_chunk(p, buf.subarray(i, i + 65536));
      }
      wasm.load_session_file_end(p);
      fetched.push(p);
    }
  }
  // Heuristic texture-hint flow, mirroring index.js: exact hint paths, then
  // fuzzy stem match against the textures index, streamed under hint paths.
  const streamBuf = (p, buf) => {
    wasm.load_session_file_begin(p, buf.length);
    for (let i = 0; i < buf.length; i += 65536) {
      wasm.load_session_file_chunk(p, buf.subarray(i, i + 65536));
    }
    wasm.load_session_file_end(p);
  };
  const stemMatchScore = (hintStem, candidate) => {
    const TIER = 2 ** 32;
    if (candidate.length === 0) return -1;
    if (candidate === hintStem) return 3 * TIER;
    if (
      (candidate.startsWith(hintStem) || hintStem.startsWith(candidate)) &&
      Math.min(hintStem.length, candidate.length) >= 4
    ) {
      return 2 * TIER + Math.min(hintStem.length, candidate.length);
    }
    if (hintStem.endsWith(`_${candidate}`)) return TIER + candidate.length;
    return -1;
  };
  const hints = wasm.load_session_texture_hints();
  let texIndex = null;
  const kinds = ["color", "normal", "rough", "metal", "ao"];
  const textureHints = { total: hints.length, matched: 0, detail: [] };
  for (const hint of hints) {
    const fetchBuf = async (p) => {
      const resp = await fetch(`/data/${p}`);
      return resp.ok ? new Uint8Array(await resp.arrayBuffer()) : undefined;
    };
    const found = {};
    for (const kind of kinds) {
      found[kind] = await fetchBuf(hint[kind]);
    }
    let via = "exact";
    if (!found.color) {
      texIndex ??= await (await fetch("/texture-index")).json();
      let best = null;
      let bestScore = -1;
      for (const [stem, entry] of Object.entries(texIndex)) {
        const score = stemMatchScore(hint.stem, stem);
        if (score > bestScore) {
          bestScore = score;
          best = entry;
        }
      }
      if (best) {
        via = "fuzzy";
        for (const kind of kinds) {
          if (!found[kind] && best[kind]) found[kind] = await fetchBuf(best[kind]);
        }
      }
    }
    let any = false;
    for (const kind of kinds) {
      if (found[kind]) {
        streamBuf(hint[kind], found[kind]);
        any = true;
      }
    }
    if (any) {
      textureHints.matched++;
      textureHints.detail.push(`${hint.mat} via ${via}`);
    }
  }
  console.log(
    `textures: ${textureHints.matched}/${textureHints.total} materials matched heuristically`
  );

  const unresolved = wasm.load_session_finish();

  // Exercise the settings exports: red-ish light + explicit camera speed.
  let settingsError = null;
  try {
    wasm.set_light(1.0, 0.9, 0.8, -0.5, -1.0, -0.3, 3.0, 0.08, true);
    wasm.set_camera_speed(0.5);
  } catch (e) {
    settingsError = String(e);
  }
  return { required, fetched, missing, unresolved, textureHints, settingsError };
}, nifRel);
console.log("load result:", JSON.stringify(result, null, 1));

await new Promise((r) => setTimeout(r, 4000)); // let the model upload + render
await page.screenshot({ path: "test-after-load.png" });

// Exercise the structure viewer: parse + render into the panel, then verify
// the DOM contains block nodes.
const structureInfo = await page.evaluate(async (nifRel) => {
  const nifBuf = new Uint8Array(
    await (await fetch(`/data/${nifRel}`)).arrayBuffer()
  );
  const s = window.nifViewer.parse_nif_structure(nifBuf);
  window.nifViewerRenderStructure?.(nifRel.split("/").pop(), nifBuf);
  const panelBlocks = document.querySelectorAll("#structure-panel details.nif-block").length;
  return { numBlocks: s.numBlocks, blockTypes: [...new Set(s.blocks.map((b) => b.type))], panelBlocks };
}, nifRel);
console.log("structure:", JSON.stringify(structureInfo, null, 1));
await page.screenshot({ path: "test-structure.png", fullPage: true });

await browser.close();
server.close();
console.log("screenshots: test-before-load.png / test-after-load.png / test-structure.png");
