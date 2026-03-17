# Build a signed Windows release locally on a Windows machine.
#
# Example using an SSL.com certificate already available in the Windows cert store:
#   $env:ABIGAIL_WINDOWS_SIGNING_MODE = "store"
#   $env:WINDOWS_CERTIFICATE_THUMBPRINT = "YOUR_CERT_THUMBPRINT"
#   $env:WINDOWS_TIMESTAMP_URL = "http://ts.ssl.com"
#   .\scripts\build-release-windows.ps1 -Version 0.0.3 -OpenOutput
#
# Optional updater signing:
#   $env:TAURI_SIGNING_PRIVATE_KEY = "<minisign secret box or base64>"
#   $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = "<password>"
#
# Optional latest.json generation:
#   .\scripts\build-release-windows.ps1 -Version 0.0.3 `
#     -ReleaseBaseUrl "https://github.com/OWNER/REPO/releases/download/v0.0.3"

param(
    [string]$Version,
    [string]$OutputDir,
    [string]$ReleaseBaseUrl,
    [ValidateSet("off", "pfx", "store")]
    [string]$WindowsSigningMode,
    [string]$WindowsCertificateThumbprint,
    [string]$WindowsTimestampUrl,
    [string]$OllamaVersion = "v0.5.13",
    [switch]$SkipWindowsSigning,
    [switch]$RequireUpdaterSigning,
    [switch]$OpenOutput,
    [switch]$DryRun
)

$ErrorActionPreference = "Stop"

function Get-RepoRoot {
    if ($PSScriptRoot) {
        return (Get-Item $PSScriptRoot).Parent.FullName
    }

    return (Get-Location).Path
}

function Resolve-RepoPath {
    param(
        [string]$RepoRoot,
        [string]$PathValue
    )

    if ([string]::IsNullOrWhiteSpace($PathValue)) {
        return $null
    }

    if ([IO.Path]::IsPathRooted($PathValue)) {
        return [IO.Path]::GetFullPath($PathValue)
    }

    return [IO.Path]::GetFullPath((Join-Path $RepoRoot $PathValue))
}

function Test-CommandExists {
    param([string]$CommandName)

    return $null -ne (Get-Command $CommandName -ErrorAction SilentlyContinue)
}

function Add-DirectoryToPath {
    param([string]$DirectoryPath)

    if ([string]::IsNullOrWhiteSpace($DirectoryPath) -or -not (Test-Path $DirectoryPath)) {
        return
    }

    $currentEntries = ($env:PATH -split ';') | Where-Object { -not [string]::IsNullOrWhiteSpace($_) }
    if ($currentEntries -contains $DirectoryPath) {
        return
    }

    $env:PATH = "$DirectoryPath;$env:PATH"
}

function Get-WindowsSdkSignToolPath {
    $signTool = Get-ChildItem "C:\Program Files (x86)\Windows Kits\10\bin" -Recurse -Filter "signtool.exe" -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -match '\\x64\\' } |
        Sort-Object FullName -Descending |
        Select-Object -First 1

    if ($null -eq $signTool) {
        return $null
    }

    return $signTool.FullName
}

function Ensure-WindowsBuildTooling {
    param(
        [bool]$SigningEnabled,
        [string]$RepoRoot
    )

    $nsisCandidates = @(
        (Join-Path $RepoRoot ".tools\nsis"),
        "C:\Program Files (x86)\NSIS",
        "C:\Program Files\NSIS",
        "C:\ProgramData\chocolatey\bin"
    )
    foreach ($candidate in $nsisCandidates) {
        Add-DirectoryToPath -DirectoryPath $candidate
    }

    if (-not (Test-CommandExists "makensis")) {
        Write-Host "makensis.exe not found on PATH. Tauri can download NSIS during bundling if needed."
    }

    if (-not $SigningEnabled) {
        return $null
    }

    $signToolPath = Get-WindowsSdkSignToolPath
    if ([string]::IsNullOrWhiteSpace($signToolPath)) {
        throw "signtool.exe was not found under the Windows SDK install path."
    }

    Add-DirectoryToPath -DirectoryPath ([IO.Path]::GetDirectoryName($signToolPath))
    return $signToolPath
}

function Invoke-Native {
    param(
        [string]$Description,
        [string]$Command,
        [string[]]$Arguments = @(),
        [string]$WorkingDirectory
    )

    Write-Host "==> $Description"
    $originalLocation = Get-Location

    try {
        if ($WorkingDirectory) {
            Set-Location $WorkingDirectory
        }

        & $Command @Arguments
        if ($LASTEXITCODE -ne 0) {
            throw "$Description failed with exit code $LASTEXITCODE."
        }
    }
    finally {
        Set-Location $originalLocation
    }
}

function Normalize-Thumbprint {
    param([string]$Value)

    if ([string]::IsNullOrWhiteSpace($Value)) {
        return ""
    }

    return ($Value.Trim().ToUpperInvariant() -replace "[^0-9A-F]", "")
}

function Resolve-WindowsSigningModeValue {
    param(
        [bool]$SkipSigning,
        [string]$ExplicitMode,
        [string]$Thumbprint
    )

    if ($SkipSigning) {
        return "off"
    }

    if (-not [string]::IsNullOrWhiteSpace($ExplicitMode)) {
        return $ExplicitMode
    }

    $envMode = [string]$env:ABIGAIL_WINDOWS_SIGNING_MODE
    if (-not [string]::IsNullOrWhiteSpace($envMode)) {
        return $envMode.Trim().ToLowerInvariant()
    }

    if (-not [string]::IsNullOrWhiteSpace($env:WINDOWS_SIGNING_CERT_BASE64) -or
        -not [string]::IsNullOrWhiteSpace($env:WINDOWS_SIGNING_CERT_PASSWORD)) {
        return "pfx"
    }

    if (-not [string]::IsNullOrWhiteSpace($Thumbprint)) {
        return "store"
    }

    return "off"
}

function Get-TauriVersion {
    param(
        [string]$ConfigPath,
        [string]$RequestedVersion
    )

    if (-not [string]::IsNullOrWhiteSpace($RequestedVersion)) {
        return $RequestedVersion.Trim()
    }

    $config = Get-Content $ConfigPath -Raw | ConvertFrom-Json
    if ([string]::IsNullOrWhiteSpace($config.version)) {
        throw "Could not determine version from $ConfigPath."
    }

    return [string]$config.version
}

function Backup-FileState {
    param(
        [string]$Path,
        [string]$BackupRoot
    )

    $state = [ordered]@{
        Path      = $Path
        Exists    = Test-Path $Path
        Backup    = $null
    }

    if ($state.Exists) {
        $backupName = [IO.Path]::GetFileName($Path) + ".bak"
        $backupPath = Join-Path $BackupRoot $backupName
        Copy-Item $Path $backupPath -Force
        $state.Backup = $backupPath
    }

    return [pscustomobject]$state
}

function Restore-FileState {
    param([pscustomobject]$State)

    if ($State.Exists) {
        Copy-Item $State.Backup $State.Path -Force
        return
    }

    if (Test-Path $State.Path) {
        Remove-Item $State.Path -Force
    }
}

function Update-TauriConfigForWindowsRelease {
    param(
        [string]$ConfigPath,
        [string]$ReleaseVersion,
        [string]$InteractiveSignWrapperPath,
        [string]$SignToolPath,
        [string]$CertificateThumbprint,
        [string]$TimestampUrl
    )

    $config = Get-Content $ConfigPath -Raw | ConvertFrom-Json
    $config.version = $ReleaseVersion

    if ($null -eq $config.build) {
        $config | Add-Member -NotePropertyName build -NotePropertyValue ([pscustomobject]@{})
    }
    $config.build.beforeBuildCommand = ""

    if ($null -eq $config.bundle) {
        $config | Add-Member -NotePropertyName bundle -NotePropertyValue ([pscustomobject]@{})
    }
    $config.bundle.resources = @("abigail-keygen.exe", "ollama.exe")

    if ($null -eq $config.bundle.windows) {
        $config.bundle | Add-Member -NotePropertyName windows -NotePropertyValue ([pscustomobject]@{})
    }

    if (-not [string]::IsNullOrWhiteSpace($SignToolPath) -and
        -not [string]::IsNullOrWhiteSpace($InteractiveSignWrapperPath) -and
        -not [string]::IsNullOrWhiteSpace($CertificateThumbprint) -and
        -not [string]::IsNullOrWhiteSpace($TimestampUrl)) {
        $signCommandConfig = [pscustomobject]@{
            cmd  = "powershell"
            args = @(
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
                $InteractiveSignWrapperPath,
                "-SignToolPath",
                $SignToolPath,
                "-CertificateThumbprint",
                $CertificateThumbprint,
                "-TimestampUrl",
                $TimestampUrl,
                "%1"
            )
        }

        if ($null -eq $config.bundle.windows.PSObject.Properties["signCommand"]) {
            $config.bundle.windows | Add-Member -NotePropertyName signCommand -NotePropertyValue $signCommandConfig
        }
        else {
            $config.bundle.windows.signCommand = $signCommandConfig
        }
    }

    $json = $config | ConvertTo-Json -Depth 100
    $utf8NoBom = New-Object System.Text.UTF8Encoding($false)
    [IO.File]::WriteAllText($ConfigPath, $json + "`n", $utf8NoBom)
}

function Ensure-Directory {
    param([string]$Path)

    if (-not (Test-Path $Path)) {
        New-Item -ItemType Directory -Path $Path | Out-Null
    }
}

function Download-File {
    param(
        [string]$Url,
        [string]$DestinationPath
    )

    if (Test-CommandExists "curl.exe") {
        & curl.exe --fail --location --output $DestinationPath $Url
        if ($LASTEXITCODE -ne 0) {
            throw "curl.exe failed while downloading $Url."
        }

        if (-not (Test-Path $DestinationPath) -or (Get-Item $DestinationPath).Length -eq 0) {
            throw "curl.exe did not produce a valid download at $DestinationPath."
        }

        return
    }

    Invoke-WebRequest -Uri $Url -OutFile $DestinationPath
}

function Stage-OllamaBinary {
    param(
        [string]$RepoRoot,
        [string]$OllamaVersionValue,
        [string]$DestinationPath,
        [string]$TempRoot
    )

    $cachedZipPath = Join-Path $RepoRoot "tauri-app\ollama-windows-amd64.zip"
    $downloadZipPath = Join-Path $TempRoot "ollama-windows-amd64.zip"
    $downloadUrl = "https://github.com/ollama/ollama/releases/download/$OllamaVersionValue/ollama-windows-amd64.zip"
    $zipPath = $cachedZipPath

    if (-not (Test-Path $zipPath)) {
        $zipPath = $downloadZipPath
        Write-Host "Downloading Ollama from $downloadUrl"
        Download-File -Url $downloadUrl -DestinationPath $zipPath
    }
    else {
        Write-Host "Using cached Ollama archive at $zipPath"
    }

    $extractDir = Join-Path $TempRoot "ollama"
    if (Test-Path $extractDir) {
        Remove-Item $extractDir -Recurse -Force
    }

    try {
        Expand-Archive -Path $zipPath -DestinationPath $extractDir -Force
    }
    catch {
        if ($zipPath -ne $cachedZipPath) {
            throw
        }

        Write-Warning "Cached Ollama archive is invalid. Downloading a fresh copy."
        if (Test-Path $extractDir) {
            Remove-Item $extractDir -Recurse -Force
        }

        Download-File -Url $downloadUrl -DestinationPath $downloadZipPath
        Expand-Archive -Path $downloadZipPath -DestinationPath $extractDir -Force
    }

    $ollamaExe = Get-ChildItem $extractDir -Recurse -File -Filter "ollama.exe" |
        Select-Object -First 1

    if ($null -eq $ollamaExe) {
        throw "Could not locate ollama.exe inside $zipPath."
    }

    if ($ollamaExe.Length -lt 10000000) {
        throw "Downloaded Ollama binary is unexpectedly small ($($ollamaExe.Length) bytes)."
    }

    Copy-Item $ollamaExe.FullName $DestinationPath -Force
}

function Get-FirstMatchingFile {
    param(
        [string[]]$Paths,
        [string[]]$NamePatterns
    )

    foreach ($path in $Paths) {
        if (Test-Path $path) {
            $file = Get-ChildItem $path -File -ErrorAction SilentlyContinue |
                Where-Object {
                    $matchesPattern = $false
                    foreach ($pattern in $NamePatterns) {
                        if ($_.Name -like $pattern) {
                            $matchesPattern = $true
                            break
                        }
                    }

                    $matchesPattern
                } |
                Sort-Object LastWriteTime -Descending |
                Select-Object -First 1
            if ($null -ne $file) {
                return $file
            }
        }
    }

    return $null
}

function Find-TargetArtifact {
    param(
        [string[]]$SearchRoots,
        [string[]]$NamePatterns,
        [string]$RequiredPathFragment
    )

    foreach ($root in $SearchRoots) {
        if (-not (Test-Path $root)) {
            continue
        }

        $match = Get-ChildItem $root -Recurse -File -ErrorAction SilentlyContinue |
            Where-Object {
                $nameMatches = $false
                foreach ($pattern in $NamePatterns) {
                    if ($_.Name -like $pattern) {
                        $nameMatches = $true
                        break
                    }
                }

                $pathMatches = [string]::IsNullOrWhiteSpace($RequiredPathFragment) -or $_.FullName -like "*$RequiredPathFragment*"
                $nameMatches -and $pathMatches
            } |
            Sort-Object LastWriteTime -Descending |
            Select-Object -First 1

        if ($null -ne $match) {
            return $match
        }
    }

    return $null
}

function Copy-ArtifactIfPresent {
    param(
        [IO.FileInfo]$Source,
        [string]$DestinationPath
    )

    if ($null -eq $Source) {
        return $false
    }

    Copy-Item $Source.FullName $DestinationPath -Force
    return $true
}

$repoRoot = Get-RepoRoot
$tauriConfigPath = Join-Path $repoRoot "tauri-app\tauri.conf.json"
$tauriAppDir = Join-Path $repoRoot "tauri-app"
$frontendDir = Join-Path $repoRoot "tauri-app\src-ui"
$tempRoot = Join-Path ([IO.Path]::GetTempPath()) ("abigail-windows-release-" + [guid]::NewGuid().ToString("N"))
Ensure-Directory $tempRoot
$interactiveSignWrapperPath = Join-Path $repoRoot "scripts\windows_interactive_sign.ps1"

$configBackup = Backup-FileState -Path $tauriConfigPath -BackupRoot $tempRoot
$keygenPath = Join-Path $tauriAppDir "abigail-keygen.exe"
$ollamaPath = Join-Path $tauriAppDir "ollama.exe"
$keygenBackup = Backup-FileState -Path $keygenPath -BackupRoot $tempRoot
$ollamaBackup = Backup-FileState -Path $ollamaPath -BackupRoot $tempRoot

$thumbprint = Normalize-Thumbprint -Value $(if ($WindowsCertificateThumbprint) { $WindowsCertificateThumbprint } else { $env:WINDOWS_CERTIFICATE_THUMBPRINT })
$timestampUrl = if ($WindowsTimestampUrl) { $WindowsTimestampUrl } else { [string]$env:WINDOWS_TIMESTAMP_URL }
$resolvedSigningMode = Resolve-WindowsSigningModeValue -SkipSigning $SkipWindowsSigning.IsPresent -ExplicitMode $WindowsSigningMode -Thumbprint $thumbprint
$releaseVersion = Get-TauriVersion -ConfigPath $tauriConfigPath -RequestedVersion $Version

if ([string]::IsNullOrWhiteSpace($OutputDir)) {
    $resolvedOutputDir = Join-Path $repoRoot "release-assets\windows\$releaseVersion"
}
else {
    $resolvedOutputDir = Resolve-RepoPath -RepoRoot $repoRoot -PathValue $OutputDir
}

$updaterKeyPresent = -not [string]::IsNullOrWhiteSpace($env:TAURI_SIGNING_PRIVATE_KEY)
$updaterPasswordPresent = -not [string]::IsNullOrWhiteSpace($env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD)
$updaterSigningEnabled = $updaterKeyPresent -and $updaterPasswordPresent

if (($updaterKeyPresent -and -not $updaterPasswordPresent) -or (-not $updaterKeyPresent -and $updaterPasswordPresent)) {
    throw "TAURI_SIGNING_PRIVATE_KEY and TAURI_SIGNING_PRIVATE_KEY_PASSWORD must either both be set or both be empty."
}

if ($RequireUpdaterSigning.IsPresent -and -not $updaterSigningEnabled) {
    throw "Updater signing was required, but TAURI_SIGNING_PRIVATE_KEY/TAURI_SIGNING_PRIVATE_KEY_PASSWORD are not both configured."
}

if ($resolvedSigningMode -ne "off") {
    if ([string]::IsNullOrWhiteSpace($thumbprint)) {
        throw "WINDOWS_CERTIFICATE_THUMBPRINT is required when Windows signing is enabled."
    }

    if ([string]::IsNullOrWhiteSpace($timestampUrl)) {
        throw "WINDOWS_TIMESTAMP_URL is required when Windows signing is enabled."
    }
}

if (-not (Test-CommandExists "cargo")) {
    throw "cargo is not installed or not on PATH."
}

if (-not (Test-CommandExists "node")) {
    throw "node is not installed or not on PATH."
}

if (-not (Test-CommandExists "npm")) {
    throw "npm is not installed or not on PATH."
}

$signToolPath = Ensure-WindowsBuildTooling -SigningEnabled ($resolvedSigningMode -ne "off") -RepoRoot $repoRoot

Write-Host "Windows release version: $releaseVersion"
Write-Host "Windows signing mode: $resolvedSigningMode"
Write-Host "Updater signing enabled: $updaterSigningEnabled"
Write-Host "Output directory: $resolvedOutputDir"
if ($resolvedSigningMode -ne "off") {
    Write-Host "Interactive signing notice: enabled"
}

try {
    Invoke-Native -Description "Verify cargo-tauri is available" -Command "cargo" -Arguments @("tauri", "--version") -WorkingDirectory $repoRoot

    if ($resolvedSigningMode -ne "off") {
        $env:ABIGAIL_WINDOWS_SIGNING_MODE = $resolvedSigningMode
        $env:WINDOWS_CERTIFICATE_THUMBPRINT = $thumbprint
        $env:WINDOWS_TIMESTAMP_URL = $timestampUrl

        Invoke-Native -Description "Prepare Windows signing certificate" -Command "powershell" -Arguments @(
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            (Join-Path $repoRoot "scripts\windows_signing_preflight.ps1")
        ) -WorkingDirectory $repoRoot
    }

    if ($updaterSigningEnabled) {
        Write-Host "==> Validate Tauri updater signing key"
        $sanitizedKey = & node (Join-Path $repoRoot "scripts\validate_tauri_signing_key.mjs")
        if ($LASTEXITCODE -ne 0) {
            throw "Tauri updater signing key validation failed."
        }
        $env:TAURI_SIGNING_PRIVATE_KEY = ($sanitizedKey | Out-String).Trim()
    }

    Update-TauriConfigForWindowsRelease `
        -ConfigPath $tauriConfigPath `
        -ReleaseVersion $releaseVersion `
        -InteractiveSignWrapperPath $interactiveSignWrapperPath `
        -SignToolPath $signToolPath `
        -CertificateThumbprint $thumbprint `
        -TimestampUrl $timestampUrl

    $env:ABIGAIL_REQUIRE_UPDATER_PUBKEY = if ($updaterSigningEnabled) { "true" } else { "false" }
    $env:ABIGAIL_ENABLE_UPDATER_ARTIFACTS = if ($updaterSigningEnabled) { "true" } else { "false" }
    $env:ABIGAIL_WINDOWS_SIGNING_NOTICE_FILE = Join-Path $tempRoot "windows-signing-notified.marker"
    $env:ABIGAIL_WINDOWS_ESIGNER_PATH = "C:\Program Files (x86)\SSL Corp eSigner CKA\eSigner CKA.exe"

    Invoke-Native -Description "Inject updater and signing config" -Command "node" -Arguments @(
        (Join-Path $repoRoot "scripts\prepare_tauri_bundle_config.mjs"),
        $tauriConfigPath
    ) -WorkingDirectory $repoRoot

    if ($DryRun.IsPresent) {
        Write-Host "Dry run complete. Config and signing prerequisites validated."
        return
    }

    Invoke-Native -Description "Install frontend dependencies" -Command "npm" -Arguments @("ci") -WorkingDirectory $frontendDir
    Invoke-Native -Description "Build frontend" -Command "npm" -Arguments @("run", "build") -WorkingDirectory $frontendDir
    Invoke-Native -Description "Build abigail-keygen" -Command "cargo" -Arguments @("build", "--release", "-p", "abigail-keygen") -WorkingDirectory $repoRoot

    $builtKeygenPath = Join-Path $repoRoot "target\release\abigail-keygen.exe"
    if (-not (Test-Path $builtKeygenPath)) {
        throw "Expected built abigail-keygen.exe at $builtKeygenPath."
    }
    Copy-Item $builtKeygenPath $keygenPath -Force

    Stage-OllamaBinary -RepoRoot $repoRoot -OllamaVersionValue $OllamaVersion -DestinationPath $ollamaPath -TempRoot $tempRoot

    Invoke-Native -Description "Build Windows installer" -Command "cargo" -Arguments @("tauri", "build") -WorkingDirectory $tauriAppDir

    Ensure-Directory $resolvedOutputDir

    $bundleSearchRoots = @(
        (Join-Path $repoRoot "target\release\bundle\nsis"),
        (Join-Path $repoRoot "tauri-app\target\release\bundle\nsis")
    )
    $msiSearchRoots = @(
        (Join-Path $repoRoot "target\release\bundle\msi"),
        (Join-Path $repoRoot "tauri-app\target\release\bundle\msi")
    )
    $targetSearchRoots = @(
        (Join-Path $repoRoot "target"),
        (Join-Path $repoRoot "tauri-app\target")
    )

    $setupExe = Get-FirstMatchingFile -Paths $bundleSearchRoots -NamePatterns @("*.exe")
    if ($null -eq $setupExe) {
        throw "Could not find the NSIS installer output."
    }
    Copy-Item $setupExe.FullName (Join-Path $resolvedOutputDir "Abigail-windows-x64-setup.exe") -Force

    $msiFile = Get-FirstMatchingFile -Paths $msiSearchRoots -NamePatterns @("*.msi")
    if ($null -ne $msiFile) {
        Copy-Item $msiFile.FullName (Join-Path $resolvedOutputDir "Abigail-windows-x64.msi") -Force
    }

    $windowsNsisUpdater = Find-TargetArtifact -SearchRoots $targetSearchRoots -NamePatterns @("*.nsis.zip") -RequiredPathFragment "\bundle\"
    $windowsNsisUpdaterSig = Find-TargetArtifact -SearchRoots $targetSearchRoots -NamePatterns @("*.nsis.zip.sig") -RequiredPathFragment "\bundle\"
    $windowsMsiUpdater = Find-TargetArtifact -SearchRoots $targetSearchRoots -NamePatterns @("*.msi.zip") -RequiredPathFragment "\bundle\"
    $windowsMsiUpdaterSig = Find-TargetArtifact -SearchRoots $targetSearchRoots -NamePatterns @("*.msi.zip.sig") -RequiredPathFragment "\bundle\"

    $hasUpdaterArtifacts = $false
    $hasUpdaterArtifacts = (Copy-ArtifactIfPresent -Source $windowsNsisUpdater -DestinationPath (Join-Path $resolvedOutputDir "Abigail-updater-windows-x64.nsis.zip")) -or $hasUpdaterArtifacts
    $hasUpdaterArtifacts = (Copy-ArtifactIfPresent -Source $windowsNsisUpdaterSig -DestinationPath (Join-Path $resolvedOutputDir "Abigail-updater-windows-x64.nsis.zip.sig")) -or $hasUpdaterArtifacts
    $hasUpdaterArtifacts = (Copy-ArtifactIfPresent -Source $windowsMsiUpdater -DestinationPath (Join-Path $resolvedOutputDir "Abigail-updater-windows-x64.msi.zip")) -or $hasUpdaterArtifacts
    $hasUpdaterArtifacts = (Copy-ArtifactIfPresent -Source $windowsMsiUpdaterSig -DestinationPath (Join-Path $resolvedOutputDir "Abigail-updater-windows-x64.msi.zip.sig")) -or $hasUpdaterArtifacts

    if ($updaterSigningEnabled -and -not $hasUpdaterArtifacts) {
        throw "Updater signing was enabled, but no Windows updater artifacts were produced."
    }

    if (-not [string]::IsNullOrWhiteSpace($ReleaseBaseUrl) -and $hasUpdaterArtifacts) {
        Invoke-Native -Description "Generate latest.json" -Command "node" -Arguments @(
            (Join-Path $repoRoot "scripts\generate_tauri_latest_manifest.mjs"),
            "--version", $releaseVersion,
            "--assets-dir", $resolvedOutputDir,
            "--base-url", $ReleaseBaseUrl,
            "--output", (Join-Path $resolvedOutputDir "latest.json")
        ) -WorkingDirectory $repoRoot
    }

    Write-Host ""
    Write-Host "Windows release assets are ready in:"
    Write-Host "  $resolvedOutputDir"

    if ($OpenOutput.IsPresent) {
        Invoke-Item $resolvedOutputDir
    }
}
finally {
    Restore-FileState -State $ollamaBackup
    Restore-FileState -State $keygenBackup
    Restore-FileState -State $configBackup

    if (Test-Path $tempRoot) {
        Remove-Item $tempRoot -Recurse -Force
    }
}
