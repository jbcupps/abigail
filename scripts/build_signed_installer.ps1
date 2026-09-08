<#
Prepare builds the current split product without signing credentials.
Sign signs those exact bytes, bundles NSIS, extracts it, and verifies the payload.
Neither phase publishes a release or changes the checked-in Tauri configuration.
#>
param(
    [Parameter(Mandatory)][ValidateSet('Prepare','Check','Sign')][string]$Phase,
    [ValidatePattern('^\d+\.\d+\.\d+$')][string]$Version = '0.0.75',
    [switch]$SkipStage
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'windows-signing.psm1') -Force
Import-Module (Join-Path $PSScriptRoot 'windows-signing-state.psm1') -Force
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$targetRoot = if ($env:CARGO_TARGET_DIR) { [IO.Path]::GetFullPath($env:CARGO_TARGET_DIR, $repoRoot) } else { Join-Path $repoRoot 'target' }
$work = Join-Path $targetRoot 'signed-release'
$app = Join-Path $repoRoot 'hive-app'
$resourceRoot = Join-Path $app 'resources'
$bootstrapRoot = Join-Path $resourceRoot 'bootstrap'
$statePath = Join-Path $work 'build-state.json'
$configPath = Join-Path $work 'bundle-config.json'
$previousSignedMainPath = $env:ABIGAIL_SIGNED_MAIN_PATH
$previousTargetDir = $env:CARGO_TARGET_DIR
$binaries = @(
    (Join-Path $targetRoot 'release/abigail-hive-app.exe'),
    (Join-Path $resourceRoot 'abigail-entity-runtime-app.exe'),
    (Join-Path $resourceRoot 'hive-daemon.exe'),
    (Join-Path $resourceRoot 'entity-daemon.exe')
)
New-Item -ItemType Directory -Force -Path $work | Out-Null
$localCli = Join-Path $repoRoot '.cache/signing/tauri-cli/node_modules/.bin/tauri.cmd'
$cli = if (Test-Path $localCli) { $localCli } else { (Get-Command tauri -ErrorAction Stop).Source }

function Invoke-Tauri([string[]]$Arguments) {
    Push-Location $app
    try { & $cli @Arguments; if ($LASTEXITCODE -ne 0) { throw 'Tauri build/bundle failed.' } }
    finally { Pop-Location }
}
function Write-Json($Value, [string]$Path) {
    [IO.File]::WriteAllText($Path, ($Value | ConvertTo-Json -Depth 12), [Text.UTF8Encoding]::new($false))
}

Push-Location $repoRoot
try {
    $env:CARGO_TARGET_DIR = $targetRoot
    $commit = (git rev-parse HEAD).Trim()
    git diff --quiet HEAD --
    if ($LASTEXITCODE -ne 0) { throw 'Commit tracked source changes before preparing or signing a release.' }
    if ($Phase -eq 'Prepare') {
        foreach ($name in @('ESIGNER_PASSWORD','ESIGNER_TOTP_SECRET')) {
            if ([Environment]::GetEnvironmentVariable($name)) { throw 'Run Prepare without signing secrets in the build environment.' }
        }
        if (-not $SkipStage) { & (Join-Path $PSScriptRoot 'stage_split_installer_resources.ps1') -Configuration release }
        $config = @{
            version = $Version
            build = @{ beforeBuildCommand = ''; beforeBundleCommand = '' }
            bundle = @{
                targets = @('nsis'); createUpdaterArtifacts = $false
                resources = @('resources/abigail-entity-runtime-app.exe','resources/hive-daemon.exe','resources/entity-daemon.exe','resources/bootstrap/**/*')
                windows = @{ signCommand = $null; certificateThumbprint = $null; digestAlgorithm = 'sha256'; timestampUrl = $null; tsp = $false }
            }
            plugins = @{ updater = $null }
        }
        Write-Json $config $configPath
        Invoke-Tauri @('build','--no-bundle','--ci','--config',$configPath,'--','--locked')
        $state = @{
            sourceCommit = $commit; version = $Version; builtUtc = [DateTime]::UtcNow.ToString('o')
            binaries = @($binaries | ForEach-Object { @{ path = $_; sha256 = (Get-FileHash -LiteralPath $_ -Algorithm SHA256).Hash } })
            configSha256 = (Get-FileHash -LiteralPath $configPath -Algorithm SHA256).Hash
            bootstrap = @(Get-AbigailFileManifest -Root $bootstrapRoot)
        }
        Write-Json $state $statePath
        Write-Host "Prepared current offline-setup binaries from $commit. Signing credentials were not used."
        return
    }

    $sevenZipCommand = Get-Command 7z -ErrorAction SilentlyContinue
    $sevenZip = if ($env:ABIGAIL_7ZIP_PATH) { (Resolve-Path -LiteralPath $env:ABIGAIL_7ZIP_PATH).Path } elseif ($sevenZipCommand) { $sevenZipCommand.Source } else { $null }
    foreach ($candidate in @('C:/Program Files/7-Zip/7z.exe', (Join-Path $repoRoot '.cache/signing/7zip/7z.exe'))) {
        if (-not $sevenZip -and (Test-Path -LiteralPath $candidate)) { $sevenZip = $candidate }
    }
    if (-not $sevenZip) { throw '7-Zip is required to verify the actual installer payload.' }
    $null = Get-AbigailSignTool
    $state = Get-Content -LiteralPath $statePath -Raw | ConvertFrom-Json
    Assert-AbigailPreparedInputs -State $state -ExpectedCommit $commit -ExpectedVersion $Version -ExpectedBinaryPaths $binaries -ConfigPath $configPath -BootstrapRoot $bootstrapRoot
    if ($Phase -eq 'Check') {
        Write-Host "Prepared source, configuration, four executables, complete offline payload, and verification tools checked: $commit"
        return
    }
    $manual = $env:ABIGAIL_ESIGNER_AUTH_MODE -eq 'manual'
    if ($manual -and $env:GITHUB_ACTIONS -eq 'true') { throw 'Manual signing requires an interactive local Windows desktop.' }
    $required = @('ESIGNER_USERNAME','ESIGNER_PASSWORD','ESIGNER_CREDENTIAL_ID','WINDOWS_CERTIFICATE_THUMBPRINT')
    if (-not $manual) { $required += 'ESIGNER_TOTP_SECRET' }
    foreach ($name in $required) {
        if (-not [Environment]::GetEnvironmentVariable($name)) { throw "$name is required. Use encrypted GitHub secrets or the private signing prompt." }
    }
    $runId = [DateTime]::UtcNow.ToString('yyyyMMddTHHmmssZ') + '-' + [guid]::NewGuid().ToString('N')
    $records = @()
    # Sign internal programs now. Tauri must sign the main program through its
    # callback after it patches the NSIS bundle marker and before compression.
    foreach ($entry in ($state.binaries | Select-Object -Skip 1)) {
        & (Join-Path $PSScriptRoot 'sign_windows_artifact.ps1') -Path $entry.path
        if ($LASTEXITCODE -ne 0) { throw "Signing failed: $($entry.path)" }
        $records += Assert-AbigailSignature -Path $entry.path -Thumbprint $env:WINDOWS_CERTIFICATE_THUMBPRINT
    }
    $captureDir = Join-Path $work ('signed-main-' + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $captureDir | Out-Null
    $env:ABIGAIL_SIGNED_MAIN_PATH = Join-Path $captureDir 'abigail-hive-app.exe'
    $config = Get-Content -LiteralPath $configPath -Raw | ConvertFrom-Json -AsHashtable
    $config.bundle.windows.signCommand = @{
        cmd = (Get-Command pwsh).Source
        args = @('-NoProfile','-STA','-NonInteractive','-File',(Join-Path $PSScriptRoot 'sign_windows_artifact.ps1'),'-Path','%1')
    }
    $signingConfigPath = Join-Path $work "bundle-signing-$runId.json"
    Write-Json $config $signingConfigPath
    Invoke-Tauri @('bundle','--ci','--bundles','nsis','--config',$signingConfigPath)
    $records += Assert-AbigailSignature -Path $env:ABIGAIL_SIGNED_MAIN_PATH -Thumbprint $env:WINDOWS_CERTIFICATE_THUMBPRINT
    $installer = Join-Path $targetRoot "release/bundle/nsis/Abigail_${Version}_x64-setup.exe"
    $records += Assert-AbigailSignature -Path $installer -Thumbprint $env:WINDOWS_CERTIFICATE_THUMBPRINT
    $extract = Join-Path $work ('extracted-' + [guid]::NewGuid().ToString('N'))
    & $sevenZip x $installer "-o$extract" -y | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'Installer extraction failed.' }
    foreach ($entry in $state.binaries) {
        $name = [IO.Path]::GetFileName($entry.path)
        $files = @(Get-ChildItem -LiteralPath $extract -Recurse -File -Filter $name)
        if ($files.Count -ne 1) { throw "Installer must contain exactly one $name." }
        $record = Assert-AbigailSignature -Path $files[0].FullName -Thumbprint $env:WINDOWS_CERTIFICATE_THUMBPRINT
        $sourceRecord = $records | Where-Object { [IO.Path]::GetFileName($_.path) -eq $name }
        if ($record.sha256 -ne $sourceRecord.sha256) { throw "Packaged $name differs from its verified source." }
        $records += $record
    }
    # NSIS creates and signs this executable during bundling as well.
    $records += Assert-AbigailSignature -Path (Join-Path $extract 'uninstall.exe') -Thumbprint $env:WINDOWS_CERTIFICATE_THUMBPRINT
    $packagedBootstrap = Join-Path $extract 'resources/bootstrap'
    Assert-AbigailFileManifest -Root $packagedBootstrap -Manifest $state.bootstrap
    $output = Join-Path $work "artifacts/$runId"
    New-Item -ItemType Directory -Path $output -Force | Out-Null
    Copy-Item -LiteralPath $installer -Destination (Join-Path $output 'Abigail-windows-x64-setup.exe') -Force
    Copy-Item -LiteralPath $env:ABIGAIL_SIGNED_MAIN_PATH -Destination (Join-Path $output 'abigail-hive-app.exe') -Force
    Write-Json @{ sourceCommit = $commit; version = $Version; verifiedUtc = [DateTime]::UtcNow.ToString('o'); artifacts = $records; bootstrapFilesVerified = $state.bootstrap.Count; extractedPayload = $extract } (Join-Path $output 'signature-report.json')
    Write-Json @{ sourceCommit = $commit; version = $Version; artifactDirectory = $output; extractedPayload = $extract; runId = $runId } (Join-Path $work 'latest-success.json')
    if ($env:GITHUB_OUTPUT) { "artifact_dir=$output" | Add-Content -LiteralPath $env:GITHUB_OUTPUT }
    Write-Host "Verified signed installer and all four Abigail executables: $output"
} finally { $env:ABIGAIL_SIGNED_MAIN_PATH = $previousSignedMainPath; $env:CARGO_TARGET_DIR = $previousTargetDir; Pop-Location }
