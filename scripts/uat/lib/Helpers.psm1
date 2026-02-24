# UAT helper functions: HTTP assertions, artifact writing, failure-plan generation.

$ErrorActionPreference = 'Stop'

# ---------------------------------------------------------------------------
# Artifact helpers
# ---------------------------------------------------------------------------

function Initialize-RunArtifacts {
    param([string]$RunRoot)
    foreach ($sub in 'http', 'process', 'assertions') {
        New-Item -ItemType Directory -Path (Join-Path $RunRoot $sub) -Force | Out-Null
    }
    $timeline = Join-Path $RunRoot 'timeline.log'
    if (-not (Test-Path $timeline)) { '' | Set-Content $timeline }
}

function Write-Timeline {
    param([string]$RunRoot, [string]$Message)
    $ts = Get-Date -Format 'yyyy-MM-dd HH:mm:ss.fff'
    "$ts  $Message" | Add-Content (Join-Path $RunRoot 'timeline.log')
}

function Write-HttpTrace {
    param([string]$RunRoot, [string]$Stage, [string]$Label, [object]$Request, [object]$Response)
    $dir = Join-Path $RunRoot 'http'
    $file = Join-Path $dir "${Stage}_${Label}.json"
    $payload = @{
        request  = $Request
        response = $Response
    }
    $payload | ConvertTo-Json -Depth 10 | Set-Content $file
}

function Write-AssertionResult {
    param([string]$RunRoot, [string]$Stage, [string]$Name, [bool]$Passed, [string]$Detail)
    $dir = Join-Path $RunRoot 'assertions'
    $file = Join-Path $dir "${Stage}_${Name}.json"
    @{
        stage  = $Stage
        name   = $Name
        passed = $Passed
        detail = $Detail
        time   = (Get-Date -Format 'o')
    } | ConvertTo-Json | Set-Content $file
}

# ---------------------------------------------------------------------------
# Failure plan generation
# ---------------------------------------------------------------------------

function Write-FailurePlan {
    param(
        [string]$RunRoot,
        [string]$Stage,
        [string]$Symptoms,
        [string[]]$LikelyCauses,
        [string[]]$ImmediateActions,
        [string]$RetryPolicy,
        [string]$FailureClass,       # soft_failure | hard_failure
        [bool]$BreakoutRequired = $false
    )
    $plan = @{
        stage             = $Stage
        symptoms          = $Symptoms
        likely_causes     = $LikelyCauses
        immediate_actions = $ImmediateActions
        retry_policy      = $RetryPolicy
        escalation_trigger = "Failure persists after retry budget exhausted"
        owner_hint        = "UAT operator"
        related_artifacts = @("$Stage*.json")
        failure_class     = $FailureClass
        breakout_required = $BreakoutRequired
    }
    $plan | ConvertTo-Json -Depth 5 | Set-Content (Join-Path $RunRoot 'failure-plan.json')

    $md = @"
# Failure Plan - Stage: $Stage

## Symptoms
$Symptoms

## Likely Causes
$($LikelyCauses | ForEach-Object { "- $_" } | Out-String)

## Immediate Actions
$($ImmediateActions | ForEach-Object { "1. $_" } | Out-String)

## Retry Policy
$RetryPolicy

## Classification
- **failure_class**: $FailureClass
- **breakout_required**: $BreakoutRequired

## Recovery
$(if ($BreakoutRequired) { "Stop the active run. Fix the issue outside plan execution. Restart from Stage 0 with a new runId." } else { "Retry the stage within the allocated budget." })
"@
    $md | Set-Content (Join-Path $RunRoot 'failure-plan.md')
}

# ---------------------------------------------------------------------------
# Hard failure breakout
# ---------------------------------------------------------------------------

function Invoke-HardFailureBreakout {
    param(
        [string]$RunRoot,
        [string]$Stage,
        [string]$Symptoms,
        [string[]]$LikelyCauses,
        [string[]]$ImmediateActions,
        [System.Diagnostics.Process[]]$Processes
    )
    Write-Timeline $RunRoot "HARD_FAILURE at $Stage"
    Write-FailurePlan -RunRoot $RunRoot -Stage $Stage -Symptoms $Symptoms `
        -LikelyCauses $LikelyCauses -ImmediateActions $ImmediateActions `
        -RetryPolicy 'No more retries - breakout required' `
        -FailureClass 'hard_failure' -BreakoutRequired $true

    foreach ($proc in $Processes) {
        if ($proc -and -not $proc.HasExited) {
            try { $proc.Kill() } catch {}
        }
    }

    $summary = @{
        run_id    = (Split-Path $RunRoot -Leaf)
        result    = 'HARD_FAIL'
        failed_at = $Stage
        time      = (Get-Date -Format 'o')
    }
    $summary | ConvertTo-Json | Set-Content (Join-Path $RunRoot 'summary.json')
}

# ---------------------------------------------------------------------------
# HTTP helpers
# ---------------------------------------------------------------------------

function Invoke-UatRequest {
    param(
        [string]$Method = 'GET',
        [string]$Uri,
        [object]$Body,
        [int]$TimeoutSec = 30
    )
    $params = @{
        Method      = $Method
        Uri         = $Uri
        ContentType = 'application/json'
        TimeoutSec  = $TimeoutSec
    }
    if ($Body) {
        $params['Body'] = ($Body | ConvertTo-Json -Depth 10)
    }
    Invoke-RestMethod @params
}

function Wait-ForHealth {
    param([string]$Url, [int]$MaxWaitSec = 60)
    $deadline = (Get-Date).AddSeconds($MaxWaitSec)
    while ((Get-Date) -lt $deadline) {
        try {
            $resp = Invoke-WebRequest -Uri $Url -TimeoutSec 3 -ErrorAction SilentlyContinue
            if ($resp.StatusCode -eq 200) { return $true }
        } catch {}
        Start-Sleep -Seconds 2
    }
    return $false
}

# ---------------------------------------------------------------------------
# Process lifecycle
# ---------------------------------------------------------------------------

function Start-DaemonProcess {
    param(
        [string]$RunRoot,
        [string]$Name,
        [string]$Command,
        [string[]]$Arguments,
        [string]$WorkingDir
    )
    $logFile = Join-Path $RunRoot "process/${Name}.log"
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $Command
    $psi.Arguments = $Arguments -join ' '
    $psi.WorkingDirectory = $WorkingDir
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $psi.UseShellExecute = $false
    $psi.CreateNoWindow = $true

    $proc = [System.Diagnostics.Process]::Start($psi)

    # Async log capture
    $job = Start-Job -ScriptBlock {
        param($pid, $logPath)
        $p = Get-Process -Id $pid -ErrorAction SilentlyContinue
        if (-not $p) { return }
        while (-not $p.HasExited) {
            $line = $p.StandardOutput.ReadLine()
            if ($line) { $line | Add-Content $logPath }
        }
    } -ArgumentList $proc.Id, $logFile

    Start-Job -ScriptBlock {
        param($pid, $logPath)
        $p = Get-Process -Id $pid -ErrorAction SilentlyContinue
        if (-not $p) { return }
        while (-not $p.HasExited) {
            $line = $p.StandardError.ReadLine()
            if ($line) { "STDERR: $line" | Add-Content $logPath }
        }
    } -ArgumentList $proc.Id, $logFile | Out-Null

    return $proc
}

function Redact-Secret {
    param([string]$Value)
    if ($Value.Length -le 8) { return '****' }
    return $Value.Substring(0, 4) + '****'
}

Export-ModuleMember -Function *
