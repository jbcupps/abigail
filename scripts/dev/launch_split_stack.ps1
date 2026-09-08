param(
    [int]$HivePort = 43141,
    [string]$EntityName = "Stability Reset Test Entity",
    [switch]$SkipDesktopApps
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Get-WorkspaceRoot {
    return (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
}

function Get-DevTargetDir {
    if ($env:CARGO_TARGET_DIR -and $env:CARGO_TARGET_DIR.Trim()) {
        return $env:CARGO_TARGET_DIR
    }

    $localAppData = [Environment]::GetFolderPath("LocalApplicationData")
    return Join-Path $localAppData "Abigail\cargo-target"
}

function Wait-HttpOk {
    param(
        [Parameter(Mandatory = $true)][string]$Url,
        [int]$TimeoutSeconds = 45
    )

    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        try {
            $response = Invoke-WebRequest -Uri $Url -UseBasicParsing -TimeoutSec 2
            if ($response.StatusCode -eq 200) {
                return
            }
        } catch {
        }
        Start-Sleep -Milliseconds 300
    }

    throw "Timed out waiting for $Url"
}

function Invoke-CargoOrThrow {
    param(
        [Parameter(Mandatory = $true)][string[]]$Arguments,
        [Parameter(Mandatory = $true)][string]$FailureMessage
    )

    & $script:cargo @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw $FailureMessage
    }
}

function Build-Frontend {
    param(
        [Parameter(Mandatory = $true)][string]$AppDir,
        [Parameter(Mandatory = $true)][string]$Label
    )

    $uiDir = Join-Path $AppDir "src-ui"
    if (-not (Test-Path (Join-Path $uiDir "package.json"))) {
        return
    }

    $npm = (Get-Command npm).Source
    Push-Location $uiDir
    try {
        if (-not (Test-Path "node_modules")) {
            Write-Host "Installing $Label frontend dependencies..."
            & $npm install
            if ($LASTEXITCODE -ne 0) { throw "npm install failed for $Label frontend." }
        }
        Write-Host "Building $Label frontend..."
        & $npm run build
        if ($LASTEXITCODE -ne 0) { throw "npm run build failed for $Label frontend." }
    } finally {
        Pop-Location
    }
}

$workspaceRoot = Get-WorkspaceRoot
$sessionRoot = Join-Path $workspaceRoot "target\manual-test\stability-reset"
$logsRoot = Join-Path $sessionRoot "logs"
$dataRoot = Join-Path $sessionRoot "data"
$sessionPath = Join-Path $sessionRoot "session.json"
$diagnosticPath = Join-Path $sessionRoot "policy-diagnostic.json"
$stopScript = Join-Path $PSScriptRoot "stop_split_stack.ps1"
$hiveUrl = "http://127.0.0.1:$HivePort"
$runtimeUrl = $null
$harnessUrl = $null
$targetDir = Get-DevTargetDir

New-Item -ItemType Directory -Force -Path $logsRoot | Out-Null
New-Item -ItemType Directory -Force -Path $dataRoot | Out-Null
New-Item -ItemType Directory -Force -Path $targetDir | Out-Null

if (Test-Path $sessionPath) {
    & $stopScript -SessionPath $sessionPath -Quiet
}

$env:CARGO_TARGET_DIR = $targetDir
$script:cargo = (Get-Command cargo).Source
$node = (Get-Command node).Source

Write-Host "Using CARGO_TARGET_DIR=$targetDir"
Write-Host "Building Hive and Entity daemons..."
Invoke-CargoOrThrow -Arguments @("build", "-p", "hive-daemon", "-p", "entity-daemon") -FailureMessage "Failed to build Hive and Entity daemons."

$desktopBuildSucceeded = $SkipDesktopApps.IsPresent
$desktopBuildError = $null
$diagnosticSummary = $null

if (-not $SkipDesktopApps) {
    try {
        Build-Frontend -AppDir (Join-Path $workspaceRoot "hive-app") -Label "Abigail Hive"
        Build-Frontend -AppDir (Join-Path $workspaceRoot "entity-runtime-app") -Label "Abigail Entity Runtime"
        Write-Host "Building Abigail Hive desktop shell..."
        Invoke-CargoOrThrow -Arguments @("build", "-p", "abigail-hive-app", "--bin", "abigail-hive-app") -FailureMessage "Failed to build Abigail Hive desktop shell."
        Write-Host "Building Abigail Entity Runtime desktop shell..."
        Invoke-CargoOrThrow -Arguments @("build", "-p", "abigail-entity-runtime-app", "--bin", "abigail-entity-runtime-app") -FailureMessage "Failed to build Abigail Entity Runtime desktop shell."
        $desktopBuildSucceeded = $true
    } catch {
        $desktopBuildError = $_
        Write-Warning "Desktop build failed; collecting Windows policy diagnostics."
        try {
            $diagnosticSummary = & (Join-Path $workspaceRoot "scripts\diagnose_windows_build_policy.ps1") `
                -OutputPath $diagnosticPath `
                -TargetDir $targetDir |
                ConvertFrom-Json
        } catch {
            if (Test-Path $diagnosticPath) {
                $diagnosticSummary = Get-Content $diagnosticPath | ConvertFrom-Json
            }
        }
    }
}

$hiveExe = Join-Path $targetDir "debug\hive-daemon.exe"
$runtimeExe = Join-Path $targetDir "debug\entity-daemon.exe"
$hiveAppExe = Join-Path $targetDir "debug\abigail-hive-app.exe"
$runtimeAppExe = Join-Path $targetDir "debug\abigail-entity-runtime-app.exe"

$hiveOut = Join-Path $logsRoot "hive-daemon.out.log"
$hiveErr = Join-Path $logsRoot "hive-daemon.err.log"
$runtimeOut = Join-Path $logsRoot "entity-daemon.out.log"
$runtimeErr = Join-Path $logsRoot "entity-daemon.err.log"
$harnessOut = Join-Path $logsRoot "browser-harness.out.log"
$harnessErr = Join-Path $logsRoot "browser-harness.err.log"

$callerToken = [Guid]::NewGuid().ToString('N') + [Guid]::NewGuid().ToString('N')
$bootstrapDir = Join-Path $workspaceRoot 'hive-app/resources/bootstrap'
if (-not (Test-Path (Join-Path $bootstrapDir 'bundle.json'))) { & (Join-Path $workspaceRoot 'scripts/stage_offline_bootstrap.ps1') }
Write-Host "Starting Abigail daemon..."
$hiveProc = Start-Process -FilePath $hiveExe -WindowStyle Hidden `
    -Environment @{ ABIGAIL_LOCAL_AUTH_TOKEN = $callerToken; ABIGAIL_BOOTSTRAP_DIR = $bootstrapDir } `
    -ArgumentList @("--port", "$HivePort", "--data-dir", $dataRoot) `
    -WorkingDirectory $workspaceRoot `
    -RedirectStandardOutput $hiveOut `
    -RedirectStandardError $hiveErr `
    -PassThru
Wait-HttpOk -Url "$hiveUrl/health" -TimeoutSeconds 30

Write-Host "Creating test entity..."
$entityResponse = Invoke-RestMethod -Method Post `
    -Uri "$hiveUrl/v1/entities" -Headers @{ Authorization = "Bearer $callerToken" } `
    -ContentType "application/json" `
    -Body (@{ name = $EntityName } | ConvertTo-Json)
if (-not $entityResponse.ok) {
    throw "Hive failed to create test entity."
}
$entityId = $entityResponse.data.id

Write-Host "Starting Entity Runtime through Abigail..."
$opened = Invoke-RestMethod -Method Post -Uri "$hiveUrl/v1/entities/$entityId/open" -Headers @{ Authorization = "Bearer $callerToken" }
if (-not $opened.ok) { throw "Could not open Entity: $($opened.error)" }
$runtimeUrl = $opened.data.local_url
$entityToken = $opened.data.auth_token
$runtimeProc = $null
Wait-HttpOk -Url "$runtimeUrl/health" -TimeoutSeconds 45

$browserHarnessProc = $null
$hiveAppProc = $null
$runtimeAppProc = $null
$mode = "desktop"

if ($desktopBuildSucceeded -and -not $SkipDesktopApps) {
    Write-Host "Launching Abigail desktop shell..."
    $hiveAppProc = Start-Process -FilePath $hiveAppExe `
        -WorkingDirectory (Join-Path $workspaceRoot "hive-app") `
        -Environment @{
            ABIGAIL_HIVE_URL = $hiveUrl
            ABIGAIL_LOCAL_AUTH_TOKEN = $callerToken
            ABIGAIL_BOOTSTRAP_DIR = $bootstrapDir
            CARGO_TARGET_DIR = $targetDir
        } `
        -PassThru

    Write-Host "Launching Abigail Entity Runtime desktop shell..."
    $runtimeAppProc = Start-Process -FilePath $runtimeAppExe `
        -WorkingDirectory (Join-Path $workspaceRoot "entity-runtime-app") `
        -Environment @{
            ABIGAIL_ENTITY_URL = $runtimeUrl
            ABIGAIL_ENTITY_AUTH_TOKEN = $entityToken
            CARGO_TARGET_DIR = $targetDir
        } `
        -PassThru
} else {
    $mode = "daemons_only"
    Write-Warning "Desktop shells are unavailable. Daemons are running; see the session file. For disposable browser setup checks, use python scripts/test_initial_setup.py --ui."
}

$session = [ordered]@{
    mode = $mode
    target_dir = $targetDir
    hive_url = $hiveUrl
    runtime_url = $runtimeUrl
    browser_harness_url = if ($browserHarnessProc) { $harnessUrl } else { $null }
    entity_id = $entityId
    data_dir = $dataRoot
    session_path = $sessionPath
    diagnostic_path = if (Test-Path $diagnosticPath) { $diagnosticPath } else { $null }
    desktop_build_succeeded = $desktopBuildSucceeded
    desktop_build_error = if ($desktopBuildError) { $desktopBuildError.Exception.Message } else { $null }
    hive_daemon_pid = $hiveProc.Id
    entity_daemon_pid = if ($runtimeProc) { $runtimeProc.Id } else { $null }
    hive_app_pid = if ($hiveAppProc) { $hiveAppProc.Id } else { $null }
    entity_app_pid = if ($runtimeAppProc) { $runtimeAppProc.Id } else { $null }
    browser_harness_pid = if ($browserHarnessProc) { $browserHarnessProc.Id } else { $null }
    hive_stdout = $hiveOut
    hive_stderr = $hiveErr
    runtime_stdout = $runtimeOut
    runtime_stderr = $runtimeErr
    browser_harness_stdout = if ($browserHarnessProc) { $harnessOut } else { $null }
    browser_harness_stderr = if ($browserHarnessProc) { $harnessErr } else { $null }
    launched_at_utc = [DateTime]::UtcNow.ToString("o")
}

if ($diagnosticSummary) {
    $session.policy_diagnostic = $diagnosticSummary
}

$sessionJson = $session | ConvertTo-Json -Depth 8
$sessionJson | Set-Content -Path $sessionPath
$sessionJson
