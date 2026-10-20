#requires -Version 7.0
<# Tests only the extracted capture function with synthetic paths and mocked copies.
   The signing entrypoint, modules, credentials, services, and executables never run. #>
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$signer = Join-Path $PSScriptRoot '../sign_windows_artifact.ps1'
$tokens = $null
$errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile($signer, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Windows signing callback contains PowerShell syntax errors.' }
$functions = @($ast.FindAll({
    param($node)
    $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -ceq 'Save-SignedMain'
}, $true))
if ($functions.Count -ne 1) { throw 'Expected exactly one signed launcher capture function.' }
. ([scriptblock]::Create($functions[0].Extent.Text))

# Only these two noncredential settings are read or changed. Restore both afterward.
$previousName = $env:ABIGAIL_SIGNED_MAIN_NAME
$previousPath = $env:ABIGAIL_SIGNED_MAIN_PATH
$fixtureRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot 'synthetic-capture-fixture'))
$script:copies = @()
$passed = 0
function Copy-Item {
    param([string]$LiteralPath, [string]$Destination, [switch]$Force)
    $script:copies += [pscustomobject]@{ source = $LiteralPath; destination = $Destination; force = $Force.IsPresent }
}
function Test-Capture {
    param(
        [string]$Label, [AllowNull()][string]$ExpectedName, [string]$InputName,
        [AllowNull()][string]$Destination, [bool]$ShouldCopy = $false, [string]$ErrorMatch = ''
    )
    $env:ABIGAIL_SIGNED_MAIN_NAME = $ExpectedName
    $env:ABIGAIL_SIGNED_MAIN_PATH = $Destination
    $script:name = $InputName
    $script:resolved = Join-Path $fixtureRoot "build/$InputName"
    $script:copies = @()
    $failure = $null
    try { Save-SignedMain } catch { $failure = $_.Exception.Message }
    if ($ErrorMatch) {
        if (-not $failure -or $failure -notmatch $ErrorMatch -or $script:copies.Count -ne 0) {
            throw "Expected capture rejection: $Label"
        }
    } elseif ($failure) {
        throw "Unexpected capture rejection: $Label ($failure)"
    } elseif ($ShouldCopy) {
        if ($script:copies.Count -ne 1 -or $script:copies[0].source -cne $script:resolved -or
            $script:copies[0].destination -cne [IO.Path]::GetFullPath($Destination) -or -not $script:copies[0].force) {
            throw "Wrong signed executable capture: $Label"
        }
    } elseif ($script:copies.Count -ne 0) {
        throw "Unexpected executable capture: $Label"
    }
    $script:passed++
    Write-Host "PASS $Label"
}
try {
    $stableCapture = Join-Path $fixtureRoot 'capture/Abigail.exe'
    $previewCapture = Join-Path $fixtureRoot 'capture/Abigail Preview.exe'
    Test-Capture -Label 'default stable launcher' -ExpectedName $null -InputName 'Abigail.exe' -Destination $stableCapture -ShouldCopy $true
    Test-Capture -Label 'explicit stable launcher' -ExpectedName 'Abigail.exe' -InputName 'Abigail.exe' -Destination $stableCapture -ShouldCopy $true
    Test-Capture -Label 'explicit preview launcher' -ExpectedName 'Abigail Preview.exe' -InputName 'Abigail Preview.exe' -Destination $previewCapture -ShouldCopy $true
    Test-Capture -Label 'stable default does not capture preview' -ExpectedName $null -InputName 'Abigail Preview.exe' -Destination $stableCapture
    Test-Capture -Label 'preview selection does not capture stable' -ExpectedName 'Abigail Preview.exe' -InputName 'Abigail.exe' -Destination $previewCapture
    Test-Capture -Label 'no capture destination' -ExpectedName 'Abigail Preview.exe' -InputName 'Abigail Preview.exe' -Destination $null
    foreach ($other in @('abigail-entity-runtime-app.exe', 'hive-daemon.exe', 'entity-daemon.exe', 'setup.exe', 'uninstaller.exe', 'nst123.tmp')) {
        Test-Capture -Label "preview selection does not capture $other" -ExpectedName 'Abigail Preview.exe' -InputName $other -Destination $previewCapture
    }
    foreach ($invalid in @('Other.exe', '../Abigail.exe', 'Abigail Preview', ' ', 'abigail.exe')) {
        Test-Capture -Label "invalid configured name [$invalid]" -ExpectedName $invalid -InputName 'Abigail.exe' -Destination $stableCapture -ErrorMatch 'capture name must be'
    }
    Test-Capture -Label 'preview cannot be captured as stable' -ExpectedName 'Abigail Preview.exe' -InputName 'Abigail Preview.exe' -Destination $stableCapture -ErrorMatch 'basename must match'
    Test-Capture -Label 'stable cannot be captured as preview' -ExpectedName 'Abigail.exe' -InputName 'Abigail.exe' -Destination $previewCapture -ErrorMatch 'basename must match'
    Test-Capture -Label 'destination casing must match selected name' -ExpectedName 'Abigail Preview.exe' -InputName 'Abigail Preview.exe' -Destination (Join-Path $fixtureRoot 'capture/ABIGAIL PREVIEW.EXE') -ErrorMatch 'basename must match'
    Test-Capture -Label 'relative destination rejected' -ExpectedName 'Abigail Preview.exe' -InputName 'Abigail Preview.exe' -Destination 'capture/Abigail Preview.exe' -ErrorMatch 'absolute path'
    Test-Capture -Label 'build executable cannot be overwritten' -ExpectedName 'Abigail Preview.exe' -InputName 'Abigail Preview.exe' -Destination (Join-Path $fixtureRoot 'build/Abigail Preview.exe') -ErrorMatch 'separate from the build'
    Test-Capture -Label 'stable capture still works after preview cases' -ExpectedName $null -InputName 'Abigail.exe' -Destination $stableCapture -ShouldCopy $true
} finally {
    $env:ABIGAIL_SIGNED_MAIN_NAME = $previousName
    $env:ABIGAIL_SIGNED_MAIN_PATH = $previousPath
}
Write-Host "PASS: $passed synthetic signed-launcher capture cases; no signing or executable execution."
