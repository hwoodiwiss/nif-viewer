// ES-module interop for NifViewer.Blazor. Wraps the wasm-pack (target=web)
// build of nif-viewer-lib and implements the streaming load-session loop.

import initWasm, * as wasm from "./nif_viewer_lib.js";

let initialized = false;

// The Rust code fetches "shaders/*.wgsl" and "resources/cube/*" with relative
// URLs (resolved against the host page). Those files ship as static web
// assets under _content/NifViewer.Blazor/, so patch window.fetch to redirect
// such requests there. Idempotent.
function installAssetRedirect() {
  if (window.__nifViewerFetchPatched) return;
  window.__nifViewerFetchPatched = true;
  const base = new URL(".", import.meta.url); // .../_content/NifViewer.Blazor/
  const originalFetch = window.fetch.bind(window);
  window.fetch = (input, init) => {
    try {
      const url = typeof input === "string" ? input : input.url;
      const m = /^(?:\.\/)?((?:shaders|resources)\/.+)$/.exec(url);
      if (m) {
        return originalFetch(new URL(m[1], base).toString(), init);
      }
    } catch {
      /* fall through to the original fetch */
    }
    return originalFetch(input, init);
  };
}

export async function init() {
  if (!initialized) {
    installAssetRedirect();
    await initWasm();
    initialized = true;
  }
}

export async function attach(canvasId) {
  await init();
  wasm.attach(canvasId);
  // GPU initialization is asynchronous; wait for navigation rather than guessing a delay.
  const deadline = performance.now() + 30000;
  while (!JSON.parse(wasm.navigation_status()).settings) {
    if (performance.now() > deadline) throw new Error("Viewer navigation initialization timed out");
    await new Promise((resolve) => setTimeout(resolve, 25));
  }
  const generation = wasm.navigation_attach();
  // Complete the queued lifecycle reset before exposing the attachment as ready.
  await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
  document.getElementById(canvasId)?.setAttribute("data-navigation-ready", String(generation));
  return generation;
}

// Blazor owns the empty container, while JS/winit own its canvas. Rehoming a
// canvas that Blazor itself rendered would invalidate Blazor's DOM bookkeeping.
export async function attachHost(canvasId) {
  const host = document.getElementById(`${canvasId}-host`);
  if (!host) throw new Error(`Viewer host '${canvasId}-host' is missing`);
  const canvas = document.createElement("canvas");
  canvas.id = canvasId;
  canvas.style.cssText = "width:100%;height:100%;display:block";
  host.replaceChildren(canvas);
  return attach(canvasId);
}

export function detachNavigation(generation) {
  wasm.navigation_detach(generation);
}

export function setNavigationSettings(settings) {
  wasm.set_navigation_settings(JSON.stringify(settings));
}

export function navigationCommand(action) {
  wasm.navigation_command(action);
}

export function navigationStatus() {
  return JSON.parse(wasm.navigation_status());
}

export function setNavigationBindings(bindings) {
  wasm.set_navigation_bindings(JSON.stringify(bindings));
}

export async function setCameraSpeed(speed) {
  await init();
  wasm.set_camera_speed(speed);
}

export async function setLight(r, g, b, x, y, z, intensity, ambient, autoOrbit) {
  await init();
  wasm.set_light(r, g, b, x, y, z, intensity, ambient, autoOrbit);
}

// Parse the raw NIF block structure (header, blocks, refs) without rendering.
// Returns a plain JS object.
export async function parseNifStructure(bytes) {
  await init();
  return wasm.parse_nif_structure(bytes);
}

const CHUNK_SIZE = 64 * 1024;

// Stream a Uint8Array into the wasm session in 64 KiB chunks.
function streamBytes(path, bytes) {
  wasm.load_session_file_begin(path, bytes.length);
  for (let offset = 0; offset < bytes.length; offset += CHUNK_SIZE) {
    wasm.load_session_file_chunk(path, bytes.subarray(offset, offset + CHUNK_SIZE));
  }
  wasm.load_session_file_end(path);
}

// Ask the .NET resolver for a dependency's bytes; null/undefined = not found.
// .NET byte[] results arrive as base64 strings over the JSON interop channel.
async function resolve(resolverRef, path) {
  if (!resolverRef) return null;
  const result = await resolverRef.invokeMethodAsync("ResolveAsync", path);
  if (result == null) return null;
  if (typeof result === "string") {
    const binary = atob(result);
    const bytes = new Uint8Array(binary.length);
    for (let i = 0; i < binary.length; i++) {
      bytes[i] = binary.charCodeAt(i);
    }
    return bytes;
  }
  return new Uint8Array(result);
}

// Full session loop, mirroring nif-viewer-webapp/index.js:
// begin -> pending rounds -> texture hints -> finish. Returns { missing }.
export async function loadNif(name, nifBytes, resolverRef) {
  await init();
  wasm.load_session_begin(name, nifBytes);

  const missing = new Set();
  for (let round = 0; round < 4; round++) {
    const pending = wasm.load_session_pending().filter((p) => !missing.has(p));
    if (pending.length === 0) break;

    let progressed = false;
    for (const path of pending) {
      console.debug(`nif-viewer: resolving ${path}`);
      const bytes = await resolve(resolverRef, path);
      if (!bytes) {
        missing.add(path);
        continue;
      }
      streamBytes(path, bytes);
      progressed = true;
    }
    if (!progressed) break;
  }

  // Heuristic texture hints for materials whose .mat never appeared: try to
  // resolve each speculative texture path via the same resolver.
  const hints = wasm.load_session_texture_hints();
  for (const hint of hints) {
    for (const kind of ["color", "normal", "rough", "metal", "ao"]) {
      const path = hint[kind];
      if (!path) continue;
      console.debug(`nif-viewer: resolving texture hint ${path}`);
      const bytes = await resolve(resolverRef, path);
      if (bytes) streamBytes(path, bytes);
    }
  }

  const unresolved = wasm.load_session_finish();
  return { missing: unresolved };
}
