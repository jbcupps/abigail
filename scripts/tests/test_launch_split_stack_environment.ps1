Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
$helperPath = Join-Path $repoRoot "scripts\dev\start_process_with_environment_overlay.ps1"
$launcherPath = Join-Path $repoRoot "scripts\dev\launch_split_stack.ps1"
. $helperPath

$childWaitTimeoutMs = 15000

function Assert-True {
    param([bool]$Condition, [string]$Message)
    if (-not $Condition) { throw $Message }
}

function Assert-SafeProcessCommands {
    param([string]$Path)

    $tokens = $null
    $errors = $null
    $ast = [Management.Automation.Language.Parser]::ParseFile(
        $Path, [ref]$tokens, [ref]$errors
    )
    $fileName = [IO.Path]::GetFileName($Path)
    if ($errors.Count) { throw "PowerShell 5.1 syntax parse failed for $fileName." }

    $commands = @($ast.FindAll({
        param($node)
        if (-not ($node -is [Management.Automation.Language.CommandAst])) { return $false }
        $name = $node.GetCommandName()
        return $name -and @("start-process", "start", "saps") -contains $name.ToLowerInvariant()
    }, $true))

    foreach ($command in $commands) {
        foreach ($element in $command.CommandElements) {
            if ($element -is [Management.Automation.Language.VariableExpressionAst] -and $element.Splatted) {
                throw "Process launch uses an unverifiable splat in $fileName."
            }
            if (
                $element -is [Management.Automation.Language.CommandParameterAst] -and
                $element.ParameterName -and
                "Environment".StartsWith($element.ParameterName, [StringComparison]::OrdinalIgnoreCase)
            ) {
                throw "Process launch uses a forbidden environment parameter in $fileName."
            }
        }
    }
    return $commands.Count
}

function Assert-ContractRejects {
    param(
        [string]$Path,
        [string]$ExpectedMessageSubstring
    )

    $message = $null
    try {
        Assert-SafeProcessCommands -Path $Path | Out-Null
        throw "AST contract accepted a forbidden process launch."
    } catch {
        $message = $_.Exception.Message
        if ($message -eq "AST contract accepted a forbidden process launch.") {
            throw
        }
    }
    Assert-True (
        $message -and $message.Contains($ExpectedMessageSubstring)
    ) "Contract rejection mismatch: $message"
}

function Assert-ParentUnchanged {
    param([string]$PresentName, [string]$AbsentName)
    $present = [Environment]::GetEnvironmentVariable(
        $PresentName, [EnvironmentVariableTarget]::Process
    )
    $absent = [Environment]::GetEnvironmentVariable(
        $AbsentName, [EnvironmentVariableTarget]::Process
    )
    Assert-True ($present -ceq "synthetic-parent") "Parent present value changed."
    Assert-True ($null -eq $absent) "Parent absent variable was created."
}

function Stop-ChildProcessIfNeeded {
    param([Diagnostics.Process]$Process)

    if ($null -eq $Process) { return }
    try {
        if (-not $Process.HasExited) {
            $Process.Kill()
            $Process.WaitForExit(5000) | Out-Null
        }
    } catch {
    } finally {
        $Process.Dispose()
    }
}

Assert-True (
    $PSVersionTable.PSEdition -eq "Desktop" -and
    $PSVersionTable.PSVersion.Major -eq 5 -and
    $PSVersionTable.PSVersion.Minor -ge 1
) "This contract must run under Windows PowerShell 5.1."

$launcherCommands = Assert-SafeProcessCommands -Path $launcherPath
Assert-True ($launcherCommands -gt 0) "Launcher AST scan found no process commands."
Assert-SafeProcessCommands -Path $helperPath | Out-Null

$helperSource = Get-Content -Raw -LiteralPath $helperPath
foreach ($required in @(
    "System\.Diagnostics\.ProcessStartInfo",
    "\.EnvironmentVariables\s*\[",
    "\[System\.Diagnostics\.Process\]::Start\s*\("
)) {
    Assert-True ($helperSource -match $required) "Helper child-environment structure is missing."
}
foreach ($forbidden in @(
    "(?i)\b(?:Add-Type|DllImport|SetEnvironmentVariable)\b",
    "(?im)\b(?:Set-Item|Remove-Item|New-Item)\b[^\r\n]*\bEnv:",
    '(?im)\$env:[^=\r\n]+\s*='
)) {
    Assert-True ($helperSource -notmatch $forbidden) "Helper can mutate the parent environment."
}

# No parent write exists, so a present-empty value is also preserved without
# setup, inspection, or round-tripping through version-sensitive APIs.
$suffix = [Guid]::NewGuid().ToString("N").ToUpperInvariant()
$presentName = "ABIGAIL_TEST_CHILD_ENV_PRESENT_$suffix"
$absentName = "ABIGAIL_TEST_CHILD_ENV_ABSENT_$suffix"
$inheritName = "ABIGAIL_TEST_CHILD_ENV_INHERIT_$suffix"
$tempRoot = Join-Path ([IO.Path]::GetTempPath()) ("abigail-child-env-" + $suffix)
New-Item -ItemType Directory -Path $tempRoot | Out-Null
$childProcess = $null

try {
    $directFixture = Join-Path $tempRoot "direct.ps1"
    $splatFixture = Join-Path $tempRoot "splat.ps1"
    Set-Content $directFixture "saps -Env @{ VALUE = 'synthetic' }"
    Set-Content $splatFixture '$p = @{ FilePath = "synthetic.exe" }; start @p'
    Assert-ContractRejects $directFixture "forbidden environment parameter in direct.ps1"
    Assert-ContractRejects $splatFixture "unverifiable splat in splat.ps1"

    [Environment]::SetEnvironmentVariable(
        $presentName, "synthetic-parent", [EnvironmentVariableTarget]::Process
    )
    [Environment]::SetEnvironmentVariable(
        $absentName, $null, [EnvironmentVariableTarget]::Process
    )
    [Environment]::SetEnvironmentVariable(
        $inheritName, "synthetic-parent-inherit", [EnvironmentVariableTarget]::Process
    )
    Assert-ParentUnchanged $presentName $absentName

    $outputPath = Join-Path $tempRoot "child.txt"
    $escapedOutputPath = $outputPath.Replace("'", "''")
    $childScript = @"
Set-StrictMode -Version Latest
`$ErrorActionPreference = 'Stop'
`$outputPath = '$escapedOutputPath'
`$present = [Environment]::GetEnvironmentVariable('$presentName', 'Process')
`$absent = [Environment]::GetEnvironmentVariable('$absentName', 'Process')
`$inherited = [Environment]::GetEnvironmentVariable('$inheritName', 'Process')
Set-Content -LiteralPath `$outputPath -Encoding ASCII -Value @(
    "present=`$present"
    "absent=`$absent"
    "inherited=`$inherited"
)
"@
    $encodedCommand = [Convert]::ToBase64String(
        [System.Text.Encoding]::Unicode.GetBytes($childScript)
    )
    $powershellExe = Join-Path $env:WINDIR "System32\WindowsPowerShell\v1.0\powershell.exe"
    $argumentString = "-NoProfile -NonInteractive -EncodedCommand $encodedCommand"

    $overlay = @{}
    $overlay[$presentName] = "synthetic-child-present"
    $overlay[$absentName] = "synthetic-child-absent"
    $childProcess = Start-ProcessWithEnvironmentOverlay `
        -FilePath $powershellExe `
        -WorkingDirectory $tempRoot `
        -EnvironmentOverlay $overlay `
        -Arguments $argumentString
    Assert-True ($childProcess -is [Diagnostics.Process]) "Helper returned no Process."
    if (-not $childProcess.WaitForExit($childWaitTimeoutMs)) {
        throw "Synthetic child did not exit within 15 seconds."
    }
    Assert-True ($childProcess.ExitCode -eq 0) "Synthetic child failed."
    $childLines = @(Get-Content $outputPath)
    Assert-True ($childLines -contains "present=synthetic-child-present") "Present overlay missing."
    Assert-True ($childLines -contains "absent=synthetic-child-absent") "Absent overlay missing."
    Assert-True ($childLines -contains "inherited=synthetic-parent-inherit") "Parent-only inherit missing."
    Assert-ParentUnchanged $presentName $absentName
    Stop-ChildProcessIfNeeded $childProcess
    $childProcess = $null

    $missing = Join-Path $tempRoot "does-not-exist.exe"
    try {
        Start-ProcessWithEnvironmentOverlay $missing $tempRoot $overlay | Out-Null
        throw "Missing child path did not fail."
    } catch {
        if ($_.Exception.Message -eq "Missing child path did not fail.") { throw }
    }
    Assert-ParentUnchanged $presentName $absentName

    $empty = @{}; $empty[$presentName] = ""
    $nullValue = @{}; $nullValue[$presentName] = $null
    $invalidName = @{ "INVALID=NAME" = "synthetic" }
    $invalidCases = @(
        [PSCustomObject]@{
            Overlay = $empty
            Message = "Environment overlay requires nonempty values for: $presentName."
        }
        [PSCustomObject]@{
            Overlay = $nullValue
            Message = "Environment overlay requires nonempty values for: $presentName."
        }
        [PSCustomObject]@{
            Overlay = $invalidName
            Message = "Environment overlay contains an invalid variable name."
        }
    )
    foreach ($invalidCase in $invalidCases) {
        $message = $null
        try {
            Start-ProcessWithEnvironmentOverlay $missing $tempRoot $invalidCase.Overlay | Out-Null
        } catch {
            $message = $_.Exception.Message
        }
        Assert-True ($message -ceq $invalidCase.Message) "Invalid overlay was not rejected before spawn."
    }
} finally {
    Stop-ChildProcessIfNeeded $childProcess
    [Environment]::SetEnvironmentVariable(
        $presentName, $null, [EnvironmentVariableTarget]::Process
    )
    [Environment]::SetEnvironmentVariable(
        $absentName, $null, [EnvironmentVariableTarget]::Process
    )
    [Environment]::SetEnvironmentVariable(
        $inheritName, $null, [EnvironmentVariableTarget]::Process
    )
    if (Test-Path $tempRoot) { Remove-Item $tempRoot -Recurse -Force }
}

Write-Host "Windows PowerShell 5.1 child environment contract passed."
