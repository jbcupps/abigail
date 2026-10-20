<#
.SYNOPSIS
Build the unsigned, self-contained Abigail Windows NSIS installer.
.DESCRIPTION
Builds both frontends and the four split-product binaries, embeds the internal
binaries, then bundles Abigail without certificate or updater requirements.
Debug is the default for a faster MVP build. Both profiles use Tauri's custom
protocol so installed apps render embedded assets without a development server.
NSIS uses zlib compression by default for faster iteration; the installer is
larger than with -Compression lzma. Compression is recorded in mvp-build.json.
Use -SkipBuild -SkipFrontend only after building the same four packages with
--features tauri/custom-protocol. -VerifyInstall checks the actual installed
payload in an isolated directory; it does not start an app using personal data.
.NOTES
Requires Windows x64, the Rust MSVC toolchain with Visual Studio C++ Build Tools
and Windows SDK, Node.js 22.12+ (24 is tested), npm, and cargo-tauri v2 (2.10.1
is tested). Tauri obtains NSIS and the WebView2 bootstrapper during first-time
bundling, so that first build needs internet unless those tools are cached.
Installing on a machine without WebView2 also needs its runtime installation.
startup.mp4 is optional: a clean checkout uses the existing branded splash
fallback. The installer contains the app/daemons; models are configured in Hive.
#>
[CmdletBinding()]
param(
    [ValidatePattern('^\d+\.\d+\.\d+$')][string]$Version = '0.0.1',
    [ValidateSet('debug', 'release')][string]$Configuration = 'debug',
    [ValidateSet('zlib', 'bzip2', 'lzma', 'none')][string]$Compression = 'zlib',
    [string]$TargetDir = $env:CARGO_TARGET_DIR,
    [string]$OutputDir,
    [switch]$SkipBuild,
    [switch]$SkipFrontend,
    [switch]$VerifyInstall,
    [switch]$PreflightOnly
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path

function Resolve-RepoPath([string]$Value, [string]$Fallback) {
    if ([string]::IsNullOrWhiteSpace($Value)) { $Value = $Fallback }
    if (-not [IO.Path]::IsPathRooted($Value)) { $Value = Join-Path $repoRoot $Value }
    [IO.Path]::GetFullPath($Value)
}

function Invoke-Checked([string]$Command, [string[]]$Arguments, [string]$Directory) {
    Push-Location $Directory
    try {
        & $Command @Arguments
        if ($LASTEXITCODE -ne 0) { throw "$Command failed with exit code $LASTEXITCODE in $Directory." }
    } finally { Pop-Location }
}

function Get-NsisLauncherHash([string]$Path) {
    # Tauri's platform.rs defines this unique, fixed-size marker. The bundler
    # temporarily changes UNK to NSS for NSIS, then restores its input file.
    $bytes = [IO.File]::ReadAllBytes($Path)
    $text = [Text.Encoding]::ASCII.GetString($bytes)
    $marker = '__TAURI_BUNDLE_TYPE_VAR_UNK'
    $offset = $text.IndexOf($marker, [StringComparison]::Ordinal)
    if ($offset -lt 0 -or $text.IndexOf($marker, $offset + $marker.Length, [StringComparison]::Ordinal) -ge 0) {
        throw 'Tauri launcher must contain exactly one unpatched bundle-type marker.'
    }
    $text = $null
    [Array]::Copy([Text.Encoding]::ASCII.GetBytes('NSS'), 0, $bytes, $offset + $marker.Length - 3, 3)
    $hasher = [Security.Cryptography.SHA256]::Create()
    try { [BitConverter]::ToString($hasher.ComputeHash($bytes)).Replace('-', '') }
    finally { $hasher.Dispose() }
}

function Read-FrontendAssets([string]$App) {
    $frontendDir = Join-Path $repoRoot "$App/src-ui/dist"
    $indexPath = Join-Path $frontendDir 'index.html'
    if (-not (Test-Path -LiteralPath $indexPath)) { throw "$App has no built index.html. Build its frontend first." }
    $index = Get-Content -LiteralPath $indexPath -Raw
    if ($index -match '(?:src|href)="/(?:assets|video)/' -or $index -match 'rel="stylesheet"') {
        throw "$App emitted asset paths or external stylesheets incompatible with the packaged WebView."
    }
    $entries = @([regex]::Matches($index, 'src="\./([^"<>]+\.js)"') | ForEach-Object { $_.Groups[1].Value })
    if ($entries.Count -eq 0) { throw "$App has no packaged JavaScript entry in index.html." }
    foreach ($entry in $entries) {
        if (-not (Test-Path -LiteralPath (Join-Path $frontendDir $entry))) { throw "$App is missing packaged entry $entry." }
    }
    $scripts = @(Get-ChildItem (Join-Path $frontendDir 'assets') -Filter '*.js' -File)
    $hasInlineStyles = [bool]($scripts | Select-String -SimpleMatch 'data-abigail-app-css' -List | Select-Object -First 1)
    $hasTheme = [bool]($scripts | Select-String -SimpleMatch '--color-primary' -List | Select-Object -First 1)
    if (-not $hasInlineStyles -or -not $hasTheme) { throw "$App is missing its embedded application stylesheet/theme." }
    [pscustomobject]@{
        app = $App; javascript_entries = $entries
        splash_video_present = Test-Path -LiteralPath (Join-Path $frontendDir 'video/startup.mp4')
    }
}

if ($env:OS -ne 'Windows_NT') { throw 'This installer lane requires Windows.' }
foreach ($command in @('cargo', 'node', 'npm.cmd')) {
    if (-not (Get-Command $command -ErrorAction SilentlyContinue)) { throw "$command is required on PATH." }
}
Invoke-Checked 'cargo' @('tauri', '--version') $repoRoot
$targetRoot = Resolve-RepoPath $TargetDir 'target'
$outputRoot = Resolve-RepoPath $OutputDir 'dist/windows-mvp'
Write-Host "Unsigned Abigail MVP: $Configuration; NSIS compression: $Compression; Cargo target: $targetRoot"
Write-Host 'NSIS is supplied by Tauri; no SSL.com certificate or updater key is required.'
if ($PreflightOnly) { return }
if ($SkipBuild -and -not $SkipFrontend) {
    throw '-SkipBuild also requires -SkipFrontend: rebuilt frontend files must be embedded by rebuilding the app binaries.'
}

$previousTargetDir = $env:CARGO_TARGET_DIR
$previousTauriConfig = $env:TAURI_CONFIG
$env:CARGO_TARGET_DIR = $targetRoot
try {
    if (-not $SkipFrontend) {
        foreach ($app in @('hive-app', 'entity-runtime-app')) {
            $frontendDir = Join-Path $repoRoot "$app/src-ui"
            if (-not (Test-Path (Join-Path $frontendDir 'node_modules'))) {
                Invoke-Checked 'npm.cmd' @('ci') $frontendDir
            }
            Invoke-Checked 'npm.cmd' @('run', 'build') $frontendDir
        }
    }
    $frontends = @(foreach ($app in @('hive-app', 'entity-runtime-app')) { Read-FrontendAssets $app })

    if (-not $SkipBuild) {
        $buildArgs = @('build', '--locked', '-p', 'hive-daemon', '-p', 'entity-daemon',
            '-p', 'abigail-hive-app', '-p', 'abigail-entity-runtime-app',
            '--features', 'tauri/custom-protocol')
        if ($Configuration -eq 'release') { $buildArgs += '--release' }
        # Apply version to both embedded Tauri contexts, not only installer metadata.
        $env:TAURI_CONFIG = (@{ version = $Version } | ConvertTo-Json -Compress)
        Invoke-Checked 'cargo' $buildArgs $repoRoot
        $env:TAURI_CONFIG = $previousTauriConfig
    }

    $binaryDir = Join-Path $targetRoot $Configuration
    $resourceDir = Join-Path $repoRoot 'hive-app/resources'
    $mainBinary = Join-Path $binaryDir 'abigail-hive-app.exe'
    if (-not (Test-Path $mainBinary)) { throw "Missing main application: $mainBinary" }
    foreach ($frontend in $frontends) {
        $shellPath = Join-Path $binaryDir "abigail-$($frontend.app).exe"
        if (-not (Test-Path -LiteralPath $shellPath)) { throw "Missing app shell: $shellPath" }
        $shellVersion = (Get-Item -LiteralPath $shellPath).VersionInfo.ProductVersion
        if ($shellVersion -ne $Version) { throw "$shellPath embeds version $shellVersion, but installer version is $Version. Rebuild with the requested version." }
        # Asset map keys are stored in the executable even when contents are
        # compressed. A dev-server build has no embedded frontend asset map.
        $shellBytes = [Text.Encoding]::ASCII.GetString([IO.File]::ReadAllBytes($shellPath))
        foreach ($entry in $frontend.javascript_entries) {
            if (-not $shellBytes.Contains($entry)) {
                throw "$shellPath does not embed current frontend entry $entry. Rebuild with --features tauri/custom-protocol after building the frontend."
            }
        }
        $shellBytes = $null
    }
    $runtimeInputs = @($mainBinary)
    $runtimeInputs += @('abigail-entity-runtime-app.exe', 'hive-daemon.exe', 'entity-daemon.exe') |
        ForEach-Object { Join-Path $binaryDir $_ }
    Invoke-Checked 'node' (@((Join-Path $PSScriptRoot 'check_windows_runtime.mjs')) + $runtimeInputs) $repoRoot
    # bundle targets mainBinaryName directly; build normally does the rename.
    # Copy here because this lane deliberately bundles an already built app.
    $bundledMainBinary = Join-Path $binaryDir 'Abigail.exe'
    Copy-Item -LiteralPath $mainBinary -Destination $bundledMainBinary -Force
    $expectedLauncherHash = Get-NsisLauncherHash $bundledMainBinary
    New-Item -ItemType Directory -Force -Path $resourceDir | Out-Null
    $internalBinaries = @('abigail-entity-runtime-app.exe', 'hive-daemon.exe', 'entity-daemon.exe')
    foreach ($binary in $internalBinaries) {
        $source = Join-Path $binaryDir $binary
        if (-not (Test-Path $source)) { throw "Missing internal application: $source" }
        Copy-Item -LiteralPath $source -Destination (Join-Path $resourceDir $binary) -Force
    }

    $runId = [DateTime]::UtcNow.ToString('yyyyMMddTHHmmssZ') + '-' + [guid]::NewGuid().ToString('N').Substring(0, 8)
    $workDir = Join-Path $repoRoot "target/mvp-packaging/$runId"
    New-Item -ItemType Directory -Force -Path $workDir | Out-Null
    $configPath = Join-Path $workDir 'tauri-unsigned-mvp.json'
    $config = @{
        version = $Version
        mainBinaryName = 'Abigail'
        build = @{ beforeBuildCommand = '' }
        bundle = @{
            active = $true
            targets = @('nsis')
            createUpdaterArtifacts = $false
            resources = @($internalBinaries | ForEach-Object { "resources/$_" })
            windows = @{
                certificateThumbprint = $null; signCommand = $null; timestampUrl = $null; tsp = $false
                nsis = @{ compression = $Compression; installerHooks = (Join-Path $PSScriptRoot 'nsis-mvp-hooks.nsh').Replace('\', '/') }
            }
        }
        plugins = @{ updater = $null }
    }
    [IO.File]::WriteAllText($configPath, ($config | ConvertTo-Json -Depth 8), [Text.UTF8Encoding]::new($false))
    $bundleStarted = [DateTime]::UtcNow
    $bundleArgs = @('tauri', 'bundle', '--bundles', 'nsis', '--no-sign', '--ci', '--config', $configPath,
        '--features', 'tauri/custom-protocol')
    if ($Configuration -eq 'debug') { $bundleArgs += '--debug' }
    Invoke-Checked 'cargo' $bundleArgs (Join-Path $repoRoot 'hive-app')

    $installers = @(Get-ChildItem (Join-Path $binaryDir 'bundle/nsis') -Filter '*.exe' -File |
        Where-Object { $_.LastWriteTimeUtc -ge $bundleStarted.AddSeconds(-2) })
    if ($installers.Count -ne 1) { throw "Expected exactly one new NSIS installer; found $($installers.Count)." }
    New-Item -ItemType Directory -Force -Path $outputRoot | Out-Null
    $installerPath = Join-Path $outputRoot 'Abigail-windows-x64-setup.exe'
    Copy-Item -LiteralPath $installers[0].FullName -Destination $installerPath -Force
    $signatureStatus = (Get-AuthenticodeSignature -LiteralPath $installerPath).Status.ToString()
    if ($signatureStatus -ne 'NotSigned') { throw "Unsigned MVP installer has unexpected signature status: $signatureStatus" }

    $payload = @([ordered]@{
        name = 'Abigail'; relative_path = 'Abigail.exe'; source_path = $mainBinary
        source_sha256 = (Get-FileHash -LiteralPath $mainBinary -Algorithm SHA256).Hash
        bundle_transform = 'unique __TAURI_BUNDLE_TYPE_VAR_UNK marker -> __TAURI_BUNDLE_TYPE_VAR_NSS'
        sha256 = $expectedLauncherHash
    })
    foreach ($binary in $internalBinaries) {
        $source = Join-Path $resourceDir $binary
        $payload += [ordered]@{
            name = $binary; relative_path = "resources/$binary"; source_path = $source
            sha256 = (Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash
        }
    }
    $manifestPath = Join-Path $outputRoot 'mvp-build.json'
    $manifest = [ordered]@{
        version = $Version; configuration = $Configuration; nsis_compression = $Compression
        signed = $false; installer_signature_status = $signatureStatus
        created_at_utc = [DateTime]::UtcNow.ToString('o'); installer_path = $installerPath
        installer_sha256 = (Get-FileHash -LiteralPath $installerPath -Algorithm SHA256).Hash
        cargo_target_dir = $targetRoot; config_override = $configPath; payload = $payload; frontends = $frontends
    }
    [IO.File]::WriteAllText($manifestPath, ($manifest | ConvertTo-Json -Depth 8), [Text.UTF8Encoding]::new($false))
    if ($VerifyInstall) {
        & (Join-Path $PSScriptRoot 'verify-mvp-windows.ps1') -InstallerPath $installerPath -ManifestPath $manifestPath
    }
    Write-Host "Installer: $installerPath"
    Write-Host "Build manifest: $manifestPath"
} finally {
    $env:CARGO_TARGET_DIR = $previousTargetDir
    $env:TAURI_CONFIG = $previousTauriConfig
}
