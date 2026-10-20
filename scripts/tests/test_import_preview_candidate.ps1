#requires -Version 7.0
<# Offline tests: actual import/provenance code, synthetic text binaries and API
   observations. Only the adjacent native signature verifier is replaced.
   No signing, installer launch, credentials, network calls or real acceptance. #>
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\', '/')
$workspace = Join-Path $tempRoot ('abigail-preview-import-tests-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $workspace | Out-Null
$harness = Join-Path $workspace 'harness'
New-Item -ItemType Directory -Path $harness | Out-Null
foreach ($name in @('import_preview_candidate.ps1', 'prepare_preview_assets.ps1', 'release_provenance.mjs',
    'preview_candidate_policy.ps1', 'preview-candidate-baseline.json')) {
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot "../$name") -Destination (Join-Path $harness $name)
}
$importer = Join-Path $harness 'import_preview_candidate.ps1'
$baselinePath = Join-Path $harness 'preview-candidate-baseline.json'
$originalBaselineBytes = [IO.File]::ReadAllBytes($baselinePath)
$baselineLfText = [Text.UTF8Encoding]::new($false, $true).GetString($originalBaselineBytes).Replace("`r`n", "`n")
$baselineLfBytes = [Text.Encoding]::UTF8.GetBytes($baselineLfText)
$baselineCrlfBytes = [Text.Encoding]::UTF8.GetBytes($baselineLfText.Replace("`n", "`r`n"))
$baseline = Get-Content -LiteralPath $baselinePath -Raw | ConvertFrom-Json
$product = $baseline.productSourceCommit
$harnessCommit = 'b' * 40
$thumbprint = $baseline.certificateThumbprint
$tag = 'v0.0.76-beta.9000'
$installerName = 'Abigail-windows-x64-setup.exe'
$script:passed = 0
$script:skipped = 0
$importNativeTestFault = ''
$nativeStub = @'
param($AssetsDirectory, $SourceCommit, $Tag, $Channel, $WindowsSigned, $CertificateThumbprint)
if ($Channel -cnotin @('candidate', 'beta') -or $WindowsSigned -cne 'true') { throw 'Wrong native policy.' }
if ($importNativeTestFault -eq 'fail') { throw 'Synthetic native refusal.' }
if ($importNativeTestFault -eq 'mutate-installer') {
    [IO.File]::AppendAllText((Join-Path $AssetsDirectory 'Abigail-windows-x64-setup.exe'), 'changed')
}
if ($importNativeTestFault -eq 'mutate-history') {
    [IO.File]::AppendAllText((Join-Path $AssetsDirectory 'hosted-provenance.json'), ' ')
}
if ($importNativeTestFault -eq 'extra') {
    [IO.File]::WriteAllText((Join-Path $AssetsDirectory 'unexpected.json'), '{}')
}
$certificate = if ($importNativeTestFault -eq 'wrong-signer') { 'D' * 40 } else { $CertificateThumbprint }
$evidence = @{
    schemaVersion = 1; sourceCommit = $SourceCommit; tag = $Tag; channel = $Channel
    windowsSigned = $true; expectedThumbprint = $CertificateThumbprint
    artifacts = @(@{
        name = 'Abigail-windows-x64-setup.exe'
        sha256 = (Get-FileHash -LiteralPath (Join-Path $AssetsDirectory 'Abigail-windows-x64-setup.exe')).Hash.ToLowerInvariant()
        signatureStatus = 'Valid'; publisher = 'Synthetic fixture only'; certificateThumbprint = $certificate; timestampType = 'RFC3161'
    })
}
[IO.File]::WriteAllText((Join-Path $AssetsDirectory 'windows-signature-evidence.json'), ($evidence | ConvertTo-Json -Depth 8))
'@
[IO.File]::WriteAllText((Join-Path $harness 'verify_release_artifacts.ps1'), $nativeStub)

function Pass([string]$Name) { $script:passed++; Write-Host "PASS $Name" }
function Hash([string]$Path) { (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant() }
function Write-Json([string]$Path, $Object) { [IO.File]::WriteAllText($Path, ($Object | ConvertTo-Json -Depth 15)) }
function Snapshot([string]$Directory) {
    (@(Get-ChildItem -LiteralPath $Directory -Recurse -Force | Sort-Object FullName | ForEach-Object {
        if ($_.PSIsContainer) { "directory:$([IO.Path]::GetRelativePath($Directory, $_.FullName))" }
        else { "$([IO.Path]::GetRelativePath($Directory, $_.FullName)):$($_.Length):$(Hash $_.FullName)" }
    }) -join "`n")
}
function Refresh-Hosted($Fixture) {
    $hostedPath = Join-Path $Fixture.path 'hosted-provenance.json'
    $hosted = Get-Content -LiteralPath $hostedPath -Raw | ConvertFrom-Json
    $hosted.files = @(Get-ChildItem -LiteralPath $Fixture.path -File | Where-Object Name -CNE 'hosted-provenance.json' | Sort-Object Name | ForEach-Object {
        @{ name = $_.Name; size = $_.Length; sha256 = Hash $_.FullName }
    })
    $hosted.hostedPreparedStateSha256 = Hash (Join-Path $Fixture.path 'prepared-build-state.json')
    Write-Json $hostedPath $hosted
    $Fixture.parameters.ExpectedHostedProvenanceSha256 = Hash $hostedPath
}
function New-Fixture([string]$Name) {
    $root = Join-Path $workspace $Name
    $directory = Join-Path $root 'candidate'
    New-Item -ItemType Directory -Path $directory | Out-Null
    foreach ($name in @($installerName, 'Abigail Preview.exe')) {
        [IO.File]::WriteAllText((Join-Path $directory $name), "Synthetic nonexecutable bytes: $name")
    }
    $installerHash = Hash (Join-Path $directory $installerName)
    $sourceHash = 'c' * 64
    $packaging = @{
        channel = 'preview'; buildProfile = 'preview'; productName = 'Abigail Preview'; launcherFileName = 'Abigail Preview.exe'
        hiveIdentifier = 'com.abigail.preview'; runtimeIdentifier = 'com.abigail.preview.entity-runtime'
        hiveOverlaySha256 = 'd' * 64; runtimeOverlaySha256 = 'e' * 64; launchContractSha256 = 'f' * 64
    }
    $mvp = @{
        signed = $true; configuration = 'release'; version = '0.0.76'; source_commit = $product
        source_state_sha256 = $sourceHash; packaging = $packaging; installer_sha256 = $installerHash
        payload = @(
            @{ relative_path = 'Abigail Preview.exe'; sha256 = Hash (Join-Path $directory 'Abigail Preview.exe') },
            @{ relative_path = 'resources/abigail-entity-runtime-app.exe'; sha256 = '1' * 64 },
            @{ relative_path = 'resources/hive-daemon.exe'; sha256 = '2' * 64 },
            @{ relative_path = 'resources/entity-daemon.exe'; sha256 = '3' * 64 }
        )
    }
    Write-Json (Join-Path $directory 'mvp-build.json') $mvp
    $signatures = @{
        sourceCommit = $product; sourceStateSha256 = $sourceHash; packaging = $packaging
        expectedThumbprint = $thumbprint; publicationEligible = $false
    }
    Write-Json (Join-Path $directory 'signature-report.json') $signatures
    Copy-Item -LiteralPath (Join-Path $directory 'signature-report.json') -Destination (Join-Path $directory 'windows-signatures.json')
    Write-Json (Join-Path $directory 'prepared-build-state.json') @{
        sourceCommit = $product; source = @{ sha256 = $sourceHash }; packaging = $packaging
    }
    Write-Json (Join-Path $directory 'preview-candidate-acceptance.json') @{
        schemaVersion = 1; productSourceCommit = $product; harnessSourceCommit = $harnessCommit
        runId = '42'; runAttempt = '1'; passed = $true; publicationEligible = $false; fullAcceptance = $false; realModelValidation = $false
        previewInstaller = @{ sha256 = $installerHash; thumbprint = $thumbprint }
        sourceStateSha256 = $sourceHash; recoveryCliSha256 = '4' * 64
    }
    Write-Json (Join-Path $directory 'hosted-provenance.json') @{
        schemaVersion = 1; repository = 'jbcupps/abigail'; productSourceCommit = $product; harnessSourceCommit = $harnessCommit
        sourceStateSha256 = $sourceHash; hostedPreparedStateSha256 = Hash (Join-Path $directory 'prepared-build-state.json')
        baselineSha256 = Hash (Join-Path $harness 'preview-candidate-baseline.json'); recoveryCliSha256 = '4' * 64
        workflowRunId = '42'; workflowRunAttempt = '1'; acceptanceOutcome = 'success'; publicationEligible = $false; realModelValidation = $false; files = @()
    }
    $artifactPath = Join-Path $root 'artifact.json'
    Write-Json $artifactPath @{
        id = 43; name = 'abigail-preview-candidate-42-1'; digest = 'sha256:' + ('5' * 64); expired = $false; size_in_bytes = 123
        workflow_run = @{ id = 42; head_sha = $harnessCommit; head_branch = 'dev/test-import'; repository_id = 1; head_repository_id = 1 }
    }
    $runPath = Join-Path $root 'run.json'
    Write-Json $runPath @{
        id = 42; run_attempt = 1; head_sha = $harnessCommit; head_branch = 'dev/test-import'
        repository = @{ id = 1; full_name = 'jbcupps/abigail' }; head_repository = @{ id = 1; full_name = 'jbcupps/abigail' }
        path = '.github/workflows/preview-candidate.yml'; event = 'workflow_dispatch'; status = 'completed'; conclusion = 'success'
    }
    $fixture = @{
        root = $root; path = $directory
        parameters = @{
            CandidateAssetsDirectory = $directory; ArtifactMetadataPath = $artifactPath; WorkflowRunMetadataPath = $runPath
            ExpectedArtifactId = 43; ExpectedArtifactArchiveSha256 = '5' * 64; ExpectedWorkflowRunId = 42; ExpectedWorkflowRunAttempt = 1
            ProductSourceCommit = $product; HarnessSourceCommit = $harnessCommit; ExpectedHostedProvenanceSha256 = ''
            ExpectedInstallerSha256 = $installerHash; ExpectedInstallerSize = (Get-Item -LiteralPath (Join-Path $directory $installerName)).Length
            CertificateThumbprint = $thumbprint; Tag = $tag; OutputDirectory = Join-Path $root 'imported'
        }
    }
    Refresh-Hosted $fixture
    return $fixture
}
function Invoke-Import($Fixture, [hashtable]$Overrides = @{}) {
    $parameters = $Fixture.parameters.Clone()
    foreach ($key in $Overrides.Keys) { $parameters[$key] = $Overrides[$key] }
    & $importer @parameters
}
function Assert-Rejected([string]$Name, $Fixture, [string]$Pattern, [hashtable]$Overrides = @{}) {
    $before = Snapshot $Fixture.path
    $failed = $false
    try { Invoke-Import $Fixture $Overrides | Out-Null }
    catch {
        $failed = $true
        if ($_.Exception.Message -notmatch $Pattern) { throw "$Name returned an unexpected error: $($_.Exception.Message)" }
    }
    if (-not $failed) { throw "$Name was incorrectly accepted." }
    if ((Snapshot $Fixture.path) -cne $before) { throw "$Name changed original artifact bytes." }
    Pass $Name
}
function Change-Json([string]$Path, [scriptblock]$Mutation) {
    $value = Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json
    & $Mutation $value
    Write-Json $Path $value
}

try {
    $fixture = New-Fixture 'happy'
    $before = Snapshot $fixture.path
    $result = Invoke-Import $fixture
    if (-not $result.imported -or $result.publicationEligible -or $result.published -or $result.signedAgain -or $result.preservedFiles -ne 7) { throw 'Incorrect import result.' }
    $output = $fixture.parameters.OutputDirectory
    $binding = Get-Content -LiteralPath (Join-Path $output 'preview-candidate-import.json') -Raw | ConvertFrom-Json
    $manifest = Get-Content -LiteralPath (Join-Path $output 'release-provenance.json') -Raw | ConvertFrom-Json
    if ($manifest.channel -cne 'candidate' -or $manifest.tag -cne $tag -or $binding.tagBinding.phase -cne 'candidate-import' -or
        $null -ne $binding.tagBinding.buildTimeTag -or $binding.productSourceCommit -ceq $binding.harnessSourceCommit -or
        $binding.fullAcceptance -or $binding.publicationEligible) { throw 'Import identity/limitations were lost.' }
    if (Test-Path -LiteralPath (Join-Path $output 'Abigail Preview.exe')) { throw 'Standalone launcher leaked into canonical release assets.' }
    foreach ($name in @('mvp-build.json', 'prepared-build-state.json', 'preview-candidate-acceptance.json', 'signature-report.json', 'windows-signatures.json', 'hosted-provenance.json', $installerName)) {
        if ((Hash (Join-Path $output $name)) -cne (Hash (Join-Path $fixture.path $name))) { throw "Historical bytes changed: $name" }
    }
    if ((Snapshot $fixture.path) -cne $before) { throw 'Successful import changed original artifact.' }
    Pass 'exact bytes, separate product/harness, first tag binding and nonpublication flags preserved'
    & (Join-Path $harness 'prepare_preview_assets.ps1') -CandidateAssetsDirectory $output -SourceCommit $product `
        -Tag $tag -CertificateThumbprint $thumbprint -OutputDirectory (Join-Path $fixture.root 'beta-staged') | Out-Null
    if ((Hash (Join-Path $fixture.root "beta-staged/$installerName")) -cne $fixture.parameters.ExpectedInstallerSha256 -or
        (Hash (Join-Path $fixture.root 'beta-staged/hosted-provenance.json')) -cne $fixture.parameters.ExpectedHostedProvenanceSha256) { throw 'Existing beta preparation changed candidate history.' }
    Pass 'existing beta metadata helper accepts import and preserves hosted history and installer'
    # This immutable candidate's real Git-blob and hosted-checkout regression:
    # the source baseline is LF; the historical Windows runner hashed CRLF.
    $realLfHash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($baselineLfBytes)).ToLowerInvariant()
    $realCrlfHash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($baselineCrlfBytes)).ToLowerInvariant()
    if ($baselineLfBytes.Length -ne 1832 -or $baselineCrlfBytes.Length -ne 1888 -or
        $realLfHash -cne '4562c5fc263b5a9d0728133ca56aa196e6a285ae78ec819222c23ca416476a83' -or
        $realCrlfHash -cne '1c64c904e21b14fcbdfff7bddb7951c96b306e1d2052d0b64cdab6448eb74adc') {
        throw 'Reviewed real baseline newline regression bytes changed.'
    }
    Pass 'real baseline LF and hosted CRLF digests reproduced exactly'
    foreach ($case in @(
        @{ name = 'host-crlf-local-lf'; hostedBytes = $baselineCrlfBytes; localBytes = $baselineLfBytes; hostForm = 'crlf'; localForm = 'lf' },
        @{ name = 'host-lf-local-crlf'; hostedBytes = $baselineLfBytes; localBytes = $baselineCrlfBytes; hostForm = 'lf'; localForm = 'crlf' },
        @{ name = 'host-crlf-local-crlf'; hostedBytes = $baselineCrlfBytes; localBytes = $baselineCrlfBytes; hostForm = 'crlf'; localForm = 'crlf' }
    )) {
        try {
            [IO.File]::WriteAllBytes($baselinePath, $case.hostedBytes)
            $newlineFixture = New-Fixture $case.name
            $newlineBefore = Snapshot $newlineFixture.path
            $hostedBaselineHash = Hash $baselinePath
            [IO.File]::WriteAllBytes($baselinePath, $case.localBytes)
            $localBaselineHash = Hash $baselinePath
            $null = Invoke-Import $newlineFixture
            $newlineBinding = (Get-Content -LiteralPath (Join-Path $newlineFixture.parameters.OutputDirectory 'preview-candidate-import.json') -Raw | ConvertFrom-Json).baselineBinding
            if ($newlineBinding.localSha256 -cne $localBaselineHash -or $newlineBinding.hostedSha256 -cne $hostedBaselineHash -or
                $newlineBinding.lfSha256 -cne $realLfHash -or $newlineBinding.localNewlineForm -cne $case.localForm -or
                $newlineBinding.hostedNewlineForm -cne $case.hostForm -or
                $newlineBinding.verification -cne 'exact-utf8-bytes-with-lf-or-crlf-checkout-only' -or
                (Snapshot $newlineFixture.path) -cne $newlineBefore -or (Hash $baselinePath) -cne $localBaselineHash -or
                (Hash (Join-Path $newlineFixture.parameters.OutputDirectory 'hosted-provenance.json')) -cne $newlineFixture.parameters.ExpectedHostedProvenanceSha256) {
                throw "$($case.name) changed baseline/history bytes or omitted the exact newline interpretation."
            }
            Pass "$($case.name) accepted with explicit byte comparison and unchanged history"
        } finally { [IO.File]::WriteAllBytes($baselinePath, $originalBaselineBytes) }
    }
    foreach ($case in @(
        @{ name = 'changed-baseline-content'; bytes = [Text.Encoding]::UTF8.GetBytes($baselineLfText.Replace('0.0.76', '0.0.77')); pattern = 'Hosted baseline digest differs' },
        @{ name = 'changed-baseline-whitespace'; bytes = [Text.Encoding]::UTF8.GetBytes($baselineLfText.Replace('  "repository"', '    "repository"')); pattern = 'Hosted baseline digest differs' },
        @{ name = 'missing-baseline-final-newline'; bytes = [Text.Encoding]::UTF8.GetBytes($baselineLfText.Substring(0, $baselineLfText.Length - 1)); pattern = 'Hosted baseline digest differs' },
        @{ name = 'mixed-baseline-newlines'; bytes = [Text.Encoding]::UTF8.GetBytes($baselineLfText.Insert($baselineLfText.IndexOf("`n"), "`r")); pattern = 'uniform LF or CRLF' },
        @{ name = 'bare-cr-baseline-newline'; bytes = [Text.Encoding]::UTF8.GetBytes([regex]::new("`n").Replace($baselineLfText, "`r", 1)); pattern = 'uniform LF or CRLF' },
        @{ name = 'added-baseline-bom'; bytes = [byte[]](@(0xef, 0xbb, 0xbf) + $baselineLfBytes); pattern = 'Hosted baseline digest differs' },
        @{ name = 'invalid-baseline-utf8'; bytes = [byte[]](@(0xff) + $baselineLfBytes); pattern = 'valid UTF-8' }
    )) {
        try {
            [IO.File]::WriteAllBytes($baselinePath, $baselineLfBytes)
            $baselineFixture = New-Fixture $case.name
            [IO.File]::WriteAllBytes($baselinePath, $case.bytes)
            Assert-Rejected $case.name $baselineFixture $case.pattern
            if (Test-Path -LiteralPath $baselineFixture.parameters.OutputDirectory) { throw "$($case.name) created an output directory before rejecting baseline bytes." }
        } finally { [IO.File]::WriteAllBytes($baselinePath, $originalBaselineBytes) }
    }
    Assert-Rejected 'existing output refused' $fixture 'must be new'
    $fixture = New-Fixture 'nested'
    Assert-Rejected 'nested output refused' $fixture 'must be separate' @{ OutputDirectory = Join-Path $fixture.path 'new-output' }
    foreach ($case in @(
        @{ name = 'wrong-artifact-id'; override = @{ ExpectedArtifactId = 99 }; pattern = 'Artifact API identity' },
        @{ name = 'wrong-artifact-digest'; override = @{ ExpectedArtifactArchiveSha256 = '6' * 64 }; pattern = 'Artifact API identity' },
        @{ name = 'wrong-run'; override = @{ ExpectedWorkflowRunId = 99 }; pattern = 'Artifact API identity' },
        @{ name = 'wrong-attempt'; override = @{ ExpectedWorkflowRunAttempt = 2 }; pattern = 'Artifact API identity' },
        @{ name = 'wrong-harness'; override = @{ HarnessSourceCommit = 'a' * 40 }; pattern = 'workflow metadata' },
        @{ name = 'wrong-product'; override = @{ ProductSourceCommit = 'a' * 40 }; pattern = 'Hosted provenance identity' },
        @{ name = 'wrong-hosted-hash'; override = @{ ExpectedHostedProvenanceSha256 = '6' * 64 }; pattern = 'explicit expected hash' },
        @{ name = 'wrong-installer-hash'; override = @{ ExpectedInstallerSha256 = '6' * 64 }; pattern = 'explicitly approved bytes' },
        @{ name = 'wrong-installer-size'; override = @{ ExpectedInstallerSize = 1 }; pattern = 'explicitly approved bytes' },
        @{ name = 'wrong-signer-pin'; override = @{ CertificateThumbprint = 'D' * 40 }; pattern = 'reviewed public baseline' },
        @{ name = 'stable-tag'; override = @{ Tag = 'v0.0.76' }; pattern = 'stable import is forbidden' },
        @{ name = 'wrong-product-version'; override = @{ Tag = 'v0.0.77-beta.1' }; pattern = 'numeric version differs' }
    )) {
        Assert-Rejected $case.name (New-Fixture $case.name) $case.pattern $case.override
    }
    foreach ($case in @(
        @{ name = 'failed-run'; mutation = { param($v) $v.conclusion = 'failure' } },
        @{ name = 'foreign-repository'; mutation = { param($v) $v.repository.full_name = 'other/repo' } },
        @{ name = 'foreign-head-repository'; mutation = { param($v) $v.head_repository.full_name = 'other/repo' } },
        @{ name = 'wrong-workflow'; mutation = { param($v) $v.path = '.github/workflows/release.yml' } },
        @{ name = 'wrong-event'; mutation = { param($v) $v.event = 'push' } },
        @{ name = 'not-completed'; mutation = { param($v) $v.status = 'in_progress' } }
    )) {
        $fixture = New-Fixture $case.name
        Change-Json $fixture.parameters.WorkflowRunMetadataPath $case.mutation
        Assert-Rejected $case.name $fixture 'workflow metadata'
    }
    foreach ($case in @(
        @{ name = 'expired-artifact'; mutation = { param($v) $v.expired = $true }; pattern = 'Artifact API identity' },
        @{ name = 'foreign-artifact-run'; mutation = { param($v) $v.workflow_run.id = 99 }; pattern = 'Artifact and workflow' },
        @{ name = 'foreign-artifact-repo'; mutation = { param($v) $v.workflow_run.repository_id = 99 }; pattern = 'Artifact and workflow' }
    )) {
        $fixture = New-Fixture $case.name
        Change-Json $fixture.parameters.ArtifactMetadataPath $case.mutation
        Assert-Rejected $case.name $fixture $case.pattern
    }
    $fixture = New-Fixture 'tampered-installer'
    [IO.File]::AppendAllText((Join-Path $fixture.path $installerName), 'changed')
    Assert-Rejected 'tampered installer rejected before native boundary' $fixture 'pinned record'
    $fixture = New-Fixture 'extra-executable'
    [IO.File]::WriteAllText((Join-Path $fixture.path 'unapproved.exe'), 'not executable')
    Assert-Rejected 'extra executable refused' $fixture 'missing or extra'
    $fixture = New-Fixture 'duplicate-provenance'
    Change-Json (Join-Path $fixture.path 'hosted-provenance.json') { param($v) $v.files[1].name = $v.files[0].name }
    $fixture.parameters.ExpectedHostedProvenanceSha256 = Hash (Join-Path $fixture.path 'hosted-provenance.json')
    Assert-Rejected 'duplicate provenance asset refused' $fixture 'missing or duplicate'
    $fixture = New-Fixture 'promoted-historical-flags'
    Change-Json (Join-Path $fixture.path 'preview-candidate-acceptance.json') { param($v) $v.fullAcceptance = $true }
    Refresh-Hosted $fixture
    Assert-Rejected 'historical full-acceptance relabel refused' $fixture 'Selected hosted acceptance'
    $fixture = New-Fixture 'wrong-packaging'
    Change-Json (Join-Path $fixture.path 'mvp-build.json') { param($v) $v.packaging.channel = 'stable' }
    Refresh-Hosted $fixture
    Assert-Rejected 'stable packaging refused' $fixture 'preview packaging identities'
    $fixture = New-Fixture 'launcher-mismatch'
    Change-Json (Join-Path $fixture.path 'mvp-build.json') { param($v) $v.payload[0].sha256 = '7' * 64 }
    Refresh-Hosted $fixture
    Assert-Rejected 'captured launcher payload mismatch refused' $fixture 'Captured launcher differs'
    foreach ($fault in @('fail', 'mutate-installer', 'mutate-history', 'extra', 'wrong-signer')) {
        $fixture = New-Fixture "native-$fault"
        $importNativeTestFault = $fault
        $pattern = switch ($fault) {
            'fail' { 'Synthetic native refusal' }
            'mutate-installer' { 'pinned record' }
            'mutate-history' { 'pinned record' }
            'extra' { 'missing or extra' }
            'wrong-signer' { 'provenance validation failed' }
        }
        Assert-Rejected "native verifier $fault" $fixture $pattern
        if (Test-Path -LiteralPath (Join-Path $fixture.parameters.OutputDirectory 'release-provenance.json')) {
            throw 'Failed native verification left completed provenance.'
        }
    }
    $importNativeTestFault = ''
    $fixture = New-Fixture 'links'
    $link = Join-Path $workspace 'linked-source'
    try { New-Item -ItemType Junction -Path $link -Target $fixture.path -ErrorAction Stop | Out-Null }
    catch {
        if ($env:OS -ne 'Windows_NT' -or $_.Exception.Message -match 'privilege|permission|denied|not supported') {
            $script:skipped++; Write-Host 'SKIP junction creation unavailable.'
        } else { throw }
    }
    if (Test-Path -LiteralPath $link) {
        try {
            Assert-Rejected 'source junction refused' $fixture 'reparse points' @{ CandidateAssetsDirectory = $link }
            Assert-Rejected 'output-parent junction refused' $fixture 'reparse points' @{ OutputDirectory = Join-Path $link 'new-output' }
        } finally { Remove-Item -LiteralPath $link -Force }
    }
    Write-Host "Hosted candidate import: $script:passed checks passed; $script:skipped skipped. Native verifier mocked; no real compatibility/signing/publication claimed."
} finally {
    $resolved = [IO.Path]::GetFullPath($workspace)
    if ([IO.Path]::GetDirectoryName($resolved) -ne $tempRoot -or [IO.Path]::GetFileName($resolved) -notmatch '^abigail-preview-import-tests-[0-9a-f]{32}$') {
        throw 'Refusing cleanup outside the generated test workspace.'
    }
    if (Test-Path -LiteralPath $resolved) { Remove-Item -LiteralPath $resolved -Recurse -Force }
}
