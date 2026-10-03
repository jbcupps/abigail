<#
.SYNOPSIS
Verify an MVP installer by installing and checking its real payload.
.DESCRIPTION
Uses an isolated install directory and compares all four executables with the
build manifest. Inspects the installed PE imports to reject undeployed C++
runtime dependencies, then runs both daemons with --help to check the loader.
This check leaves the installed files and logs available for runtime acceptance;
it does not launch Hive or open the user's personal data.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$InstallerPath,
    [string]$ManifestPath,
    [string]$InstallDir
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if ($env:OS -ne 'Windows_NT') { throw 'Installer verification requires Windows.' }
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$installer = (Resolve-Path -LiteralPath $InstallerPath).Path
if (-not $ManifestPath) { $ManifestPath = Join-Path (Split-Path $installer -Parent) 'mvp-build.json' }
$manifest = Get-Content -LiteralPath $ManifestPath -Raw | ConvertFrom-Json
$expectedPayload = @('Abigail.exe', 'resources/abigail-entity-runtime-app.exe', 'resources/hive-daemon.exe', 'resources/entity-daemon.exe')
if (@($manifest.payload).Count -ne 4 -or
    (@($manifest.payload.relative_path | Sort-Object) -join '|') -ne (($expectedPayload | Sort-Object) -join '|')) {
    throw 'Installer verification requires the four exact Abigail executable paths.'
}
if ((Get-FileHash -LiteralPath $installer -Algorithm SHA256).Hash -ne $manifest.installer_sha256) {
    throw 'Installer SHA256 differs from the build manifest.'
}
if (-not $InstallDir) {
    $InstallDir = Join-Path $repoRoot ('target/manual-test/mvp-installed-' + [guid]::NewGuid().ToString('N').Substring(0, 8))
}
if (-not [IO.Path]::IsPathRooted($InstallDir)) { $InstallDir = Join-Path $repoRoot $InstallDir }
$InstallDir = [IO.Path]::GetFullPath($InstallDir)
if (Test-Path -LiteralPath $InstallDir) { throw "Verification requires a fresh directory: $InstallDir" }
# NSIS can uninstall a registered prior version before honoring a new /D path.
# Verification may replace only a prior isolated acceptance installation.
$acceptanceRoot = [IO.Path]::GetFullPath((Join-Path $repoRoot 'target/manual-test'))
$acceptancePrefix = $acceptanceRoot.TrimEnd([IO.Path]::DirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar
foreach ($registryRoot in @('HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall',
    'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall',
    'HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall')) {
    if (-not (Test-Path -LiteralPath $registryRoot)) { continue }
    foreach ($key in Get-ChildItem -LiteralPath $registryRoot) {
        $entry = Get-ItemProperty -LiteralPath $key.PSPath
        $display = $entry.PSObject.Properties['DisplayName']
        if (-not $display -or $display.Value -ne 'Abigail') { continue }
        $location = $entry.PSObject.Properties['InstallLocation']
        $previousInstall = if ($location -and $location.Value) { [IO.Path]::GetFullPath($location.Value.Trim().Trim('"')) } else { $null }
        if (-not $previousInstall -or -not $previousInstall.StartsWith($acceptancePrefix, [StringComparison]::OrdinalIgnoreCase)) {
            throw "Installer verification cannot replace an existing Abigail installation outside $acceptanceRoot. Existing location: $previousInstall"
        }
    }
}
$logDir = "$InstallDir-verification"
New-Item -ItemType Directory -Force -Path $logDir | Out-Null

# NSIS requires /D last and its path must not be quoted, even with spaces.
$process = Start-Process -FilePath $installer -ArgumentList @('/S', "/D=$InstallDir") -Wait -PassThru -WindowStyle Hidden
if ($process.ExitCode -ne 0) { throw "NSIS installation exited with code $($process.ExitCode)." }

$checks = @(foreach ($item in $manifest.payload) {
    $installedPath = Join-Path $InstallDir $item.relative_path
    if (-not (Test-Path -LiteralPath $installedPath)) {
        throw "Required installed payload missing: $installedPath"
    }
    $actualHash = (Get-FileHash -LiteralPath $installedPath -Algorithm SHA256).Hash
    if ($actualHash -ne $item.sha256) { throw "Installed binary SHA256 mismatch: $installedPath" }
    [ordered]@{ path = $installedPath; sha256 = $actualHash; matches_build = $true }
})
$runtimeReportPath = Join-Path $logDir 'windows-runtime.json'
$runtimePaths = @($checks | ForEach-Object { $_.path })
& node (Join-Path $PSScriptRoot 'check_windows_runtime.mjs') --report $runtimeReportPath @runtimePaths
if ($LASTEXITCODE -ne 0) { throw 'Installed executables failed standalone Windows runtime verification.' }
$runtimeReport = Get-Content -LiteralPath $runtimeReportPath -Raw | ConvertFrom-Json
if ($runtimeReport.schema_version -ne 1 -or $runtimeReport.passed -ne $true -or
    $runtimeReport.architecture -ne 'x64' -or @($runtimeReport.executables).Count -ne 4) {
    throw 'Installed Windows runtime evidence has an invalid schema or executable count.'
}
$runtimeReportedPaths = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
foreach ($runtimeExecutable in $runtimeReport.executables) {
    $runtimePath = [IO.Path]::GetFullPath($runtimeExecutable.path)
    if (-not $runtimeReportedPaths.Add($runtimePath)) { throw 'Installed Windows runtime evidence repeats an executable.' }
    $matchingPayload = @($checks | Where-Object { [IO.Path]::GetFullPath($_.path) -eq $runtimePath })
    if ($matchingPayload.Count -ne 1 -or $matchingPayload[0].sha256 -ne $runtimeExecutable.sha256 -or
        $runtimeExecutable.requires_external_vc_runtime -ne $false) {
        throw 'Installed Windows runtime evidence differs from the verified payload bytes.'
    }
}
foreach ($binary in @('hive-daemon.exe', 'entity-daemon.exe')) {
    $exe = Join-Path $InstallDir "resources/$binary"
    $stdout = Join-Path $logDir "$binary.stdout.txt"
    $stderr = Join-Path $logDir "$binary.stderr.txt"
    $previousLocalAppData = $env:LOCALAPPDATA
    try {
        # Daemon logging initializes even for --help; keep those logs isolated.
        $env:LOCALAPPDATA = Join-Path $logDir 'local-app-data'
        $probe = Start-Process -FilePath $exe -ArgumentList '--help' -PassThru -WindowStyle Hidden `
            -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    } finally { $env:LOCALAPPDATA = $previousLocalAppData }
    if (-not $probe.WaitForExit(30000)) {
        $probe.Kill()
        throw "Installed $binary did not finish its --help loader check."
    }
    if ($probe.ExitCode -ne 0) { throw "Installed $binary loader check failed. See $stderr" }
}
$reportPath = Join-Path $logDir 'installed-payload.json'
$report = [ordered]@{
    verified_at_utc = [DateTime]::UtcNow.ToString('o'); installer = $installer
    install_dir = $InstallDir; payload = $checks; daemon_loader_checks = 'passed'
    windows_runtime = $runtimeReport
    runtime_acceptance = 'pending; this check does not launch the application'
}
[IO.File]::WriteAllText($reportPath, ($report | ConvertTo-Json -Depth 8), [Text.UTF8Encoding]::new($false))
Write-Host "Installed payload verified: $InstallDir"
Write-Host "Verification report: $reportPath"
