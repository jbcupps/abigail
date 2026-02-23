param(
    [string]$EntityId = "adam",
    [ValidateSet("quick_start", "direct", "soul_crystallization", "soul_forge")]
    [string]$BirthPath = "quick_start",
    [string]$Message = "Hello Adam. This is a development chat session check.",
    [Alias("KeepDeamons")]
    [switch]$KeepDaemons,
    [switch]$RestartDaemons,
    [string]$EnvFile = ".env.e2e.local"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$RepoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
Set-Location $RepoRoot

function Invoke-CheckedCommand {
    param(
        [string]$Executable,
        [string[]]$Arguments
    )

    Write-Host ("+ {0} {1}" -f $Executable, ($Arguments -join " ")) -ForegroundColor DarkCyan
    & $Executable @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw ("Command failed with exit code {0}: {1} {2}" -f $LASTEXITCODE, $Executable, ($Arguments -join " "))
    }
}

function Import-LocalEnvFile {
    param([string]$Path)

    if (-not (Test-Path $Path)) {
        Write-Host "No env file found at $Path. Continuing without env import." -ForegroundColor Yellow
        return
    }

    $loaded = [System.Collections.Generic.List[string]]::new()
    foreach ($raw in Get-Content $Path) {
        $line = $raw.Trim()
        if ([string]::IsNullOrWhiteSpace($line)) { continue }
        if ($line.StartsWith("#")) { continue }

        $parts = $line -split "=", 2
        if ($parts.Count -ne 2) { continue }

        $name = $parts[0].Trim()
        $value = $parts[1]
        if ([string]::IsNullOrWhiteSpace($name)) { continue }

        [System.Environment]::SetEnvironmentVariable($name, $value, "Process")
        $loaded.Add($name)
    }

    if ($loaded.Count -gt 0) {
        Write-Host ("Loaded env vars from {0}: {1}" -f $Path, (($loaded | Sort-Object -Unique) -join ", ")) -ForegroundColor Green
    }
}

function Test-ApiReady {
    param([string]$Uri)
    try {
        $response = Invoke-RestMethod -Method Get -Uri $Uri -TimeoutSec 3
        return ($response.ok -eq $true)
    } catch {
        return $false
    }
}

function Wait-ForApi {
    param(
        [string]$Uri,
        [int]$TimeoutSeconds = 60
    )

    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        if (Test-ApiReady -Uri $Uri) {
            return
        }
        Start-Sleep -Milliseconds 500
    }
    throw "Timed out waiting for API readiness at $Uri"
}

function Stop-ListenerByPort {
    param([int]$Port)

    $listenerPids = Get-NetTCPConnection -LocalPort $Port -State Listen -ErrorAction SilentlyContinue |
        Select-Object -ExpandProperty OwningProcess -Unique
    foreach ($listenerPid in $listenerPids) {
        Stop-Process -Id $listenerPid -Force -ErrorAction SilentlyContinue
    }
}

function Start-DaemonIfNeeded {
    param(
        [string]$Name,
        [string]$HealthUri,
        [string[]]$CargoArgs,
        [string]$OutLog,
        [string]$ErrLog,
        [switch]$ForceRestart
    )

    $uri = [System.Uri]$HealthUri
    $port = $uri.Port
    if ($ForceRestart) {
        Write-Host "Restart requested for $Name." -ForegroundColor Yellow
        Stop-ListenerByPort -Port $port
        Start-Sleep -Milliseconds 400
    }

    if (Test-ApiReady -Uri $HealthUri) {
        Write-Host "$Name is already running." -ForegroundColor Green
        return @{
            StartedByScript = $false
            Process = $null
        }
    }

    if (Test-Path $OutLog) { Remove-Item $OutLog -Force }
    if (Test-Path $ErrLog) { Remove-Item $ErrLog -Force }

    Write-Host "Starting $Name..." -ForegroundColor Cyan
    $proc = Start-Process `
        -FilePath "cargo" `
        -ArgumentList $CargoArgs `
        -WorkingDirectory $RepoRoot `
        -RedirectStandardOutput $OutLog `
        -RedirectStandardError $ErrLog `
        -PassThru

    Wait-ForApi -Uri $HealthUri
    Write-Host "$Name is ready." -ForegroundColor Green

    return @{
        StartedByScript = $true
        Process = $proc
    }
}

$logDir = Join-Path $RepoRoot "target\dev-session-logs"
New-Item -Path $logDir -ItemType Directory -Force | Out-Null
$hiveOutLog = Join-Path $logDir "hive-daemon.out.log"
$hiveErrLog = Join-Path $logDir "hive-daemon.err.log"
$entityOutLog = Join-Path $logDir "entity-daemon.out.log"
$entityErrLog = Join-Path $logDir "entity-daemon.err.log"

Import-LocalEnvFile -Path $EnvFile

$hive = Start-DaemonIfNeeded `
    -Name "hive-daemon" `
    -HealthUri "http://127.0.0.1:7701/v1/status" `
    -CargoArgs @("run", "-p", "hive-daemon") `
    -OutLog $hiveOutLog `
    -ErrLog $hiveErrLog `
    -ForceRestart:$RestartDaemons

$entity = Start-DaemonIfNeeded `
    -Name "entity-daemon" `
    -HealthUri "http://127.0.0.1:7702/v1/status" `
    -CargoArgs @("run", "-p", "entity-daemon") `
    -OutLog $entityOutLog `
    -ErrLog $entityErrLog `
    -ForceRestart:$RestartDaemons

$birthPathCli = $BirthPath -replace "_", "-"

try {
    Write-Host ""
    Write-Host "=== Development entity setup ===" -ForegroundColor Cyan
    Invoke-CheckedCommand "cargo" @("run", "-p", "hive-cli", "--", "entity", "birth", $EntityId, "--path", $birthPathCli)
    Invoke-CheckedCommand "cargo" @("run", "-p", "hive-cli", "--", "entity", "start", $EntityId)
    Invoke-CheckedCommand "cargo" @("run", "-p", "hive-cli", "--", "entity", "list")

    Write-Host ""
    Write-Host "=== Working chat session check ===" -ForegroundColor Cyan
    Invoke-CheckedCommand "cargo" @("run", "-p", "entity-cli", "--", "status")
    Invoke-CheckedCommand "cargo" @("run", "-p", "entity-cli", "--", "chat", $Message)
} finally {
    if ($KeepDaemons) {
        Write-Host ""
        Write-Host "Daemons kept running." -ForegroundColor Yellow
        Write-Host "hive log:   $hiveOutLog"
        Write-Host "entity log: $entityOutLog"
    } else {
        foreach ($entry in @($hive, $entity)) {
            if ($entry.StartedByScript -and $null -ne $entry.Process -and -not $entry.Process.HasExited) {
                Stop-Process -Id $entry.Process.Id -Force
                try { Wait-Process -Id $entry.Process.Id -Timeout 5 } catch {}
            }
        }
    }
}

Write-Host ""
Write-Host "Development session bootstrapped for entity '$EntityId' using birth path '$BirthPath'." -ForegroundColor Green
