#! /usr/bin/env pwsh
#Requires -Version 7.0
#Requires -PSEdition Core

$ErrorActionPreference = 'Stop'

# Check native exit codes explicitly to support PowerShell 7.0 as well.
function Invoke-BuildCommand {
    param([string] $Command, [string[]] $Arguments)

    & $Command @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "$Command $Arguments failed with exit code $LASTEXITCODE"
    }
}

$originalRustFlags = $env:RUSTFLAGS
$originalEncodedRustFlags = $env:CARGO_ENCODED_RUSTFLAGS
Push-Location $PSScriptRoot
try {
    # Global flags override .cargo/config.toml and can select the Wasm backend
    # for native builds. Use the repository's target-specific flags instead.
    $env:RUSTFLAGS = $null
    $env:CARGO_ENCODED_RUSTFLAGS = $null

    Invoke-BuildCommand cargo @('clippy', '--', '-D', 'warnings')

    Push-Location './nif-viewer-lib'
    try {
        Invoke-BuildCommand wasm-pack @('build', '--release')
    }
    finally {
        Pop-Location
    }

    Push-Location './nif-viewer-webapp'
    try {
        Remove-Item './node_modules' -Recurse -ErrorAction SilentlyContinue
        Remove-Item './dist' -Recurse -ErrorAction SilentlyContinue
        Invoke-BuildCommand npm @('i')
        Invoke-BuildCommand npm @('run', 'build')
    }
    finally {
        Pop-Location
    }

    # Browser-native ES module build (no bundler) for the Blazor package.
    Push-Location './nif-viewer-lib'
    try {
        Invoke-BuildCommand wasm-pack @('build', '--release', '--target', 'web', '--out-dir', 'pkg-web')
    }
    finally {
        Pop-Location
    }

    Invoke-BuildCommand dotnet @('pack', 'nif-viewer-blazor/NifViewer.Blazor', '-c', 'Release', '-o', 'artifacts/')
}
finally {
    $env:RUSTFLAGS = $originalRustFlags
    $env:CARGO_ENCODED_RUSTFLAGS = $originalEncodedRustFlags
    Pop-Location
}
