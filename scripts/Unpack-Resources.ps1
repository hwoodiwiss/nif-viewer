[CmdletBinding()]
param(
    [string]$StarfieldInstallDir = "C:\Program Files (x86)\Steam\steamapps\common\Starfield",
    [string]$TargetGameDir = $null,
    [string]$ArchivePattern = "*",
    [string]$OutputDirectory = "$env:USERPROFILE\Documents\StarfieldResources"
)

$ErrorActionPreference = "Stop"
$archive2 = Join-Path $StarfieldInstallDir "Tools\Archive2\Archive2.exe"

if ($null -eq $TargetGameDir) {
    throw "Game Data directory was not found at '$TargetGameDir'."
    $TargetGameDir = $StarfieldInstallDir
}

$dataDirectory = Join-Path $TargetGameDir "Data"

if (-not (Test-Path -LiteralPath $archive2 -PathType Leaf)) {
    throw "Archive2.exe was not found at '$archive2'. Install Starfield or pass -StarfieldInstallDir."
}
if (-not (Test-Path -LiteralPath $dataDirectory -PathType Container)) {
    throw "Game Data directory was not found at '$dataDirectory'."
}

New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null
$archives = @(Get-ChildItem -LiteralPath $dataDirectory -File |
    Where-Object { $_.Name -like $ArchivePattern -and ($_.Name.EndsWith(".ba2") -or $_.Name.EndsWith(".bsa")) } |
    Sort-Object Name)

if ($archives.Count -eq 0) {
    throw "No archives matching '$ArchivePattern' were found in '$dataDirectory'."
}

for ($i = 0; $i -lt $archives.Count; $i++) {
    $archive = $archives[$i]
    Write-Progress -Activity "Unpacking Game resources" `
        -Status "$($i + 1)/$($archives.Count): $($archive.Name)" `
        -PercentComplete ((($i + 1) / $archives.Count) * 100)

    # Extract in name order. Archive2 overwrites duplicate paths, so later
    # archives win naturally.
    & $archive2 $archive.FullName "-extract=$OutputDirectory" | Out-Host
    if ($LASTEXITCODE -ne 0) {
        throw "Archive2 failed for '$($archive.Name)' with exit code $LASTEXITCODE."
    }
}

Write-Progress -Activity "Unpacking Game resources" -Completed
Write-Host "Unpacked $($archives.Count) archives to '$OutputDirectory'."
