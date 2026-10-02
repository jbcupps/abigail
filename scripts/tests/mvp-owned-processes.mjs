// Stop only daemon processes belonging to one fresh acceptance data directory.
// Exact executable and --data-dir checks also find children after Hive crashes.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { resolve } from 'node:path';

const cleanupScript = String.raw`
$ErrorActionPreference = 'Stop'
$dataDir = [IO.Path]::GetFullPath($env:ABIGAIL_TEST_CLEANUP_DATA_DIR)
$binaryDir = [IO.Path]::GetFullPath($env:ABIGAIL_TEST_CLEANUP_BIN_DIR)
$expectedPaths = @((Join-Path $binaryDir 'hive-daemon.exe'), (Join-Path $binaryDir 'entity-daemon.exe'))
$argumentPattern = '(?:^|\s)--data-dir(?:\s+|=)(?:"([^"]*)"|(\S+))(?=\s|$)'
function Get-OwnedDaemons {
    @(Get-CimInstance Win32_Process -Filter "Name = 'hive-daemon.exe' OR Name = 'entity-daemon.exe'" | Where-Object {
        $candidate = $_
        $pathMatches = $false
        if ($candidate.ExecutablePath) {
            $path = [IO.Path]::GetFullPath($candidate.ExecutablePath)
            $pathMatches = [bool]($expectedPaths | Where-Object { $_.Equals($path, [StringComparison]::OrdinalIgnoreCase) })
        }
        $dataMatches = $false
        if ($candidate.CommandLine -and $candidate.CommandLine -match $argumentPattern) {
            $argument = if ($Matches[1]) { $Matches[1] } else { $Matches[2] }
            $dataMatches = [IO.Path]::GetFullPath($argument).Equals($dataDir, [StringComparison]::OrdinalIgnoreCase)
        }
        $pathMatches -and $dataMatches
    })
}
$stopped = @()
# Stop the supervisor first so it cannot respawn a helper during cleanup.
$owned = @(Get-OwnedDaemons | Sort-Object @{ Expression = { if ($_.Name -eq 'hive-daemon.exe') { 0 } else { 1 } } })
foreach ($snapshot in $owned) {
    $handle = $null
    try {
        $handle = [Diagnostics.Process]::GetProcessById($snapshot.ProcessId)
        # Opening the process handle before rechecking binds Kill to this
        # process instance even if Windows later recycles its numeric PID.
        [void]$handle.Handle
        $current = Get-CimInstance Win32_Process -Filter "ProcessId = $($snapshot.ProcessId)"
        if (-not $current) { continue }
        if ($current.CreationDate -ne $snapshot.CreationDate -or
            $current.ExecutablePath -ne $snapshot.ExecutablePath -or
            $current.CommandLine -ne $snapshot.CommandLine) { continue }
        $handle.Kill()
        if (-not $handle.WaitForExit(5000)) { throw "Owned daemon did not exit: $($snapshot.ProcessId)" }
        $stopped += @{ pid = $snapshot.ProcessId; executable = $snapshot.ExecutablePath }
    } catch [ArgumentException] {
        # A verified process may have exited naturally before its handle opens.
    } finally {
        if ($handle) { $handle.Dispose() }
    }
}
$remaining = @(Get-OwnedDaemons)
if ($remaining.Count) { throw "Acceptance-owned daemons remain: $($remaining.ProcessId -join ', ')" }
ConvertTo-Json -Compress -InputObject @($stopped)
`;

export async function stopIsolatedWindowsDaemons({ binaryDir, dataDir }) {
  assert.equal(process.platform, 'win32', 'Windows process verification requires Windows');
  const encodedCommand = Buffer.from(cleanupScript, 'utf16le').toString('base64');
  const child = spawn('powershell.exe', ['-NoProfile', '-NonInteractive', '-EncodedCommand', encodedCommand], {
    windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'],
    env: { ...process.env, ABIGAIL_TEST_CLEANUP_BIN_DIR: resolve(binaryDir), ABIGAIL_TEST_CLEANUP_DATA_DIR: resolve(dataDir) }
  });
  let stdout = ''; let stderr = '';
  child.stdout.on('data', chunk => { stdout += chunk; });
  child.stderr.on('data', chunk => { stderr += chunk; });
  const timer = setTimeout(() => child.kill(), 30000);
  try {
    const code = await new Promise((resolvePromise, reject) => {
      child.once('error', reject); child.once('exit', resolvePromise);
    });
    assert.equal(code, 0, `Scoped daemon cleanup failed: ${stderr.trim() || 'PowerShell timed out'}`);
    return JSON.parse(stdout.trim() || '[]');
  } finally { clearTimeout(timer); }
}
