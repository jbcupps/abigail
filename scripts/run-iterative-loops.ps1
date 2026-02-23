param(
    [ValidateSet("all", "1", "2", "3", "4")]
    [string]$Loop = "all",
    [string]$EnvFile = ".env.e2e.local"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$RepoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
Set-Location $RepoRoot

$RequiredProviderKeys = @(
    "OPENAI_API_KEY",
    "ANTHROPIC_API_KEY",
    "XAI_API_KEY",
    "TAVILY_API_KEY",
    "GOOGLE_API_KEY",
    "PERPLEXITY_API_KEY"
)

function Write-LoopHeader {
    param([string]$Title)
    Write-Host ""
    Write-Host "============================================================" -ForegroundColor Cyan
    Write-Host $Title -ForegroundColor Cyan
    Write-Host "============================================================" -ForegroundColor Cyan
}

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
        throw "Env file not found: $Path"
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

    $loadedList = $loaded | Sort-Object -Unique
    Write-Host ("Loaded env vars from {0}: {1}" -f $Path, ($loadedList -join ", ")) -ForegroundColor Green
    return ,$loadedList
}

function Assert-ProviderKeysLoaded {
    param([string[]]$KeyNames)

    $missing = [System.Collections.Generic.List[string]]::new()
    foreach ($name in $KeyNames) {
        $value = [System.Environment]::GetEnvironmentVariable($name, "Process")
        if ([string]::IsNullOrWhiteSpace($value)) {
            $missing.Add($name)
            continue
        }
        Write-Host ("Key loaded: {0} (length={1})" -f $name, $value.Length) -ForegroundColor Green
    }

    if ($missing.Count -gt 0) {
        throw ("Missing or empty provider keys: {0}" -f ($missing -join ", "))
    }
}

function Wait-ForApi {
    param(
        [string]$Uri,
        [int]$TimeoutSeconds = 60
    )

    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        try {
            $response = Invoke-RestMethod -Method Get -Uri $Uri -TimeoutSec 5
            if ($response.ok -eq $true) {
                return
            }
        } catch {
            Start-Sleep -Milliseconds 500
            continue
        }
    }

    throw "Timed out waiting for API readiness at $Uri"
}

function Invoke-ApiCheck {
    param(
        [ValidateSet("GET", "POST")]
        [string]$Method,
        [string]$Uri,
        [object]$Body = $null
    )

    Write-Host ("+ {0} {1}" -f $Method, $Uri) -ForegroundColor DarkCyan
    if ($null -eq $Body) {
        $response = Invoke-RestMethod -Method $Method -Uri $Uri -TimeoutSec 20
    } else {
        $json = $Body | ConvertTo-Json -Compress
        $response = Invoke-RestMethod -Method $Method -Uri $Uri -Body $json -ContentType "application/json" -TimeoutSec 20
    }

    if ($response.ok -ne $true) {
        throw "API call failed (ok=false): $Method $Uri"
    }

    Write-Host ("  -> {0}" -f ($response | ConvertTo-Json -Compress -Depth 8))
}

function Invoke-Loop1 {
    Write-LoopHeader "Loop 1 - Hive + Entity Hookup and Function Validation"

    # Keep loop 1 deterministic: function/lifecycle checks only, no cloud provider dependency.
    $savedProviderEnv = @{}
    foreach ($name in $RequiredProviderKeys) {
        $savedProviderEnv[$name] = [System.Environment]::GetEnvironmentVariable($name, "Process")
        [System.Environment]::SetEnvironmentVariable($name, $null, "Process")
    }

    Invoke-CheckedCommand "cargo" @(
        "test",
        "-p", "hive-core",
        "-p", "entity-core",
        "-p", "hive-daemon",
        "-p", "hive-cli",
        "-p", "entity-daemon",
        "-p", "entity-cli"
    )

    $logDir = Join-Path $RepoRoot "target\loop-logs"
    New-Item -Path $logDir -ItemType Directory -Force | Out-Null
    $hiveLog = Join-Path $logDir "loop1-hive-daemon.out.log"
    $hiveErrLog = Join-Path $logDir "loop1-hive-daemon.err.log"
    $entityLog = Join-Path $logDir "loop1-entity-daemon.out.log"
    $entityErrLog = Join-Path $logDir "loop1-entity-daemon.err.log"

    if (Test-Path $hiveLog) { Remove-Item $hiveLog -Force }
    if (Test-Path $hiveErrLog) { Remove-Item $hiveErrLog -Force }
    if (Test-Path $entityLog) { Remove-Item $entityLog -Force }
    if (Test-Path $entityErrLog) { Remove-Item $entityErrLog -Force }

    $hiveProc = $null
    $entityProc = $null

    try {
        # Ensure deterministic loop startup (no stale daemon instances bound to ports).
        foreach ($port in @(7701, 7702)) {
            $listenerPids = Get-NetTCPConnection -LocalPort $port -State Listen -ErrorAction SilentlyContinue |
                Select-Object -ExpandProperty OwningProcess -Unique
            foreach ($listenerPid in $listenerPids) {
                Stop-Process -Id $listenerPid -Force -ErrorAction SilentlyContinue
            }
        }
        Start-Sleep -Milliseconds 400

        Write-Host "+ start hive-daemon and entity-daemon" -ForegroundColor DarkCyan
        $hiveProc = Start-Process -FilePath "cargo" -ArgumentList @("run", "-p", "hive-daemon") -WorkingDirectory $RepoRoot -RedirectStandardOutput $hiveLog -RedirectStandardError $hiveErrLog -PassThru
        $entityProc = Start-Process -FilePath "cargo" -ArgumentList @("run", "-p", "entity-daemon") -WorkingDirectory $RepoRoot -RedirectStandardOutput $entityLog -RedirectStandardError $entityErrLog -PassThru

        Wait-ForApi "http://127.0.0.1:7701/v1/status"
        Wait-ForApi "http://127.0.0.1:7702/v1/status"

        Invoke-ApiCheck -Method GET -Uri "http://127.0.0.1:7701/v1/status"
        Invoke-ApiCheck -Method GET -Uri "http://127.0.0.1:7701/v1/entity/list"
        Invoke-ApiCheck -Method POST -Uri "http://127.0.0.1:7701/v1/entity/birth" -Body @{ id = "loop1-entity"; path = "quick_start" }
        Invoke-ApiCheck -Method POST -Uri "http://127.0.0.1:7701/v1/entity/start" -Body @{ id = "loop1-entity" }
        Invoke-ApiCheck -Method POST -Uri "http://127.0.0.1:7701/v1/entity/stop" -Body @{ id = "loop1-entity" }
        Invoke-ApiCheck -Method GET -Uri "http://127.0.0.1:7701/v1/logs"

        Invoke-ApiCheck -Method GET -Uri "http://127.0.0.1:7702/v1/status"
        Invoke-ApiCheck -Method POST -Uri "http://127.0.0.1:7702/v1/run" -Body @{ task = "loop1 smoke task" }
        Invoke-ApiCheck -Method POST -Uri "http://127.0.0.1:7702/v1/chat" -Body @{ message = "loop1 hello" }
        Invoke-ApiCheck -Method GET -Uri "http://127.0.0.1:7702/v1/logs"

        Invoke-CheckedCommand "cargo" @("run", "-p", "hive-cli", "--", "status")
        Invoke-CheckedCommand "cargo" @("run", "-p", "hive-cli", "--", "entity", "list")
        Invoke-CheckedCommand "cargo" @("run", "-p", "hive-cli", "--", "entity", "birth", "loop1-cli-entity", "--path", "quick-start")
        Invoke-CheckedCommand "cargo" @("run", "-p", "hive-cli", "--", "entity", "start", "loop1-cli-entity")
        Invoke-CheckedCommand "cargo" @("run", "-p", "hive-cli", "--", "entity", "stop", "loop1-cli-entity")
        Invoke-CheckedCommand "cargo" @("run", "-p", "hive-cli", "--", "logs")

        Invoke-CheckedCommand "cargo" @("run", "-p", "entity-cli", "--", "status")
        Invoke-CheckedCommand "cargo" @("run", "-p", "entity-cli", "--", "run", "loop1 task from cli")
        Invoke-CheckedCommand "cargo" @("run", "-p", "entity-cli", "--", "chat", "loop1 chat from cli")
        Invoke-CheckedCommand "cargo" @("run", "-p", "entity-cli", "--", "logs")
        Invoke-CheckedCommand "cargo" @("run", "-p", "entity-cli", "--", "--oneshot", "status")
        Invoke-CheckedCommand "cargo" @("run", "-p", "entity-cli", "--", "--oneshot", "run", "loop1 oneshot task")
        Invoke-CheckedCommand "cargo" @("run", "-p", "entity-cli", "--", "--oneshot", "chat", "loop1 oneshot chat")
        Invoke-CheckedCommand "cargo" @("run", "-p", "entity-cli", "--", "--oneshot", "logs")
    } finally {
        foreach ($name in $RequiredProviderKeys) {
            [System.Environment]::SetEnvironmentVariable($name, $savedProviderEnv[$name], "Process")
        }

        foreach ($proc in @($hiveProc, $entityProc)) {
            if ($null -ne $proc -and -not $proc.HasExited) {
                Stop-Process -Id $proc.Id -Force
                try { Wait-Process -Id $proc.Id -Timeout 5 } catch {}
            }
        }

        Write-Host "loop1 hive-daemon tail:" -ForegroundColor DarkGray
        if (Test-Path $hiveLog) { Get-Content $hiveLog -Tail 20 }
        if (Test-Path $hiveErrLog) { Get-Content $hiveErrLog -Tail 20 }

        Write-Host "loop1 entity-daemon tail:" -ForegroundColor DarkGray
        if (Test-Path $entityLog) { Get-Content $entityLog -Tail 20 }
        if (Test-Path $entityErrLog) { Get-Content $entityErrLog -Tail 20 }
    }
}

function Invoke-Loop2 {
    Write-LoopHeader "Loop 2 - Cryptographic Keys, KeyVault, and SkillVault"
    Assert-ProviderKeysLoaded -KeyNames $RequiredProviderKeys

    Invoke-CheckedCommand "cargo" @("test", "-p", "abigail-core", "secrets::tests", "--", "--nocapture")
    Invoke-CheckedCommand "cargo" @("test", "-p", "abigail-core", "vault::tests", "--", "--nocapture")
    Invoke-CheckedCommand "cargo" @("test", "-p", "abigail-auth", "manager::tests", "--", "--nocapture")
    Invoke-CheckedCommand "cargo" @("test", "-p", "abigail-hive", "provider_registry::tests::build_ego_from_env_provider_keys", "--", "--nocapture")
    Invoke-CheckedCommand "cargo" @("test", "-p", "abigail-hive", "provider_registry::tests", "--", "--nocapture")
    Invoke-CheckedCommand "cargo" @("test", "-p", "abigail-skills", "hive::tests", "--", "--nocapture")
}

function Invoke-Loop3 {
    Write-LoopHeader "Loop 3 - Full Birth Cycle and All Birth Paths"
    Invoke-CheckedCommand "cargo" @("test", "-p", "abigail-birth", "genesis::tests", "--", "--nocapture")
    Invoke-CheckedCommand "cargo" @("test", "-p", "abigail-birth", "stages::tests", "--", "--nocapture")
    Invoke-CheckedCommand "cargo" @("test", "-p", "abigail-birth", "prompts::tests", "--", "--nocapture")
}

function Invoke-Loop4 {
    Write-LoopHeader "Loop 4 - Chat Capability Validation"
    Assert-ProviderKeysLoaded -KeyNames $RequiredProviderKeys

    Invoke-CheckedCommand "cargo" @("test", "-p", "abigail-router", "router::tests", "--", "--nocapture")
    Invoke-CheckedCommand "cargo" @("test", "-p", "abigail-router", "orchestration::tests", "--", "--nocapture")
    Invoke-CheckedCommand "cargo" @("test", "-p", "abigail-router", "planner::tests", "--", "--nocapture")
    Invoke-CheckedCommand "cargo" @("test", "-p", "abigail-router", "council::tests", "--", "--nocapture")
    Invoke-CheckedCommand "cargo" @("test", "-p", "abigail-capabilities", "cognitive::local_http::tests", "--", "--nocapture")
    Invoke-CheckedCommand "cargo" @("test", "-p", "abigail-capabilities", "cognitive::openai_compatible::tests", "--", "--nocapture")
    Invoke-CheckedCommand "cargo" @("test", "-p", "abigail-capabilities", "cognitive::openai::tests::test_openai_provider", "--", "--nocapture")
    Invoke-CheckedCommand "cargo" @("test", "-p", "abigail-capabilities", "cognitive::anthropic::tests::test_anthropic_provider_with_real_key", "--", "--nocapture")
}

$resolvedEnv = Resolve-Path $EnvFile
Import-LocalEnvFile -Path $resolvedEnv | Out-Null

$executed = [System.Collections.Generic.List[string]]::new()

if ($Loop -eq "all" -or $Loop -eq "1") {
    Invoke-Loop1
    $executed.Add("1")
}
if ($Loop -eq "all" -or $Loop -eq "2") {
    Invoke-Loop2
    $executed.Add("2")
}
if ($Loop -eq "all" -or $Loop -eq "3") {
    Invoke-Loop3
    $executed.Add("3")
}
if ($Loop -eq "all" -or $Loop -eq "4") {
    Invoke-Loop4
    $executed.Add("4")
}

Write-Host ""
Write-Host ("Completed loops: {0}" -f ($executed -join ", ")) -ForegroundColor Green
Write-Host "All requested loop checks passed."
