# Pure policy and sequencing helpers. Dot-sourcing this file performs no actions.
function Assert-CandidateCondition {
    param([bool]$Condition, [string]$Message)
    if (-not $Condition) { throw $Message }
}

function Get-CandidateFullPath {
    param([string]$Path)
    Assert-CandidateCondition (-not [string]::IsNullOrWhiteSpace($Path)) 'A nonblank absolute path is required.'
    Assert-CandidateCondition ($Path -match '^[A-Za-z]:[\\/]' -and $Path -notmatch '[*?"<>|]' -and
        $Path.Substring(2) -notmatch ':' -and $Path -notmatch '(^|[\\/])[^\\/]*[. ]([\\/]|$)') 'Unsafe Windows path.'
    return [IO.Path]::GetFullPath($Path).TrimEnd('\', '/')
}

function Test-CandidateWithin {
    param([string]$Child, [string]$Parent)
    $fullChild = Get-CandidateFullPath $Child
    $fullParent = Get-CandidateFullPath $Parent
    return $fullChild.StartsWith($fullParent + '\', [StringComparison]::OrdinalIgnoreCase)
}

function Assert-CandidateSeparatePaths {
    param([string]$First, [string]$Second)
    Assert-CandidateCondition ((Get-CandidateFullPath $First) -ine (Get-CandidateFullPath $Second) -and
        -not (Test-CandidateWithin $First $Second) -and -not (Test-CandidateWithin $Second $First)) 'Paths must be separate, nonoverlapping directories.'
}

function Assert-CandidateHostedContext {
    param([hashtable]$Context, [bool]$Approved, [string]$HarnessCommit)
    Assert-CandidateCondition $Approved 'Execution requires explicit approval for disposable DPAPI/test-identity fixtures.'
    Assert-CandidateCondition ($Context.GITHUB_ACTIONS -ceq 'true' -and
        $Context.RUNNER_ENVIRONMENT -ceq 'github-hosted' -and
        $Context.RUNNER_OS -ceq 'Windows' -and
        $Context.GITHUB_REPOSITORY -ceq 'jbcupps/abigail') 'Execution requires the configured GitHub-hosted Windows repository runner.'
    Assert-CandidateCondition ($Context.GITHUB_RUN_ID -match '^[1-9][0-9]*$' -and
        $Context.GITHUB_RUN_ATTEMPT -match '^[1-9][0-9]*$') 'A hosted run identity is required.'
    Assert-CandidateCondition ($HarnessCommit -cmatch '^[a-f0-9]{40}$' -and
        $HarnessCommit -ceq $Context.GITHUB_SHA) 'Harness commit must equal the hosted workflow commit.'
    $temp = Get-CandidateFullPath $Context.RUNNER_TEMP
    Assert-CandidateCondition ($temp.Length -gt 3) 'RUNNER_TEMP cannot be a volume root.'
    $null = Get-CandidateFullPath $Context.GITHUB_WORKSPACE
}

function Assert-CandidateExistingWebView {
    param([bool]$Is64BitOperatingSystem, [Parameter(Mandatory)][scriptblock]$ReadRegistry)
    # Exact x64/x86 NSIS detection branches from Tauri CLI 2.11.4:
    # crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi
    # blob d372e3c391770cf231db974422a1e4f8adaac3a6. x64 NSIS sets
    # RegView64 and reads HKLM\SOFTWARE\WOW6432Node, equivalent to the
    # Registry32 SOFTWARE key below; its HKCU fallback remains RegView64.
    # Both pinned product configurations omit minimumWebview2Version.
    $key = 'SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}'
    $version = & $ReadRegistry 'LocalMachine' 'Registry32' $key 'pv'
    if ([string]::IsNullOrEmpty([string]$version)) {
        $userView = if ($Is64BitOperatingSystem) { 'Registry64' } else { 'Registry32' }
        $version = & $ReadRegistry 'CurrentUser' $userView $key 'pv'
    }
    # Fail closed for invalid/zero registrations as well as the NSIS empty case.
    Assert-CandidateCondition ($version -is [string] -and
        $version -match '^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+$' -and $version -ne '0.0.0.0') 'An existing WebView2 runtime registration is required. Bootstrap installation/update is not authorized.'
    return [ordered]@{ existingWebView2 = $true; detectedVersion = $version; prerequisiteInstallationAllowed = $false }
}

function Assert-CandidatePayloadPolicy {
    param($Payload, [ValidateSet('preview', 'stable')][string]$Channel)
    $launcher = if ($Channel -eq 'preview') { 'Abigail Preview.exe' } else { 'Abigail.exe' }
    $expected = @($launcher, 'resources/abigail-entity-runtime-app.exe', 'resources/hive-daemon.exe', 'resources/entity-daemon.exe')
    Assert-CandidateCondition (@($Payload).Count -eq 4) 'Exactly four payload records are required.'
    $actual = @($Payload | ForEach-Object { $_.relative_path })
    foreach ($relative in $expected) {
        Assert-CandidateCondition (@($actual | Where-Object { $_ -ceq $relative }).Count -eq 1) 'Payload names must exactly match the four reviewed installed executables.'
    }
    foreach ($item in $Payload) {
        Assert-CandidateCondition ($item.sha256 -match '^[a-f0-9]{64}$') 'Payload hashes must be complete SHA256 values.'
    }
}

function Assert-CandidateMetadata {
    param($Manifest, $Signatures, $Prepared, $Baseline, [string]$ProductCommit, [string]$Thumbprint)
    Assert-CandidateCondition ($ProductCommit -ceq $Baseline.productSourceCommit -and
        $ProductCommit -ceq 'a18249a3b622e8b34d0f20ec1f0e24fc0dc7183d') 'Product source must be the reviewed immutable candidate.'
    Assert-CandidateCondition ($Thumbprint -ceq $Baseline.certificateThumbprint) 'Certificate must match the reviewed public baseline.'
    Assert-CandidateCondition ($Manifest.signed -is [bool] -and $Manifest.signed -and
        $Manifest.configuration -ceq 'release' -and $Manifest.version -ceq $Baseline.version) 'Candidate must be a signed release build at the pinned version.'
    Assert-CandidateCondition ($Manifest.source_commit -ceq $ProductCommit -and
        $Signatures.sourceCommit -ceq $ProductCommit -and $Prepared.sourceCommit -ceq $ProductCommit) 'Candidate source provenance does not agree.'
    Assert-CandidateCondition ($Manifest.source_state_sha256 -match '^[a-fA-F0-9]{64}$' -and
        $Manifest.source_state_sha256 -ieq $Signatures.sourceStateSha256 -and
        $Manifest.source_state_sha256 -ieq $Prepared.source.sha256) 'Candidate prepared source hashes do not agree.'
    Assert-CandidateCondition ($Signatures.expectedThumbprint -ceq $Thumbprint -and
        $Signatures.publicationEligible -is [bool] -and -not $Signatures.publicationEligible) 'Preview signing report must retain its nonpublication boundary.'
    foreach ($packaging in @($Manifest.packaging, $Signatures.packaging, $Prepared.packaging)) {
        Assert-CandidateCondition ($packaging.channel -ceq 'preview' -and $packaging.buildProfile -ceq 'preview' -and
            $packaging.productName -ceq 'Abigail Preview' -and $packaging.launcherFileName -ceq 'Abigail Preview.exe' -and
            $packaging.hiveIdentifier -ceq 'com.abigail.preview' -and
            $packaging.runtimeIdentifier -ceq 'com.abigail.preview.entity-runtime') 'Candidate must retain all preview packaging identities.'
        foreach ($property in @('hiveOverlaySha256', 'runtimeOverlaySha256', 'launchContractSha256')) {
            Assert-CandidateCondition ($packaging.$property -match '^[a-fA-F0-9]{64}$' -and
                $packaging.$property -ieq $Manifest.packaging.$property) 'Candidate overlay and launch contract hashes must agree.'
        }
    }
    Assert-CandidateCondition ($Manifest.installer_sha256 -match '^[a-fA-F0-9]{64}$') 'Candidate installer SHA256 is required.'
    Assert-CandidatePayloadPolicy $Manifest.payload preview
}

function Assert-CandidateRegistrationOwnership {
    param($Records, [string]$InstallRoot, [bool]$MustExist)
    $uninstall = @($Records | Where-Object { $_.kind -eq 'uninstall' })
    if ($MustExist) { Assert-CandidateCondition ($uninstall.Count -ge 1) 'Expected installed registration was not created.' }
    foreach ($record in $Records) {
        Assert-CandidateCondition ($record.hive -ceq 'CurrentUser') 'Only current-user fixture registrations are allowed.'
        $location = if ($record.kind -eq 'uninstall') { $record.values.InstallLocation } else { $record.values.'(default)' }
        $location = ([string]$location).Trim('"')
        Assert-CandidateCondition ((Get-CandidateFullPath $location) -ieq (Get-CandidateFullPath $InstallRoot)) 'Registration points outside its owned installation.'
        if ($record.kind -eq 'uninstall') {
            $command = ([string]$record.values.UninstallString).Trim()
            $expected = Join-Path $InstallRoot 'uninstall.exe'
            Assert-CandidateCondition ($command -ieq ('"' + $expected + '"') -or $command -ieq $expected) 'Uninstall registration does not name the exact owned uninstaller.'
        }
    }
}

function Invoke-CandidateAcceptanceSequence {
    # The callback is the only effect boundary, which permits full ordering/failure tests.
    param([Parameter(Mandatory)][scriptblock]$Action)
    foreach ($step in @(
        'fresh-host-guard', 'install-stable', 'verify-stable-payload', 'install-preview', 'verify-preview-payload',
        'lifecycle-seed', 'cold-process-guard', 'capture-before-reinstall',
        'reinstall-preview', 'verify-preview-payload', 'compare-after-reinstall',
        'lifecycle-verify', 'cold-process-guard', 'capture-before-preview-uninstall',
        'uninstall-preview', 'compare-after-preview-uninstall', 'lifecycle-stable-return',
        'cold-process-guard', 'uninstall-stable', 'cleanup-owned-fixture'
    )) {
        & $Action $step
    }
}

function ConvertTo-CandidatePublicLifecycle {
    param($Report, [string]$ExpectedPhase, [switch]$AllowFailed)
    # Copy only reviewed scalar results. Arbitrary log/error/path/state fields cannot escape.
    Assert-CandidateCondition ($Report.schema_version -eq 1 -and
        $Report.kind -ceq 'hosted-preview-candidate-lifecycle') 'Lifecycle report schema mismatch.'
    Assert-CandidateCondition ($Report.phase -ceq $ExpectedPhase) 'Lifecycle report phase mismatch.'
    Assert-CandidateCondition ($Report.passed -is [bool] -and
        $Report.executed -is [bool] -and $Report.executed) 'Lifecycle phase did not report an executed scenario.'
    Assert-CandidateCondition ($Report.processes_quiesced -is [bool]) 'Lifecycle process shutdown status is missing.'
    if (-not $AllowFailed) {
        Assert-CandidateCondition $Report.passed 'Lifecycle phase did not pass an executed scenario.'
        Assert-CandidateCondition $Report.processes_quiesced 'Lifecycle phase has not confirmed owned process shutdown.'
    }
    Assert-CandidateCondition (-not $Report.passed -or $Report.processes_quiesced) 'A passed lifecycle phase cannot leave owned processes unconfirmed.'
    Assert-CandidateCondition ($Report.product_source -ceq 'a18249a3b622e8b34d0f20ec1f0e24fc0dc7183d' -and
        $Report.stable_source -ceq 'e3ad51181da1ea11bded24ca0c39e2dcfe5fa944') 'Lifecycle source pins changed.'
    $phaseAssertions = @{
        seed = @('stable-first-start', 'preview-second-start', 'stable-synthetic-family',
            'preview-synthetic-family-and-routing-guards', 'preview-acknowledged-shutdown', 'stable-owned-job-stop')
        verify = @('preview-first-start', 'stable-second-start', 'preview-upgrade-retained-state',
            'stable-coexistence-retained-state', 'preview-acknowledged-shutdown', 'stable-owned-job-stop',
            'cold-snapshot-verify-restore-and-reopen')
        'stable-return' = @('stable-return-start', 'stable-after-preview-uninstall', 'stable-owned-job-stop')
    }
    Assert-CandidateCondition ($phaseAssertions.ContainsKey($ExpectedPhase)) 'Unknown lifecycle phase.'
    $expected = $phaseAssertions[$ExpectedPhase]
    $actualCount = @($Report.assertions).Count
    Assert-CandidateCondition ($actualCount -le $expected.Count -and
        (-not $Report.passed -or $actualCount -eq $expected.Count)) 'Lifecycle report assertion coverage mismatch.'
    $publicAssertions = @()
    for ($index = 0; $index -lt $actualCount; $index++) {
        $assertion = $Report.assertions[$index]
        Assert-CandidateCondition ($assertion.name -ceq $expected[$index]) 'Lifecycle report assertion name or order mismatch.'
        Assert-CandidateCondition ($assertion.passed -is [bool] -and
            ($assertion.passed -or ($AllowFailed -and -not $Report.passed -and $index -eq $actualCount - 1))) 'Lifecycle report contains a failed assertion.'
        $publicAssertions += [ordered]@{ name = $expected[$index]; passed = $assertion.passed }
    }
    $public = [ordered]@{ phase = $ExpectedPhase; passed = $Report.passed; processesQuiesced = $Report.processes_quiesced; assertions = $publicAssertions }
    if (-not $Report.passed) {
        $failed = @($publicAssertions | Where-Object { -not $_.passed })
        $public.failurePoint = if ($failed.Count) { $failed[0].name } else { 'outside-reviewed-assertion' }
    }
    return $public
}
