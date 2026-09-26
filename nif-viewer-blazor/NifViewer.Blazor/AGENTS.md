# Blazor package guidance

## Contracts

- Preserve the project's declared target frameworks and public resolver/component
  APIs unless the task explicitly requires a change. Keep JS interop and Rust WASM
  exports compatible with the wrapper.
- Package browser-native ES modules (`wasm-pack --target web`), the matching WASM
  binary, and required shaders as static web assets under `_content/NifViewer.Blazor/`.
  Do not substitute bundler-target output from `pkg` for `pkg-web`.
- Respect viewer initialization ordering: initialize/attach before queuing rendering,
  and report dependency failures through the existing load-result contract.
- Keep dependencies host-resolved via `INifDependencyResolver`; game-directory or BSA
  access belongs to consumers such as EsmParser, not this presentation package.
- Follow existing component lifetime/disposal patterns. Escape or render external
  names as literal text and keep JavaScript errors diagnosable from the host.

## Build and package

From repository root:

```shell
dotnet pack nif-viewer-blazor/NifViewer.Blazor -c Release -o artifacts -m:1
```

- Use serial packing: the target-framework builds share `pkg-web`; parallel builds
  were observed racing while wasm-pack wrote that directory.
- `NifViewerWasm.targets` rebuilds WASM when tracked Rust inputs are newer. Do not use
  `SkipNifViewerWasmBuild=true` unless matching assets have already been built and checked.
- For flag/shader/build-configuration changes, explicitly rebuild WASM if the target's
  incremental input list does not cover the changed files.
- Use a new package version for changed assets to avoid stale global NuGet caches.
  Preserve a version bump already made by the user rather than resetting it.
- A request to produce a local package does not authorize a public NuGet upload.

## EsmParser integration

When requested to update the sibling EsmParser consumer, copy the new `.nupkg` from
`artifacts` into that repository's `.packages` feed and update its centrally managed
`NifViewer.Blazor` version in `Directory.Packages.props`. Replace obsolete local
packages only within the requested scope. Restore and build the consumer, run its
relevant tests, and verify browser loading/rendering for changed WASM behaviour.

The package version currently used by the verified BSA integration is preview.5;
consult the project file and consumer configuration rather than assuming this remains
the latest version. Do not commit game fixtures or unrelated build output with a package.
