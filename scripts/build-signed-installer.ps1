<#
.SYNOPSIS
Build the split Abigail Windows installer using an existing SSL.com certificate.
.DESCRIPTION
Uses the Windows certificate store (eSigner CKA or a hardware token), signs
internal resources before packaging, and verifies every shipped signature.
DryRun checks prerequisites without building, signing, or publishing anything.
#>
param(
    [Parameter(Mandatory)][ValidatePattern('^\d+\.\d+\.\d+$')][string]$Version,
    [string]$CertificateThumbprint = $env:WINDOWS_CERTIFICATE_THUMBPRINT,
    [string]$TimestampUrl = 'http://ts.ssl.com',
    [switch]$NoNotice,
    [switch]$DryRun
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'windows_signing_common.ps1')
$repoRoot = (Get-Item $PSScriptRoot).Parent.FullName
$thumbprint = ConvertTo-WindowsSigningThumbprint $CertificateThumbprint
$certificate = Resolve-WindowsSigningCertificate $thumbprint
$signTool = Resolve-WindowsSignToolPath
foreach ($command in @('cargo', 'node', 'npm')) {
    if (-not (Get-Command $command -ErrorAction SilentlyContinue)) { throw "$command is required on PATH." }
}
& cargo tauri --version
if ($LASTEXITCODE -ne 0) { throw 'Install Tauri CLI v2 (cargo install tauri-cli --version "^2" --locked).' }
Write-Host "Signing publisher: $($certificate.Certificate.Subject)"
Write-Host "Certificate store: $($certificate.StoreLocation)"
Write-Host "Timestamp authority: $TimestampUrl"
# Validate timestamp format before spending time building.
$null = Get-WindowsSignArguments -Thumbprint $thumbprint -StoreLocation $certificate.StoreLocation -TimestampUrl $TimestampUrl -Files @('preflight.exe')
if ($DryRun) {
    Write-Host 'Preflight passed. Private-key access, provider authorization, and online trust are proven only by an actual signing operation.'
    return
}

$runId = [DateTime]::UtcNow.ToString('yyyyMMddTHHmmssZ') + '-' + [guid]::NewGuid().ToString('N').Substring(0, 8)
$workDir = Join-Path $repoRoot "artifacts\windows-signing\$runId"
$outputDir = Join-Path $repoRoot "release-assets\windows\$Version\$runId"
New-Item -ItemType Directory -Force -Path $workDir | Out-Null
$configPath = Join-Path $workDir 'tauri-signing.json'
$config = @{
    version = $Version
    build = @{ beforeBuildCommand = '' }
    bundle = @{
        active = $true
        targets = @('nsis', 'msi')
        createUpdaterArtifacts = $false
        resources = @('resources/abigail-entity-runtime-app.exe', 'resources/hive-daemon.exe', 'resources/entity-daemon.exe')
        windows = @{ digestAlgorithm = 'sha256'; timestampUrl = $TimestampUrl; tsp = $true }
    }
}
[IO.File]::WriteAllText($configPath, ($config | ConvertTo-Json -Depth 10), [Text.UTF8Encoding]::new($false))
Set-WindowsSigningCommand -ConfigPath $configPath -Thumbprint $thumbprint -TimestampUrl $TimestampUrl -NoNotice:$NoNotice

$previousTargetDir = $env:CARGO_TARGET_DIR
$targetRoot = if ([string]::IsNullOrWhiteSpace($previousTargetDir)) {
    Join-Path $repoRoot 'target'
} elseif ([IO.Path]::IsPathRooted($previousTargetDir)) {
    [IO.Path]::GetFullPath($previousTargetDir)
} else {
    [IO.Path]::GetFullPath((Join-Path $repoRoot $previousTargetDir))
}
# Cargo runs from both the repository root and hive-app; use one absolute path.
$env:CARGO_TARGET_DIR = $targetRoot
$previousNoticeFile = $env:ABIGAIL_WINDOWS_SIGNING_NOTICE_FILE
$env:ABIGAIL_WINDOWS_SIGNING_NOTICE_FILE = Join-Path $workDir 'notice.marker'
Push-Location $repoRoot
try {
    & (Join-Path $PSScriptRoot 'stage_split_installer_resources.ps1') -Configuration release
    $resources = @('abigail-entity-runtime-app.exe', 'hive-daemon.exe', 'entity-daemon.exe') |
        ForEach-Object { Join-Path $repoRoot "hive-app\resources\$_" }
    & (Join-Path $PSScriptRoot 'windows_interactive_sign.ps1') -SignToolPath $signTool `
        -CertificateThumbprint $thumbprint -TimestampUrl $TimestampUrl -NoNotice:$NoNotice -Files $resources

    # Reject stale bundles left by earlier builds in a shared Cargo target directory.
    $buildStarted = [DateTime]::UtcNow
    Push-Location (Join-Path $repoRoot 'hive-app')
    try {
        & cargo tauri build --config $configPath
        if ($LASTEXITCODE -ne 0) { throw 'Signed Abigail installer build failed.' }
    } finally { Pop-Location }
    $binaryDir = Join-Path $targetRoot 'release'
    $installers = @(foreach ($kind in @('nsis', 'msi')) {
        $extension = if ($kind -eq 'nsis') { '*.exe' } else { '*.msi' }
        $files = @(Get-ChildItem (Join-Path $binaryDir "bundle\$kind") -Filter $extension -File |
            Where-Object { $_.LastWriteTimeUtc -ge $buildStarted })
        if ($files.Count -ne 1) { throw "Expected one newly built $kind installer; found $($files.Count). No release assets copied." }
        $files[0]
    })
    $payload = @((Join-Path $binaryDir 'abigail-hive-app.exe')) + @($resources)
    & (Join-Path $PSScriptRoot 'verify_windows_signatures.ps1') -Files ($payload + @($installers.FullName)) `
        -CertificateThumbprint $thumbprint -ReportPath (Join-Path $workDir 'build-signatures.json')
    New-Item -ItemType Directory -Path $outputDir | Out-Null
    Copy-Item -LiteralPath $installers[0].FullName -Destination (Join-Path $outputDir 'Abigail-windows-x64-setup.exe')
    Copy-Item -LiteralPath $installers[1].FullName -Destination (Join-Path $outputDir 'Abigail-windows-x64.msi')
    $releaseFiles = @((Join-Path $outputDir 'Abigail-windows-x64-setup.exe'), (Join-Path $outputDir 'Abigail-windows-x64.msi'))
    & (Join-Path $PSScriptRoot 'verify_windows_signatures.ps1') -Files $releaseFiles `
        -CertificateThumbprint $thumbprint -ReportPath (Join-Path $outputDir 'windows-signatures.json')
    Copy-Item -LiteralPath (Join-Path $workDir 'build-signatures.json') -Destination $outputDir
    Write-Host "Verified signed installers: $outputDir"
    Write-Host 'Nothing has been published. Verify the installed payload and a browser download before distribution.'
} finally {
    Pop-Location
    $env:CARGO_TARGET_DIR = $previousTargetDir
    $env:ABIGAIL_WINDOWS_SIGNING_NOTICE_FILE = $previousNoticeFile
}
