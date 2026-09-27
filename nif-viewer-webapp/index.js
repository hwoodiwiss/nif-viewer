import * as wasm from "../nif-viewer-lib/pkg/nif_viewer_lib";

// Expose the module for external hosts (Blazor interop, debugging, tests).
window.nifViewer = wasm;

wasm.attach("nif-canvas");

const status = document.getElementById("status");
const setStatus = (text) => {
  status.textContent = text;
};

// --- Camera / light controls -------------------------------------------

const camSpeedInput = document.getElementById("cam-speed");
camSpeedInput.addEventListener("change", () => {
  const speed = parseFloat(camSpeedInput.value);
  if (Number.isFinite(speed) && speed > 0) configureNavigation({ speed, automatic_speed: false });
});

function configureNavigation(changes) {
  const status = JSON.parse(wasm.navigation_status());
  if (status.settings) wasm.set_navigation_settings(JSON.stringify({ ...status.settings, ...changes }));
}
document.getElementById("nav-mode").addEventListener("change", (event) => configureNavigation({ mode: event.target.value }));
document.getElementById("nav-frame").addEventListener("click", () => wasm.navigation_command("Frame"));
document.getElementById("nav-reset").addEventListener("click", () => wasm.navigation_command("Reset"));
setInterval(() => {
  const state = JSON.parse(wasm.navigation_status());
  if (!state.settings) return;
  document.getElementById("nav-status").textContent = `${state.settings.mode} · ${state.settings.speed.toPrecision(4)} units/s · ${state.device} · ${state.active ? "Active" : "Focus canvas + Enter, click, or press controller A"}${state.captured ? " · Mouse captured (Escape releases)" : ""}`;
  document.getElementById("nav-help").textContent = state.help;
  if (document.activeElement !== camSpeedInput) camSpeedInput.value = state.settings.speed;
  const mode = document.getElementById("nav-mode");
  if (document.activeElement !== mode) mode.value = state.settings.mode;
}, 250);

const pushLight = () => {
  const hex = document.getElementById("light-color").value;
  const r = parseInt(hex.slice(1, 3), 16) / 255;
  const g = parseInt(hex.slice(3, 5), 16) / 255;
  const b = parseInt(hex.slice(5, 7), 16) / 255;
  const az = (parseFloat(document.getElementById("light-azimuth").value) * Math.PI) / 180;
  const el = (parseFloat(document.getElementById("light-elevation").value) * Math.PI) / 180;
  // Direction the light TRAVELS: elevation>0 means light shines downwards.
  const x = Math.cos(el) * Math.cos(az);
  const z = Math.cos(el) * Math.sin(az);
  const y = -Math.sin(el);
  const intensity = parseFloat(document.getElementById("light-intensity").value);
  const orbit = document.getElementById("light-orbit").checked;
  wasm.set_light(r, g, b, x, y, z, intensity, 0.25, orbit);
};
for (const id of ["light-color", "light-azimuth", "light-elevation", "light-intensity"]) {
  document.getElementById(id).addEventListener("input", () => {
    // User set an explicit direction/colour: turn auto-orbit off.
    if (id === "light-azimuth" || id === "light-elevation") {
      document.getElementById("light-orbit").checked = false;
    }
    pushLight();
  });
}
document.getElementById("light-orbit").addEventListener("change", pushLight);

// Normalize a path to lowercase forward slashes.
const normalize = (p) => p.toLowerCase().replaceAll("\\", "/");

// Directory picks: strip the top-level folder from webkitRelativePath.
const dirKey = (file) => {
  const rel = file.webkitRelativePath || file.name;
  const idx = rel.indexOf("/");
  return normalize(idx >= 0 ? rel.slice(idx + 1) : rel);
};

// Data root granted via the File System Access API (Chromium). Lets us
// resolve dependency paths on demand without enumerating the whole folder.
let dataRootHandle = null;

const dataRootBtn = document.getElementById("data-root-btn");
if ("showDirectoryPicker" in window) {
  dataRootBtn.addEventListener("click", async () => {
    try {
      dataRootHandle = await window.showDirectoryPicker({ mode: "read" });
      dataRootBtn.textContent = `Data root: ${dataRootHandle.name}`;
      setStatus("Data root granted; dependencies will be found automatically.");
    } catch (e) {
      if (e.name !== "AbortError") throw e;
    }
  });
} else {
  dataRootBtn.style.display = "none";
}

// Look up a pending path (e.g. "geometries/ab/hash.mesh") directly inside the
// granted data root, walking directory handles case-insensitively.
const findInDataRoot = async (path) => {
  if (!dataRootHandle) return undefined;
  const segments = path.split("/").filter(Boolean);
  let dir = dataRootHandle;
  try {
    for (let i = 0; i < segments.length - 1; i++) {
      dir = await getEntryIgnoreCase(dir, segments[i], "directory");
      if (!dir) return undefined;
    }
    const fileHandle = await getEntryIgnoreCase(
      dir,
      segments[segments.length - 1],
      "file"
    );
    return fileHandle ? await fileHandle.getFile() : undefined;
  } catch {
    return undefined;
  }
};

// Exact-name handle lookup first (fast), then a case-insensitive scan.
const getEntryIgnoreCase = async (dir, name, kind) => {
  try {
    return kind === "directory"
      ? await dir.getDirectoryHandle(name)
      : await dir.getFileHandle(name);
  } catch {
    const lower = name.toLowerCase();
    for await (const [entryName, handle] of dir.entries()) {
      if (entryName.toLowerCase() === lower && handle.kind === kind) {
        return handle;
      }
    }
    return undefined;
  }
};

// Find a File for a pending path: exact key, then suffix match, then filename.
const findFile = (fileMap, path) => {
  const exact = fileMap.get(path);
  if (exact) return exact;
  for (const [key, file] of fileMap) {
    if (key.endsWith(path)) return file;
  }
  const filename = path.split("/").pop();
  const byName = fileMap.get(filename);
  if (byName) return byName;
  for (const [key, file] of fileMap) {
    if (key.split("/").pop() === filename) return file;
  }
  return undefined;
};

// Stream a File's bytes into the wasm session in chunks — no whole-file
// copy is materialized in JS.
const streamFile = async (path, file) => {
  wasm.load_session_file_begin(path, file.size);
  const reader = file.stream().getReader();
  try {
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      wasm.load_session_file_chunk(path, value);
    }
  } finally {
    reader.releaseLock();
  }
  wasm.load_session_file_end(path);
};

// Cached stem index of the data root's textures/ subtree, built lazily on
// first heuristic-texture miss. Maps "<stem>" → { color: File?, normal: File? }.
let textureStemIndex = null;
let textureStemIndexRoot = null;

const buildTextureStemIndex = async () => {
  if (textureStemIndex && textureStemIndexRoot === dataRootHandle) {
    return textureStemIndex;
  }
  textureStemIndex = new Map();
  textureStemIndexRoot = dataRootHandle;
  const texDir = await getEntryIgnoreCase(dataRootHandle, "textures", "directory");
  if (!texDir) return textureStemIndex;
  let seen = 0;
  const stack = [texDir];
  while (stack.length > 0) {
    const dir = stack.pop();
    for await (const [name, handle] of dir.entries()) {
      if (handle.kind === "directory") {
        stack.push(handle);
        continue;
      }
      if (++seen > 30000) {
        console.warn("texture stem index capped at 30000 files; matches may be incomplete");
        return textureStemIndex;
      }
      const lower = name.toLowerCase();
      for (const kind of ["color", "normal", "rough", "metal", "ao"]) {
        const suffix = `_${kind}.dds`;
        if (lower.endsWith(suffix)) {
          const stem = lower.slice(0, -suffix.length);
          const entry = textureStemIndex.get(stem) ?? {};
          entry[kind] = handle;
          textureStemIndex.set(stem, entry);
        }
      }
      // Yield to the event loop occasionally to stay responsive.
      if (seen % 2000 === 0) await new Promise((r) => setTimeout(r, 0));
    }
  }
  return textureStemIndex;
};

// Score a candidate texture stem against a material stem (higher = better;
// -1 = no match): exact, then prefix either direction (longest common prefix),
// then "_"-boundary suffix (dumps often drop a model prefix, e.g.
// "ar99_receiver" → "receiver").
const stemMatchScore = (hintStem, candidate) => {
  const TIER = 2 ** 32;
  if (candidate.length === 0) return -1;
  if (candidate === hintStem) return 3 * TIER;
  // Prefix either direction; require a meaningful shared prefix so tiny
  // candidates never win.
  if (
    (candidate.startsWith(hintStem) || hintStem.startsWith(candidate)) &&
    Math.min(hintStem.length, candidate.length) >= 4
  ) {
    return 2 * TIER + Math.min(hintStem.length, candidate.length);
  }
  if (hintStem.endsWith(`_${candidate}`)) return TIER + candidate.length;
  return -1;
};

// Heuristic texture discovery for required .mat files that were never found
// (vanilla Starfield keeps materials in a binary cdb). Streams discovered
// textures under the hint paths so the Rust-side fallback picks them up.
// Returns { matched, total }.
const resolveTextureHints = async () => {
  const hints = wasm.load_session_texture_hints();
  if (hints.length === 0 || !dataRootHandle) {
    return { matched: 0, total: hints.length };
  }
  let matched = 0;
  const kinds = ["color", "normal", "rough", "metal", "ao"];
  for (const hint of hints) {
    // 1) Exact mirrored paths inside the data root.
    const found = {};
    for (const kind of kinds) {
      found[kind] = await findInDataRoot(hint[kind]);
    }
    // 2) Fuzzy stem match against the textures/ index.
    if (!found.color) {
      const index = await buildTextureStemIndex();
      let best = null;
      let bestScore = -1;
      for (const [stem, entry] of index) {
        const score = stemMatchScore(hint.stem, stem);
        if (score > bestScore) {
          bestScore = score;
          best = entry;
        }
      }
      if (best) {
        for (const kind of kinds) {
          if (!found[kind] && best[kind]) {
            found[kind] = await best[kind].getFile();
          }
        }
      }
    }
    let any = false;
    for (const kind of kinds) {
      if (found[kind]) {
        await streamFile(hint[kind], found[kind]);
        any = true;
      }
    }
    if (any) matched++;
  }
  return { matched, total: hints.length };
};

// --- NIF structure viewer -------------------------------------------------

const MAX_FIELD_DEPTH = 6;

const el = (tag, className, text) => {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
};

// Render a fields value (object/array/scalar) as nested tables.
const renderValue = (value, depth) => {
  if (value === null || typeof value !== "object") {
    return el("span", null, String(value));
  }
  if (depth >= MAX_FIELD_DEPTH) {
    return el("span", null, "…");
  }
  if (Array.isArray(value)) {
    // Inline short scalar arrays (vectors, matrices).
    if (value.length <= 16 && value.every((v) => typeof v !== "object" || v === null)) {
      return el("span", null, `[${value.map((v) => formatScalar(v)).join(", ")}]`);
    }
    const table = el("table", "nif-fields");
    value.forEach((item, i) => {
      const row = el("tr");
      row.append(el("td", null, String(i)));
      const cell = el("td");
      cell.append(renderValue(item, depth + 1));
      row.append(cell);
      table.append(row);
    });
    return table;
  }
  const table = el("table", "nif-fields");
  for (const [key, item] of Object.entries(value)) {
    const row = el("tr");
    row.append(el("td", null, key));
    const cell = el("td");
    cell.append(renderValue(item, depth + 1));
    row.append(cell);
    table.append(row);
  }
  return table;
};

const formatScalar = (v) =>
  typeof v === "number" && !Number.isInteger(v) ? v.toPrecision(6) : String(v);

// Render one block node (recursively via `children`), guarding cycles with a
// per-branch visited set.
const renderBlockNode = (structure, index, visited) => {
  const block = structure.blocks[index];
  if (!block) return el("div", "nif-leaf", `[${index}] (invalid ref)`);
  if (visited.has(index)) return el("div", "nif-leaf", `[${index}] (cycle)`);

  const label = document.createElement("span");
  label.append(el("span", "nif-idx", `[${block.index}] `));
  label.append(el("span", "nif-type", block.type));
  if (block.name) label.append(el("span", "nif-name", ` — ${block.name}`));

  const children = (block.children ?? []).filter((c) => c >= 0);
  const hasFields =
    block.fields && !(typeof block.fields === "object" && block.fields.skipped);

  const details = el("details", "nif-block");
  const summary = el("summary");
  summary.append(label);
  details.append(summary);

  if (hasFields) details.append(renderValue(block.fields, 0));
  else details.append(el("div", "nif-leaf", block.fields?.skipped ? `(skipped, ${block.size} bytes)` : "(no fields)"));

  const branchVisited = new Set(visited);
  branchVisited.add(index);
  for (const child of children) {
    details.append(renderBlockNode(structure, child, branchVisited));
  }
  return details;
};

// Blocks reachable from the roots via children refs.
const reachableBlocks = (structure) => {
  const seen = new Set();
  const stack = [...(structure.roots ?? [])];
  while (stack.length > 0) {
    const i = stack.pop();
    if (i < 0 || seen.has(i) || !structure.blocks[i]) continue;
    seen.add(i);
    stack.push(...(structure.blocks[i].children ?? []));
  }
  return seen;
};

const renderStructure = (name, nifBuf) => {
  const panel = document.getElementById("structure-panel");
  panel.replaceChildren();
  let structure;
  try {
    structure = wasm.parse_nif_structure(nifBuf);
  } catch (e) {
    panel.append(el("div", "nif-leaf", `structure parse failed: ${e.message ?? e}`));
    return;
  }

  const outer = el("details");
  outer.open = true;
  outer.append(el("summary", null, `Structure: ${name}`));
  const header = el(
    "div",
    null,
    `${structure.headerString ?? ""} | BS version ${structure.bsVersion} | ${structure.numBlocks} blocks | ${structure.strings?.length ?? 0} strings`
  );
  header.id = "structure-header";
  outer.append(header);

  for (const root of structure.roots ?? []) {
    outer.append(renderBlockNode(structure, root, new Set()));
  }

  const reachable = reachableBlocks(structure);
  const orphans = structure.blocks.filter((b) => !reachable.has(b.index));
  if (orphans.length > 0) {
    const unref = el("details", "nif-block");
    unref.append(el("summary", null, `Unreferenced (${orphans.length})`));
    for (const b of orphans) {
      unref.append(renderBlockNode(structure, b.index, new Set()));
    }
    outer.append(unref);
  }

  panel.append(outer);
};

// Exposed for external hosts / headless tests.
window.nifViewerRenderStructure = renderStructure;

document.getElementById("load-btn").addEventListener("click", async () => {
  const nifInput = document.getElementById("nif-file");
  const nifFile = nifInput.files[0];
  if (!nifFile) {
    setStatus("Select a .nif file first.");
    return;
  }

  try {
    setStatus("Parsing…");

    // Index the selected files WITHOUT reading their contents.
    const fileMap = new Map();
    for (const file of document.getElementById("dep-dir").files) {
      fileMap.set(dirKey(file), file);
    }
    for (const file of document.getElementById("dep-files").files) {
      fileMap.set(normalize(file.name), file);
    }

    // Only the (small) .nif is read fully; deps stream on demand.
    const nifBuf = new Uint8Array(await nifFile.arrayBuffer());
    renderStructure(nifFile.name, nifBuf);
    wasm.load_session_begin(nifFile.name, nifBuf);

    const missing = new Set();
    let streamed = 0;
    for (let round = 0; round < 4; round++) {
      const pending = wasm
        .load_session_pending()
        .filter((p) => !missing.has(p));
      if (pending.length === 0) break;

      let progressed = false;
      for (let i = 0; i < pending.length; i++) {
        const path = pending[i];
        const file =
          findFile(fileMap, path) ?? (await findInDataRoot(path));
        if (!file) {
          missing.add(path);
          continue;
        }
        setStatus(`streaming ${path} (${i + 1}/${pending.length})`);
        await streamFile(path, file);
        streamed++;
        progressed = true;
      }
      if (!progressed) break;
    }

    // Heuristic texture discovery for materials whose .mat never appeared.
    setStatus("Looking for textures heuristically…");
    const texResult = await resolveTextureHints();

    const unresolved = wasm.load_session_finish();
    const missingNote =
      unresolved.length > 0
        ? ` (${unresolved.length} dependencies missing)`
        : "";
    const texNote =
      texResult.total > 0
        ? `; textures: ${texResult.matched}/${texResult.total} materials matched heuristically`
        : "";
    setStatus(
      `Loaded ${nifFile.name}, ${streamed} dependency file(s) streamed${missingNote}${texNote}.`
    );
    if (unresolved.length > 0) {
      console.warn("missing dependencies:", unresolved);
    }
  } catch (e) {
    console.error(e);
    setStatus(`Error: ${e.message ?? e}`);
  }
});
