Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
$helperPath = Join-Path $repoRoot "scripts\dev\start_process_with_environment_overlay.ps1"
$launcherPath = Join-Path $repoRoot "scripts\dev\launch_split_stack.ps1"

. $helperPath

function Assert-Condition {
    param(
        [Parameter(Mandatory = $true)][bool]$Condition,
        [Parameter(Mandatory = $true)][string]$Message
    )

    if (-not $Condition) {
        throw $Message
    }
}

function Assert-Equal {
    param(
        [AllowNull()][object]$Expected,
        [AllowNull()][object]$Actual,
        [Parameter(Mandatory = $true)][string]$Message
    )

    if (-not [object]::Equals($Expected, $Actual)) {
        throw $Message
    }
}

function Test-IsEnvironmentParameterPrefix {
    param([AllowNull()][string]$Name)

    if ([string]::IsNullOrEmpty($Name)) {
        return $false
    }
    return "Environment".StartsWith($Name, [StringComparison]::OrdinalIgnoreCase)
}

function Get-StaticAstString {
    param([AllowNull()][Management.Automation.Language.Ast]$Node)

    if ($Node -is [Management.Automation.Language.StringConstantExpressionAst]) {
        return $Node.Value
    }
    if (
        $Node -is [Management.Automation.Language.ExpandableStringExpressionAst] -and
        $Node.NestedExpressions.Count -eq 0
    ) {
        return $Node.Value
    }
    return $null
}

function Assert-NoStartProcessEnvironmentParameter {
    param([Parameter(Mandatory = $true)][string]$Path)

    $tokens = $null
    $parseErrors = $null
    $ast = [Management.Automation.Language.Parser]::ParseFile(
        $Path,
        [ref]$tokens,
        [ref]$parseErrors
    )
    $fileName = [IO.Path]::GetFileName($Path)
    if ($parseErrors.Count -ne 0) {
        throw "PowerShell 5.1 syntax parse failed for $fileName."
    }

    $commands = @($ast.FindAll({
        param($node)
        $node -is [Management.Automation.Language.CommandAst] -and
        $node.GetCommandName() -eq "Start-Process"
    }, $true))

    foreach ($command in $commands) {
        foreach ($element in $command.CommandElements) {
            if (
                $element -is [Management.Automation.Language.CommandParameterAst] -and
                (Test-IsEnvironmentParameterPrefix -Name $element.ParameterName)
            ) {
                throw "Start-Process uses a forbidden environment parameter in $fileName."
            }
        }

        $splatVariables = @(
            foreach ($element in $command.CommandElements) {
                if (
                    $element -is [Management.Automation.Language.VariableExpressionAst] -and
                    $element.Splatted
                ) {
                    $element.VariablePath.UserPath
                }
            }
        )

        foreach ($splatVariable in $splatVariables) {
            $foundDefinition = $false
            $hashTables = @($ast.FindAll({
                param($node)
                $node -is [Management.Automation.Language.HashtableAst]
            }, $true))

            foreach ($hashTable in $hashTables) {
                if ($hashTable.Extent.StartOffset -gt $command.Extent.StartOffset) {
                    continue
                }

                $assignment = $hashTable.Parent
                while (
                    $null -ne $assignment -and
                    -not ($assignment -is [Management.Automation.Language.AssignmentStatementAst])
                ) {
                    $assignment = $assignment.Parent
                }
                if (
                    $null -eq $assignment -or
                    -not ($assignment.Left -is [Management.Automation.Language.VariableExpressionAst]) -or
                    $assignment.Left.VariablePath.UserPath -ne $splatVariable
                ) {
                    continue
                }

                $foundDefinition = $true
                foreach ($pair in $hashTable.KeyValuePairs) {
                    $keyName = Get-StaticAstString -Node $pair.Item1
                    if (Test-IsEnvironmentParameterPrefix -Name $keyName) {
                        throw "Start-Process uses a forbidden splatted environment key in $fileName."
                    }
                }
            }

            $assignments = @($ast.FindAll({
                param($node)
                $node -is [Management.Automation.Language.AssignmentStatementAst]
            }, $true))
            foreach ($assignment in $assignments) {
                if ($assignment.Extent.StartOffset -gt $command.Extent.StartOffset) {
                    continue
                }

                $keyName = $null
                if (
                    $assignment.Left -is [Management.Automation.Language.IndexExpressionAst] -and
                    $assignment.Left.Target -is [Management.Automation.Language.VariableExpressionAst] -and
                    $assignment.Left.Target.VariablePath.UserPath -eq $splatVariable
                ) {
                    $keyName = Get-StaticAstString -Node $assignment.Left.Index
                } elseif (
                    $assignment.Left -is [Management.Automation.Language.MemberExpressionAst] -and
                    $assignment.Left.Expression -is [Management.Automation.Language.VariableExpressionAst] -and
                    $assignment.Left.Expression.VariablePath.UserPath -eq $splatVariable
                ) {
                    $keyName = Get-StaticAstString -Node $assignment.Left.Member
                }

                if (Test-IsEnvironmentParameterPrefix -Name $keyName) {
                    throw "Start-Process uses a forbidden splatted environment key in $fileName."
                }
            }

            if (-not $foundDefinition) {
                throw "Start-Process uses an unverifiable splat in $fileName."
            }
        }
    }
}

function Assert-EnvironmentState {
    param(
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)][bool]$WasPresent,
        [AllowNull()][string]$Value,
        [Parameter(Mandatory = $true)][string]$Message
    )

    $state = Get-ProcessEnvironmentVariableExact -Name $Name
    Assert-Equal -Expected $WasPresent -Actual $state.WasPresent -Message $Message
    if ($WasPresent) {
        Assert-Equal -Expected $Value -Actual $state.Value -Message $Message
    }
}

function Assert-AstContractRejects {
    param([Parameter(Mandatory = $true)][string]$Path)

    $rejected = $false
    try {
        Assert-NoStartProcessEnvironmentParameter -Path $Path
    } catch {
        $rejected = $true
    }
    Assert-Condition -Condition $rejected -Message "AST contract accepted a forbidden Start-Process environment form."
}

Assert-Condition `
    -Condition (
        $PSVersionTable.PSEdition -eq "Desktop" -and
        $PSVersionTable.PSVersion.Major -eq 5 -and
        $PSVersionTable.PSVersion.Minor -ge 1
    ) `
    -Message "This contract must run under Windows PowerShell 5.1."

Assert-NoStartProcessEnvironmentParameter -Path $launcherPath
Assert-NoStartProcessEnvironmentParameter -Path $helperPath

$presentNonemptyName = "ABIGAIL_TEST_OVERLAY_PRESENT_NONEMPTY"
$presentEmptyName = "ABIGAIL_TEST_OVERLAY_PRESENT_EMPTY"
$absentName = "ABIGAIL_TEST_OVERLAY_ABSENT"
$names = @($presentNonemptyName, $presentEmptyName, $absentName)
$originalSnapshot = @{}
foreach ($name in $names) {
    $originalSnapshot[$name] = Get-ProcessEnvironmentVariableExact -Name $name
}

$tempRoot = Join-Path ([IO.Path]::GetTempPath()) ("abigail-overlay-" + [Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $tempRoot | Out-Null

try {
    $directFixture = Join-Path $tempRoot "direct-environment.ps1"
    $splatFixture = Join-Path $tempRoot "splatted-environment.ps1"
    Set-Content -LiteralPath $directFixture -Encoding ASCII -Value "Start-Process -Env @{ VALUE = 'synthetic' }"
    Set-Content -LiteralPath $splatFixture -Encoding ASCII -Value @'
$parameters = @{ Environment = @{ VALUE = "synthetic" } }
Start-Process @parameters
'@
    Assert-AstContractRejects -Path $directFixture
    Assert-AstContractRejects -Path $splatFixture

    Set-ProcessEnvironmentVariableExact `
        -Name $presentNonemptyName `
        -Present $true `
        -Value "synthetic-parent-nonempty"
    Set-ProcessEnvironmentVariableExact `
        -Name $presentEmptyName `
        -Present $true `
        -Value ""
    Set-ProcessEnvironmentVariableExact `
        -Name $absentName `
        -Present $false `
        -Value $null

    Assert-EnvironmentState `
        -Name $presentNonemptyName `
        -WasPresent $true `
        -Value "synthetic-parent-nonempty" `
        -Message "Failed to establish the present nonempty parent state."
    Assert-EnvironmentState `
        -Name $presentEmptyName `
        -WasPresent $true `
        -Value "" `
        -Message "Failed to establish the present empty parent state."
    Assert-EnvironmentState `
        -Name $absentName `
        -WasPresent $false `
        -Value $null `
        -Message "Failed to establish the absent parent state."

    $overlay = @{}
    $overlay[$presentNonemptyName] = "synthetic-child-nonempty"
    $overlay[$presentEmptyName] = "synthetic-child-from-empty"
    $overlay[$absentName] = "synthetic-child-from-absent"

    $childOutputPath = Join-Path $tempRoot "child-environment.json"
    $escapedOutputPath = $childOutputPath.Replace("'", "''")
    $childCommand = @"
`$values = [ordered]@{
    present_nonempty = [Environment]::GetEnvironmentVariable('$presentNonemptyName', [EnvironmentVariableTarget]::Process)
    present_empty = [Environment]::GetEnvironmentVariable('$presentEmptyName', [EnvironmentVariableTarget]::Process)
    absent = [Environment]::GetEnvironmentVariable('$absentName', [EnvironmentVariableTarget]::Process)
}
[IO.File]::WriteAllText('$escapedOutputPath', (`$values | ConvertTo-Json -Compress))
"@
    $encodedCommand = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($childCommand))
    $powershellExe = Join-Path $PSHOME "powershell.exe"
    $childProcess = Start-ProcessWithEnvironmentOverlay `
        -FilePath $powershellExe `
        -ArgumentList @("-NoProfile", "-NonInteractive", "-EncodedCommand", $encodedCommand) `
        -WorkingDirectory $tempRoot `
        -EnvironmentOverlay $overlay `
        -Wait `
        -PassThru

    Assert-Equal -Expected 0 -Actual $childProcess.ExitCode -Message "Overlay child failed."
    Assert-Condition -Condition (Test-Path -LiteralPath $childOutputPath) -Message "Overlay child wrote no result."
    $childValues = Get-Content -Raw -LiteralPath $childOutputPath | ConvertFrom-Json
    Assert-Equal `
        -Expected "synthetic-child-nonempty" `
        -Actual $childValues.present_nonempty `
        -Message "Child did not receive the nonempty overlay."
    Assert-Equal `
        -Expected "synthetic-child-from-empty" `
        -Actual $childValues.present_empty `
        -Message "Child did not receive the overlay for the empty parent variable."
    Assert-Equal `
        -Expected "synthetic-child-from-absent" `
        -Actual $childValues.absent `
        -Message "Child did not receive the overlay for the absent parent variable."

    Assert-EnvironmentState `
        -Name $presentNonemptyName `
        -WasPresent $true `
        -Value "synthetic-parent-nonempty" `
        -Message "Present nonempty state was not restored after a successful spawn."
    Assert-EnvironmentState `
        -Name $presentEmptyName `
        -WasPresent $true `
        -Value "" `
        -Message "Present empty state was not restored after a successful spawn."
    Assert-EnvironmentState `
        -Name $absentName `
        -WasPresent $false `
        -Value $null `
        -Message "Absent state was not restored after a successful spawn."

    $missingFile = Join-Path $tempRoot "does-not-exist.exe"
    $spawnFailed = $false
    try {
        Start-ProcessWithEnvironmentOverlay `
            -FilePath $missingFile `
            -WorkingDirectory $tempRoot `
            -EnvironmentOverlay $overlay `
            -PassThru | Out-Null
    } catch {
        $spawnFailed = $true
    }
    Assert-Condition -Condition $spawnFailed -Message "Missing child path did not fail."

    Assert-EnvironmentState `
        -Name $presentNonemptyName `
        -WasPresent $true `
        -Value "synthetic-parent-nonempty" `
        -Message "Present nonempty state was not restored after spawn failure."
    Assert-EnvironmentState `
        -Name $presentEmptyName `
        -WasPresent $true `
        -Value "" `
        -Message "Present empty state was not restored after spawn failure."
    Assert-EnvironmentState `
        -Name $absentName `
        -WasPresent $false `
        -Value $null `
        -Message "Absent state was not restored after spawn failure."

    Set-ProcessEnvironmentVariableExact `
        -Name $absentName `
        -Present $true `
        -Value "synthetic-before-aggregate-restore"
    $aggregateSnapshot = @{}
    $aggregateSnapshot["A=INVALID"] = [PSCustomObject]@{ WasPresent = $false; Value = $null }
    $aggregateSnapshot[$absentName] = [PSCustomObject]@{ WasPresent = $false; Value = $null }
    $aggregateSnapshot["Z=INVALID"] = [PSCustomObject]@{ WasPresent = $false; Value = $null }
    $aggregateMessage = $null
    try {
        Restore-ProcessEnvironmentSnapshotExact -Snapshot $aggregateSnapshot
    } catch {
        $aggregateMessage = $_.Exception.Message
    }
    Assert-Equal `
        -Expected "Process environment restoration failed for: A=INVALID, Z=INVALID." `
        -Actual $aggregateMessage `
        -Message "Restore failures were not aggregated with variable names only."
    Assert-EnvironmentState `
        -Name $absentName `
        -WasPresent $false `
        -Value $null `
        -Message "A restore failure prevented a later variable restoration attempt."

    $invalidOverlays = @()
    $emptyOverlay = @{}
    $emptyOverlay[$presentNonemptyName] = ""
    $invalidOverlays += $emptyOverlay
    $nullOverlay = @{}
    $nullOverlay[$presentNonemptyName] = $null
    $invalidOverlays += $nullOverlay

    foreach ($invalidOverlay in $invalidOverlays) {
        $validationMessage = $null
        try {
            Start-ProcessWithEnvironmentOverlay `
                -FilePath $missingFile `
                -WorkingDirectory $tempRoot `
                -EnvironmentOverlay $invalidOverlay `
                -PassThru | Out-Null
        } catch {
            $validationMessage = $_.Exception.Message
        }
        Assert-Equal `
            -Expected "Environment overlay requires nonempty values for: $presentNonemptyName." `
            -Actual $validationMessage `
            -Message "Null or empty overlay value was not rejected before spawning."
    }
} finally {
    Restore-ProcessEnvironmentSnapshotExact -Snapshot $originalSnapshot
    if (Test-Path -LiteralPath $tempRoot) {
        Remove-Item -LiteralPath $tempRoot -Recurse -Force
    }
}

Write-Host "Windows PowerShell 5.1 environment overlay contract passed."
