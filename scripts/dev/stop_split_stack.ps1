param(
    [string]$SessionPath = "",
    [switch]$Quiet
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Get-WorkspaceRoot {
    return (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
}

$workspaceRoot = Get-WorkspaceRoot
$resolvedSessionPath = if ($SessionPath) {
    $SessionPath
} else {
    Join-Path $workspaceRoot "target\manual-test\stability-reset\session.json"
}

if (-not (Test-Path $resolvedSessionPath)) {
    if (-not $Quiet) {
        Write-Host "No split-stack session file found at $resolvedSessionPath"
    }
    exit 0
}

$session = Get-Content $resolvedSessionPath | ConvertFrom-Json
$pidFields = @(
    "hive_daemon_pid",
    "entity_daemon_pid",
    "hive_app_pid",
    "entity_app_pid",
    "browser_harness_pid"
)

foreach ($field in $pidFields) {
    $property = $session.PSObject.Properties[$field]
    if (-not $property) {
        continue
    }

    $processId = $property.Value
    if (-not $processId) {
        continue
    }

    try {
        $process = Get-Process -Id $processId -ErrorAction Stop
        Stop-Process -Id $process.Id -ErrorAction Stop
        if (-not $Quiet) {
            Write-Host "Stopped $field ($processId)"
        }
    } catch {
        if (-not $Quiet) {
            Write-Warning "Could not stop $field ($processId): $($_.Exception.Message)"
        }
    }
}

$session | Add-Member -NotePropertyName stopped_at_utc -NotePropertyValue ([DateTime]::UtcNow.ToString("o")) -Force
($session | ConvertTo-Json -Depth 8) | Set-Content -Path $resolvedSessionPath
