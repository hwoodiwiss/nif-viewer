# Rust test guidance

- Use generated fixtures for portable regression tests. Keep their binary writer
  independent of production parsing helpers and use independently known expectations.
- Do not require a Steam installation or hard-coded personal resource directory in
  new default tests. Existing sample tests may return early when assets are absent;
  do not count those returns as evidence that real assets were verified.
- Prefer explicitly ignored, opt-in tests for proprietary local assets, with paths
  supplied by environment variables. Do not commit the asset bytes.
- Test malformed references and indices as well as valid geometry. For strips, cover
  winding, degenerate connectors, multiple strips, attributes, transforms, and legacy
  texture references. See `tri_strips.rs` for generated examples.
- Successful parsing is not sufficient: assert expected mesh contents, index bounds,
  and dependency paths. Where relevant, also check model conversion or rendering.

From repository root:

```shell
cargo test -p nif-viewer-lib --test tri_strips
cargo test -p nif-viewer-lib --lib
```

The optional `tri_strips` local check uses `NIF_TEST_FILE` and `NIF_TEST_DATA_ROOT`:

```shell
cargo test -p nif-viewer-lib --test tri_strips -- --ignored --nocapture
```

Supply a local NiTriStrips NIF and its resource root before running that check.
It verifies renderable geometry and referenced texture paths, not a pixel-perfect image.
