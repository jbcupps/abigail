#requires -Version 7.0
# Exercise the actual workflow guard with a native gh stub. No network or credentials.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if (-not $IsWindows) { throw 'The publication guard regression requires Windows.' }
function Get-WorkflowGuard([string]$Workflow, [string]$StepName) {
    $lines = [IO.File]::ReadAllLines((Join-Path $PSScriptRoot "../../.github/workflows/$Workflow"))
    $start = @((0..($lines.Length - 1)) | Where-Object {
        $lines[$_] -eq "      - name: $StepName"
    })
    if ($start.Count -ne 1) { throw "Expected exactly one release absence guard in $Workflow." }
    $body = [Collections.Generic.List[string]]::new()
    $inRun = $false
    for ($index = $start[0] + 1; $index -lt $lines.Length; $index++) {
        $line = $lines[$index]
        if ($line.StartsWith('      - name:')) { break }
        if (-not $inRun) {
            if ($line -eq '        run: |') { $inRun = $true }
            continue
        }
        if ([string]::IsNullOrWhiteSpace($line)) { $body.Add(''); continue }
        if (-not $line.StartsWith('          ')) { throw 'Unexpected guard script indentation.' }
        $body.Add($line.Substring(10))
    }
    if (-not $inRun -or $body.Count -eq 0) { throw 'Release absence guard has no script.' }
    return ($body -join "`n")
}

$tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\', '/')
$workspace = Join-Path $tempRoot ('abigail-release-guard-tests-' + [guid]::NewGuid().ToString('N'))
$stubDir = Join-Path $workspace 'bin'
New-Item -ItemType Directory -Path $stubDir -Force | Out-Null
$stubPath = Join-Path $stubDir 'gh.cmd'
$runnerPath = Join-Path $workspace 'actions-step.ps1'
$pwshPath = Join-Path $PSHOME 'pwsh.exe'
$script:passed = 0
$script:guardName = ''

# GitHub's built-in pwsh shell prepends this preference and appends this exact
# exit-code epilogue. The path assertion ensures no real gh command can be used.
$prefix = @'
$ErrorActionPreference = 'stop'
if ((Get-Command gh).Source -ne $env:ABIGAIL_GUARD_TEST_STUB) {
    throw 'The harmless gh stub was not selected.'
}
'@
$epilogue = 'if ((Test-Path -LiteralPath variable:\LASTEXITCODE)) { exit $LASTEXITCODE }'

function Assert-GuardResult([string]$Name, [int]$NativeExit, [string]$Response, [int]$ExpectedExit, [string]$ExpectedError = '') {
    # Response strings below are fixed synthetic text, never external input.
    $stub = @"
@echo off
if not "%~1"=="api" exit /b 91
if "%~2"=="repos/example/fixture/releases/latest" (
  echo {"tag_name":"v0.0.75","draft":false,"prerelease":false}
  exit /b 0
)
if not "%~2"=="repos/example/fixture/releases/tags/v9.9.9" exit /b 92
echo $Response 1>&2
exit /b $NativeExit
"@
    [IO.File]::WriteAllText($stubPath, $stub, [Text.ASCIIEncoding]::new())
    $startInfo = [Diagnostics.ProcessStartInfo]::new($pwshPath)
    $startInfo.UseShellExecute = $false
    $startInfo.CreateNoWindow = $true
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    foreach ($argument in @('-NoLogo', '-NoProfile', '-NonInteractive', '-File', $runnerPath)) {
        $startInfo.ArgumentList.Add($argument)
    }
    $startInfo.Environment['PATH'] = $stubDir + [IO.Path]::PathSeparator + $env:PATH
    $startInfo.Environment['ABIGAIL_GUARD_TEST_STUB'] = $stubPath
    $startInfo.Environment['GITHUB_REPOSITORY'] = 'example/fixture'
    $startInfo.Environment['RELEASE_TAG'] = 'v9.9.9'
    $null = $startInfo.Environment.Remove('GH_TOKEN')
    $null = $startInfo.Environment.Remove('GITHUB_TOKEN')
    $process = [Diagnostics.Process]::new()
    $process.StartInfo = $startInfo
    try {
        if (-not $process.Start()) { throw 'Unable to start the guard regression process.' }
        $stdout = $process.StandardOutput.ReadToEndAsync()
        $stderr = $process.StandardError.ReadToEndAsync()
        if (-not $process.WaitForExit(15000)) {
            $process.Kill($true)
            throw "$Name exceeded the test timeout."
        }
        $diagnostic = $stdout.GetAwaiter().GetResult() + $stderr.GetAwaiter().GetResult()
        if ($process.ExitCode -ne $ExpectedExit) {
            throw "$Name returned $($process.ExitCode), expected $ExpectedExit. $diagnostic"
        }
        if ($ExpectedError -and $diagnostic -notmatch $ExpectedError) {
            throw "$Name failed for an unexpected reason. $diagnostic"
        }
    } finally { $process.Dispose() }
    $script:passed++
    Write-Host "PASS $script:guardName - $Name"
}

try {
    $guards = @(
        @{ Workflow = 'release.yml'; StepName = 'Refuse replacement of an existing release' },
        @{ Workflow = 'recover-stable-0.0.76.yml'; StepName = 'Refuse replacement or latest-channel regression' }
    )
    foreach ($guard in $guards) {
        $script:guardName = $guard.Workflow
        $body = Get-WorkflowGuard $guard.Workflow $guard.StepName
        [IO.File]::WriteAllText($runnerPath, "$prefix`n$body`n$epilogue`n", [Text.UTF8Encoding]::new($false))
        Assert-GuardResult 'confirmed missing release survives the Actions exit wrapper' 1 'gh: Not Found (HTTP 404)' 0
        Assert-GuardResult 'existing release is refused' 0 '{"id":123}' 1 'already exists'
        Assert-GuardResult 'authentication failure is refused' 1 'gh: Bad credentials (HTTP 401)' 1 'Unable to confirm'
        Assert-GuardResult 'permission failure is refused' 1 'gh: Forbidden (HTTP 403)' 1 'Unable to confirm'
        Assert-GuardResult 'transport failure is refused' 1 'connection timed out' 1 'Unable to confirm'
        Assert-GuardResult 'successful response cannot masquerade as missing' 0 'HTTP 404' 1 'already exists'
    }
    Write-Host "Release absence guard: $script:passed checks passed using the workflow script and a harmless native stub."
} finally {
    $resolvedWorkspace = [IO.Path]::GetFullPath($workspace)
    if ([IO.Path]::GetDirectoryName($resolvedWorkspace) -ne $tempRoot -or
        [IO.Path]::GetFileName($resolvedWorkspace) -notmatch '^abigail-release-guard-tests-[0-9a-f]{32}$') {
        throw 'Refusing to clean an unexpected guard test directory.'
    }
    if (Test-Path -LiteralPath $resolvedWorkspace) { Remove-Item -LiteralPath $resolvedWorkspace -Recurse -Force }
}
