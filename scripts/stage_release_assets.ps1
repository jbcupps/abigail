# Stage the final public filenames once. No signing or installer execution.
#requires -Version 7.0
param(
    [Parameter(Mandatory)][string]$Version,
    [Parameter(Mandatory)][string]$Tag,
    [Parameter(Mandatory)][string]$Repository,
    [Parameter(Mandatory)][ValidateSet('true', 'false')][string]$WindowsSigned,
    [Parameter(Mandatory)][ValidateSet('true', 'false')][string]$UpdaterSigned,
    [Parameter(Mandatory)][ValidateSet('true', 'false')][string]$CloudSigning,
    [string]$Destination = 'release-assets'
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if ($Version -notmatch '^\d+\.\d+\.\d+$' -or $Tag -notmatch '^v\d+\.\d+\.\d+(?:-beta\.\d+)?$') { throw 'Invalid release version or tag.' }
if ($Repository -notmatch '^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$') { throw 'Invalid repository.' }
if (Test-Path -LiteralPath $Destination) { throw 'Final release asset directory must be new; refuse stale assets.' }
New-Item -ItemType Directory -Path $Destination | Out-Null
function Copy-ReleaseFile([string]$Source, [string]$Name) {
    $file = Get-Item -LiteralPath $Source
    if ($file.PSIsContainer -or ($file.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Release assets must be regular files.' }
    if ($Name -ne [IO.Path]::GetFileName($Name)) { throw 'Release asset names cannot contain directories.' }
    $target = Join-Path $Destination $Name
    if (Test-Path -LiteralPath $target) { throw "Duplicate release asset: $Name" }
    Copy-Item -LiteralPath $file.FullName -Destination $target
    if ((Get-FileHash -LiteralPath $target).Hash -ne (Get-FileHash -LiteralPath $file.FullName).Hash) { throw "Copy changed release bytes: $Name" }
}
$installers = @(Get-ChildItem -LiteralPath 'target/release/bundle/nsis' -File -Filter '*.exe')
if ($installers.Count -ne 1) { throw 'Expected exactly one final NSIS installer.' }
Copy-ReleaseFile $installers[0].FullName 'Abigail-windows-x64-setup.exe'
if ($CloudSigning -ne 'true' -and (Test-Path -LiteralPath 'target/release/bundle/msi')) {
    $msi = @(Get-ChildItem -LiteralPath 'target/release/bundle/msi' -File -Filter '*.msi')
    if ($msi.Count -gt 1) { throw 'Expected at most one final MSI installer.' }
    if ($msi.Count -eq 1) { Copy-ReleaseFile $msi[0].FullName 'Abigail-windows-x64.msi' }
}
if ($WindowsSigned -eq 'true') { Copy-ReleaseFile 'artifacts/windows-signatures.json' 'windows-signatures.json' }
if ($UpdaterSigned -eq 'true') {
    $updaters = @(Get-ChildItem -LiteralPath target -File -Recurse | Where-Object { $_.FullName -match '[\\/]bundle[\\/].*\.(?:nsis|msi)\.zip(?:\.sig)?$' })
    if (-not $updaters.Count) { throw 'Updater signing was requested but no updater artifacts exist.' }
    foreach ($file in $updaters) { Copy-ReleaseFile $file.FullName $file.Name }
    node (Join-Path $PSScriptRoot 'generate_tauri_latest_manifest.mjs') --version $Version --assets-dir $Destination --base-url "https://github.com/$Repository/releases/download/$Tag" --output (Join-Path $Destination 'latest.json')
    if ($LASTEXITCODE -ne 0) { throw 'Updater manifest generation failed.' }
}
