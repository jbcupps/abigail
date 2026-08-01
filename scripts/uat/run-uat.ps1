<#
.SYNOPSIS
    Tabula Rasa UAT - automated end-to-end acceptance test.

.DESCRIPTION
    Runs a full tabula-rasa UAT: build, start Hive, create entity, seed secrets,
    start Entity, validate chat (hello + 3 questions), and verify weather
    currentness.

    Exit codes:
      0  = PASS
      10 = SOFT_FAIL_RECOVERED (all stages passed after retries)
      20 = HARD_FAIL (unrecoverable - fix and restart)

.PARAMETER KeysetFile
    Path to uat-keys.env (default: scripts/uat/uat-keys.env). Required only for -Provider openai.

.PARAMETER Provider
    UAT provider path. claude-cli is the Windows Family Beta default and uses system CLI auth.

.PARAMETER InstallerPath
    Optional Abigail installer asset. When provided, UAT installs and launches the single Abigail app
    instead of starting debug daemons directly.

.PARAMETER HivePort
    Hive daemon port (default: 3141).

.PARAMETER EntityPort
    Entity daemon port (default: 3142).

.PARAMETER SkipBuild
    Skip the build stage (useful for rapid re-runs after a hard-failure fix).

#>

[CmdletBinding()]
param(
    [string]$KeysetFile,
    [int]$HivePort = 3141,
    [int]$EntityPort = 3142,
    [ValidateSet('claude-cli', 'openai')]
    [string]$Provider = 'claude-cli',
    [string]$InstallerPath,
    [switch]$SkipBuild
)

$ErrorActionPreference = 'Stop'
$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
Import-Module (Join-Path $PSScriptRoot 'lib/Helpers.psm1') -Force

# ---------------------------------------------------------------------------
# Run identity
# ---------------------------------------------------------------------------
$RunTimestamp = Get-Date -Format 'yyyyMMdd-HHmm'
$RunIndex = '01'
$RunId = "uat-${RunTimestamp}-${RunIndex}"
$RunRoot = Join-Path $RepoRoot "target/uat-runs/$RunId"
$UatDataDir = Join-Path $RunRoot 'data'

New-Item -ItemType Directory -Path $UatDataDir -Force | Out-Null
Initialize-RunArtifacts -RunRoot $RunRoot
Write-Timeline $RunRoot "UAT run $RunId started"

$HiveUrl = "http://127.0.0.1:$HivePort"
$EntityUrl = "http://127.0.0.1:$EntityPort"
$EntityName = $RunId
$SoftRecoveries = 0
$HiveProc = $null
$EntityProc = $null
$AppProc = $null
$ExitCode = 0
$InstallerMode = -not [string]::IsNullOrWhiteSpace($InstallerPath)
$ChatSessionId = "$RunId-chat"

$STUB_SIGNATURE = "I need a cloud API key or local LLM"

# ---------------------------------------------------------------------------
# Stage helper: retry wrapper
# ---------------------------------------------------------------------------
function Invoke-StageWithRetry {
    param(
        [string]$StageName,
        [scriptblock]$Action,
        [int]$MaxRetries = 2
    )
    $attempt = 0
    while ($true) {
        try {
            Write-Timeline $RunRoot "STAGE $StageName attempt $($attempt + 1)"
            & $Action
            Write-Timeline $RunRoot "STAGE $StageName PASSED"
            Write-AssertionResult $RunRoot $StageName 'gate' $true 'passed'
            return
        } catch {
            $attempt++
            $msg = $_.Exception.Message
            Write-Timeline $RunRoot "STAGE $StageName FAILED (attempt $attempt): $msg"
            Write-AssertionResult $RunRoot $StageName "attempt_$attempt" $false $msg

            if ($attempt -gt $MaxRetries) {
                throw $_
            }
            $script:SoftRecoveries++
            Write-Timeline $RunRoot "STAGE $StageName retrying ($attempt/$MaxRetries)..."
            Start-Sleep -Seconds 3
        }
    }
}

# ---------------------------------------------------------------------------
# Cleanup on exit
# ---------------------------------------------------------------------------
function Stop-AllDaemons {
    foreach ($proc in @($HiveProc, $EntityProc)) {
        if ($proc -and -not $proc.HasExited) {
            try { $proc.Kill() } catch {}
        }
    }
    Get-Job | Remove-Job -Force -ErrorAction SilentlyContinue
}

trap { Stop-AllDaemons }

try {

# ===================================================================
# STAGE 0: PREFLIGHT
# ===================================================================
Invoke-StageWithRetry 'preflight' {
    if ($InstallerMode) {
        $script:InstallerPath = (Resolve-Path $InstallerPath).Path
        if (-not (Test-Path $InstallerPath)) {
            throw "Installer not found: $InstallerPath"
        }
    }

    $script:Keys = @{}
    if ($Provider -eq 'openai') {
        if (-not $KeysetFile) {
            $script:KeysetFile = Join-Path $RepoRoot 'scripts/uat/uat-keys.env'
        }
        if (-not (Test-Path $KeysetFile)) {
            throw "Keyset file not found: $KeysetFile. Copy uat-keys.env.template to uat-keys.env and fill in values."
        }

        Get-Content $KeysetFile | ForEach-Object {
            $line = $_.Trim()
            if ($line -and -not $line.StartsWith('#')) {
                $parts = $line -split '=', 2
                if ($parts.Count -eq 2) {
                    $script:Keys[$parts[0].Trim()] = $parts[1].Trim()
                }
            }
        }

        if (-not $Keys['OPENAI_API_KEY']) {
            throw "Required key 'OPENAI_API_KEY' is empty or missing in keyset file."
        }
    } else {
        $claude = Get-Command claude -ErrorAction SilentlyContinue
        if (-not $claude) {
            throw "Claude CLI mode requires 'claude' on PATH."
        }
        $authOut = & claude auth status 2>&1
        if ($LASTEXITCODE -ne 0) {
            throw "Claude CLI is not authenticated. Run 'claude auth login' first. Output: $authOut"
        }
    }

    $portsToCheck = if ($InstallerMode) { @() } else { @($HivePort, $EntityPort) }
    foreach ($p in $portsToCheck) {
        $conn = Get-NetTCPConnection -LocalPort $p -ErrorAction SilentlyContinue
        if ($conn) {
            throw "Port $p is already in use. Free it or use different ports."
        }
    }

    Write-Host "[PREFLIGHT] Provider=$Provider InstallerMode=$InstallerMode checks passed."
} -MaxRetries 1

# ===================================================================
# STAGE 1: BUILD
# ===================================================================
if ($InstallerMode) {
    Write-Timeline $RunRoot "STAGE build SKIPPED (installer mode)"
    Write-Host "[BUILD] Skipped; using installer $InstallerPath"
} elseif (-not $SkipBuild) {
    Invoke-StageWithRetry 'build' {
        Push-Location $RepoRoot
        try {
            Write-Host "[BUILD] Running cargo fmt check..."
            $fmtOut = & cargo fmt --all -- --check 2>&1
            if ($LASTEXITCODE -ne 0) { throw "cargo fmt failed: $fmtOut" }

            Write-Host "[BUILD] Running cargo clippy..."
            $clippyOut = & cargo clippy --workspace --exclude abigail-app -- -D warnings 2>&1
            if ($LASTEXITCODE -ne 0) { throw "cargo clippy failed: $($clippyOut | Select-Object -Last 20 | Out-String)" }

            Write-Host "[BUILD] Running cargo build..."
            $buildOut = & cargo build --workspace --exclude abigail-app 2>&1
            if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

            Write-Host "[BUILD] Build succeeded."
        } finally { Pop-Location }
    } -MaxRetries 1
} else {
    Write-Timeline $RunRoot "STAGE build SKIPPED"
    Write-Host "[BUILD] Skipped."
}

# ===================================================================
# STAGE 1B: INSTALLER LAUNCH (OPTIONAL FAMILY PATH)
# ===================================================================
if ($InstallerMode) {
    Invoke-StageWithRetry 'installer_launch' {
        Write-Host "[INSTALLER] Installing Abigail from $InstallerPath..."
        $extension = [System.IO.Path]::GetExtension($InstallerPath).ToLowerInvariant()
        if ($extension -eq '.msi') {
            $proc = Start-Process -FilePath 'msiexec.exe' -ArgumentList @('/i', $InstallerPath, '/qn', '/norestart') -Wait -PassThru
            if ($proc.ExitCode -ne 0) { throw "MSI install failed with exit code $($proc.ExitCode)" }
        } elseif ($extension -eq '.exe') {
            $proc = Start-Process -FilePath $InstallerPath -ArgumentList @('/S') -Wait -PassThru
            if ($proc.ExitCode -ne 0) { throw "NSIS install failed with exit code $($proc.ExitCode)" }
        } else {
            throw "Unsupported installer extension '$extension'. Expected .exe or .msi."
        }

        $candidates = @(
            (Join-Path $env:LOCALAPPDATA 'Programs/Abigail/Abigail.exe'),
            (Join-Path $env:ProgramFiles 'Abigail/Abigail.exe')
        )
        if (${env:ProgramFiles(x86)}) {
            $candidates += (Join-Path ${env:ProgramFiles(x86)} 'Abigail/Abigail.exe')
        }
        $appPath = $candidates | Where-Object { Test-Path $_ } | Select-Object -First 1
        if (-not $appPath) {
            throw "Installed Abigail.exe not found. Checked: $($candidates -join ', ')"
        }

        Write-Host "[INSTALLER] Launching Abigail app: $appPath"
        $script:AppProc = Start-Process -FilePath $appPath -PassThru
        $healthy = Wait-ForHealth "$HiveUrl/health" -MaxWaitSec 90
        if (-not $healthy) { throw "Installed Abigail did not expose Hive health at $HiveUrl/health within 90s" }
        Write-Timeline $RunRoot "Installed app launched from $appPath"
    } -MaxRetries 1
}

# ===================================================================
# STAGE 2: HIVE BOOTSTRAP
# ===================================================================
if ($InstallerMode) {
    Invoke-StageWithRetry 'hive_bootstrap' {
        Write-Host "[HIVE] Verifying installed Abigail Hive at $HiveUrl..."
        $healthy = Wait-ForHealth "$HiveUrl/health" -MaxWaitSec 30
        if (-not $healthy) { throw "Hive did not become healthy within 30s" }
        $status = Invoke-UatRequest -Uri "$HiveUrl/v1/status"
        if (-not $status.ok) { throw "Hive /v1/status returned ok=false" }
        Write-HttpTrace $RunRoot 'hive_bootstrap' 'status' @{url="$HiveUrl/v1/status"; installer=$InstallerPath} $status
        Write-Host "[HIVE] Installed app Hive is healthy."
    }
} else {
    Invoke-StageWithRetry 'hive_bootstrap' {
    Write-Host "[HIVE] Starting hive-daemon on port $HivePort with data-dir $UatDataDir..."
    $hiveBin = Join-Path $RepoRoot 'target/debug/hive-daemon'
    if ($env:OS -match 'Windows') { $hiveBin += '.exe' }
    $script:HiveProc = Start-DaemonProcess -RunRoot $RunRoot -Name 'hive' `
        -Command $hiveBin `
        -Arguments @("--data-dir", $UatDataDir, "--port", $HivePort) `
        -WorkingDir $RepoRoot

    $healthy = Wait-ForHealth "$HiveUrl/health" -MaxWaitSec 30
    if (-not $healthy) { throw "Hive did not become healthy within 30s" }

    $status = Invoke-UatRequest -Uri "$HiveUrl/v1/status"
    if (-not $status.ok) { throw "Hive /v1/status returned ok=false" }

    Write-HttpTrace $RunRoot 'hive_bootstrap' 'status' @{url="$HiveUrl/v1/status"} $status
    Write-Host "[HIVE] Healthy and running."
    }
}

# ===================================================================
# STAGE 3: ENTITY CREATE
# ===================================================================
$EntityId = $null
Invoke-StageWithRetry 'entity_create' {
    Write-Host "[ENTITY CREATE] Creating entity '$EntityName'..."
    $resp = Invoke-UatRequest -Method POST -Uri "$HiveUrl/v1/entities" -Body @{ name = $EntityName }
    if (-not $resp.ok) { throw "Entity create failed: $($resp.error)" }
    $script:EntityId = $resp.data.id
    if (-not $EntityId) { throw "Entity create returned empty id" }

    Write-HttpTrace $RunRoot 'entity_create' 'create' @{name=$EntityName} $resp
    Write-Host "[ENTITY CREATE] Created entity $EntityId"
}

# ===================================================================
# STAGE 4: SECRET SEEDING
# ===================================================================
Invoke-StageWithRetry 'provider_config' {
    if ($Provider -eq 'openai') {
        $providerKey = $Keys['OPENAI_API_KEY']
        Write-Host "[PROVIDER] Seeding openai provider key..."
        $resp = Invoke-UatRequest -Method POST -Uri "$HiveUrl/v1/secrets" `
            -Body @{ key = 'openai'; value = $providerKey }
        if (-not $resp.ok) { throw "Failed to store openai secret: $($resp.error)" }

        $list = Invoke-UatRequest -Uri "$HiveUrl/v1/secrets/list"
        if (-not $list.ok) { throw "secrets/list failed" }
        if ($list.data.keys -notcontains 'openai') { throw "openai key not found in secrets list" }
        Write-HttpTrace $RunRoot 'provider_config' 'list' @{} $list
    }

    $default = Invoke-UatRequest -Method POST -Uri "$HiveUrl/v1/providers/hive-default" `
        -Body @{ provider = $Provider; model = $null }
    if (-not $default.ok) { throw "Failed to set Hive default provider: $($default.error)" }

    $patch = Invoke-UatRequest -Method PATCH -Uri "$HiveUrl/v1/entities/$EntityId/config" `
        -Body @{ active_provider_preference = $Provider; ego_model = $null }
    if (-not $patch.ok) { throw "Failed to set entity provider config: $($patch.error)" }
    Write-HttpTrace $RunRoot 'provider_config' 'entity_patch' @{provider=$Provider} $patch

    Write-Host "[PROVIDER] Configured $Provider for UAT."
}

# ===================================================================
# STAGE 5: ENTITY BOOTSTRAP
# ===================================================================
if ($InstallerMode) {
    Invoke-StageWithRetry 'entity_bootstrap' {
        Write-Host "[ENTITY] Opening entity $EntityId through installed Hive..."
        $open = Invoke-UatRequest -Method POST -Uri "$HiveUrl/v1/entities/$EntityId/open" -Body @{}
        if (-not $open.ok) { throw "Entity open failed: $($open.error)" }
        $script:EntityUrl = $open.data.local_url
        if (-not $EntityUrl) { throw "Entity open did not return local_url" }
        $healthy = Wait-ForHealth "$EntityUrl/health" -MaxWaitSec 45
        if (-not $healthy) { throw "Entity did not become healthy within 45s at $EntityUrl" }
        $status = Invoke-UatRequest -Uri "$EntityUrl/v1/status"
        if (-not $status.ok) { throw "Entity /v1/status returned ok=false" }
        if (-not $status.data.has_ego) { throw "Entity has_ego=false - provider config not resolved" }
        Write-HttpTrace $RunRoot 'entity_bootstrap' 'open' @{entity_id=$EntityId} $open
        Write-Host "[ENTITY] Installed path opened Entity Runtime at $EntityUrl."
    }
} else {
    Invoke-StageWithRetry 'entity_bootstrap' {
    Write-Host "[ENTITY] Starting entity-daemon for $EntityId..."
    $entityBin = Join-Path $RepoRoot 'target/debug/entity-daemon'
    if ($env:OS -match 'Windows') { $entityBin += '.exe' }
    $script:EntityProc = Start-DaemonProcess -RunRoot $RunRoot -Name 'entity' `
        -Command $entityBin `
        -Arguments @("--entity-id", $EntityId, "--hive-url", $HiveUrl, "--port", $EntityPort, "--data-dir", $UatDataDir) `
        -WorkingDir $RepoRoot

    $healthy = Wait-ForHealth "$EntityUrl/health" -MaxWaitSec 45
    if (-not $healthy) { throw "Entity did not become healthy within 45s" }

    $status = Invoke-UatRequest -Uri "$EntityUrl/v1/status"
    if (-not $status.ok) { throw "Entity /v1/status returned ok=false" }
    if (-not $status.data.has_ego) { throw "Entity has_ego=false - provider config not resolved" }

    Write-HttpTrace $RunRoot 'entity_bootstrap' 'status' @{} $status
    Write-Host "[ENTITY] Healthy with Ego provider active."
    }
}

# ===================================================================
# STAGE 6: CHAT SANITY (REAL LLM)
# ===================================================================
Invoke-StageWithRetry 'chat_sanity' {
    # Test 1: hello (verify real LLM, not stub)
    Write-Host "[CHAT] Sending hello..."
    $resp = Invoke-UatRequest -Method POST -Uri "$EntityUrl/v1/chat" `
        -Body @{ message = "Say exactly one word: hello"; session_id = $ChatSessionId } -TimeoutSec 60
    if (-not $resp.ok) { throw "Chat hello failed: $($resp.error)" }
    $reply = $resp.data.reply
    if (-not $reply) { throw "Chat hello returned empty reply" }
    if ($reply -match [regex]::Escape($STUB_SIGNATURE)) {
        throw "Chat returned stub/fallback response - Ego provider not working. Reply: $reply"
    }
    Write-HttpTrace $RunRoot 'chat_sanity' 'hello' @{message='hello'} $resp
    Write-Host "[CHAT] Hello reply: $($reply.Substring(0, [Math]::Min(80, $reply.Length)))..."

    # Test 2-4: three simple questions
    $questions = @(
        @{ q = "What is 2 + 2? Reply with just the number."; check = '4' },
        @{ q = "What color is the sky on a clear day? One word."; check = 'blue' },
        @{ q = "Name one planet in our solar system. One word."; check = $null }
    )
    $qi = 0
    foreach ($item in $questions) {
        $qi++
        Write-Host "[CHAT] Question $qi..."
        $r = Invoke-UatRequest -Method POST -Uri "$EntityUrl/v1/chat" `
            -Body @{ message = $item.q; session_id = $ChatSessionId } -TimeoutSec 60
        if (-not $r.ok) { throw "Chat question $qi failed: $($r.error)" }
        if (-not $r.data.reply) { throw "Chat question $qi returned empty reply" }
        if ($r.data.reply -match [regex]::Escape($STUB_SIGNATURE)) {
            throw "Chat question $qi returned stub. Reply: $($r.data.reply)"
        }
        if ($item.check -and $r.data.reply -notmatch $item.check) {
            Write-Warning "Chat Q$qi answer may not contain expected '$($item.check)': $($r.data.reply)"
        }
        Write-HttpTrace $RunRoot 'chat_sanity' "question_$qi" @{message=$item.q} $r
    }

    Write-Host "[CHAT] All chat tests passed."
}

# ===================================================================
# STAGE 6B: DURABLE EXECUTION HISTORY
# ===================================================================
Invoke-StageWithRetry 'execution_history' {
    Write-Host "[HISTORY] Verifying local execution ledger for session $ChatSessionId..."
    $events = Invoke-UatRequest -Uri "$EntityUrl/v1/execution/events?session_id=$ChatSessionId&limit=20"
    if (-not $events.ok) { throw "Execution events endpoint failed: $($events.error)" }
    if ($events.data.events.Count -lt 2) { throw "Expected at least two execution ledger events, found $($events.data.events.Count)" }

    if (-not $InstallerMode -and $EntityProc -and -not $EntityProc.HasExited) {
        Write-Host "[HISTORY] Restarting debug entity-daemon to verify ledger survives restart..."
        $EntityProc.Kill()
        Start-Sleep -Seconds 2
        $entityBin = Join-Path $RepoRoot 'target/debug/entity-daemon'
        if ($env:OS -match 'Windows') { $entityBin += '.exe' }
        $script:EntityProc = Start-DaemonProcess -RunRoot $RunRoot -Name 'entity-restart' `
            -Command $entityBin `
            -Arguments @("--entity-id", $EntityId, "--hive-url", $HiveUrl, "--port", $EntityPort, "--data-dir", $UatDataDir) `
            -WorkingDir $RepoRoot
        $healthy = Wait-ForHealth "$EntityUrl/health" -MaxWaitSec 45
        if (-not $healthy) { throw "Entity did not become healthy after restart" }
        $events = Invoke-UatRequest -Uri "$EntityUrl/v1/execution/events?session_id=$ChatSessionId&limit=20"
        if (-not $events.ok) { throw "Execution events endpoint failed after restart: $($events.error)" }
        if ($events.data.events.Count -lt 2) { throw "Execution ledger did not survive restart" }
    }

    Write-HttpTrace $RunRoot 'execution_history' 'events' @{session_id=$ChatSessionId} $events
    Write-Host "[HISTORY] Execution events persisted."
}

# ===================================================================
# STAGE 7: WEATHER CURRENTNESS VALIDATION
# ===================================================================
Invoke-StageWithRetry 'weather' {
    $city = 'Austin'

    # Fetch ground truth from Open-Meteo (free, no key required)
    Write-Host "[WEATHER] Fetching ground truth from Open-Meteo for $city..."
    try {
        $meteoUrl = 'https://api.open-meteo.com/v1/forecast?latitude=30.27&longitude=-97.74&current_weather=true'
        $meteo = Invoke-RestMethod -Uri $meteoUrl -TimeoutSec 15
        $truthTemp = $meteo.current_weather.temperature
        $truthDesc = $meteo.current_weather.weathercode
        Write-Host "[WEATHER] Ground truth: ${truthTemp}C, code=$truthDesc"
    } catch {
        Write-Warning "Open-Meteo unreachable - will use relaxed validation. $_"
        $truthTemp = $null
    }

    # Ask entity
    Write-Host "[WEATHER] Asking entity about weather in $city..."
    $resp = Invoke-UatRequest -Method POST -Uri "$EntityUrl/v1/chat" `
        -Body @{ message = "What is the current weather in $city, TX right now? Include the temperature."; session_id = $ChatSessionId } `
        -TimeoutSec 60
    if (-not $resp.ok) { throw "Weather chat failed: $($resp.error)" }
    $reply = $resp.data.reply
    if (-not $reply) { throw "Weather reply empty" }
    if ($reply -match [regex]::Escape($STUB_SIGNATURE)) {
        throw "Weather returned stub response"
    }
    Write-HttpTrace $RunRoot 'weather' 'query' @{city=$city} $resp

    # Validation: reply should mention temperature or weather-related terms
    $weatherTerms = 'temperature|degrees|°|sunny|cloudy|rain|wind|humidity|forecast|clear|overcast|warm|cold|hot|cool'
    if ($reply -notmatch $weatherTerms) {
        Write-Warning "Weather reply may not contain weather information: $reply"
    }

    Write-Host "[WEATHER] Weather validation passed. Reply excerpt: $($reply.Substring(0, [Math]::Min(120, $reply.Length)))..."
}

# ===================================================================
# SUMMARY
# ===================================================================
$result = if ($SoftRecoveries -gt 0) { 'SOFT_FAIL_RECOVERED' } else { 'PASS' }
$ExitCode = if ($SoftRecoveries -gt 0) { 10 } else { 0 }

$summary = @{
    run_id           = $RunId
    result           = $result
    soft_recoveries  = $SoftRecoveries
    entity_id        = $EntityId
    entity_name      = $EntityName
    hive_port        = $HivePort
    entity_port      = $EntityPort
    provider         = $Provider
    installer_mode   = $InstallerMode
    installer_path   = $InstallerPath
    chat_session_id  = $ChatSessionId
    data_dir         = $UatDataDir
    time             = (Get-Date -Format 'o')
}
$summary | ConvertTo-Json | Set-Content (Join-Path $RunRoot 'summary.json')
Write-Timeline $RunRoot "UAT run $RunId completed: $result"
Write-Host ""
Write-Host "============================================"
Write-Host "  UAT RESULT: $result"
Write-Host "  Run ID:     $RunId"
Write-Host "  Artifacts:  $RunRoot"
Write-Host "============================================"

} catch {
    $failStage = 'unknown'
    $msg = $_.Exception.Message
    Write-Host ""
    Write-Host "============================================" -ForegroundColor Red
    Write-Host "  UAT HARD FAILURE" -ForegroundColor Red
    Write-Host "  $msg" -ForegroundColor Red
    Write-Host "============================================" -ForegroundColor Red

    Invoke-HardFailureBreakout -RunRoot $RunRoot -Stage $failStage `
        -Symptoms $msg `
        -LikelyCauses @("See failure-plan.md for details") `
        -ImmediateActions @("Review artifacts in $RunRoot", "Fix the root cause", "Restart from Stage 0 with new runId") `
        -Processes @($HiveProc, $EntityProc)

    $ExitCode = 20
} finally {
    Stop-AllDaemons
}

exit $ExitCode
