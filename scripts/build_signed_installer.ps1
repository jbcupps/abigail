#requires -Version 7.0
<#
.SYNOPSIS
Prepare, check, and sign Abigail's current one-app Windows NSIS installer.
.DESCRIPTION
Prepare builds both embedded desktop shells and the two daemons without a
password or TOTP secret. Check validates the frozen source/configuration/bytes
and signing tools without signing. Sign uses automated SSL.com credentials,
verifies the actual installed payload, and runs the isolated synthetic contract.
No phase publishes a release. The existing unsigned installer lane is unchanged.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory)][ValidateSet('Prepare', 'Check', 'Sign')][string]$Phase,
    [ValidatePattern('^\d+\.\d+\.\d+$')][string]$Version = '0.0.75',
    [string]$TargetDir = $env:CARGO_TARGET_DIR,
    [string]$WorkDir,
    [ValidateSet('zlib', 'bzip2', 'lzma', 'none')][string]$Compression = 'zlib'
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$appRoot = Join-Path $repoRoot 'hive-app'
$hiveConfigPath = Join-Path $appRoot 'tauri.conf.json'
$resourceRoot = Join-Path $appRoot 'resources'

function Resolve-BuildPath([string]$Value, [string]$Fallback) {
    if ([string]::IsNullOrWhiteSpace($Value)) { $Value = $Fallback }
    if (-not [IO.Path]::IsPathFullyQualified($Value)) { $Value = Join-Path $repoRoot $Value }
    [IO.Path]::GetFullPath($Value)
}
function Write-BuildJson($Value, [string]$Path) {
    [IO.File]::WriteAllText($Path, ($Value | ConvertTo-Json -Depth 20), [Text.UTF8Encoding]::new($false))
}
function Get-TextHash([string]$Text) {
    [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($Text)))
}
function Invoke-Checked([string]$Command, [string[]]$Arguments, [string]$Directory) {
    Push-Location $Directory
    try {
        & $Command @Arguments
        if ($LASTEXITCODE -ne 0) { throw "$Command failed with exit code $LASTEXITCODE." }
    } finally { Pop-Location }
}
function Invoke-SourceGit([string[]]$Arguments) {
    $result = @(& git -C $repoRoot -c core.quotepath=false @Arguments)
    if ($LASTEXITCODE -ne 0) { throw 'Could not inspect the release source state.' }
    return $result
}
function Get-SourceState {
    # Tool downloads are separately pinned by the signer. They are not source
    # inputs; generated build/test artifacts are excluded by the repository.
    $scope = @('--', '.', ':(exclude).cache')
    $commit = [string](@(Invoke-SourceGit @('rev-parse', 'HEAD'))[0])
    if ($commit -notmatch '^[0-9a-fA-F]{40}$') { throw 'Expected the complete Git source commit.' }
    $status = @(Invoke-SourceGit (@('status', '--porcelain=v1', '--untracked-files=all') + $scope))
    $paths = [string[]]@(Invoke-SourceGit (@('ls-files', '--cached', '--others', '--exclude-standard') + $scope) | Sort-Object -Unique)
    [Array]::Sort($paths, [StringComparer]::Ordinal)
    $files = @(foreach ($path in $paths) {
        $absolute = Join-Path $repoRoot $path
        if (Test-Path -LiteralPath $absolute -PathType Leaf) {
            [ordered]@{ path = $path; sha256 = (Get-FileHash -LiteralPath $absolute -Algorithm SHA256).Hash }
        } else { [ordered]@{ path = $path; sha256 = $null } }
    })
    $snapshot = [ordered]@{ commit = $commit; status = $status; files = $files }
    [pscustomobject]@{ commit = $commit; sha256 = Get-TextHash ($snapshot | ConvertTo-Json -Depth 8 -Compress); snapshot = $snapshot }
}
function Resolve-TauriCli {
    foreach ($name in @('tauri.cmd', 'tauri')) {
        $command = Get-Command $name -ErrorAction SilentlyContinue
        if ($command) { return $command.Source }
    }
    $local = Join-Path $repoRoot '.cache/signing/tauri-cli/node_modules/.bin/tauri.cmd'
    if (Test-Path -LiteralPath $local) { return $local }
    throw 'Install the pinned npm Tauri CLI v2 before preparing or signing.'
}
function Read-FrontendAssets([string]$App) {
    $frontend = Join-Path $repoRoot "$App/src-ui/dist"
    $index = Get-Content -LiteralPath (Join-Path $frontend 'index.html') -Raw
    if ($index -match '(?:src|href)="/(?:assets|video)/' -or $index -match 'rel="stylesheet"') {
        throw "$App emitted paths or stylesheets incompatible with the packaged WebView."
    }
    $entries = @([regex]::Matches($index, 'src="\./([^"<>]+\.js)"') | ForEach-Object { $_.Groups[1].Value })
    if ($entries.Count -eq 0) { throw "$App has no packaged JavaScript entry." }
    foreach ($entry in $entries) {
        if (-not (Test-Path -LiteralPath (Join-Path $frontend $entry))) { throw "$App is missing $entry." }
    }
    $scripts = @(Get-ChildItem -LiteralPath (Join-Path $frontend 'assets') -Filter '*.js' -File)
    if (-not ($scripts | Select-String -SimpleMatch 'data-abigail-app-css' -List) -or
        -not ($scripts | Select-String -SimpleMatch '--color-primary' -List)) {
        throw "$App does not contain the packaged application stylesheet/theme."
    }
    [pscustomobject]@{ app = $App; javascript_entries = $entries }
}
function Assert-EmbeddedShell([string]$Path, $Frontend, [string]$ExpectedVersion) {
    if ((Get-Item -LiteralPath $Path).VersionInfo.ProductVersion -ne $ExpectedVersion) {
        throw "$Path does not embed the requested installer version."
    }
    $contents = [Text.Encoding]::ASCII.GetString([IO.File]::ReadAllBytes($Path))
    foreach ($entry in $Frontend.javascript_entries) {
        if (-not $contents.Contains($entry)) { throw "$Path does not embed current frontend entry $entry." }
    }
}
function Assert-PreparedState($State) {
    $currentSource = Get-SourceState
    if ($State.sourceCommit -ne $currentSource.commit -or $State.source.sha256 -ne $currentSource.sha256) {
        throw 'Source changed since preparation. Run Prepare again.'
    }
    if ($State.version -ne $Version -or $State.compression -ne $Compression) {
        throw 'Requested version or compression changed. Run Prepare again.'
    }
    if ($State.configSha256 -ne (Get-FileHash -LiteralPath $configPath -Algorithm SHA256).Hash) {
        throw 'Prepared configuration changed. Run Prepare again.'
    }
    $config = Get-Content -LiteralPath $configPath -Raw | ConvertFrom-Json
    if ($config.version -ne $Version -or $config.mainBinaryName -ne 'Abigail' -or
        $config.bundle.createUpdaterArtifacts -ne $false -or $config.bundle.targets.Count -ne 1 -or
        $config.bundle.targets[0] -ne 'nsis' -or
        ($config.bundle.resources -join '|') -ne ($resources -join '|')) {
        throw 'Prepared configuration is not the current one-app NSIS release.'
    }
    if (@($State.binaries).Count -ne 4) { throw 'Exactly four prepared executables are required.' }
    for ($index = 0; $index -lt 4; $index++) {
        $record = $State.binaries[$index]
        $expected = $binaryInputs[$index]
        if ($record.source_path -ne $expected.source_path -or $record.prepared_path -ne $expected.prepared_path -or
            $record.relative_path -ne $expected.relative_path -or $record.sha256 -notmatch '^[0-9A-Fa-f]{64}$') {
            throw 'Prepared executable paths/order are invalid. Run Prepare again.'
        }
        foreach ($path in @($expected.source_path, $expected.prepared_path)) {
            if ((Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash -ne $record.sha256) {
                throw 'Prepared executable bytes changed. Run Prepare again.'
            }
        }
    }
}

if ($env:OS -ne 'Windows_NT') { throw 'Cloud installer signing requires Windows.' }
$targetRoot = Resolve-BuildPath $TargetDir 'target'
$work = Resolve-BuildPath $WorkDir (Join-Path $targetRoot "signed-release/$Version")
$binaryRoot = Join-Path $targetRoot 'release'
$preparedRoot = Join-Path $work 'prepared'
$statePath = Join-Path $work 'build-state.json'
$configPath = Join-Path $work 'bundle-config.json'
$resources = @('resources/abigail-entity-runtime-app.exe', 'resources/hive-daemon.exe', 'resources/entity-daemon.exe')
$binaryInputs = @([pscustomobject]@{
    source_path = Join-Path $binaryRoot 'abigail-hive-app.exe'
    prepared_path = Join-Path $preparedRoot 'Abigail.exe'
    relative_path = 'Abigail.exe'
})
foreach ($resource in $resources) {
    $binaryInputs += [pscustomobject]@{
        source_path = Join-Path $appRoot $resource
        prepared_path = Join-Path $preparedRoot ([IO.Path]::GetFileName($resource))
        relative_path = $resource
    }
}
$previousTargetDir = $env:CARGO_TARGET_DIR
$previousTauriConfig = $env:TAURI_CONFIG
$previousSignedMainPath = $env:ABIGAIL_SIGNED_MAIN_PATH
$previousProcessPath = $env:PATH
$previousSignToolPath = $env:TAURI_WINDOWS_SIGNTOOL_PATH
$previousSigningEnvironment = @{}
$restoreFiles = @()
try {
    $env:CARGO_TARGET_DIR = $targetRoot
    if ($Phase -eq 'Prepare') {
        foreach ($name in @('ESIGNER_PASSWORD', 'ESIGNER_TOTP_SECRET')) {
            if ([Environment]::GetEnvironmentVariable($name)) { throw 'Prepare must run without signing passwords or TOTP secrets.' }
        }
        foreach ($command in @('git', 'cargo', 'node', 'npm.cmd')) {
            if (-not (Get-Command $command -ErrorAction SilentlyContinue)) { throw "$command is required on PATH." }
        }
        $tauriCli = Resolve-TauriCli
        Invoke-Checked $tauriCli @('--version') $repoRoot
        $source = Get-SourceState
        New-Item -ItemType Directory -Force -Path $preparedRoot | Out-Null
        $originalConfig = [IO.File]::ReadAllBytes($hiveConfigPath)
        try {
            # Stage reads Hive's version to compile both Tauri contexts. Apply
            # it before any frontend/native build, then restore exact bytes.
            $hiveConfig = [Text.Encoding]::UTF8.GetString($originalConfig) | ConvertFrom-Json -AsHashtable
            $hiveConfig.version = $Version
            $hiveConfig.mainBinaryName = 'Abigail'
            $hiveConfig.bundle.resources = @()
            $hiveConfig.bundle.windows.signCommand = $null
            $hiveConfig.bundle.windows.certificateThumbprint = $null
            Write-BuildJson $hiveConfig $hiveConfigPath
            $env:TAURI_CONFIG = $null
            & (Join-Path $PSScriptRoot 'stage_split_installer_resources.ps1') -Configuration release
        } finally {
            [IO.File]::WriteAllBytes($hiveConfigPath, $originalConfig)
            $env:TAURI_CONFIG = $previousTauriConfig
        }
        $frontends = @(foreach ($app in @('hive-app', 'entity-runtime-app')) { Read-FrontendAssets $app })
        Assert-EmbeddedShell $binaryInputs[0].source_path $frontends[0] $Version
        Assert-EmbeddedShell $binaryInputs[1].source_path $frontends[1] $Version
        $records = @(foreach ($binaryInput in $binaryInputs) {
            if ((Get-AuthenticodeSignature -LiteralPath $binaryInput.source_path).Status -ne 'NotSigned') {
                throw 'Prepare requires unsigned build outputs; do not reuse signed inputs.'
            }
            Copy-Item -LiteralPath $binaryInput.source_path -Destination $binaryInput.prepared_path -Force
            [ordered]@{
                source_path = $binaryInput.source_path; prepared_path = $binaryInput.prepared_path; relative_path = $binaryInput.relative_path
                sha256 = (Get-FileHash -LiteralPath $binaryInput.prepared_path -Algorithm SHA256).Hash
            }
        })
        $config = @{
            version = $Version; mainBinaryName = 'Abigail'
            build = @{ beforeBuildCommand = ''; beforeBundleCommand = '' }
            bundle = @{
                active = $true; targets = @('nsis'); createUpdaterArtifacts = $false; resources = $resources
                windows = @{
                    signCommand = $null; certificateThumbprint = $null; timestampUrl = $null; tsp = $false
                    nsis = @{ compression = $Compression; installerHooks = (Join-Path $PSScriptRoot 'nsis-mvp-hooks.nsh').Replace('\', '/') }
                }
            }
            plugins = @{ updater = $null }
        }
        Write-BuildJson $config $configPath
        if ((Get-SourceState).sha256 -ne $source.sha256) { throw 'Source changed during preparation. Run Prepare again.' }
        $state = [ordered]@{
            sourceCommit = $source.commit; source = $source; version = $Version; compression = $Compression
            preparedUtc = [DateTime]::UtcNow.ToString('o'); binaries = $records; frontends = $frontends
            configSha256 = (Get-FileHash -LiteralPath $configPath -Algorithm SHA256).Hash
        }
        Write-BuildJson $state $statePath
        Write-Host "Prepared four unsigned executables from $($source.commit). Signing credentials were not used."
        return
    }

    $state = Get-Content -LiteralPath $statePath -Raw | ConvertFrom-Json
    Assert-PreparedState $state
    $tauriCli = Resolve-TauriCli
    Import-Module (Join-Path $PSScriptRoot 'windows-signing.psm1') -Force
    $signTool = Get-AbigailSignTool
    $codeSignToolRoot = if ($env:ESIGNER_TOOL_PATH) { $env:ESIGNER_TOOL_PATH } else { Join-Path $PSScriptRoot '../.cache/signing/CodeSignTool' }
    $null = Get-AbigailCodeSignTool -ToolRoot $codeSignToolRoot
    # Tauri verifies resource signatures through its own SDK discovery. Its
    # resolver uses this explicit path rather than the process PATH alone.
    $env:TAURI_WINDOWS_SIGNTOOL_PATH = $signTool
    $env:PATH = (Split-Path $signTool) + [IO.Path]::PathSeparator + $previousProcessPath
    if ($Phase -eq 'Check') {
        Write-Host "Prepared source, configuration, four executables, and signing tools checked: $($state.sourceCommit)"
        return
    }
    foreach ($name in @('ESIGNER_USERNAME', 'ESIGNER_PASSWORD', 'ESIGNER_CREDENTIAL_ID', 'ESIGNER_TOTP_SECRET', 'WINDOWS_CERTIFICATE_THUMBPRINT')) {
        if ([string]::IsNullOrWhiteSpace([Environment]::GetEnvironmentVariable($name))) { throw "$name is required for automated cloud signing." }
    }
    if ($env:ABIGAIL_ESIGNER_AUTH_MODE -eq 'manual') { throw 'This lane supports automated cloud signing only.' }
    foreach ($name in @('ESIGNER_USERNAME', 'ESIGNER_PASSWORD', 'ESIGNER_CREDENTIAL_ID', 'ESIGNER_TOTP_SECRET')) {
        $previousSigningEnvironment[$name] = [Environment]::GetEnvironmentVariable($name)
    }
    $thumbprint = ConvertTo-AbigailThumbprint $env:WINDOWS_CERTIFICATE_THUMBPRINT
    $runId = [DateTime]::UtcNow.ToString('yyyyMMddTHHmmssZ') + '-' + [guid]::NewGuid().ToString('N').Substring(0, 8)
    $output = Join-Path $work "artifacts/$runId"
    $validationDir = Join-Path $output 'validation'
    $backupRoot = Join-Path $work "backups/$runId"
    New-Item -ItemType Directory -Force -Path $output, $validationDir, $backupRoot | Out-Null
    $mainPath = Join-Path $binaryRoot 'Abigail.exe'
    $destinations = @($mainPath) + @($binaryInputs | Select-Object -Skip 1 | ForEach-Object { $_.source_path })
    for ($index = 0; $index -lt 4; $index++) {
        $path = $destinations[$index]
        $exists = Test-Path -LiteralPath $path
        $backup = Join-Path $backupRoot ([IO.Path]::GetFileName($path))
        if ($exists) { Copy-Item -LiteralPath $path -Destination $backup }
        $restoreFiles += [pscustomobject]@{ path = $path; existed = $exists; backup = $backup }
        Copy-Item -LiteralPath $binaryInputs[$index].prepared_path -Destination $path -Force
    }
    $records = @()
    foreach ($path in ($destinations | Select-Object -Skip 1)) {
        & (Join-Path $PSScriptRoot 'sign_windows_artifact.ps1') -Path $path
        $records += Assert-AbigailSignature -Path $path -Thumbprint $thumbprint
    }
    $env:ABIGAIL_SIGNED_MAIN_PATH = Join-Path $output 'Abigail.exe'
    $config = Get-Content -LiteralPath $configPath -Raw | ConvertFrom-Json -AsHashtable
    $config.bundle.windows.signCommand = @{
        cmd = (Get-Command pwsh).Source
        args = @('-NoProfile', '-NonInteractive', '-File', (Join-Path $PSScriptRoot 'sign_windows_artifact.ps1'), '-Path', '%1')
    }
    $signingConfigPath = Join-Path $output 'bundle-signing.json'
    Write-BuildJson $config $signingConfigPath
    $env:TAURI_CONFIG = $null
    $bundleStarted = [DateTime]::UtcNow
    Invoke-Checked $tauriCli @('bundle', '--ci', '--bundles', 'nsis', '--config', $signingConfigPath, '--features', 'tauri/custom-protocol') $appRoot
    # Bundle callbacks are finished. Product loaders and the contract must not
    # inherit cloud signing account credentials or the OTP seed.
    foreach ($name in $previousSigningEnvironment.Keys) { [Environment]::SetEnvironmentVariable($name, $null) }
    $mainRecord = Assert-AbigailSignature -Path $env:ABIGAIL_SIGNED_MAIN_PATH -Thumbprint $thumbprint
    $records += $mainRecord
    $installers = @(Get-ChildItem -LiteralPath (Join-Path $binaryRoot 'bundle/nsis') -Filter '*.exe' -File |
        Where-Object { $_.LastWriteTimeUtc -ge $bundleStarted.AddSeconds(-2) })
    if ($installers.Count -ne 1) { throw 'Expected exactly one newly signed NSIS installer.' }
    $installerPath = Join-Path $output 'Abigail-windows-x64-setup.exe'
    Copy-Item -LiteralPath $installers[0].FullName -Destination $installerPath
    $installerRecord = Assert-AbigailSignature -Path $installerPath -Thumbprint $thumbprint
    $records += $installerRecord
    $payload = @([ordered]@{ name = 'Abigail'; relative_path = 'Abigail.exe'; sha256 = $mainRecord.sha256 })
    for ($index = 1; $index -lt 4; $index++) {
        $payload += [ordered]@{
            name = [IO.Path]::GetFileName($binaryInputs[$index].source_path)
            relative_path = $binaryInputs[$index].relative_path
            sha256 = (Get-FileHash -LiteralPath $binaryInputs[$index].source_path -Algorithm SHA256).Hash
        }
    }
    $manifestPath = Join-Path $output 'mvp-build.json'
    Write-BuildJson ([ordered]@{
        version = $Version; configuration = 'release'; signed = $true; source_commit = $state.sourceCommit
        source_state_sha256 = $state.source.sha256; created_at_utc = [DateTime]::UtcNow.ToString('o')
        installer_path = $installerPath; installer_sha256 = $installerRecord.sha256
        payload = $payload; frontends = $state.frontends
    }) $manifestPath
    Copy-Item -LiteralPath $manifestPath -Destination (Join-Path $validationDir 'mvp-build.json')
    $installDir = Join-Path $repoRoot "target/manual-test/signed-installed-$runId"
    & (Join-Path $PSScriptRoot 'verify-mvp-windows.ps1') -InstallerPath $installerPath -ManifestPath $manifestPath -InstallDir $installDir
    Copy-Item -LiteralPath "$installDir-verification/installed-payload.json" -Destination (Join-Path $validationDir 'installed-payload.json')
    foreach ($entry in $payload) {
        $path = Join-Path $installDir $entry.relative_path
        $record = Assert-AbigailSignature -Path $path -Thumbprint $thumbprint
        if ($record.sha256 -ne $entry.sha256) { throw 'Installed signed executable differs from the captured signed build.' }
        $records += $record
    }
    $records += Assert-AbigailSignature -Path (Join-Path $installDir 'uninstall.exe') -Thumbprint $thumbprint
    $contractOutput = Join-Path $validationDir 'contract'
    & (Join-Path $PSScriptRoot 'tests/run-mvp-acceptance.ps1') -BinaryDir $installDir -OutputDir $contractOutput
    $latestContract = Get-Content -LiteralPath (Join-Path $contractOutput 'latest.json') -Raw | ConvertFrom-Json
    $contract = Get-Content -LiteralPath (Join-Path $latestContract.run_dir 'result.json') -Raw | ConvertFrom-Json
    if (-not $contract.passed -or @($contract.stages).Count -ne 12 -or @($contract.stages | Where-Object { -not $_.passed }).Count) {
        throw 'Installed synthetic contract did not pass all twelve stages.'
    }
    $report = [ordered]@{
        sourceCommit = $state.sourceCommit; sourceStateSha256 = $state.source.sha256; version = $Version
        verifiedUtc = [DateTime]::UtcNow.ToString('o'); expectedThumbprint = $thumbprint; artifacts = $records
        installedDirectory = $installDir; installedContract = [ordered]@{ passed = $true; stages = 12; realModelValidation = $false }
    }
    Write-BuildJson $report (Join-Path $output 'signature-report.json')
    Write-BuildJson $report (Join-Path $output 'windows-signatures.json')
    Copy-Item -LiteralPath $statePath -Destination (Join-Path $output 'prepared-build-state.json')
    $latestSuccess = [ordered]@{
        source_commit = $state.sourceCommit; version = $Version; artifact_dir = $output; install_dir = $installDir
    }
    Write-BuildJson $latestSuccess (Join-Path $work 'latest-success.json')
    New-Item -ItemType Directory -Force -Path (Join-Path $targetRoot 'signed-release') | Out-Null
    Write-BuildJson $latestSuccess (Join-Path $targetRoot 'signed-release/latest-success.json')
    if ($env:GITHUB_OUTPUT) { "artifact_dir=$output" | Add-Content -LiteralPath $env:GITHUB_OUTPUT }
    Write-Host "Verified signed installer, installed executables/uninstaller, and twelve-stage contract: $output"
} finally {
    # Restore exactly the unsigned staging files and any pre-existing launcher.
    # A failed signing attempt does not invalidate the frozen preparation state.
    try {
        foreach ($entry in $restoreFiles) {
            if ($entry.existed) { Copy-Item -LiteralPath $entry.backup -Destination $entry.path -Force }
            elseif (Test-Path -LiteralPath $entry.path) { Remove-Item -LiteralPath $entry.path -Force }
        }
    } finally {
        $env:CARGO_TARGET_DIR = $previousTargetDir
        $env:TAURI_CONFIG = $previousTauriConfig
        $env:ABIGAIL_SIGNED_MAIN_PATH = $previousSignedMainPath
        $env:PATH = $previousProcessPath
        $env:TAURI_WINDOWS_SIGNTOOL_PATH = $previousSignToolPath
        foreach ($name in $previousSigningEnvironment.Keys) {
            [Environment]::SetEnvironmentVariable($name, $previousSigningEnvironment[$name])
        }
    }
}
