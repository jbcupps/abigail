#requires -Version 7.0
<# Import a hosted preview artifact into a NEW release-format candidate directory.
   No build, signing, installation, credentials, network requests, tags or publication.
   The original artifact and its historical reports are never changed. The beta
   tag is first bound here as promotion metadata, not claimed as a build-time tag.
   Saved GitHub API metadata is an input observation, not independently fetched
   authority; callers must obtain it through the reviewed read-only release path.
   Required compatibility/source-CI/public-download gates remain separate.
   Native verification is the existing adjacent verifier; there is no skip flag.
   Failure may leave an incomplete NEW destination for inspection. #>
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$CandidateAssetsDirectory,
    [Parameter(Mandatory)][string]$ArtifactMetadataPath,
    [Parameter(Mandatory)][string]$WorkflowRunMetadataPath,
    [Parameter(Mandatory)][long]$ExpectedArtifactId,
    [Parameter(Mandatory)][string]$ExpectedArtifactArchiveSha256,
    [Parameter(Mandatory)][long]$ExpectedWorkflowRunId,
    [Parameter(Mandatory)][int]$ExpectedWorkflowRunAttempt,
    [Parameter(Mandatory)][string]$ProductSourceCommit,
    [Parameter(Mandatory)][string]$HarnessSourceCommit,
    [Parameter(Mandatory)][string]$ExpectedHostedProvenanceSha256,
    [Parameter(Mandatory)][string]$ExpectedInstallerSha256,
    [Parameter(Mandatory)][long]$ExpectedInstallerSize,
    [Parameter(Mandatory)][string]$CertificateThumbprint,
    [Parameter(Mandatory)][string]$Tag,
    [Parameter(Mandatory)][string]$OutputDirectory
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Assert-Import([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
}

function Assert-ImportPath([string]$Path, [ValidateSet('directory', 'file', 'new-directory')][string]$Kind) {
    Assert-Import (-not [string]::IsNullOrWhiteSpace($Path) -and [IO.Path]::IsPathFullyQualified($Path)) 'Import paths must be explicit absolute paths.'
    $absolute = [IO.Path]::GetFullPath($Path)
    Assert-Import (-not $absolute.StartsWith('\\') -and -not $absolute.StartsWith('//')) 'Import paths must be local.'
    $cursor = $absolute
    $leaf = $true
    while ($cursor) {
        if (Test-Path -LiteralPath $cursor) {
            $item = Get-Item -LiteralPath $cursor -Force
            Assert-Import (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -eq 0) 'Import paths cannot traverse reparse points.'
            if ($leaf) {
                Assert-Import ($Kind -ne 'new-directory') 'OutputDirectory must be new; existing output is never overwritten.'
                Assert-Import (($Kind -eq 'directory') -eq $item.PSIsContainer) 'Import path has the wrong file/directory type.'
            } else {
                Assert-Import $item.PSIsContainer 'Import path parent must be a directory.'
            }
        } else {
            Assert-Import ($leaf -and $Kind -eq 'new-directory') 'Import source or output parent is missing.'
        }
        $leaf = $false
        $cursor = [IO.Path]::GetDirectoryName($cursor)
    }
    return $absolute.TrimEnd([IO.Path]::DirectorySeparatorChar, [IO.Path]::AltDirectorySeparatorChar)
}

function Get-ImportHash([string]$Path) {
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Get-ImportBaselineBinding([string]$Path, [string]$HostedSha256) {
    # The historical workflow hashed the checked-out baseline. Git may expand
    # LF to CRLF on Windows. Compare only those two exact UTF-8 representations:
    # do not trim, parse/reserialize, or normalize any other bytes or whitespace.
    $bytes = [IO.File]::ReadAllBytes($Path)
    $utf8 = [Text.UTF8Encoding]::new($false, $true)
    try { $text = $utf8.GetString($bytes) }
    catch { throw 'Candidate baseline must be valid UTF-8.' }
    $lfText = $text.Replace("`r`n", "`n")
    Assert-Import (-not $lfText.Contains("`r") -and
        -not ($text.Contains("`r`n") -and [regex]::IsMatch($text, '(?<!\r)\n'))) 'Candidate baseline must use uniform LF or CRLF newlines; mixed and bare CR forms are refused.'
    $localHash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($bytes)).ToLowerInvariant()
    $lfHash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($utf8.GetBytes($lfText))).ToLowerInvariant()
    $crlfHash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($utf8.GetBytes($lfText.Replace("`n", "`r`n")))).ToLowerInvariant()
    Assert-Import ($HostedSha256 -ceq $lfHash -or $HostedSha256 -ceq $crlfHash) 'Hosted baseline digest differs beyond the permitted LF/CRLF checkout representation.'
    return [ordered]@{
        name = 'preview-candidate-baseline.json'
        localSha256 = $localHash; hostedSha256 = $HostedSha256; lfSha256 = $lfHash
        localNewlineForm = if ($text.Contains("`r`n")) { 'crlf' } elseif ($text.Contains("`n")) { 'lf' } else { 'none' }
        hostedNewlineForm = if ($HostedSha256 -ceq $lfHash) { 'lf' } else { 'crlf' }
        verification = 'exact-utf8-bytes-with-lf-or-crlf-checkout-only'
    }
}

function Assert-ImportRecord($Record, [string]$Directory) {
    $file = Get-Item -LiteralPath (Join-Path $Directory $Record.name) -Force
    Assert-Import (-not $file.PSIsContainer -and ($file.Attributes -band [IO.FileAttributes]::ReparsePoint) -eq 0) 'Imported files must remain regular files.'
    Assert-Import ($file.Length -eq $Record.size -and (Get-ImportHash $file.FullName) -ceq $Record.sha256) 'Imported file bytes differ from their pinned record.'
}

function Assert-ImportFileSet([string]$Directory, [string[]]$Names) {
    $entries = @(Get-ChildItem -LiteralPath $Directory -Force)
    Assert-Import ($entries.Count -eq $Names.Count) 'Import file set has missing or extra entries.'
    foreach ($entry in $entries) {
        Assert-Import (-not $entry.PSIsContainer -and ($entry.Attributes -band [IO.FileAttributes]::ReparsePoint) -eq 0 -and $entry.Name -cin $Names) 'Import file set contains an unapproved file, directory or link.'
    }
}

foreach ($pin in @($ProductSourceCommit, $HarnessSourceCommit)) {
    Assert-Import ($pin -cmatch '^[a-f0-9]{40}$') 'Product and harness pins must be complete lowercase commit SHAs.'
}
foreach ($pin in @($ExpectedArtifactArchiveSha256, $ExpectedHostedProvenanceSha256, $ExpectedInstallerSha256)) {
    Assert-Import ($pin -cmatch '^[a-f0-9]{64}$') 'Expected digests must be complete lowercase SHA256 values.'
}
Assert-Import ($CertificateThumbprint -cmatch '^[A-F0-9]{40}$') 'Certificate pin must be a complete uppercase thumbprint.'
Assert-Import ($ExpectedArtifactId -gt 0 -and $ExpectedWorkflowRunId -gt 0 -and $ExpectedWorkflowRunAttempt -gt 0 -and $ExpectedInstallerSize -gt 0) 'Artifact/run identity and installer size must be positive.'
Assert-Import ($Tag -cmatch '^v(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)-beta\.(?:0|[1-9]\d*)$') 'A numeric beta tag is required; stable import is forbidden.'
$source = Assert-ImportPath $CandidateAssetsDirectory directory
$output = Assert-ImportPath $OutputDirectory new-directory
$artifactPath = Assert-ImportPath $ArtifactMetadataPath file
$runPath = Assert-ImportPath $WorkflowRunMetadataPath file
$comparison = if ([OperatingSystem]::IsWindows()) { [StringComparison]::OrdinalIgnoreCase } else { [StringComparison]::Ordinal }
Assert-Import (-not $output.Equals($source, $comparison) -and
    -not $output.StartsWith($source + [IO.Path]::DirectorySeparatorChar, $comparison) -and
    -not $source.StartsWith($output + [IO.Path]::DirectorySeparatorChar, $comparison)) 'Import output must be separate from the source.'

$installerName = 'Abigail-windows-x64-setup.exe'
$provenanceName = 'hosted-provenance.json'
$importName = 'preview-candidate-import.json'
$evidenceName = 'windows-signature-evidence.json'
$manifestName = 'release-provenance.json'
$recordNames = @('Abigail Preview.exe', $installerName, 'mvp-build.json', 'prepared-build-state.json',
    'preview-candidate-acceptance.json', 'signature-report.json', 'windows-signatures.json')
$inputNames = $recordNames + @($provenanceName)
$preservedNames = @($inputNames | Where-Object { $_ -cne 'Abigail Preview.exe' })
$baselinePath = Join-Path $PSScriptRoot 'preview-candidate-baseline.json'
$locks = [Collections.Generic.List[IO.FileStream]]::new()
try {
    Assert-ImportFileSet $source $inputNames
    # Hold every input against mutation/deletion while validating and copying.
    foreach ($path in (@($inputNames | ForEach-Object { Join-Path $source $_ }) + @($artifactPath, $runPath, $baselinePath))) {
        $null = Assert-ImportPath $path file
        $locks.Add([IO.File]::Open($path, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read))
    }
    Assert-Import ((Get-ImportHash (Join-Path $source $provenanceName)) -ceq $ExpectedHostedProvenanceSha256) 'Hosted provenance does not match its explicit expected hash.'
    $hosted = Get-Content -LiteralPath (Join-Path $source $provenanceName) -Raw | ConvertFrom-Json
    $artifact = Get-Content -LiteralPath $artifactPath -Raw | ConvertFrom-Json
    $run = Get-Content -LiteralPath $runPath -Raw | ConvertFrom-Json
    $expectedName = "abigail-preview-candidate-$ExpectedWorkflowRunId-$ExpectedWorkflowRunAttempt"
    Assert-Import ($artifact.id -eq $ExpectedArtifactId -and $artifact.name -ceq $expectedName -and
        $artifact.digest -ceq "sha256:$ExpectedArtifactArchiveSha256" -and
        $artifact.expired -is [bool] -and -not $artifact.expired -and $artifact.size_in_bytes -gt 0) 'Artifact API identity/digest/availability differs from expected pins.'
    Assert-Import ($run.id -eq $ExpectedWorkflowRunId -and $run.run_attempt -eq $ExpectedWorkflowRunAttempt -and
        $run.repository.full_name -ceq 'jbcupps/abigail' -and $run.head_repository.full_name -ceq 'jbcupps/abigail' -and
        $run.head_sha -ceq $HarnessSourceCommit -and $run.head_branch -cmatch '^dev/[A-Za-z0-9._/-]+$' -and
        $run.path -ceq '.github/workflows/preview-candidate.yml' -and $run.event -ceq 'workflow_dispatch' -and
        $run.status -ceq 'completed' -and $run.conclusion -ceq 'success') 'Candidate workflow metadata does not identify the successful reviewed harness execution.'
    Assert-Import ($artifact.workflow_run.id -eq $run.id -and $artifact.workflow_run.head_sha -ceq $HarnessSourceCommit -and
        $artifact.workflow_run.head_branch -ceq $run.head_branch -and
        $artifact.workflow_run.repository_id -eq $run.repository.id -and
        $artifact.workflow_run.head_repository_id -eq $run.head_repository.id) 'Artifact and workflow repository/head identities differ.'
    Assert-Import ($hosted.schemaVersion -eq 1 -and $hosted.repository -ceq 'jbcupps/abigail' -and
        $hosted.productSourceCommit -ceq $ProductSourceCommit -and $hosted.harnessSourceCommit -ceq $HarnessSourceCommit -and
        [string]$hosted.workflowRunId -ceq [string]$ExpectedWorkflowRunId -and
        [string]$hosted.workflowRunAttempt -ceq [string]$ExpectedWorkflowRunAttempt -and
        $hosted.acceptanceOutcome -ceq 'success' -and
        $hosted.publicationEligible -is [bool] -and -not $hosted.publicationEligible -and
        $hosted.realModelValidation -is [bool] -and -not $hosted.realModelValidation) 'Hosted provenance identity or limited-acceptance boundary differs.'
    Assert-Import (@($hosted.files).Count -eq $recordNames.Count) 'Hosted provenance has an unexpected asset count.'
    foreach ($name in $recordNames) {
        Assert-Import (@($hosted.files | Where-Object name -CEQ $name).Count -eq 1) 'Hosted provenance has a missing or duplicate asset name.'
    }
    foreach ($record in $hosted.files) {
        Assert-Import ($record.name -cin $recordNames -and $record.sha256 -cmatch '^[a-f0-9]{64}$' -and $record.size -gt 0) 'Hosted provenance has an invalid asset record.'
        Assert-ImportRecord $record $source
    }
    $installer = @($hosted.files | Where-Object name -CEQ $installerName)[0]
    Assert-Import ($installer.sha256 -ceq $ExpectedInstallerSha256 -and $installer.size -eq $ExpectedInstallerSize) 'Installer identity differs from the explicitly approved bytes.'
    $mvp = Get-Content -LiteralPath (Join-Path $source 'mvp-build.json') -Raw | ConvertFrom-Json
    $signatures = Get-Content -LiteralPath (Join-Path $source 'signature-report.json') -Raw | ConvertFrom-Json
    $prepared = Get-Content -LiteralPath (Join-Path $source 'prepared-build-state.json') -Raw | ConvertFrom-Json
    $acceptance = Get-Content -LiteralPath (Join-Path $source 'preview-candidate-acceptance.json') -Raw | ConvertFrom-Json
    . (Join-Path $PSScriptRoot 'preview_candidate_policy.ps1')
    $baselineBinding = Get-ImportBaselineBinding $baselinePath $hosted.baselineSha256
    $baseline = Get-Content -LiteralPath $baselinePath -Raw | ConvertFrom-Json
    Assert-CandidateMetadata $mvp $signatures $prepared $baseline $ProductSourceCommit $CertificateThumbprint
    Assert-Import ($mvp.installer_sha256 -ieq $ExpectedInstallerSha256 -and
        $mvp.source_state_sha256 -ieq $hosted.sourceStateSha256 -and
        $hosted.hostedPreparedStateSha256 -ceq (Get-ImportHash (Join-Path $source 'prepared-build-state.json'))) 'Hosted source/preparation/installer bindings disagree.'
    Assert-Import ($Tag -cmatch ('^v' + [regex]::Escape($mvp.version) + '-beta\.')) 'Promotion tag numeric version differs from the signed product.'
    Assert-Import ((Get-ImportHash (Join-Path $source 'signature-report.json')) -ceq
        (Get-ImportHash (Join-Path $source 'windows-signatures.json'))) 'Historical signature reports differ.'
    $launcherRecord = @($hosted.files | Where-Object name -CEQ 'Abigail Preview.exe')[0]
    Assert-Import (@($mvp.payload | Where-Object { $_.relative_path -ceq 'Abigail Preview.exe' -and $_.sha256 -ieq $launcherRecord.sha256 }).Count -eq 1) 'Captured launcher differs from installed payload metadata.'
    Assert-Import ($acceptance.schemaVersion -eq 1 -and $acceptance.productSourceCommit -ceq $ProductSourceCommit -and
        $acceptance.harnessSourceCommit -ceq $HarnessSourceCommit -and
        [string]$acceptance.runId -ceq [string]$ExpectedWorkflowRunId -and
        [string]$acceptance.runAttempt -ceq [string]$ExpectedWorkflowRunAttempt -and
        $acceptance.passed -is [bool] -and $acceptance.passed -and
        $acceptance.publicationEligible -is [bool] -and -not $acceptance.publicationEligible -and
        $acceptance.fullAcceptance -is [bool] -and -not $acceptance.fullAcceptance -and
        $acceptance.realModelValidation -is [bool] -and -not $acceptance.realModelValidation -and
        $acceptance.previewInstaller.sha256 -ceq $ExpectedInstallerSha256 -and
        $acceptance.previewInstaller.thumbprint -ceq $CertificateThumbprint -and
        $acceptance.sourceStateSha256 -ieq $hosted.sourceStateSha256 -and
        $acceptance.recoveryCliSha256 -ceq $hosted.recoveryCliSha256) 'Selected hosted acceptance identity or limitation flags differ.'

    $preserved = @($preservedNames | ForEach-Object {
        $item = Get-Item -LiteralPath (Join-Path $source $_)
        [pscustomobject]@{ name = $_; size = $item.Length; sha256 = Get-ImportHash $item.FullName }
    })
    New-Item -ItemType Directory -Path $output | Out-Null
    $null = Assert-ImportPath $output directory
    foreach ($record in $preserved) {
        [IO.File]::Copy((Join-Path $source $record.name), (Join-Path $output $record.name), $false)
        Assert-ImportRecord $record $output
    }
    $binding = [ordered]@{
        schemaVersion = 1; kind = 'hosted-preview-candidate-import'
        repository = 'jbcupps/abigail'; productSourceCommit = $ProductSourceCommit; harnessSourceCommit = $HarnessSourceCommit
        sourceStateSha256 = $hosted.sourceStateSha256.ToLowerInvariant()
        baselineBinding = $baselineBinding
        workflow = [ordered]@{ id = $ExpectedWorkflowRunId; attempt = $ExpectedWorkflowRunAttempt; path = $run.path; headSha = $HarnessSourceCommit }
        artifact = [ordered]@{
            id = $ExpectedArtifactId; name = $expectedName; archiveSha256 = $ExpectedArtifactArchiveSha256
            archiveDigestVerification = 'matched-reviewed-api-metadata-not-rehashed-archive'
            hostedProvenanceSha256 = $ExpectedHostedProvenanceSha256
            artifactMetadataSha256 = Get-ImportHash $artifactPath; workflowMetadataSha256 = Get-ImportHash $runPath
        }
        tagBinding = [ordered]@{ tag = $Tag; phase = 'candidate-import'; buildTimeTag = $null }
        installer = [ordered]@{ name = $installerName; size = $ExpectedInstallerSize; sha256 = $ExpectedInstallerSha256; certificateThumbprint = $CertificateThumbprint }
        preservedFiles = $preserved
        omittedCapturedLauncher = [ordered]@{ name = $launcherRecord.name; size = $launcherRecord.size; sha256 = $launcherRecord.sha256; retainedInOriginalArtifact = $true }
        publicationEligible = $false; fullAcceptance = $false; published = $false; signedAgain = $false
        requiredBeforePublication = @('Exact-product required CI gate', 'Complete independently verified preview compatibility evidence', 'Reviewed publication-only path', 'Public-download verification before preview selection')
    }
    [IO.File]::WriteAllText((Join-Path $output $importName), ($binding | ConvertTo-Json -Depth 10) + "`n", [Text.UTF8Encoding]::new($false))
    $bindingRecord = [pscustomobject]@{ name = $importName; size = (Get-Item -LiteralPath (Join-Path $output $importName)).Length; sha256 = Get-ImportHash (Join-Path $output $importName) }
    # Never trust a recorded signature alone. This verifier performs no signing.
    & (Join-Path $PSScriptRoot 'verify_release_artifacts.ps1') -AssetsDirectory $output `
        -SourceCommit $ProductSourceCommit -Tag $Tag -Channel candidate -WindowsSigned true -CertificateThumbprint $CertificateThumbprint
    foreach ($record in ($preserved + @($bindingRecord))) { Assert-ImportRecord $record $output }
    Assert-ImportFileSet $output ($preservedNames + @($importName, $evidenceName))
    $node = (Get-Command node -CommandType Application -ErrorAction Stop | Select-Object -First 1).Source
    $policy = @('--assets-dir', $output, '--source-commit', $ProductSourceCommit, '--tag', $Tag, '--channel', 'candidate',
        '--windows-signed', 'true', '--expected-thumbprint', $CertificateThumbprint)
    foreach ($action in @('create', 'verify')) {
        & $node (Join-Path $PSScriptRoot 'release_provenance.mjs') $action @policy | Out-Host
        if ($LASTEXITCODE -ne 0) { throw 'Imported candidate provenance validation failed.' }
    }
    Assert-ImportFileSet $output ($preservedNames + @($importName, $evidenceName, $manifestName))
    foreach ($record in ($preserved + @($bindingRecord))) { Assert-ImportRecord $record $output }
    Assert-ImportFileSet $source $inputNames
    foreach ($record in $hosted.files) { Assert-ImportRecord $record $source }
    Assert-Import ((Get-ImportHash (Join-Path $source $provenanceName)) -ceq $ExpectedHostedProvenanceSha256) 'Original hosted provenance changed.'
    [pscustomobject]@{
        imported = $true; publicationEligible = $false; published = $false; signedAgain = $false
        sourceCommit = $ProductSourceCommit; harnessSourceCommit = $HarnessSourceCommit; tag = $Tag; channel = 'candidate'
        outputDirectory = $output; installerSha256 = $ExpectedInstallerSha256; preservedFiles = $preserved.Count
    }
} finally {
    foreach ($lock in $locks) { $lock.Dispose() }
}
