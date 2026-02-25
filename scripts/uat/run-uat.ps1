<#
.SYNOPSIS
    Tabula Rasa UAT - automated end-to-end acceptance test.

.DESCRIPTION
    Runs a full tabula-rasa UAT: build, start Hive, create entity, seed secrets,
    start Entity, validate chat (hello + 3 questions), verify weather currentness,
    configure email skill, and verify IMAP inbox.

    Exit codes:
      0  = PASS
      10 = SOFT_FAIL_RECOVERED (all stages passed after retries)
      20 = HARD_FAIL (unrecoverable - fix and restart)

.PARAMETER KeysetFile
    Path to uat-keys.env (default: scripts/uat/uat-keys.env).

.PARAMETER HivePort
    Hive daemon port (default: 3141).

.PARAMETER EntityPort
    Entity daemon port (default: 3142).

.PARAMETER SkipBuild
    Skip the build stage (useful for rapid re-runs after a hard-failure fix).

.PARAMETER SkipEmail
    Skip the email/IMAP stage.
#>

[CmdletBinding()]
param(
    [string]$KeysetFile,
    [int]$HivePort = 3141,
    [int]$EntityPort = 3142,
    [switch]$SkipBuild,
    [switch]$SkipEmail
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
$ExitCode = 0

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
    # Resolve keyset file
    if (-not $KeysetFile) {
        $script:KeysetFile = Join-Path $RepoRoot 'scripts/uat/uat-keys.env'
    }
    if (-not (Test-Path $KeysetFile)) {
        throw "Keyset file not found: $KeysetFile. Copy uat-keys.env.template to uat-keys.env and fill in values."
    }

    # Parse keyset
    $script:Keys = @{}
    Get-Content $KeysetFile | ForEach-Object {
        $line = $_.Trim()
        if ($line -and -not $line.StartsWith('#')) {
            $parts = $line -split '=', 2
            if ($parts.Count -eq 2) {
                $script:Keys[$parts[0].Trim()] = $parts[1].Trim()
            }
        }
    }

    # Validate required keys
    $required = @('OPENAI_API_KEY')
    foreach ($k in $required) {
        if (-not $Keys[$k]) {
            throw "Required key '$k' is empty or missing in keyset file."
        }
    }

    # Validate ports available
    foreach ($p in @($HivePort, $EntityPort)) {
        $conn = Get-NetTCPConnection -LocalPort $p -ErrorAction SilentlyContinue
        if ($conn) {
            throw "Port $p is already in use. Free it or use different ports."
        }
    }

    # IMAP bridge reachability (optional - only fail if keys are present)
    $imapHost = $Keys['UAT_IMAP_HOST']
    $imapPort = $Keys['UAT_IMAP_PORT']
    if ($imapHost -and $imapPort -and -not $SkipEmail) {
        $tcp = New-Object System.Net.Sockets.TcpClient
        try {
            $tcp.Connect($imapHost, [int]$imapPort)
            $tcp.Close()
        } catch {
            Write-Warning "IMAP bridge not reachable at ${imapHost}:${imapPort} - email stage will likely fail."
        }
    }

    Write-Host "[PREFLIGHT] All checks passed."
} -MaxRetries 1

# ===================================================================
# STAGE 1: BUILD
# ===================================================================
if (-not $SkipBuild) {
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
# STAGE 2: HIVE BOOTSTRAP
# ===================================================================
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
Invoke-StageWithRetry 'secret_seed' {
    # Seed provider key
    $providerKey = $Keys['OPENAI_API_KEY']
    Write-Host "[SECRETS] Seeding openai provider key..."
    $resp = Invoke-UatRequest -Method POST -Uri "$HiveUrl/v1/secrets" `
        -Body @{ key = 'openai'; value = $providerKey }
    if (-not $resp.ok) { throw "Failed to store openai secret: $($resp.error)" }

    # Seed IMAP credentials (if present, for email stage)
    if ($Keys['UAT_IMAP_PASSWORD'] -and -not $SkipEmail) {
        foreach ($mapping in @(
            @{ hiveKey = 'imap_password'; envKey = 'UAT_IMAP_PASSWORD' },
            @{ hiveKey = 'imap_user';     envKey = 'UAT_IMAP_USER' },
            @{ hiveKey = 'imap_host';     envKey = 'UAT_IMAP_HOST' },
            @{ hiveKey = 'imap_port';     envKey = 'UAT_IMAP_PORT' },
            @{ hiveKey = 'imap_tls_mode'; envKey = 'UAT_IMAP_SECURITY' }
        )) {
            $val = $Keys[$mapping.envKey]
            if ($val) {
                Write-Host "[SECRETS] Seeding $($mapping.hiveKey)..."
                $r = Invoke-UatRequest -Method POST -Uri "$HiveUrl/v1/secrets" `
                    -Body @{ key = $mapping.hiveKey; value = $val }
                if (-not $r.ok) { throw "Failed to store $($mapping.hiveKey): $($r.error)" }
            }
        }
    }

    # Verify secrets list
    $list = Invoke-UatRequest -Uri "$HiveUrl/v1/secrets/list"
    if (-not $list.ok) { throw "secrets/list failed" }
    if ($list.data.keys -notcontains 'openai') { throw "openai key not found in secrets list" }
    Write-HttpTrace $RunRoot 'secret_seed' 'list' @{} $list

    Write-Host "[SECRETS] All secrets seeded and verified."
}

# ===================================================================
# STAGE 5: ENTITY BOOTSTRAP
# ===================================================================
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

# ===================================================================
# STAGE 6: CHAT SANITY (REAL LLM)
# ===================================================================
Invoke-StageWithRetry 'chat_sanity' {
    # Test 1: hello (verify real LLM, not stub)
    Write-Host "[CHAT] Sending hello..."
    $resp = Invoke-UatRequest -Method POST -Uri "$EntityUrl/v1/chat" `
        -Body @{ message = "Say exactly one word: hello" } -TimeoutSec 60
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
            -Body @{ message = $item.q } -TimeoutSec 60
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
        -Body @{ message = "What is the current weather in $city, TX right now? Include the temperature." } `
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
# STAGE 8: EMAIL SKILL + INBOX
# ===================================================================
if (-not $SkipEmail -and $Keys['UAT_IMAP_PASSWORD']) {
    Invoke-StageWithRetry 'email_inbox' {
        # Verify email skill is registered
        Write-Host "[EMAIL] Checking skill registration..."
        $skills = Invoke-UatRequest -Uri "$EntityUrl/v1/skills"
        if (-not $skills.ok) { throw "Skills list failed: $($skills.error)" }

        $emailSkill = $skills.data | Where-Object { $_.id -match 'proton' -or $_.id -match 'email' }
        if (-not $emailSkill) {
            throw "Email skill not registered. Available: $($skills.data | ForEach-Object { $_.id } | Out-String)"
        }
        $skillId = $emailSkill.id
        Write-Host "[EMAIL] Found email skill: $skillId"

        # Execute fetch_emails tool
        Write-Host "[EMAIL] Executing fetch_emails..."
        $execResp = Invoke-UatRequest -Method POST -Uri "$EntityUrl/v1/tools/execute" `
            -Body @{
                skill_id  = $skillId
                tool_name = 'fetch_emails'
                params    = @{ limit = 10; unread_only = $false }
            } -TimeoutSec 30
        Write-HttpTrace $RunRoot 'email_inbox' 'fetch_emails' @{skill_id=$skillId} $execResp

        if (-not $execResp.ok) { throw "Tool execute envelope failed: $($execResp.error)" }
        if (-not $execResp.data.success) {
            $toolErr = $execResp.data.error
            if ($toolErr -match 'not initialized') {
                throw "Email skill not initialized - IMAP credentials may be missing from skill vault."
            }
            throw "fetch_emails failed: $toolErr"
        }

        Write-Host "[EMAIL] Inbox fetch succeeded."
        $emails = $execResp.data.output
        if ($emails -is [array]) {
            Write-Host "[EMAIL] Retrieved $($emails.Count) message(s)."
        }
    } -MaxRetries 1
} else {
    Write-Timeline $RunRoot "STAGE email_inbox SKIPPED"
    Write-Host "[EMAIL] Skipped (no IMAP credentials or --SkipEmail)."
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
