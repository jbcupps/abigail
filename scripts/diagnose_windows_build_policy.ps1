param(
    [string]$Package = "abigail-hive-app",
    [string]$Binary = "abigail-hive-app",
    [string]$TargetDir = "",
    [string]$OutputPath = ""
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Get-WorkspaceRoot {
    return (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
}

function Get-DefaultTargetDir {
    $localAppData = [Environment]::GetFolderPath("LocalApplicationData")
    return Join-Path $localAppData "Abigail\cargo-target"
}

function Extract-PolicyId {
    param([string]$Message)

    if ($Message -match "Policy ID:\{(?<policy>[^\}]+)\}") {
        return $Matches.policy
    }

    return $null
}

function Extract-BlockedExe {
    param([string]$Message)

    if ($Message -match "attempted to load (?<exe>.+?) that did not meet") {
        return $Matches.exe
    }

    return $null
}

$workspaceRoot = Get-WorkspaceRoot
$resolvedTargetDir = if ($TargetDir) { $TargetDir } else { Get-DefaultTargetDir }
$cargo = (Get-Command cargo).Source
$buildStartedAt = Get-Date

New-Item -ItemType Directory -Force -Path $resolvedTargetDir | Out-Null
$env:CARGO_TARGET_DIR = $resolvedTargetDir

$cargoArgs = @("build", "-p", $Package, "--bin", $Binary)
$cargoCommand = "$cargo $($cargoArgs -join ' ')"

$buildOutput = & $cargo @cargoArgs 2>&1
$exitCode = $LASTEXITCODE
$buildText = ($buildOutput | ForEach-Object { "$_" }) -join [Environment]::NewLine

$ciEvents = @()
try {
    $ciEvents = Get-WinEvent -LogName "Microsoft-Windows-CodeIntegrity/Operational" -MaxEvents 200 |
        Where-Object { $_.TimeCreated -ge $buildStartedAt.AddSeconds(-2) }
} catch {
    $ciEvents = @()
}

$relevantEvent = $ciEvents |
    Where-Object {
        $_.Id -in 3033, 3077 -and
        ($_.Message -match [regex]::Escape("build-script-build.exe") -or $_.Message -match [regex]::Escape("cargo.exe"))
    } |
    Sort-Object TimeCreated -Descending |
    Select-Object -First 1

$summary = [ordered]@{
    success = ($exitCode -eq 0)
    package = $Package
    binary = $Binary
    cargo_command = $cargoCommand
    target_dir = $resolvedTargetDir
    timestamp_utc = [DateTime]::UtcNow.ToString("o")
    build_exit_code = $exitCode
    blocked_exe = $null
    event_id = $null
    policy_id = $null
    build_output_excerpt = if ($buildText.Length -gt 4000) { $buildText.Substring(0, 4000) } else { $buildText }
}

if ($relevantEvent) {
    $summary.success = $false
    $summary.blocked_exe = Extract-BlockedExe -Message $relevantEvent.Message
    $summary.event_id = $relevantEvent.Id
    $summary.policy_id = Extract-PolicyId -Message $relevantEvent.Message
    $summary.timestamp_utc = $relevantEvent.TimeCreated.ToUniversalTime().ToString("o")
}

if (-not $relevantEvent -and $buildText -match "os error 4551") {
    $summary.success = $false
    $summary.blocked_exe = "build-script-build.exe"
}

$json = $summary | ConvertTo-Json -Depth 6

if ($OutputPath) {
    $outputDir = Split-Path -Parent $OutputPath
    if ($outputDir) {
        New-Item -ItemType Directory -Force -Path $outputDir | Out-Null
    }
    $json | Set-Content -Path $OutputPath
}

$json

if (-not $summary.success -and $summary.blocked_exe) {
    Write-Error "Windows application-control policy blocked Cargo from executing '$($summary.blocked_exe)'. Allow cargo.exe to execute build artifacts under '$resolvedTargetDir'."
    exit 1
}

if (-not $summary.success) {
    Write-Error "Desktop build probe failed. See JSON summary above for the captured cargo command and output."
    exit 2
}
