#requires -Version 7.0
# Static checks, interpretation of the actual pure NSIS overlap macro, and
# compile-only fixtures. Never launch a produced installer or touch registry.
# Native hook execution/coexistence remains a disposable hosted acceptance gate.
param([string]$MakeNsis)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$hook = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../nsis-preview-hooks.nsh'))
$source = [IO.File]::ReadAllText($hook)
$script:passed = 0
function Assert-That([bool]$Condition, [string]$Name) {
    if (-not $Condition) { throw "FAIL $Name" }
    $script:passed++
    Write-Host "PASS $Name"
}
function Get-Macro([string]$Name) {
    $match = [regex]::Match($source, '(?ms)^!macro ' + [regex]::Escape($Name) + '(?: [^\r\n]*)?\r?\n(.*?)^!macroend')
    if (-not $match.Success) { throw "Missing actual macro $Name" }
    return $match.Groups[1].Value
}

# Execute only the three documented string operations in the source macro.
# Unknown instructions fail the test; this is not a separately rewritten
# StartsWith predicate that could pass while the actual NSIS policy regresses.
$overlap = Get-Macro 'AbigailPreviewCheckOverlap'
function Test-ActualOverlapMacro([string]$Candidate, [string]$Protected) {
    $registers = @{}
    foreach ($raw in $overlap.Split("`n")) {
        $line = $raw.Trim()
        if ($line.Length -eq 0 -or $line.StartsWith(';')) { continue }
        $line = $line.Replace('${Candidate}', $Candidate).Replace('${Protected}', $Protected).Replace('${FailureLabel}', 'REJECT')
        $tokens = @([regex]::Matches($line, '"([^"]*)"|(\S+)') | ForEach-Object {
            if ($_.Groups[1].Success) { $_.Groups[1].Value } else { $_.Groups[2].Value }
        })
        $value = {
            param([string]$Token)
            if ($Token -match '^\$R[89]$') { return $registers[$Token] }
            return $Token
        }
        switch ($tokens[0]) {
            'StrCmp' {
                if ($tokens.Count -ne 4 -or $tokens[3] -ne 'REJECT') { throw 'Unsupported StrCmp form' }
                if ([string]::Equals((& $value $tokens[1]), (& $value $tokens[2]), [StringComparison]::OrdinalIgnoreCase)) { return $true }
            }
            'StrLen' {
                if ($tokens.Count -ne 3) { throw 'Unsupported StrLen form' }
                $registers[$tokens[1]] = [string](& $value $tokens[2]).Length
            }
            'StrCpy' {
                if ($tokens.Count -ne 4) { throw 'Unsupported StrCpy form' }
                $text = [string](& $value $tokens[2])
                $length = [int](& $value $tokens[3])
                $registers[$tokens[1]] = $text.Substring(0, [Math]::Min($text.Length, $length))
            }
            default { throw "Unsupported instruction in actual overlap macro: $($tokens[0])" }
        }
    }
    return $false
}

$volume = '\\?\Volume{00000000-0000-0000-0000-000000000001}'
$stable = "$volume\Users\fixture\AppData\Local\Abigail"
$profile = "$volume\Users\fixture\AppData\Local\AbigailPreview\profiles\ongoing"
foreach ($case in @(
    @{ name='stable equality'; candidate=$stable; protected=$stable; denied=$true },
    @{ name='stable child'; candidate="$stable\Preview"; protected=$stable; denied=$true },
    @{ name='stable ancestor'; candidate="$volume\Users\fixture\AppData\Local"; protected=$stable; denied=$true },
    @{ name='case insensitive equality'; candidate=$stable.ToUpperInvariant(); protected=$stable; denied=$true },
    @{ name='separate preview sibling'; candidate="$stable Preview"; protected=$stable; denied=$false },
    @{ name='similar prefix sibling'; candidate="${stable}Other\child"; protected=$stable; denied=$false },
    @{ name='profile equality'; candidate=$profile; protected=$profile; denied=$true },
    @{ name='profile child'; candidate="$profile\install"; protected=$profile; denied=$true },
    @{ name='profile ancestor'; candidate="$volume\Users\fixture\AppData\Local\AbigailPreview"; protected=$profile; denied=$true },
    @{ name='profile similar sibling'; candidate="${profile}2"; protected=$profile; denied=$false },
    @{ name='different native volume'; candidate=$stable.Replace('000000000001','000000000002'); protected=$stable; denied=$false },
    @{ name='registered custom install ancestor'; candidate="$volume\Apps"; protected="$volume\Apps\Family"; denied=$true }
)) {
    Assert-That ((Test-ActualOverlapMacro $case.candidate $case.protected) -eq $case.denied) $case.name
}

$preInstall = Get-Macro 'NSIS_HOOK_PREINSTALL'
$preUninstall = Get-Macro 'NSIS_HOOK_PREUNINSTALL'
Assert-That ($preInstall -match 'Call AbigailPreviewGuard' -and $preUninstall -match 'Call un\.AbigailPreviewGuard') 'both vendor hooks call their own guarded function'
Assert-That ($preInstall -match 'AbigailPreviewAssertIdentity' -and $preUninstall -match 'AbigailPreviewAssertIdentity') 'both hooks enforce the compile-time preview identity'
Assert-That ($source.Contains('Push "$LOCALAPPDATA\Abigail"') -and $source.Contains('AbigailPreview\profiles\ongoing')) 'default stable installation and preview profile are protected'
foreach ($hive in @('0x80000001','0x80000002')) {
    foreach ($view in @('0x0101','0x0201')) {
        Assert-That ($source.Contains('!insertmacro AbigailPreviewReadLocations "${Prefix}" ' + $hive + ' ' + $view)) "stable registry query covers $hive / $view"
    }
}
$locations = Get-Macro 'AbigailPreviewReadLocations'
Assert-That ($locations.Contains('${ABIGAIL_STABLE_UNINSTALL_KEY}') -and $locations.Contains('Push "InstallLocation"') -and
    $locations.Contains('${ABIGAIL_STABLE_LOCATION_KEY}') -and $locations.Contains('Push ""')) 'both uninstall and remembered stable locations are queried'
Assert-That ($source -match 'RegOpenKeyExW' -and $source -match 'RegGetValueW' -and
    $source.Contains('StrCmp $6 0 0 registry_fail') -and $source.Contains('StrCmp $5 "" registry_fail')) 'unreadable, missing-value and empty registrations fail closed'
Assert-That ($source.Contains('GetFileAttributesW(w r1) i.r2 ?e') -and $source.Contains('IntOp $5 $2 & 0x400') -and
    $source.Contains('StrCmp $5 0 0 canonical_fail') -and $source.Contains('Goto canonical_walk')) 'all existing path ancestors reject reparse points with immediate native error capture'
Assert-That ($source.Contains('StrCmp $5 2 canonical_missing') -and $source.Contains('StrCmp $5 3 canonical_missing canonical_fail') -and
    $source.Contains('StrCmp $4 "" 0 canonical_fail')) 'only absent leaves are permitted; access errors are rejected'
Assert-That ($source.Contains('GetFinalPathNameByHandleW(p r6, w .r7, i ${NSIS_MAX_STRLEN}, i 1)') -and
    $source.Contains('GetFullPathNameW') -and $source.Contains('CloseHandle')) 'native volume identity, normalization and handle cleanup are required'
Assert-That ($source.Contains('GetDriveTypeW') -and $source.Contains('StrCmp $2 3 0 canonical_fail') -and
    $source.Contains('IntCmp $8 ${NSIS_MAX_STRLEN} canonical_fail +1 canonical_fail')) 'unsupported volumes and truncated native paths fail closed'
$activeLines = ($source -split '\r?\n' | Where-Object { $_ -notmatch '^\s*;' }) -join "`n"
Assert-That ($activeLines -notmatch '(?im)^\s*(Delete|RMDir|WriteReg\w*|Exec\w*|KillProc\w*)\b' -and
    $activeLines -notmatch '::(?:KillProcess|RegSetValue|RegCreateKey|DeleteFile|RemoveDirectory)') 'hooks contain no deletion, execution, process kills or registry mutation'
Assert-That ($source.Contains('SetOutPath (which can create INSTDIR)') -and $source.Contains('not an atomic')) 'vendor placement and filesystem race limits remain explicit'

if (-not $MakeNsis) {
    $command = Get-Command makensis.exe -ErrorAction SilentlyContinue
    if ($command) { $MakeNsis = $command.Source }
    foreach ($candidate in @(
        (Join-Path $env:LOCALAPPDATA 'tauri/NSIS/makensis.exe'),
        'C:\Program Files (x86)\NSIS\makensis.exe'
    )) {
        if (-not $MakeNsis -and (Test-Path -LiteralPath $candidate -PathType Leaf)) { $MakeNsis = $candidate }
    }
}
if (-not $MakeNsis) {
    Write-Host "SKIP NSIS compilation: makensis unavailable. $script:passed static/synthetic checks passed; native acceptance is still required."
    exit 0
}
$tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\','/')
$workspace = Join-Path $tempRoot ('abigail-preview-hook-compile-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $workspace | Out-Null
try {
    $template = @'
Unicode true
Name "Preview guard compilation fixture"
OutFile "fixture-never-run.exe"
RequestExecutionLevel user
!define PRODUCTNAME "__PRODUCT__"
!define BUNDLEID "__BUNDLE__"
!define MAINBINARYNAME "__BINARY__"
!define INSTALLMODE "__MODE__"
!include "__HOOK__"
Section
  !insertmacro NSIS_HOOK_PREINSTALL
  WriteUninstaller "$TEMP\fixture-uninstall-never-run.exe"
SectionEnd
Section Uninstall
  !insertmacro NSIS_HOOK_PREUNINSTALL
SectionEnd
'@
    foreach ($case in @(
        @{ name='exact preview identity compiles'; product='Abigail Preview'; bundle='com.abigail.preview'; binary='Abigail Preview'; mode='currentUser'; succeeds=$true },
        @{ name='stable product cannot use preview hooks'; product='Abigail'; bundle='com.abigail.preview'; binary='Abigail Preview'; mode='currentUser'; succeeds=$false },
        @{ name='stable identifier cannot use preview hooks'; product='Abigail Preview'; bundle='com.abigail'; binary='Abigail Preview'; mode='currentUser'; succeeds=$false },
        @{ name='stable executable cannot use preview hooks'; product='Abigail Preview'; bundle='com.abigail.preview'; binary='Abigail'; mode='currentUser'; succeeds=$false },
        @{ name='machine installation cannot use preview hooks'; product='Abigail Preview'; bundle='com.abigail.preview'; binary='Abigail Preview'; mode='perMachine'; succeeds=$false }
    )) {
        $fixture = $template.Replace('__PRODUCT__',$case.product).Replace('__BUNDLE__',$case.bundle).Replace('__BINARY__',$case.binary).
            Replace('__MODE__',$case.mode).Replace('__HOOK__',$hook)
        $path = Join-Path $workspace 'fixture.nsi'
        [IO.File]::WriteAllText($path, $fixture, [Text.UTF8Encoding]::new($false))
        Push-Location -LiteralPath $workspace
        try {
            $compilerOutput = & $MakeNsis /V2 $path 2>&1
            $code = $LASTEXITCODE
        } finally { Pop-Location }
        if (($code -eq 0) -ne $case.succeeds) { throw "$($case.name): compiler exit $code`n$($compilerOutput -join "`n")" }
        if (-not $case.succeeds -and ($compilerOutput -join "`n") -notmatch 'Preview hooks require') {
            throw "Negative fixture failed for an unrelated compiler error: $($compilerOutput -join "`n")"
        }
        Assert-That $true $case.name
    }
} finally {
    $resolved = [IO.Path]::GetFullPath($workspace)
    if (-not $resolved.StartsWith($tempRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw 'Unsafe fixture cleanup target' }
    Remove-Item -LiteralPath $resolved -Recurse -Force
}
Write-Host "$script:passed preview NSIS checks passed. Compiled fixtures were never executed; hosted coexistence acceptance remains required."
