# Rust library guidance

## Architecture and portability

- Keep `src/nif` parsing independent of filesystem, browser, and GPU access. Return
  parsed data and dependency paths; let loaders/hosts resolve external resources.
- Preserve both `rlib` native use and `cdylib` WASM exports. Browser work must not
  block on native I/O or introduce native-only dependencies into shared code.
- Keep the staged dependency-loading contract in `load_session.rs`: NIF parsing
  reveals dependencies, supplied materials may reveal further textures, then finish
  queues the model. Preserve normalized paths and missing-dependency reporting.
- Avoid loading every candidate asset or logging whole vertex arrays/file contents
  on normal load paths. Prefer the existing chunked session API for large resources.
- Keep exported Rust functions, JavaScript interop, and C# wrapper APIs synchronized.

## Format and geometry correctness

- Ground version gates and field layouts in format references and verified fixtures.
  Record evidence near non-obvious branches; distinguish known support from assumptions.
- Use checked/bounded reads and validate block references before indexing. Negative
  refs and wrong-type or out-of-range refs must not panic during scene traversal.
- Preserve block-size resynchronization and existing diagnostics for unsupported
  blocks. Do not silently claim support for skipped geometry or material types.
- `NiTriStrips` owns transforms and property references; `NiTriStripsData` owns vertex
  attributes and strip indices. Legacy Bethesda property lists differ from later
  dedicated shader/alpha fields.
- Triangle-strip winding alternates at every source step, including degenerate
  connectors. Skip degenerate triangles without changing that parity; restart parity
  for each strip and validate vertex indices before passing geometry to the renderer.
- Preserve UVs, normals, vertex colors, world transforms, and texture references when
  converting geometry. Tests should verify more than a nonempty mesh count.
- Scene transforms are column-major; model conversion handles NIF Z-up to renderer
  Y-up. Avoid applying axis conversion or transforms twice.

## Verification

Follow `tests/AGENTS.md` for generated and external fixtures. For parser changes,
exercise both malformed inputs and valid geometry, then build the WASM target.
For rendering changes, inspect a rendered result when tooling is available; report
any unverified visual behaviour. Native unit tests alone cannot verify WebGPU output.
