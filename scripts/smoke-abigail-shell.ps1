param(
    [string]$Provider = "openai",
    [string]$ApiKey = "",
    [string]$EntityId = "",
    [string]$DataDir = "",
    [int]$TimeoutSeconds = 420,
    [switch]$Cleanup
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
Set-Location $repoRoot

$timestamp = Get-Date -Format "yyyyMMdd-HHmmss"
if ([string]::IsNullOrWhiteSpace($DataDir)) {
    $DataDir = Join-Path $repoRoot "target\smoke-abigail-shell\$timestamp"
}
if ([string]::IsNullOrWhiteSpace($EntityId)) {
    $EntityId = "smoke-$timestamp"
}
if ([string]::IsNullOrWhiteSpace($ApiKey)) {
    $ApiKey = "sk-smoke-$([Guid]::NewGuid().ToString('N'))"
}

New-Item -Path $DataDir -ItemType Directory -Force | Out-Null
$inputFile = Join-Path $DataDir "shell-input.txt"
$stdoutFile = Join-Path $DataDir "stdout.log"
$stderrFile = Join-Path $DataDir "stderr.log"
$combinedLog = Join-Path $DataDir "smoke.log"

$env:ABIGAIL_DATA_DIR = $DataDir
$env:ABIGAIL_SHELL_SKIP_LOCAL_PROVIDER = "1"
$env:HIVE_REGISTRY_PATH = Join-Path $DataDir "hive-registry.json"

$inputLines = @(
    $Provider
    $ApiKey
    "n"
    ""
    "Smoke Mentor"
    "Abigail Smoke"
    "Validate the automated shell smoke path."
    "Direct and practical."
    $EntityId
    "exit"
    "exit"
)

Set-Content -Path $inputFile -Value ($inputLines -join "`r`n") -NoNewline

$cmdLine = "/c type `"$inputFile`" | cargo run -p abigail-cli --bin abigail"
Write-Host ("+ cmd.exe {0}" -f $cmdLine) -ForegroundColor DarkCyan

$proc = Start-Process `
    -FilePath "cmd.exe" `
    -ArgumentList $cmdLine `
    -WorkingDirectory $repoRoot `
    -RedirectStandardOutput $stdoutFile `
    -RedirectStandardError $stderrFile `
    -PassThru

try {
    Wait-Process -Id $proc.Id -Timeout $TimeoutSeconds
} catch {
    Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
    throw "Smoke test timed out after $TimeoutSeconds seconds."
}

$proc.Refresh()
$exitCode = if ($null -eq $proc.ExitCode) { 0 } else { [int]$proc.ExitCode }

$stdout = if (Test-Path $stdoutFile) { Get-Content $stdoutFile -Raw } else { "" }
$stderr = if (Test-Path $stderrFile) { Get-Content $stderrFile -Raw } else { "" }
$combined = $stdout + "`r`n" + $stderr
Set-Content -Path $combinedLog -Value $combined

if ($exitCode -ne 0) {
    Write-Host $combined
    throw "Smoke test failed: abigail exited with code $exitCode."
}

$required = @(
    "starting simple birth cycle",
    "stored key for",
    "birth complete:",
    "entity '$EntityId' ready",
    "chat ended"
)

foreach ($needle in $required) {
    if ($combined -notlike "*$needle*") {
        throw "Smoke test failed: expected output fragment not found: $needle"
    }
}

Write-Host ""
Write-Host "Smoke test passed." -ForegroundColor Green
Write-Host "Data dir: $DataDir"
Write-Host "Log file: $combinedLog"

if ($Cleanup) {
    Remove-Item -Path $DataDir -Recurse -Force
    Write-Host "Cleaned up data dir." -ForegroundColor Yellow
}
