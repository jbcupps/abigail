#requires -Version 7.0
<#
Default: print a read-only plan. Execute is restricted to the approved, disposable
GitHub-hosted Windows workflow. This runner never signs or publishes anything.
No product, installer, DPAPI, registry or process operation occurs in plan mode.
#>
[CmdletBinding()]
param(
    [string]$CandidateArtifactDir,
    [string]$ProductSourceCommit = 'f7c7848d25726480b253ec1ca637c3ffa5ea3c77',
    [string]$HarnessSourceCommit,
    [string]$RecoveryCliPath,
    [string]$RecoveryCliSha256,
    [Alias('CertificateThumbprint')][string]$ExpectedCertificateThumbprint,
    [string]$StableArtifactDir,
    [string]$StableSourceCommit,
    [string]$StableInstallerSha256,
    [string]$OutputDirectory,
    [switch]$Execute,
    [switch]$CheckPrerequisites,
    [switch]$ApproveDpapiFixtures
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'preview_candidate_policy.ps1')

function Assert-CandidateNoLinks {
    param([string]$Path)
    $current = Get-CandidateFullPath $Path
    while ($current) {
        if (Test-Path -LiteralPath $current) {
            $item = Get-Item -LiteralPath $current -Force
            Assert-CandidateCondition (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -eq 0) 'Filesystem links are not allowed in acceptance paths.'
        }
        $parent = [IO.Path]::GetDirectoryName($current)
        if ($parent -eq $current) { break }
        $current = $parent
    }
}

function Read-CandidateJson {
    param([string]$Path)
    Assert-CandidateNoLinks $Path
    return Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json
}

function Assert-CandidateHash {
    param([string]$Path, [string]$Expected)
    Assert-CandidateCondition ($Expected -match '^[a-f0-9]{64}$') 'Expected SHA256 is invalid.'
    Assert-CandidateNoLinks $Path
    Assert-CandidateCondition (Test-Path -LiteralPath $Path -PathType Leaf) 'Expected artifact is absent.'
    Assert-CandidateCondition ((Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash -ieq $Expected) 'Artifact SHA256 does not match its pinned bytes.'
}

function Get-CandidateTextHash {
    param([string]$Text)
    return [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($Text))).ToLowerInvariant()
}

function Get-CandidateTreeFingerprint {
    param([string]$Root)
    Assert-CandidateNoLinks $Root
    if (-not (Test-Path -LiteralPath $Root -PathType Container)) { return 'absent' }
    $pending = [Collections.Generic.Stack[string]]::new()
    $pending.Push((Get-CandidateFullPath $Root))
    $files = [Collections.Generic.List[object]]::new()
    while ($pending.Count -gt 0) {
        foreach ($entry in Get-ChildItem -LiteralPath $pending.Pop() -Force) {
            Assert-CandidateCondition (($entry.Attributes -band [IO.FileAttributes]::ReparsePoint) -eq 0) 'Fixture fingerprint refuses filesystem links.'
            $relative = [IO.Path]::GetRelativePath($Root, $entry.FullName)
            if ($entry.PSIsContainer) {
                $files.Add([ordered]@{ name = $relative; kind = 'directory' })
                $pending.Push($entry.FullName)
            } else {
                $files.Add([ordered]@{ name = $relative; kind = 'file'; size = $entry.Length; sha256 = (Get-FileHash -LiteralPath $entry.FullName -Algorithm SHA256).Hash })
            }
        }
    }
    return Get-CandidateTextHash (ConvertTo-Json -InputObject @($files | Sort-Object { $_.name }) -Depth 6 -Compress)
}

function Get-CandidateRegistrations {
    param([ValidateSet('Abigail', 'Abigail Preview')][string]$Product)
    foreach ($hiveName in @('CurrentUser', 'LocalMachine')) {
        foreach ($viewName in @('Registry64', 'Registry32')) {
            $base = [Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::$hiveName, [Microsoft.Win32.RegistryView]::$viewName)
            try {
                foreach ($kind in @('uninstall', 'remembered')) {
                    $keyName = if ($kind -eq 'uninstall') { "Software\Microsoft\Windows\CurrentVersion\Uninstall\$Product" } else { "Software\abigail\$Product" }
                    $key = $base.OpenSubKey($keyName, $false)
                    if ($null -eq $key) { continue }
                    try {
                        $values = [ordered]@{}
                        foreach ($name in @($key.GetValueNames() | Sort-Object)) {
                            $label = if ($name -eq '') { '(default)' } else { $name }
                            $values[$label] = $key.GetValue($name, $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
                        }
                        [pscustomobject]@{ hive = $hiveName; view = $viewName; kind = $kind; key = $keyName; values = $values }
                    } finally { $key.Dispose() }
                }
            } finally { $base.Dispose() }
        }
    }
}

function Get-CandidateShortcutPaths {
    param([ValidateSet('Abigail', 'Abigail Preview')][string]$Product)
    # Tauri may use a start-menu folder, depending on the pinned channel config.
    foreach ($folder in @('DesktopDirectory', 'CommonDesktopDirectory')) {
        Join-Path ([Environment]::GetFolderPath($folder)) "$Product.lnk"
    }
    foreach ($folder in @('Programs', 'CommonPrograms')) {
        $root = [Environment]::GetFolderPath($folder)
        Join-Path $root "$Product.lnk"
        Join-Path $root "$Product\$Product.lnk"
    }
}

function Get-CandidateShortcuts {
    param([string]$Product, [string]$InstallRoot, [switch]$RequireStartMenu)
    $found = @()
    foreach ($path in Get-CandidateShortcutPaths $Product) {
        Assert-CandidateNoLinks $path
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { continue }
        $shell = New-Object -ComObject WScript.Shell
        try {
            $shortcut = $shell.CreateShortcut($path)
            try {
                Assert-CandidateCondition ((Get-CandidateFullPath $shortcut.TargetPath) -ieq (Join-Path $InstallRoot "$Product.exe")) 'Product shortcut targets an unowned executable.'
                $found += [pscustomobject]@{ path = $path; sha256 = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash }
            } finally { $null = [Runtime.InteropServices.Marshal]::ReleaseComObject($shortcut) }
        } finally { $null = [Runtime.InteropServices.Marshal]::ReleaseComObject($shell) }
    }
    if ($RequireStartMenu) {
        $programs = [Environment]::GetFolderPath('Programs')
        Assert-CandidateCondition (@($found | Where-Object { Test-CandidateWithin $_.path $programs }).Count -ge 1) 'The expected current-user Start Menu shortcut is missing.'
    }
    return $found
}

function Assert-CandidateNoProductProcesses {
    # Refuse NSIS's same-name termination path. Never terminate an enumerated PID.
    $names = @('Abigail', 'Abigail Preview', 'abigail-hive-app', 'abigail-entity-runtime-app', 'hive-daemon', 'entity-daemon')
    $matching = @(Get-Process -ErrorAction Stop | Where-Object { $_.ProcessName -in $names })
    Assert-CandidateCondition ($matching.Count -eq 0) 'Installer boundary refused: a matching product process is still running. No process was killed.'
}

function Get-CandidateRegistryValue {
    param([string]$Hive, [string]$View, [string]$Key, [string]$Name)
    $base = [Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::$Hive, [Microsoft.Win32.RegistryView]::$View)
    try {
        $entry = $base.OpenSubKey($Key, $false)
        if ($null -eq $entry) { return $null }
        try { return $entry.GetValue($Name, $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames) }
        finally { $entry.Dispose() }
    } finally { $base.Dispose() }
}

function Assert-CandidateHostPrerequisites {
    return Assert-CandidateExistingWebView ([Environment]::Is64BitOperatingSystem) {
        param($hive, $view, $key, $name)
        Get-CandidateRegistryValue $hive $view $key $name
    }
}

function Get-CandidateStableFingerprint {
    $registrations = @(Get-CandidateRegistrations 'Abigail')
    Assert-CandidateRegistrationOwnership $registrations $script:stableInstall $true
    $shortcuts = @(Get-CandidateShortcuts 'Abigail' $script:stableInstall -RequireStartMenu)
    return [ordered]@{
        installedFiles = Get-CandidateTreeFingerprint $script:stableInstall
        profile = Get-CandidateTreeFingerprint (Join-Path $script:fixtureRoot 'profiles/stable')
        registrations = Get-CandidateTextHash (ConvertTo-Json -InputObject $registrations -Depth 10 -Compress)
        shortcuts = Get-CandidateTextHash (ConvertTo-Json -InputObject $shortcuts -Depth 5 -Compress)
    }
}

function Assert-CandidateFingerprintSame {
    param($Before, $After)
    Assert-CandidateCondition ((ConvertTo-Json $Before -Depth 10 -Compress) -ceq (ConvertTo-Json $After -Depth 10 -Compress)) 'A protected stable or preview fixture fingerprint changed across an installer operation.'
}

function Assert-CandidateNativeSignature {
    param([string]$Path)
    $record = Assert-AbigailSignature -Path $Path -Thumbprint $ExpectedCertificateThumbprint
    return [ordered]@{ sha256 = ([string]$record.sha256).ToLowerInvariant(); thumbprint = $ExpectedCertificateThumbprint; verified = $true }
}

function Assert-CandidateInstalledPayload {
    param([string]$InstallRoot, $Payload, [string]$Product)
    $records = @()
    foreach ($item in $Payload) {
        $file = Join-Path $InstallRoot $item.relative_path
        Assert-CandidateHash $file $item.sha256
        $verified = Assert-CandidateNativeSignature $file
        $records += [ordered]@{ relativePath = $item.relative_path; sha256 = $verified.sha256; signatureVerified = $true }
    }
    $uninstaller = Join-Path $InstallRoot 'uninstall.exe'
    Assert-CandidateNoLinks $uninstaller
    $uninstallerSignature = Assert-CandidateNativeSignature $uninstaller
    Assert-CandidateRegistrationOwnership @(Get-CandidateRegistrations $Product) $InstallRoot $true
    $null = Get-CandidateShortcuts $Product $InstallRoot -RequireStartMenu
    return [ordered]@{ payload = $records; uninstaller = $uninstallerSignature; registrationAndShortcutsOwned = $true }
}

function Invoke-CandidateInstaller {
    param([string]$Path, [string]$Hash, [string]$InstallRoot, [string]$Product, [switch]$Reinstall)
    Assert-CandidateNoProductProcesses
    $null = Assert-CandidateHostPrerequisites
    Assert-CandidateHash $Path $Hash
    $null = Assert-CandidateNativeSignature $Path
    Assert-CandidateNoLinks $InstallRoot
    $existing = @(Get-CandidateRegistrations $Product)
    if ($Reinstall) { Assert-CandidateRegistrationOwnership $existing $InstallRoot $true }
    else {
        Assert-CandidateCondition ($existing.Count -eq 0 -and -not (Test-Path -LiteralPath $InstallRoot)) 'Initial installation requires a fresh path and no product registration.'
    }
    # NSIS requires /D last and unquoted, even when its value contains spaces.
    $process = Start-Process -FilePath $Path -ArgumentList "/S /D=$InstallRoot" -PassThru -WindowStyle Hidden
    try {
        Assert-CandidateCondition ($process.WaitForExit(180000)) 'Installer timed out; its outcome is uncertain. No forced cleanup is permitted.'
        Assert-CandidateCondition ($process.ExitCode -eq 0) 'Installer returned a failure code.'
    } finally { $process.Dispose() }
    Assert-CandidateNoProductProcesses
}

function Invoke-CandidateUninstall {
    param([string]$InstallRoot, [string]$Product, [string]$ExpectedUninstallerHash)
    Assert-CandidateNoProductProcesses
    Assert-CandidateRegistrationOwnership @(Get-CandidateRegistrations $Product) $InstallRoot $true
    $uninstaller = Join-Path $InstallRoot 'uninstall.exe'
    Assert-CandidateHash $uninstaller $ExpectedUninstallerHash
    $null = Assert-CandidateNativeSignature $uninstaller
    $process = Start-Process -FilePath $uninstaller -ArgumentList '/S' -PassThru -WindowStyle Hidden
    try {
        Assert-CandidateCondition ($process.WaitForExit(180000)) 'Uninstaller timed out; its outcome is uncertain. No forced cleanup is permitted.'
        Assert-CandidateCondition ($process.ExitCode -eq 0) 'Uninstaller returned a failure code.'
    } finally { $process.Dispose() }
    # NSIS can hand off to its temporary self-deleting child. Observe removal, not just parent exit.
    $deadline = [DateTime]::UtcNow.AddSeconds(60)
    do {
        $registrations = @(Get-CandidateRegistrations $Product)
        $pending = @($registrations | Where-Object kind -eq 'uninstall').Count -gt 0 -or
            (Test-Path -LiteralPath (Join-Path $InstallRoot "$Product.exe")) -or
            (Test-Path -LiteralPath $uninstaller)
        if ($pending) { Start-Sleep -Milliseconds 250 }
    } while ($pending -and [DateTime]::UtcNow -lt $deadline)
    Assert-CandidateCondition (-not $pending) 'Uninstaller did not finish removing its exact payload and registration.'
    Assert-CandidateNoProductProcesses
    foreach ($path in Get-CandidateShortcutPaths $Product) {
        Assert-CandidateCondition (-not (Test-Path -LiteralPath $path)) 'Uninstaller left a product shortcut.'
    }
    # Tauri may remember the install directory after uninstall. Remove only the
    # exact run-created leaf after validating its location; never a parent key.
    Assert-CandidateRegistrationOwnership $registrations $InstallRoot $false
    foreach ($record in $registrations) {
        Assert-CandidateCondition ($record.kind -ceq 'remembered' -and $record.values.Count -eq 1) 'Unexpected residual registration; cleanup refused.'
        $base = [Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::CurrentUser, [Microsoft.Win32.RegistryView]::$($record.view))
        try {
            $key = $base.OpenSubKey($record.key, $false)
            if ($null -eq $key) { continue }
            try { Assert-CandidateCondition ($key.SubKeyCount -eq 0) 'Residual registration has unexpected child keys.' }
            finally { $key.Dispose() }
            $base.DeleteSubKey($record.key, $false)
        } finally { $base.Dispose() }
    }
    Assert-CandidateCondition (@(Get-CandidateRegistrations $Product).Count -eq 0) 'Product registration cleanup was incomplete.'
}

function Invoke-CandidateLifecycle {
    param([string]$Phase)
    $phaseReport = Join-Path $script:fixtureRoot "control/$Phase-report.json"
    $arguments = @((Join-Path $PSScriptRoot 'tests/preview-candidate-lifecycle.mjs'), '--execute', '--phase', $Phase,
        '--preview-install', $script:previewInstall, '--stable-install', $script:stableInstall,
        '--fixture-root', $script:fixtureRoot, '--recovery-cli', $script:ownedRecoveryCli,
        '--report', $phaseReport, '--product-source', $ProductSourceCommit, '--approved-dpapi')
    Assert-CandidateHash $script:ownedRecoveryCli $RecoveryCliSha256
    # The lifecycle runner emits sanitized protocol results. Its product children
    # use private fixture files / NUL, never the Actions console.
    & node @arguments | Out-Null
    $exitCode = $LASTEXITCODE
    if (-not (Test-Path -LiteralPath $phaseReport -PathType Leaf)) {
        $script:report.lifecycle += [ordered]@{ phase = $Phase; passed = $false; processesQuiesced = $false; failurePoint = 'phase-report-unavailable' }
        throw 'Synthetic lifecycle phase did not produce a report; process outcome is uncertain.'
    }
    $public = ConvertTo-CandidatePublicLifecycle (Read-CandidateJson $phaseReport) $Phase -AllowFailed
    if ($exitCode -ne 0 -and $public.passed) {
        $public.passed = $false
        $public.failurePoint = 'process-exit-status'
    }
    $script:report.lifecycle += $public
    Assert-CandidateCondition ($exitCode -eq 0 -and $public.passed -and $public.processesQuiesced) 'Synthetic lifecycle phase failed; the reviewed assertion prefix and failure point are recorded in the public report.'
    Assert-CandidateNoProductProcesses
}

function Remove-CandidateOwnedFixture {
    Assert-CandidateNoProductProcesses
    Assert-CandidateCondition (@(Get-CandidateRegistrations 'Abigail').Count -eq 0 -and
        @(Get-CandidateRegistrations 'Abigail Preview').Count -eq 0) 'Registrations must be removed by the owned uninstallers before fixture cleanup.'
    Assert-CandidateCondition (Test-CandidateWithin $script:fixtureRoot $env:RUNNER_TEMP) 'Fixture cleanup must stay inside RUNNER_TEMP.'
    Assert-CandidateNoLinks $script:fixtureRoot
    $owner = Read-CandidateJson (Join-Path $script:fixtureRoot 'owner.json')
    Assert-CandidateCondition ($owner.runId -ceq $env:GITHUB_RUN_ID -and $owner.runAttempt -ceq $env:GITHUB_RUN_ATTEMPT -and
        $owner.nonce -ceq $script:fixtureNonce) 'Fixture ownership marker changed; cleanup refused.'
    # Walk before recursive removal so no junction can redirect cleanup. This
    # deletes only the freshly created run root, including its DPAPI ciphertext.
    $null = Get-CandidateTreeFingerprint $script:fixtureRoot
    Remove-Item -LiteralPath $script:fixtureRoot -Recurse -Force
}

$baseline = Read-CandidateJson (Join-Path $PSScriptRoot 'preview-candidate-baseline.json')
Assert-CandidateCondition (-not ($Execute -and $CheckPrerequisites)) 'Choose Execute or read-only CheckPrerequisites, not both.'
if (-not $Execute -and -not $CheckPrerequisites) {
    [ordered]@{
        mode = 'plan'; executed = $false; productSource = $baseline.productSourceCommit
        stableSource = $baseline.stable.sourceCommit; stableInstallerSha256 = $baseline.stable.assets[0].sha256
        approvalRequired = 'Disposable current-user DPAPI encryption keys and synthetic Entity signing identities, in the configured ephemeral Windows hosted runner.'
        installerOperations = @('install stable', 'install preview', 'reinstall exact preview', 'uninstall preview', 'uninstall stable')
        lifecyclePhases = @('seed', 'verify', 'stable-return')
        reportName = 'preview-candidate-acceptance.json'
        prerequisites = 'Existing WebView2 registration required; missing runtime blocks before signing and installation.'
        limitations = @('No visual GUI acceptance', 'No real provider/model credentials or calls', 'No stable-active-during-installer acceptance', 'No full ten-stage release acceptance')
    } | ConvertTo-Json -Depth 6
    return
}

# No native OS/app action is allowed above this explicit execution gate.
$context = @{}
foreach ($name in @('GITHUB_ACTIONS', 'RUNNER_ENVIRONMENT', 'RUNNER_OS', 'GITHUB_REPOSITORY', 'GITHUB_RUN_ID',
    'GITHUB_RUN_ATTEMPT', 'GITHUB_SHA', 'RUNNER_TEMP', 'GITHUB_WORKSPACE')) { $context[$name] = [Environment]::GetEnvironmentVariable($name) }
# CheckPrerequisites permits only the hosted read-only prerequisite check; it
# does not grant fixture/DPAPI approval and returns before any artifact/mutation.
Assert-CandidateHostedContext $context ($ApproveDpapiFixtures.IsPresent -or $CheckPrerequisites.IsPresent) $HarnessSourceCommit
Assert-CandidateCondition ($IsWindows) 'Windows is required.'
if ($CheckPrerequisites) {
    Assert-CandidateHostPrerequisites | ConvertTo-Json
    return
}
$prerequisites = Assert-CandidateHostPrerequisites
foreach ($path in @($CandidateArtifactDir, $StableArtifactDir, $RecoveryCliPath, $OutputDirectory, $env:RUNNER_TEMP, $env:GITHUB_WORKSPACE)) {
    Assert-CandidateNoLinks $path
}
Assert-CandidateCondition ((Test-CandidateWithin $CandidateArtifactDir $env:RUNNER_TEMP) -and
    (Test-CandidateWithin $StableArtifactDir $env:RUNNER_TEMP) -and
    ((Test-CandidateWithin $RecoveryCliPath $env:RUNNER_TEMP) -or (Test-CandidateWithin $RecoveryCliPath $env:GITHUB_WORKSPACE))) 'Artifact inputs must belong to this hosted workspace or temporary root.'
Assert-CandidateCondition ((Test-CandidateWithin $OutputDirectory $env:RUNNER_TEMP) -or
    (Test-CandidateWithin $OutputDirectory $env:GITHUB_WORKSPACE)) 'Public output must belong to this hosted run.'
Assert-CandidateSeparatePaths $CandidateArtifactDir $StableArtifactDir
Assert-CandidateSeparatePaths $OutputDirectory $CandidateArtifactDir
Assert-CandidateSeparatePaths $OutputDirectory $StableArtifactDir
Assert-CandidateCondition (-not (Test-Path -LiteralPath $OutputDirectory)) 'Public output requires a fresh directory.'
$script:fixtureNonce = [guid]::NewGuid().ToString('D')
$script:fixtureRoot = Join-Path $env:RUNNER_TEMP "abigail-preview-$($env:GITHUB_RUN_ID)-$($env:GITHUB_RUN_ATTEMPT)-$script:fixtureNonce"
$script:stableInstall = Join-Path $script:fixtureRoot 'install-stable'
$script:previewInstall = Join-Path $script:fixtureRoot 'install-preview'
Assert-CandidateSeparatePaths $script:fixtureRoot $OutputDirectory
Assert-CandidateCondition (-not (Test-Path -LiteralPath $script:fixtureRoot)) 'Fixture root already exists.'
Assert-CandidateCondition ($StableSourceCommit -ceq $baseline.stable.sourceCommit -and
    $StableInstallerSha256 -ceq $baseline.stable.assets[0].sha256) 'Stable fixture must be the exact reviewed published baseline.'
$manifest = Read-CandidateJson (Join-Path $CandidateArtifactDir 'mvp-build.json')
$signatures = Read-CandidateJson (Join-Path $CandidateArtifactDir 'signature-report.json')
$prepared = Read-CandidateJson (Join-Path $CandidateArtifactDir 'prepared-build-state.json')
Assert-CandidateMetadata $manifest $signatures $prepared $baseline $ProductSourceCommit $ExpectedCertificateThumbprint
Assert-CandidatePayloadPolicy $baseline.stable.payload stable
Assert-CandidateHash $RecoveryCliPath $RecoveryCliSha256
$previewInstaller = Join-Path $CandidateArtifactDir 'Abigail-windows-x64-setup.exe'
$stableInstaller = Join-Path $StableArtifactDir 'Abigail-windows-x64-setup.exe'
Assert-CandidateHash $previewInstaller $manifest.installer_sha256
foreach ($asset in $baseline.stable.assets) {
    $file = Join-Path $StableArtifactDir $asset.name
    Assert-CandidateHash $file $asset.sha256
    Assert-CandidateCondition ((Get-Item -LiteralPath $file).Length -eq $asset.size) 'Stable release asset size does not match the reviewed baseline.'
}
Import-Module (Join-Path $PSScriptRoot 'windows-signing.psm1') -Force
$previewInstallerSignature = Assert-CandidateNativeSignature $previewInstaller
$stableInstallerSignature = Assert-CandidateNativeSignature $stableInstaller
& node (Join-Path $PSScriptRoot 'release_provenance.mjs') verify --assets-dir $StableArtifactDir --source-commit $StableSourceCommit --tag $baseline.stable.tag --channel stable --windows-signed true --expected-thumbprint $ExpectedCertificateThumbprint | Out-Null
Assert-CandidateCondition ($LASTEXITCODE -eq 0) 'Stable release provenance verification failed.'
New-Item -ItemType Directory -Path $OutputDirectory | Out-Null
New-Item -ItemType Directory -Path $script:fixtureRoot | Out-Null
New-Item -ItemType Directory -Path (Join-Path $script:fixtureRoot 'control') | Out-Null
[ordered]@{ runId = $env:GITHUB_RUN_ID; runAttempt = $env:GITHUB_RUN_ATTEMPT; nonce = $script:fixtureNonce } |
    ConvertTo-Json | Set-Content -LiteralPath (Join-Path $script:fixtureRoot 'owner.json') -Encoding utf8
$script:ownedRecoveryCli = Join-Path $script:fixtureRoot 'control/abigail-profile.exe'
Copy-Item -LiteralPath $RecoveryCliPath -Destination $script:ownedRecoveryCli
Assert-CandidateHash $script:ownedRecoveryCli $RecoveryCliSha256
$script:report = [ordered]@{
    schemaVersion = 1; productSourceCommit = $ProductSourceCommit; harnessSourceCommit = $HarnessSourceCommit
    stableSourceCommit = $StableSourceCommit; recoveryCliSha256 = $RecoveryCliSha256.ToLowerInvariant()
    runner = 'github-hosted/windows'; runId = $env:GITHUB_RUN_ID; runAttempt = $env:GITHUB_RUN_ATTEMPT
    sourceStateSha256 = $manifest.source_state_sha256; launchContractSha256 = $manifest.packaging.launchContractSha256
    previewInstaller = $previewInstallerSignature; stableInstaller = $stableInstallerSignature
    prerequisites = $prerequisites
    passed = $false; publicationEligible = $false; realModelValidation = $false; fullAcceptance = $false
    completedSteps = @(); lifecycle = @(); cleanup = 'pending'
    limitations = @('Visual GUI reopening is untested.', 'No real model/provider integration was attempted.',
        'Stable-active-during-installer behavior is untested.', 'Reinstall uses the same candidate, not a prior accepted preview upgrade.',
        'Stable daemon shutdown uses the explicitly owned fixture job boundary, not a graceful stable product shutdown.',
        'This report does not satisfy all ten preview compatibility stages or authorize publication.')
}
$failedStep = 'initialization'
try {
    Invoke-CandidateAcceptanceSequence {
        param($step)
        $script:failedStep = $step
        switch ($step) {
            'fresh-host-guard' {
                Assert-CandidateNoProductProcesses
                foreach ($product in @('Abigail', 'Abigail Preview')) {
                    Assert-CandidateCondition (@(Get-CandidateRegistrations $product).Count -eq 0) 'A preexisting product registration prevents this disposable run.'
                    foreach ($shortcut in Get-CandidateShortcutPaths $product) {
                        Assert-CandidateCondition (-not (Test-Path -LiteralPath $shortcut)) 'A preexisting product shortcut prevents this disposable run.'
                    }
                }
            }
            'install-stable' { Invoke-CandidateInstaller $stableInstaller $StableInstallerSha256 $script:stableInstall 'Abigail' }
            'install-preview' { Invoke-CandidateInstaller $previewInstaller $manifest.installer_sha256 $script:previewInstall 'Abigail Preview' }
            'verify-stable-payload' { $script:report.stableInstalled = Assert-CandidateInstalledPayload $script:stableInstall $baseline.stable.payload 'Abigail' }
            'verify-preview-payload' { $script:report.previewInstalled = Assert-CandidateInstalledPayload $script:previewInstall $manifest.payload 'Abigail Preview' }
            'lifecycle-seed' { Invoke-CandidateLifecycle seed }
            'cold-process-guard' { Assert-CandidateNoProductProcesses }
            'capture-before-reinstall' {
                $script:beforeStable = Get-CandidateStableFingerprint
                $script:beforePreviewProfile = Get-CandidateTreeFingerprint (Join-Path $script:fixtureRoot 'profiles/preview')
            }
            'reinstall-preview' { Invoke-CandidateInstaller $previewInstaller $manifest.installer_sha256 $script:previewInstall 'Abigail Preview' -Reinstall }
            'compare-after-reinstall' {
                Assert-CandidateFingerprintSame $script:beforeStable (Get-CandidateStableFingerprint)
                Assert-CandidateFingerprintSame $script:beforePreviewProfile (Get-CandidateTreeFingerprint (Join-Path $script:fixtureRoot 'profiles/preview'))
            }
            'lifecycle-verify' { Invoke-CandidateLifecycle verify }
            'capture-before-preview-uninstall' {
                $script:beforeStable = Get-CandidateStableFingerprint
                $script:beforePreviewProfile = Get-CandidateTreeFingerprint (Join-Path $script:fixtureRoot 'profiles/preview')
            }
            'uninstall-preview' { Invoke-CandidateUninstall $script:previewInstall 'Abigail Preview' $script:report.previewInstalled.uninstaller.sha256 }
            'compare-after-preview-uninstall' {
                Assert-CandidateFingerprintSame $script:beforeStable (Get-CandidateStableFingerprint)
                Assert-CandidateFingerprintSame $script:beforePreviewProfile (Get-CandidateTreeFingerprint (Join-Path $script:fixtureRoot 'profiles/preview'))
            }
            'lifecycle-stable-return' { Invoke-CandidateLifecycle stable-return }
            'uninstall-stable' { Invoke-CandidateUninstall $script:stableInstall 'Abigail' $script:report.stableInstalled.uninstaller.sha256 }
            'cleanup-owned-fixture' { Remove-CandidateOwnedFixture; $script:report.cleanup = 'owned-fixture-and-registrations-removed' }
            default { throw 'Unknown acceptance step.' }
        }
        $script:report.completedSteps += $step
    }
    $script:report.passed = $true
} catch {
    # Fail closed: do not run another installer or delete data after an uncertain
    # installer/process outcome. Hosted machine disposal is the final backstop.
    $script:report.failure = [ordered]@{ step = $script:failedStep; state = 'failed-or-uncertain'; details = 'Private diagnostics were not exported. No forced process cleanup was attempted.' }
    $script:report.cleanup = 'retained-on-ephemeral-runner-for-disposal'
    throw "Preview acceptance stopped at $script:failedStep. See the sanitized acceptance report; do not treat partial evidence as acceptance."
} finally {
    $script:report | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath (Join-Path $OutputDirectory 'preview-candidate-acceptance.json') -Encoding utf8
}
