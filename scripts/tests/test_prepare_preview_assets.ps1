#requires -Version 7.0
<# Offline staging/policy tests only. The real helper and Node provenance code
   run in a temporary harness; ONLY its adjacent native verifier is replaced.
   Every binary and signature report is synthetic. These tests do not establish
   real Authenticode validity, compatibility, signing, installation, or release
   readiness. No network, credentials, or installer execution occurs. #>
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\', '/')
$workspace = Join-Path $tempRoot ('abigail-preview-stage-tests-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $workspace | Out-Null
$harness = Join-Path $workspace 'harness'
New-Item -ItemType Directory -Path $harness | Out-Null
foreach ($name in @('prepare_preview_assets.ps1', 'release_provenance.mjs')) {
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot "../$name") -Destination (Join-Path $harness $name)
}
$prepare = Join-Path $harness 'prepare_preview_assets.ps1'
$provenance = Join-Path $harness 'release_provenance.mjs'
$sourceCommit = 'a' * 40
$thumbprint = 'B' * 40
$tag = 'v1.2.3-beta.4'
$installerName = 'Abigail-windows-x64-setup.exe'
$script:passed = 0
$script:skipped = 0
$previewNativeTestFault = ''
$stub = @'
param($AssetsDirectory, $SourceCommit, $Tag, $Channel, $WindowsSigned, $CertificateThumbprint)
if ($Channel -cne 'beta' -or $WindowsSigned -cne 'true') { throw 'Mock verifier received an unexpected policy.' }
if ($previewNativeTestFault -eq 'native-failure') { throw 'Synthetic native verification failure.' }
if ($previewNativeTestFault -eq 'mutate-copy') {
    [IO.File]::AppendAllText((Join-Path $AssetsDirectory 'Abigail-windows-x64-setup.exe'), 'changed')
}
if ($previewNativeTestFault -eq 'extra-output') {
    [IO.File]::WriteAllText((Join-Path $AssetsDirectory 'unapproved.json'), '{}')
}
$records = @(Get-ChildItem -LiteralPath $AssetsDirectory -File | Where-Object Extension -In @('.exe', '.msi') | ForEach-Object {
    @{
        name = $_.Name; sha256 = (Get-FileHash -LiteralPath $_.FullName).Hash.ToLowerInvariant()
        signatureStatus = 'Valid'; publisher = 'Synthetic fixture only'
        certificateThumbprint = $CertificateThumbprint.ToUpperInvariant()
        timestampType = if ($_.Extension -eq '.exe') { 'RFC3161' } else { 'Authenticode' }
    }
})
if ($previewNativeTestFault -eq 'bad-evidence') { $records[0].certificateThumbprint = 'D' * 40 }
$report = @{
    schemaVersion = 1; sourceCommit = $SourceCommit.ToLowerInvariant(); tag = $Tag
    channel = $Channel; windowsSigned = $true; expectedThumbprint = $CertificateThumbprint.ToUpperInvariant()
    artifacts = $records
}
[IO.File]::WriteAllText((Join-Path $AssetsDirectory 'windows-signature-evidence.json'), ($report | ConvertTo-Json -Depth 10))
'@
[IO.File]::WriteAllText((Join-Path $harness 'verify_release_artifacts.ps1'), $stub)

function Pass([string]$Name) { $script:passed++; Write-Host "PASS $Name" }
function Snapshot([string]$Directory) {
    return (@(Get-ChildItem -LiteralPath $Directory -File | Sort-Object Name | ForEach-Object {
        "$($_.Name):$($_.Length):$((Get-FileHash -LiteralPath $_.FullName).Hash)"
    }) -join "`n")
}
function New-Fixture([string]$Name, [string]$Channel = 'candidate', [string]$FixtureTag = $tag, [bool]$Signed = $true) {
    $directory = Join-Path $workspace $Name
    New-Item -ItemType Directory -Path $directory | Out-Null
    foreach ($name in @($installerName, 'Abigail-windows-x64.msi', 'windows-signatures.json', 'Abigail-updater.nsis.zip', 'Abigail-updater.nsis.zip.sig', 'latest.json')) {
        [IO.File]::WriteAllText((Join-Path $directory $name), "Synthetic bytes: $name")
    }
    $records = @(foreach ($name in @($installerName, 'Abigail-windows-x64.msi')) {
        @{
            name = $name; sha256 = (Get-FileHash -LiteralPath (Join-Path $directory $name)).Hash.ToLowerInvariant()
            signatureStatus = if ($Signed) { 'Valid' } else { 'NotSigned' }
            publisher = if ($Signed) { 'Synthetic fixture only' } else { $null }
            certificateThumbprint = if ($Signed) { $thumbprint } else { $null }
            timestampType = if (-not $Signed) { $null } elseif ($name.EndsWith('.exe')) { 'RFC3161' } else { 'Authenticode' }
        }
    })
    $report = @{
        schemaVersion = 1; sourceCommit = $sourceCommit; tag = $FixtureTag; channel = $Channel
        windowsSigned = $Signed; expectedThumbprint = if ($Signed) { $thumbprint } else { $null }; artifacts = $records
    }
    [IO.File]::WriteAllText((Join-Path $directory 'windows-signature-evidence.json'), ($report | ConvertTo-Json -Depth 10))
    $arguments = @('create', '--assets-dir', $directory, '--source-commit', $sourceCommit, '--tag', $FixtureTag, '--channel', $Channel,
        '--windows-signed', $Signed.ToString().ToLowerInvariant())
    if ($Signed) { $arguments += @('--expected-thumbprint', $thumbprint) }
    & node $provenance @arguments | Out-Host
    if ($LASTEXITCODE -ne 0) { throw 'Synthetic fixture provenance creation failed.' }
    return $directory
}
function Invoke-Prepare([string]$Directory, [hashtable]$Overrides = @{}) {
    $parameters = @{
        CandidateAssetsDirectory = $Directory; SourceCommit = $sourceCommit; Tag = $tag
        CertificateThumbprint = $thumbprint; OutputDirectory = $Directory + '-output'
    }
    foreach ($name in $Overrides.Keys) { $parameters[$name] = $Overrides[$name] }
    & $prepare @parameters
}
function Assert-Rejected([string]$Name, [string]$Directory, [hashtable]$Overrides = @{}, [string]$Pattern = 'Preview provenance validation failed') {
    $before = Snapshot $Directory
    $rejected = $false
    try { Invoke-Prepare $Directory $Overrides | Out-Null }
    catch {
        $rejected = $true
        if ($_.Exception.Message -notmatch $Pattern) { throw "$Name raised an unexpected error: $($_.Exception.Message)" }
    }
    if (-not $rejected) { throw "$Name was incorrectly accepted." }
    if ((Snapshot $Directory) -cne $before) { throw "$Name changed the candidate source." }
    Pass $Name
}

try {
    $candidate = New-Fixture 'happy'
    $before = Snapshot $candidate
    $result = Invoke-Prepare $candidate
    $output = $candidate + '-output'
    if (-not $result.prepared -or $result.published -or $result.preservedFiles -ne 6) { throw 'Incorrect preparation result.' }
    $manifest = Get-Content -LiteralPath (Join-Path $output 'release-provenance.json') -Raw | ConvertFrom-Json
    $evidence = Get-Content -LiteralPath (Join-Path $output 'windows-signature-evidence.json') -Raw | ConvertFrom-Json
    if ($manifest.channel -cne 'beta' -or $evidence.channel -cne 'beta' -or $manifest.tag -cne $tag) { throw 'Beta metadata was not generated.' }
    foreach ($file in @(Get-ChildItem -LiteralPath $candidate -File | Where-Object Name -NotIn @('release-provenance.json', 'windows-signature-evidence.json'))) {
        if ((Get-FileHash -LiteralPath (Join-Path $output $file.Name)).Hash -cne (Get-FileHash -LiteralPath $file.FullName).Hash) {
            throw "Approved bytes changed: $($file.Name)"
        }
    }
    if ((Snapshot $candidate) -cne $before) { throw 'Successful preparation changed its source.' }
    Pass 'same signed beta-tag candidate retains every nonmetadata byte and source file'
    Assert-Rejected 'existing output is never overwritten' $candidate @{} 'OutputDirectory must be new'
    Assert-Rejected 'output cannot equal candidate' $candidate @{ OutputDirectory = $candidate } 'OutputDirectory must be new'
    Assert-Rejected 'output cannot be inside candidate' $candidate @{ OutputDirectory = Join-Path $candidate 'nested' } 'must be separate'
    Assert-Rejected 'missing output parent' $candidate @{ OutputDirectory = Join-Path $workspace 'missing/child' } 'does not exist'
    Assert-Rejected 'wrong exact source' (New-Fixture 'wrong-source') @{ SourceCommit = 'c' * 40 }
    Assert-Rejected 'wrong beta tag' (New-Fixture 'wrong-tag') @{ Tag = 'v1.2.3-beta.5' }
    Assert-Rejected 'wrong publisher pin' (New-Fixture 'wrong-pin') @{ CertificateThumbprint = 'D' * 40 }
    Assert-Rejected 'unsigned candidate' (New-Fixture 'unsigned' -Signed $false)
    Assert-Rejected 'already beta is not a candidate' (New-Fixture 'already-beta' -Channel beta)
    Assert-Rejected 'stable tag input cannot convert' (New-Fixture 'stable-tag' -FixtureTag 'v1.2.3') @{ Tag = 'v1.2.3' } 'stable-tag conversion is forbidden'
    Assert-Rejected 'stable candidate cannot relabel as beta' (New-Fixture 'stable-to-beta' -FixtureTag 'v1.2.3')
    $candidate = New-Fixture 'tampered'
    [IO.File]::AppendAllText((Join-Path $candidate $installerName), 'changed')
    Assert-Rejected 'tampered installer' $candidate
    $candidate = New-Fixture 'extra'
    [IO.File]::WriteAllText((Join-Path $candidate 'extra.json'), '{}')
    Assert-Rejected 'unapproved extra source file' $candidate
    $candidate = New-Fixture 'missing-evidence'
    Remove-Item -LiteralPath (Join-Path $candidate 'windows-signature-evidence.json')
    Assert-Rejected 'missing source evidence' $candidate
    $candidate = New-Fixture 'subdirectory'
    New-Item -ItemType Directory -Path (Join-Path $candidate 'nested') | Out-Null
    Assert-Rejected 'source subdirectory' $candidate @{} 'flat regular'
    foreach ($fault in @('native-failure', 'bad-evidence', 'mutate-copy', 'extra-output')) {
        $candidate = New-Fixture $fault
        $previewNativeTestFault = $fault
        $pattern = switch ($fault) {
            'native-failure' { 'Synthetic native verification failure' }
            'bad-evidence' { 'Preview provenance validation failed' }
            'mutate-copy' { 'changed approved bytes' }
            'extra-output' { 'missing or unapproved files' }
        }
        Assert-Rejected $fault $candidate @{} $pattern
        if (Test-Path -LiteralPath (Join-Path ($candidate + '-output') 'release-provenance.json')) {
            throw 'Failed preparation left a completed beta manifest.'
        }
    }
    $previewNativeTestFault = ''
    $candidate = New-Fixture 'linked'
    $link = Join-Path $workspace 'linked-parent'
    try {
        New-Item -ItemType Junction -Path $link -Target $candidate -ErrorAction Stop | Out-Null
    } catch {
        if ($env:OS -ne 'Windows_NT' -or $_.Exception.Message -match 'privilege|permission|denied|not supported') {
            $script:skipped++
            Write-Host 'SKIP junction fixture unavailable; rerun on Windows with junction access.'
        } else { throw }
    }
    if (Test-Path -LiteralPath $link) {
        try {
            Assert-Rejected 'linked source' $link @{} 'reparse points'
            Assert-Rejected 'linked output parent' $candidate @{ OutputDirectory = Join-Path $link 'new-output' } 'reparse points'
        } finally { Remove-Item -LiteralPath $link -Force }
    }
    Write-Host "Preview preparation: $script:passed checks passed; $script:skipped skipped. Native verification was mocked; no real signing or acceptance is established."
} finally {
    $resolved = [IO.Path]::GetFullPath($workspace)
    if ([IO.Path]::GetDirectoryName($resolved) -ne $tempRoot -or
        [IO.Path]::GetFileName($resolved) -notmatch '^abigail-preview-stage-tests-[0-9a-f]{32}$') {
        throw 'Refusing to remove an unexpected test workspace.'
    }
    if (Test-Path -LiteralPath $resolved) { Remove-Item -LiteralPath $resolved -Recurse -Force }
}
