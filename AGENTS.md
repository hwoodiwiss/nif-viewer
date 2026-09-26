# Repository guidance

## Layout

This repository provides a Rust NIF parser and wgpu renderer, a native application,
a JavaScript web host, and a Blazor NuGet wrapper. See `README.md` for usage and
`docs/nif-format-spec.md` for format notes. Read scoped `AGENTS.md` files before
changing the library, its tests, or the Blazor package.

- `nif-viewer-lib`: parsing, model conversion, dependency loading, rendering, WASM exports.
- `nif-viewer-app`: native application host.
- `nif-viewer-webapp`: JavaScript/web bundler host.
- `nif-viewer-blazor/NifViewer.Blazor`: C#/Razor wrapper and packaged static web assets.
- `shaders`: WGSL rendering shaders shared by consumers.

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
cargo test -p nif-viewer-lib
cargo clippy --workspace --all-targets -- -D warnings
```

For browser-facing Rust changes, also build from `nif-viewer-lib`:

```shell
wasm-pack build --release --target web --out-dir pkg-web
```

Use `.cargo/config.toml`'s target-specific flags. Global `RUSTFLAGS` or
`CARGO_ENCODED_RUSTFLAGS` can override these and accidentally select WASM settings
for native compilation. Check the environment when native/WASM builds disagree.
Use the SDK specified by `global.json` for .NET work; report unavailable tooling
or SDK fallback accurately. See the Blazor guidance for packing and downstream checks.
