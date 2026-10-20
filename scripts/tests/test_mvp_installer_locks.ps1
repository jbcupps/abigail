<#
.SYNOPSIS
Compile the real MVP NSIS hooks and exercise Windows payload locks.
.DESCRIPTION
Uses a unique fixture registry key and isolated files. Never installs Abigail,
opens family data, or stops existing Abigail processes. Tests the early guard,
silent installer, directory selection, and uninstaller with live Windows locks.
#>
[CmdletBinding()]
param([string]$MakeNsisPath)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if ($env:OS -ne 'Windows_NT') { throw 'NSIS file-lock acceptance requires Windows.' }
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
if (-not $MakeNsisPath) {
    $command = Get-Command makensis.exe -ErrorAction SilentlyContinue
    $candidates = @(@(
        $(if ($command) { $command.Source }),
        "${env:ProgramFiles(x86)}\NSIS\makensis.exe",
        "$env:LOCALAPPDATA\tauri\NSIS\makensis.exe"
    ) | Where-Object { $_ -and (Test-Path -LiteralPath $_ -PathType Leaf) })
    if (-not $candidates) { throw 'Install NSIS or supply -MakeNsisPath.' }
    $MakeNsisPath = $candidates[0]
}
$MakeNsisPath = (Resolve-Path -LiteralPath $MakeNsisPath).Path
$fixtureId = [guid]::NewGuid().ToString('N')
$fixtureRoot = Join-Path $repoRoot "target/manual-test/installer-locks-$fixtureId"
$registryRoot = "Software\AbigailInstallerLockFixture\$fixtureId"
$registryPath = "HKCU:\$registryRoot"
New-Item -ItemType Directory -Path $fixtureRoot | Out-Null
$hooksPath = Join-Path $repoRoot 'scripts/nsis-mvp-hooks.nsh'
$installerPath = Join-Path $fixtureRoot 'lock-fixture.exe'
$sourcePath = Join-Path $fixtureRoot 'lock-fixture.nsi'
$source = @'
Unicode true
RequestExecutionLevel user
!include LogicLib.nsh
!include FileFunc.nsh
!define ABIGAIL_INSTALL_LOCATION_REGISTRY_KEY "@@REGISTRY@@\Product"
!define ABIGAIL_UNINSTALL_REGISTRY_ROOT "@@REGISTRY@@\Uninstall"
!include "@@HOOKS@@"
Name "Abigail lock acceptance fixture"
OutFile "@@INSTALLER@@"
InstallDir "@@ROOT@@\unused"
Page instfiles
UninstPage instfiles

Function .onInit
  SetRegView 64
  ReadEnvStr $0 ABIGAIL_LOCK_FIXTURE_EARLY
  ${If} $0 == "1"
    ; Emulate entering the reinstall page before its old-uninstaller action.
    Call AbigailGuardRegisteredInstall
    FileOpen $0 "$INSTDIR\old-uninstaller-was-run.txt" w
    FileWrite $0 "This must never happen with a locked prior payload."
    FileClose $0
  ${EndIf}
FunctionEnd

Section "Fixture mutation"
  ReadEnvStr $0 ABIGAIL_LOCK_FIXTURE_SELECTED_ROOT
  ${If} $0 != ""
    StrCpy $INSTDIR $0
  ${EndIf}
  !insertmacro NSIS_HOOK_PREINSTALL
  CreateDirectory "$INSTDIR"
  FileOpen $0 "$INSTDIR\mutation.txt" w
  FileWrite $0 "Installer passed all guards."
  FileClose $0
  WriteUninstaller "$INSTDIR\uninstall-fixture.exe"
SectionEnd

Section "Uninstall"
  !insertmacro NSIS_HOOK_PREUNINSTALL
  FileOpen $0 "$INSTDIR\uninstall-mutation.txt" w
  FileWrite $0 "Uninstaller passed the guard."
  FileClose $0
SectionEnd
'@
$source = $source.Replace('@@REGISTRY@@', $registryRoot).Replace('@@HOOKS@@', $hooksPath).
    Replace('@@INSTALLER@@', $installerPath).Replace('@@ROOT@@', $fixtureRoot)
[IO.File]::WriteAllText($sourcePath, $source, [Text.UTF8Encoding]::new($false))
& $MakeNsisPath /V2 $sourcePath
if ($LASTEXITCODE -ne 0) { throw 'Failed to compile the real installer lock hooks.' }

$payloadPaths = @('Abigail.exe', 'resources/abigail-entity-runtime-app.exe',
    'resources/hive-daemon.exe', 'resources/entity-daemon.exe')
$script:stages = [Collections.Generic.List[string]]::new()
function New-PayloadFixture([string]$Name) {
    $root = Join-Path $fixtureRoot $Name
    New-Item -ItemType Directory -Path (Join-Path $root 'resources') -Force | Out-Null
    foreach ($relative in $payloadPaths) {
        [IO.File]::WriteAllText((Join-Path $root $relative), "old immutable payload: $relative")
    }
    [IO.File]::WriteAllText((Join-Path $root 'family-data-canary.txt'), 'family data stays untouched')
    return $root
}
function Get-FixtureHashes([string]$Root) {
    $hashes = @{}
    foreach ($file in Get-ChildItem -LiteralPath $Root -File -Recurse) {
        $hashes[$file.FullName] = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash
    }
    return $hashes
}
function Assert-Unchanged([hashtable]$Before, [string]$Root) {
    $after = Get-FixtureHashes $Root
    if ($after.Count -ne $Before.Count) { throw "Guard changed the file set in $Root" }
    foreach ($path in $Before.Keys) {
        if ($after[$path] -ne $Before[$path]) { throw "Guard changed bytes in $path" }
    }
}
function Invoke-Fixture([string]$Root, [string]$Executable = $installerPath,
    [hashtable]$Environment = @{}, [switch]$Uninstall) {
    $arguments = if ($Uninstall) { @('/S', "_?=$Root") } else { @('/S', "/D=$Root") }
    $startArguments = @{
        FilePath = $Executable; ArgumentList = $arguments; PassThru = $true;
        WindowStyle = 'Hidden'; Environment = $Environment
    }
    $process = Start-Process @startArguments
    try {
        if (-not $process.WaitForExit(15000)) {
            $process.Kill($true)
            throw 'NSIS fixture did not exit; a silent guard may have displayed a dialog.'
        }
        return $process.ExitCode
    } finally { $process.Dispose() }
}
function Record-Pass([string]$Name) {
    $script:stages.Add($Name)
    Write-Host "PASS $Name"
}
try {
    $fresh = Join-Path $fixtureRoot 'fresh path with spaces'
    if ((Invoke-Fixture $fresh) -ne 0 -or -not (Test-Path -LiteralPath "$fresh/mutation.txt")) {
        throw 'Fresh installation was blocked.'
    }
    Record-Pass 'fresh directory including spaces is accepted'

    $clear = New-PayloadFixture 'all-clear'
    $before = Get-FixtureHashes $clear
    if ((Invoke-Fixture $clear) -ne 0) { throw 'Unlocked existing payload was blocked.' }
    foreach ($path in $before.Keys) {
        if ((Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash -ne $before[$path]) {
            throw "Probe changed existing bytes in $path"
        }
    }
    Record-Pass 'unlocked existing payload is probed without writing'

    for ($index = 0; $index -lt $payloadPaths.Count; $index++) {
        $root = New-PayloadFixture "locked-$index"
        $before = Get-FixtureHashes $root
        $lock = [IO.File]::Open((Join-Path $root $payloadPaths[$index]), 'Open', 'Read', 'Read')
        try {
            if ((Invoke-Fixture $root) -ne 32) { throw "Silent install accepted locked $($payloadPaths[$index])" }
            Assert-Unchanged $before $root
        } finally { $lock.Dispose() }
        if ((Invoke-Fixture $root) -ne 0) { throw 'Released lock did not allow the next install.' }
        Record-Pass "silent install blocks $($payloadPaths[$index]) before mutation and accepts release"
    }

    $registered = New-PayloadFixture 'registered-prior'
    $newRoot = New-PayloadFixture 'new-destination'
    $before = Get-FixtureHashes $registered
    $newBefore = Get-FixtureHashes $newRoot
    New-Item -Path "$registryPath\Product" -Force | Out-Null
    Set-Item -LiteralPath "$registryPath\Product" -Value $registered
    $lock = [IO.File]::Open((Join-Path $registered 'resources/hive-daemon.exe'), 'Open', 'Read', 'Read')
    try {
        foreach ($environment in @(@{}, @{ ABIGAIL_LOCK_FIXTURE_EARLY = '1' })) {
            if ((Invoke-Fixture $newRoot -Environment $environment) -ne 32) {
                throw 'Locked registered installation was accepted before old uninstall.'
            }
            Assert-Unchanged $before $registered
            Assert-Unchanged $newBefore $newRoot
        }
    } finally { $lock.Dispose() }
    Remove-Item -LiteralPath "$registryPath\Product" -Force
    Record-Pass 'silent and early reinstall guard protect registered prior installation'

    foreach ($keyName in @('Abigail', '{895CC44B-5D9D-48F1-92A6-ED2C2E2052CE}')) {
        $keyPath = "$registryPath\Uninstall\$keyName"
        New-Item -Path $keyPath -Force | Out-Null
        Set-ItemProperty -LiteralPath $keyPath -Name InstallLocation -Value ('"' + $registered + '"')
        Set-ItemProperty -LiteralPath $keyPath -Name DisplayName -Value 'Abigail'
        Set-ItemProperty -LiteralPath $keyPath -Name Publisher -Value 'abigail'
        $lock = [IO.File]::Open((Join-Path $registered 'resources/entity-daemon.exe'), 'Open', 'Read', 'Read')
        try {
            if ((Invoke-Fixture $newRoot -Environment @{ ABIGAIL_LOCK_FIXTURE_EARLY = '1' }) -ne 32) {
                throw "Quoted registered installation was accepted: $keyName"
            }
            Assert-Unchanged $before $registered
            Assert-Unchanged $newBefore $newRoot
        } finally { $lock.Dispose(); Remove-Item -LiteralPath $keyPath -Force }
        Record-Pass "early guard protects quoted InstallLocation in uninstall key $keyName"
    }

    $selected = New-PayloadFixture 'new-directory-selection'
    $before = Get-FixtureHashes $selected
    $lock = [IO.File]::Open((Join-Path $selected 'resources/entity-daemon.exe'), 'Open', 'Read', 'Read')
    try {
        if ((Invoke-Fixture $fresh -Environment @{ ABIGAIL_LOCK_FIXTURE_SELECTED_ROOT = $selected }) -ne 32) {
            throw 'Preinstall guard accepted a locked newly selected directory.'
        }
        Assert-Unchanged $before $selected
    } finally { $lock.Dispose() }
    Record-Pass 'preinstall checks the directory selected after early checks'

    $partial = New-PayloadFixture 'partial-installation'
    Remove-Item -LiteralPath (Join-Path $partial 'Abigail.exe') -Force
    Remove-Item -LiteralPath (Join-Path $partial 'resources/abigail-entity-runtime-app.exe') -Force
    $before = Get-FixtureHashes $partial
    $lock = [IO.File]::Open((Join-Path $partial 'resources/entity-daemon.exe'), 'Open', 'Read', 'Read')
    try {
        if ((Invoke-Fixture $partial) -ne 32) { throw 'Missing GUIs hid a locked background daemon.' }
        Assert-Unchanged $before $partial
    } finally { $lock.Dispose() }
    Record-Pass 'missing GUI files do not hide a locked background daemon'

    $readOnly = New-PayloadFixture 'read-only-payload'
    $readOnlyPath = Join-Path $readOnly 'resources/hive-daemon.exe'
    [IO.File]::SetAttributes($readOnlyPath, [IO.FileAttributes]::ReadOnly)
    $before = Get-FixtureHashes $readOnly
    try {
        if ((Invoke-Fixture $readOnly) -ne 32) { throw 'Unwritable payload was accepted.' }
        Assert-Unchanged $before $readOnly
    } finally { [IO.File]::SetAttributes($readOnlyPath, [IO.FileAttributes]::Normal) }
    Record-Pass 'access-denied payload stops before any mutation'

    $running = New-PayloadFixture 'running-daemon'
    $daemonPath = Join-Path $running 'resources/hive-daemon.exe'
    Copy-Item -LiteralPath "$env:SystemRoot/System32/ping.exe" -Destination $daemonPath -Force
    $before = Get-FixtureHashes $running
    $ownedProcess = Start-Process -FilePath $daemonPath -ArgumentList @('-t', '127.0.0.1') -PassThru -WindowStyle Hidden `
        -RedirectStandardOutput (Join-Path $fixtureRoot 'ping.stdout.txt') `
        -RedirectStandardError (Join-Path $fixtureRoot 'ping.stderr.txt')
    try {
        if ($ownedProcess.WaitForExit(500)) { throw 'Native running-executable fixture did not start.' }
        if ((Invoke-Fixture $running) -ne 32) { throw 'Running daemon executable was accepted.' }
        Assert-Unchanged $before $running
        if ((Invoke-Fixture $fresh) -ne 0 -or $ownedProcess.HasExited) {
            throw 'A running executable in a different unregistered directory affected setup.'
        }
        Record-Pass 'same executable name in an unrelated directory is left running'
    } finally {
        if (-not $ownedProcess.HasExited) { $ownedProcess.Kill(); $ownedProcess.WaitForExit() }
        $ownedProcess.Dispose()
    }
    if ((Invoke-Fixture $running) -ne 0) { throw 'Stopped daemon still prevented installation.' }
    Record-Pass 'actual running executable blocks replacement and stopping it allows installation'

    for ($index = 0; $index -lt $payloadPaths.Count; $index++) {
        $root = New-PayloadFixture "uninstall-$index"
        if ((Invoke-Fixture $root) -ne 0) { throw 'Could not prepare uninstaller fixture.' }
        $before = Get-FixtureHashes $root
        $lock = [IO.File]::Open((Join-Path $root $payloadPaths[$index]), 'Open', 'Read', 'Read')
        try {
            if ((Invoke-Fixture $root -Executable "$root/uninstall-fixture.exe" -Uninstall) -ne 32) {
                throw "Uninstaller accepted locked $($payloadPaths[$index])"
            }
            Assert-Unchanged $before $root
        } finally { $lock.Dispose() }
        if ((Invoke-Fixture $root -Executable "$root/uninstall-fixture.exe" -Uninstall) -ne 0) {
            throw 'Unlocked payload was still blocked by uninstaller.'
        }
        Record-Pass "uninstall blocks $($payloadPaths[$index]) before mutation and accepts release"
    }
    [IO.File]::WriteAllText((Join-Path $fixtureRoot 'acceptance.json'),
        (@{ passed = $true; stages = @($script:stages); stage_count = $script:stages.Count } | ConvertTo-Json -Depth 4))
    Write-Host "Installer lock acceptance passed $($script:stages.Count) stages: $fixtureRoot"
} finally {
    if (Test-Path -LiteralPath $registryPath) { Remove-Item -LiteralPath $registryPath -Recurse -Force }
}
