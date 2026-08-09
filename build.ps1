#! /usr/bin/env pwsh
#Requires -Version 7.0
#Requires -PSEdition Core

cargo clippy -- -D warnings

Push-Location ".\nif-viewer-lib"
$env:RUSTFLAGS = '--cfg=web_sys_unstable_apis --cfg getrandom_backend="wasm_js"'
wasm-pack build --release
Pop-Location

Push-Location ".\nif-viewer-webapp"
Remove-Item "./node_modules" -Recurse -ErrorAction SilentlyContinue
Remove-Item "./dist" -Recurse -ErrorAction SilentlyContinue
npm i
npm run build
Pop-Location

# Browser-native ES module build (no bundler) for the Blazor package.
Push-Location ".\nif-viewer-lib"
$env:RUSTFLAGS = '--cfg=web_sys_unstable_apis --cfg getrandom_backend="wasm_js"'
wasm-pack build --release --target web --out-dir pkg-web
Pop-Location

dotnet pack nif-viewer-blazor/NifViewer.Blazor -c Release -o artifacts/