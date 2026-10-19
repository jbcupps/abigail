#requires -Version 7.0
# Synthetic file staging only: no installer execution, signing, or network calls.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$stager = Join-Path $PSScriptRoot '../stage_release_assets.ps1'
$tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\', '/')
$workspace = Join-Path $tempRoot ('abigail-release-stage-tests-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $workspace | Out-Null
$script:passed = 0

function New-Case([string]$Name, [switch]$WithoutInstaller) {
    $directory = Join-Path $workspace $Name
    New-Item -ItemType Directory -Path (Join-Path $directory 'target/release/bundle/nsis') -Force | Out-Null
    if (-not $WithoutInstaller) { Write-Fixture $directory 'target/release/bundle/nsis/versioned-setup.exe' 'synthetic NSIS bytes' }
    return $directory
}
function Write-Fixture([string]$Directory, [string]$RelativePath, [string]$Content) {
    $path = Join-Path $Directory $RelativePath
    New-Item -ItemType Directory -Path ([IO.Path]::GetDirectoryName($path)) -Force | Out-Null
    [IO.File]::WriteAllBytes($path, [Text.Encoding]::UTF8.GetBytes($Content))
}
function Invoke-Stage([string]$Directory, [hashtable]$Overrides = @{}) {
    $parameters = @{
        Version = '0.0.76'; Tag = 'v0.0.76-beta.131'; Repository = 'jbcupps/abigail'
        WindowsSigned = 'false'; UpdaterSigned = 'false'; CloudSigning = 'false'
        Destination = Join-Path $Directory 'release-assets'
    }
    foreach ($key in $Overrides.Keys) { $parameters[$key] = $Overrides[$key] }
    Push-Location -LiteralPath $Directory
    try { & $stager @parameters }
    finally { Pop-Location }
}
function Assert-Names([string]$Directory, [string[]]$Expected) {
    $actual = @((Get-ChildItem -LiteralPath (Join-Path $Directory 'release-assets') -File).Name | Sort-Object)
    if (($actual -join '|') -cne (($Expected | Sort-Object) -join '|')) { throw "Unexpected staged filenames: $($actual -join ', ')" }
}
function Assert-Copy([string]$Directory, [string]$Source, [string]$Destination) {
    $before = (Get-FileHash -LiteralPath (Join-Path $Directory $Source) -Algorithm SHA256).Hash
    $after = (Get-FileHash -LiteralPath (Join-Path $Directory "release-assets/$Destination") -Algorithm SHA256).Hash
    if ($before -ne $after) { throw "Staging changed bytes: $Destination" }
}
function Pass([string]$Name) { $script:passed++; Write-Host "PASS $Name" }
function Assert-Rejected([string]$Name, [scriptblock]$Action, [string]$Pattern) {
    $failed = $false
    try { & $Action }
    catch {
        $failed = $true
        if ($_.Exception.Message -notmatch $Pattern) { throw "$Name raised an unexpected error: $($_.Exception.Message)" }
    }
    if (-not $failed) { throw "$Name was not rejected." }
    Pass $Name
}
# The failure cases must stop before updater manifest generation. This also
# prevents an unexpected branch from launching an external process in tests.
function node { throw 'Unexpected Node invocation in a staging failure test.' }

try {
    $caseRoot = New-Case 'single-exe'
    $original = (Get-FileHash -LiteralPath (Join-Path $caseRoot 'target/release/bundle/nsis/versioned-setup.exe')).Hash
    Invoke-Stage $caseRoot
    Assert-Names $caseRoot @('Abigail-windows-x64-setup.exe')
    Assert-Copy $caseRoot 'target/release/bundle/nsis/versioned-setup.exe' 'Abigail-windows-x64-setup.exe'
    if ((Get-FileHash -LiteralPath (Join-Path $caseRoot 'target/release/bundle/nsis/versioned-setup.exe')).Hash -ne $original) { throw 'Staging modified its source file.' }
    Pass 'one installer receives the canonical name with unchanged bytes'

    Write-Fixture $caseRoot 'release-assets/sentinel.txt' 'existing destination content'
    Assert-Rejected 'stale destination' { Invoke-Stage $caseRoot } 'new; refuse stale assets'
    if ([IO.File]::ReadAllText((Join-Path $caseRoot 'release-assets/sentinel.txt')) -ne 'existing destination content') { throw 'Staging changed an existing destination.' }

    $caseRoot = New-Case 'missing-nsis' -WithoutInstaller
    Assert-Rejected 'missing NSIS installer' { Invoke-Stage $caseRoot } 'exactly one final NSIS'
    $caseRoot = New-Case 'multiple-nsis'
    Write-Fixture $caseRoot 'target/release/bundle/nsis/second-setup.exe' 'second synthetic installer'
    Assert-Rejected 'multiple NSIS installers' { Invoke-Stage $caseRoot } 'exactly one final NSIS'

    $caseRoot = New-Case 'local-msi'
    Write-Fixture $caseRoot 'target/release/bundle/msi/versioned.msi' 'synthetic MSI bytes'
    Invoke-Stage $caseRoot
    Assert-Names $caseRoot @('Abigail-windows-x64-setup.exe', 'Abigail-windows-x64.msi')
    Assert-Copy $caseRoot 'target/release/bundle/msi/versioned.msi' 'Abigail-windows-x64.msi'
    Pass 'local lane includes one canonical unchanged MSI'

    $caseRoot = New-Case 'multiple-msi'
    Write-Fixture $caseRoot 'target/release/bundle/msi/first.msi' 'first synthetic MSI'
    Write-Fixture $caseRoot 'target/release/bundle/msi/second.msi' 'second synthetic MSI'
    Assert-Rejected 'multiple MSI installers' { Invoke-Stage $caseRoot } 'at most one final MSI'

    $caseRoot = New-Case 'cloud-no-msi'
    Write-Fixture $caseRoot 'target/release/bundle/msi/versioned.msi' 'unused synthetic MSI'
    Write-Fixture $caseRoot 'artifacts/windows-signatures.json' '{"fixture":true}'
    Invoke-Stage $caseRoot @{ CloudSigning = 'true'; WindowsSigned = 'true' }
    Assert-Names $caseRoot @('Abigail-windows-x64-setup.exe', 'windows-signatures.json')
    Assert-Copy $caseRoot 'artifacts/windows-signatures.json' 'windows-signatures.json'
    Pass 'cloud lane omits MSI and preserves signature evidence bytes'

    $caseRoot = New-Case 'missing-updater'
    Assert-Rejected 'required updater artifacts missing' { Invoke-Stage $caseRoot @{ UpdaterSigned = 'true' } } 'no updater artifacts'
    $caseRoot = New-Case 'duplicate-updater'
    Write-Fixture $caseRoot 'target/first/bundle/nsis/same.nsis.zip' 'first synthetic updater'
    Write-Fixture $caseRoot 'target/second/bundle/nsis/same.nsis.zip' 'second synthetic updater'
    Assert-Rejected 'duplicate updater filenames' { Invoke-Stage $caseRoot @{ UpdaterSigned = 'true' } } 'Duplicate release asset: same\.nsis\.zip'
    if (Test-Path -LiteralPath (Join-Path $caseRoot 'release-assets/latest.json')) { throw 'A failed updater stage generated a manifest.' }
    Write-Host "Release asset staging: $script:passed checks passed. Only synthetic bytes were used."
} finally {
    $resolvedWorkspace = [IO.Path]::GetFullPath($workspace)
    if ([IO.Path]::GetDirectoryName($resolvedWorkspace) -ne $tempRoot -or
        [IO.Path]::GetFileName($resolvedWorkspace) -notmatch '^abigail-release-stage-tests-[0-9a-f]{32}$') {
        throw 'Refusing to clean an unexpected staging test directory.'
    }
    if (Test-Path -LiteralPath $resolvedWorkspace) { Remove-Item -LiteralPath $resolvedWorkspace -Recurse -Force }
}
