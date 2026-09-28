# Browser tests

These are Playwright Test integration tests of the real WASM renderer. Each named
test has a fresh browser context; no test relies on a previous test's camera,
bindings, controller selection, or attachment state. A single worker avoids
competing continuous GPU loops. The Rust crate retains its pure navigation tests.

## Setup and run

Build both WASM formats from `nif-viewer-lib`:

```powershell
wasm-pack build --release --target web --out-dir pkg-web
wasm-pack build --release --target bundler --out-dir pkg
```

Then, from `nif-viewer-webapp`:

```powershell
npm ci
npx playwright install chromium
npm run build
npm run test:navigation
```

The `web` project uses `fixtures/web.html`; `bundler` tests the built webapp.
Playwright starts/stops the local asset server. Neither checkout names nor sibling
repositories are used to choose behavior. To run a subset or debug:

```powershell
npx playwright test --project=web gamepad.spec.mjs
npx playwright test --project=bundler --headed
npx playwright test --ui
npx playwright show-report
```

CI and normal local runs use Chromium matched to the pinned Playwright version.
`BROWSER_EXECUTABLE` can override it for an explicit local Edge/Chrome check.
WebGPU is required: an unavailable adapter fails with diagnostics rather than
silently skipping rendering. Software fallback is allowed by Chromium launch flags.

## Layout

- `navigation.spec.mjs`: keyboard, mouse drag, focus/Tab, rebinding.
- `gamepad.spec.mjs`: standard mapping, neutral activation, disconnect, unsupported mapping.
- `rendering.spec.mjs`: generated NIF landmarks and displayed-image changes.
- `attachment.spec.mjs`: stale disposal protection, in both WASM formats.
- `interop.web.spec.mjs`: browser-native package interop using the same WASM module instance.
- `blazor.spec.mjs`: separately configured, packaged .NET host lifecycle/settings tests.
- `support/viewer.mjs`: viewer operations, state assertions and automatic diagnostics.
- `support/gamepad.mjs`: fresh Gamepad-shaped snapshots; waits for actual polling.
- `support/server.mjs`: read-only, explicitly routed test assets.
- `fixtures/navigation-nif.mjs`: generated geometry, without proprietary assets.

Assertions wait for observable state, not arbitrary delays or two animation frames.
Rendering uses compositor screenshots rather than `drawImage` on a WebGPU drawing
buffer that may already have been cleared. The smoke check requires contrasting
interior geometry and at least 1% changed pixels after turning; it is not a
pixel-perfect cross-driver golden image test. Controller simulation does not prove
physical Xbox driver/capture behavior.

Failures retain traces, screenshots, console/request logs and navigation state.
CI uploads `playwright-report/` and `test-results/` (both gitignored).

## Blazor package

Pack and build `nif-viewer-blazor/NavigationSmoke` following its README, then run
`npm run test:blazor`. `playwright.blazor.config.mjs` manages the .NET server;
the default navigation command does not require a .NET SDK or built package.

The separate opt-in `scripts/webgpu-test.mjs` asset-inspection utility now uses
Playwright's browser API as well, retaining its existing command-line interface.
It is not part of the generated-fixture CI suite and requires local game assets.
