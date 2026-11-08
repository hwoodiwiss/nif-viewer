# Repository guidance

## Layout

This repository provides a Rust NIF parser and wgpu renderer, a native application,
a JavaScript web host, and a Blazor NuGet wrapper. See `README.md` for usage and
`docs/nif-format-spec.md` for format notes. Read scoped `AGENTS.md` files before
changing the library, its tests, or the Blazor package.

- `scene-types`: renderer-neutral CPU scene descriptors, validation, mesh preparation.
- `scene-renderer`: wgpu renderer, GPU resources/handles, material-type registration and
  batching, capture, and its shader package (`scene-renderer/shaders`: prelude/material ABI,
  compose, stock `basic` material; ABI version in `src/shaders.rs`).
- `scene-viewer`: optional window/canvas host, navigation, `ViewerHandle` commands/status.
- `nif-scene`: NIF parsing, dependency sessions, NIF → scene conversion, the `bethesda-pbr`
  material type (`nif-scene/shaders`), native data-folder resolution. See its scoped
  `AGENTS.md` and `tests/AGENTS.md`.
- `nif-viewer-lib`: entry points only (native `run`, WASM exports) orchestrating the crates above.
- `nif-viewer-app`: native application host.
- `nif-viewer-webapp`: JavaScript/web bundler host.
- `nif-viewer-blazor/NifViewer.Blazor`: C#/Razor wrapper and packaged static web assets.

Keep NIF/game knowledge out of `scene-*` crates, GPU/window/browser code out of
`nif-scene`, and parsing/conversion logic out of `nif-viewer-lib`. Shading belongs to
material types, not the core renderer: add new looks as registered material types.
Changing the prelude's bind groups, vertex/instance layout, entry points or the material
binding convention requires bumping `SHADER_ABI_VERSION` and every `manifest.json`
together. Record NuGet-visible API changes
in `nif-viewer-blazor/NifViewer.Blazor/CHANGELOG.md`.

## Working rules

- Preserve native and browser/WASM consumers. Keep platform-specific dependencies and
  filesystem/browser access behind the existing target-specific boundaries.
- Follow existing Rust edition, manifests, `.editorconfig`, and formatting conventions.
  Do not upgrade the edition, toolchain, or dependencies incidentally.
- Fix compiler and Clippy diagnostics rather than suppressing them to pass checks.
- Use braces for C# control-flow bodies, including single-line statements.
- Preserve existing user changes. Commit, push, or publish to a remote registry only
  when requested. A task-specific safety commit is not blanket permission to commit.
- Do not commit proprietary game assets, extracted resources, screenshots, or build
  outputs. Use generated fixtures and opt-in local asset checks.
- Distinguish successful parsing, dependency resolution, and actual rendering in reports.
  A parser returning a scene does not prove that geometry appeared correctly on screen.

## Verification

Run affected tests during development. For shared Rust changes, use:

```shell
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

`nif-viewer-lib/tests/regression.rs` holds pixel baselines for the renderer; change
them only for intended visual changes and say so. For browser-facing changes, run
`nif-viewer-webapp/scripts/webgpu-smoke.mjs` against a built `dist/` when a WebGPU
browser is available.

For browser-facing Rust changes, also build from `nif-viewer-lib`:

```shell
wasm-pack build --release --target web --out-dir pkg-web
```

Use `.cargo/config.toml`'s target-specific flags. Global `RUSTFLAGS` or
`CARGO_ENCODED_RUSTFLAGS` can override these and accidentally select WASM settings
for native compilation. Check the environment when native/WASM builds disagree.
Use the SDK specified by `global.json` for .NET work; report unavailable tooling
or SDK fallback accurately. See the Blazor guidance for packing and downstream checks.
