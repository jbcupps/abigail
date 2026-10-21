#requires -Version 7.0
<# Integration tests for publication verification. A copy of an existing trusted
   executable is data only: no installer is launched or signed. GitHub API, Git
   and download operations are mocked; native signature checks use normal host
   trust. No credentials, certificates, trust settings or releases are changed. #>
[CmdletBinding()]
param([string]$TrustedProbe = (Get-Command node -ErrorAction Stop).Source)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$nativeExitVariable = Get-Variable -Name LASTEXITCODE -Scope Global -ErrorAction SilentlyContinue
$hadNativeExitCode = $null -ne $nativeExitVariable
$savedNativeExitCode = if ($hadNativeExitCode) { $nativeExitVariable.Value } else { $null }
if ($env:OS -ne 'Windows_NT') { throw 'Published release integration tests require Windows.' }
$verifier = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../verify_published_release.ps1'))
$artifactVerifier = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../verify_release_artifacts.ps1'))
$provenance = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../release_provenance.mjs'))
$signature = Get-AuthenticodeSignature -LiteralPath $TrustedProbe
if ($signature.Status -ne 'Valid' -or -not $signature.SignerCertificate) {
    throw 'TrustedProbe must already have a trusted SHA256/RFC3161 Authenticode signature.'
}
$thumbprint = $signature.SignerCertificate.Thumbprint
$source = 'a' * 40
$repository = 'example-owner/example-repository'
$installerName = 'Abigail-windows-x64-setup.exe'
$tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\', '/')
$workspace = Join-Path $tempRoot ('abigail-published-release-tests-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $workspace | Out-Null
$passed = 0

function Assert-Equal($Actual, $Expected, [string]$Description) {
    if ($Actual -cne $Expected) { throw "$Description (expected '$Expected', got '$Actual')." }
}

function New-PublicationFixture([string]$Channel, [string]$Tag) {
    $assets = Join-Path $workspace "$Channel-assets"
    New-Item -ItemType Directory -Path $assets | Out-Null
    Copy-Item -LiteralPath $TrustedProbe -Destination (Join-Path $assets $installerName)
    # Exercise the actual Windows verifier and actual Node provenance writer.
    & $artifactVerifier -AssetsDirectory $assets -SourceCommit $source -Tag $Tag -Channel $Channel -WindowsSigned true -CertificateThumbprint $thumbprint
    & node $provenance create --assets-dir $assets --source-commit $source --tag $Tag --channel $Channel --windows-signed true --expected-thumbprint $thumbprint | Out-Host
    if ($LASTEXITCODE -ne 0) { throw 'Could not create the integration fixture provenance.' }
    $manifest = Get-Content -LiteralPath (Join-Path $assets 'release-provenance.json') -Raw | ConvertFrom-Json
    $names = @($manifest.assets.name) + @('release-provenance.json')
    $metadataAssets = @(foreach ($name in $names) {
        $file = Get-Item -LiteralPath (Join-Path $assets $name)
        [ordered]@{
            name = $name; state = 'uploaded'; size = $file.Length
            digest = 'sha256:' + (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
        }
    })
    $release = [ordered]@{
        tag_name = $Tag; target_commitish = $source; draft = $false
        prerelease = ($Channel -eq 'beta'); assets = $metadataAssets
    }
    $latest = if ($Channel -eq 'stable') { $release } else {
        [ordered]@{ tag_name = 'v1.2.2'; target_commitish = ('c' * 40); draft = $false; prerelease = $false; assets = @() }
    }
    return @{
        assets = $assets; manifest = $manifest; names = $names; tag = $Tag; channel = $Channel
        releaseJson = ($release | ConvertTo-Json -Depth 12 -Compress)
        latestJson = ($latest | ConvertTo-Json -Depth 12 -Compress)
        originalInstallerHash = (Get-FileHash -LiteralPath (Join-Path $assets $installerName) -Algorithm SHA256).Hash
        originalManifestHash = (Get-FileHash -LiteralPath (Join-Path $assets 'release-provenance.json') -Algorithm SHA256).Hash
    }
}

function Invoke-PublicationCase([string]$Name, [hashtable]$Fixture, [string]$Fault = '', [string]$ExpectedError = '') {
    $output = Join-Path $workspace $Name
    $tagPrefix = "https://github.com/$repository/releases/download/$($Fixture.tag)/"
    $defaultUrl = "https://github.com/$repository/releases/latest/download/$installerName"
    $knownUrls = @{}
    foreach ($assetName in $Fixture.names) { $knownUrls[$tagPrefix + $assetName] = Join-Path $Fixture.assets $assetName }
    $knownUrls[$defaultUrl] = Join-Path $Fixture.assets $installerName
    # Caller-scope functions are inherited by the invoked verifier script. They
    # fail on unknown operations instead of falling through to real executables
    # or network cmdlets. The mutable hashtable also records all requested URLs.
    $publishedReleaseTestState = @{
        fixture = $Fixture; output = $output; fault = $Fault; source = $source
        repository = $repository; knownUrls = $knownUrls; defaultUrl = $defaultUrl
        tagInstallerUrl = $tagPrefix + $installerName
        manifestUrl = $tagPrefix + 'release-provenance.json'
        requests = [Collections.Generic.List[string]]::new()
        apiRequests = [Collections.Generic.List[string]]::new()
        gitRequests = [Collections.Generic.List[string]]::new()
    }

    function gh {
        param([Parameter(ValueFromRemainingArguments)][string[]]$CommandArguments)
        if ($CommandArguments.Count -ne 2 -or $CommandArguments[0] -cne 'api') { throw 'Mock gh refused an unexpected command.' }
        $endpoint = $CommandArguments[1]
        $publishedReleaseTestState.apiRequests.Add($endpoint)
        $global:LASTEXITCODE = 0
        if ($endpoint -ceq "repos/$($publishedReleaseTestState.repository)/releases/tags/$($publishedReleaseTestState.fixture.tag)") {
            return $publishedReleaseTestState.fixture.releaseJson
        }
        if ($endpoint -ceq "repos/$($publishedReleaseTestState.repository)/releases/latest") {
            return $publishedReleaseTestState.fixture.latestJson
        }
        throw "Mock gh refused an unexpected endpoint: $endpoint"
    }

    function git {
        param([Parameter(ValueFromRemainingArguments)][string[]]$CommandArguments)
        $tag = $publishedReleaseTestState.fixture.tag
        $expected = @('ls-remote', "https://github.com/$($publishedReleaseTestState.repository).git", "refs/tags/$tag", "refs/tags/$tag^{}")
        if (($CommandArguments -join '|') -cne ($expected -join '|')) { throw 'Mock git refused an unexpected command.' }
        $publishedReleaseTestState.gitRequests.Add($CommandArguments -join ' ')
        $global:LASTEXITCODE = 0
        $commit = if ($publishedReleaseTestState.fault -eq 'wrong-tag-sha') { 'b' * 40 } else { $publishedReleaseTestState.source }
        return "$commit`trefs/tags/$tag"
    }

    function Invoke-WebRequest {
        [CmdletBinding()]
        param(
            [Parameter(Mandatory)][string]$Uri,
            [Parameter(Mandatory)][string]$OutFile,
            [int]$TimeoutSec,
            [int]$MaximumRetryCount,
            [int]$RetryIntervalSec
        )
        if (-not $publishedReleaseTestState.knownUrls.ContainsKey($Uri)) { throw "Mock download refused an unexpected URL: $Uri" }
        $destination = [IO.Path]::GetFullPath($OutFile)
        $allowedRoot = [IO.Path]::GetFullPath($publishedReleaseTestState.output).TrimEnd('\', '/') + [IO.Path]::DirectorySeparatorChar
        if (-not $destination.StartsWith($allowedRoot, [StringComparison]::OrdinalIgnoreCase)) { throw 'Mock download destination escaped the test case.' }
        $publishedReleaseTestState.requests.Add($Uri)
        Copy-Item -LiteralPath $publishedReleaseTestState.knownUrls[$Uri] -Destination $destination
        $changeInstaller = ($publishedReleaseTestState.fault -eq 'altered-installer' -and $Uri -ceq $publishedReleaseTestState.tagInstallerUrl) -or
            ($publishedReleaseTestState.fault -eq 'substituted-default' -and $Uri -ceq $publishedReleaseTestState.defaultUrl)
        if ($changeInstaller) {
            # Corrupt only an owned download copy, preserving its size. Never run it.
            $stream = [IO.File]::Open($destination, [IO.FileMode]::Open, [IO.FileAccess]::ReadWrite)
            try {
                $stream.Position = 64
                $value = $stream.ReadByte()
                $stream.Position = 64
                $stream.WriteByte([byte]($value -bxor 1))
            } finally { $stream.Dispose() }
        }
        if ($publishedReleaseTestState.fault -eq 'replaced-manifest' -and $Uri -ceq $publishedReleaseTestState.manifestUrl) {
            # Semantically equivalent JSON must still fail the exact-byte check.
            [IO.File]::AppendAllText($destination, " `n", [Text.UTF8Encoding]::new($false))
        }
    }

    $failure = $null
    try {
        & $verifier -Repository $repository -Tag $Fixture.tag -SourceCommit $source -Channel $Fixture.channel -WindowsSigned true -CertificateThumbprint $thumbprint -ExpectedAssetsDirectory $Fixture.assets -OutputDirectory $output
    } catch { $failure = $_.Exception.Message }
    $resultPath = Join-Path $output 'result.json'
    if (-not (Test-Path -LiteralPath $resultPath -PathType Leaf)) { throw "$Name did not write result.json. Failure: $failure" }
    $result = Get-Content -LiteralPath $resultPath -Raw | ConvertFrom-Json
    Assert-Equal $result.repository $repository "$Name repository"
    Assert-Equal $result.tag $Fixture.tag "$Name tag"
    Assert-Equal $result.sourceCommit $source "$Name source commit"
    Assert-Equal $result.channel $Fixture.channel "$Name channel"
    Assert-Equal $publishedReleaseTestState.apiRequests.Count 2 "$Name uses release and latest metadata mocks"
    Assert-Equal $publishedReleaseTestState.gitRequests.Count 1 "$Name resolves the actual tag through the mock"
    if ($ExpectedError) {
        if (-not $failure -or $failure -notmatch $ExpectedError) { throw "$Name did not fail as expected ($ExpectedError): $failure" }
        Assert-Equal $result.verified $false "$Name records failure"
        if (-not $result.error -or $result.error -notmatch $ExpectedError) { throw "$Name did not retain its failure reason." }
        if ($result.PSObject.Properties.Name -contains 'verifiedUtc') { throw "$Name incorrectly recorded a successful verification time." }
    } else {
        if ($failure) { throw "$Name unexpectedly failed: $failure" }
        Assert-Equal $result.verified $true "$Name records success"
        if ($result.PSObject.Properties.Name -contains 'error') { throw "$Name recorded an error on success." }
        Assert-Equal $result.installerSha256 $Fixture.originalInstallerHash "$Name verified the expected installer bytes"
        $expectedUrl = if ($Fixture.channel -eq 'stable') { $defaultUrl } else { $tagPrefix + $installerName }
        Assert-Equal $result.downloadUrl $expectedUrl "$Name records the actual public route"
        foreach ($assetName in $Fixture.names) {
            Assert-Equal @($publishedReleaseTestState.requests | Where-Object { $_ -ceq ($tagPrefix + $assetName) }).Count 1 "$Name downloads $assetName exactly once"
        }
    }
    if ($Fixture.channel -eq 'beta') {
        Assert-Equal @($publishedReleaseTestState.requests | Where-Object { $_ -like '*/releases/latest/download/*' }).Count 0 "$Name must not fetch a beta from the stable default route"
    } elseif (-not $ExpectedError -or $Fault -eq 'substituted-default') {
        Assert-Equal @($publishedReleaseTestState.requests | Where-Object { $_ -ceq $defaultUrl }).Count 1 "$Name checks the exact public stable URL"
    } else {
        Assert-Equal @($publishedReleaseTestState.requests | Where-Object { $_ -ceq $defaultUrl }).Count 0 "$Name stops before the default download after an earlier failure"
    }
    if ($Fault -eq 'wrong-tag-sha') { Assert-Equal $publishedReleaseTestState.requests.Count 0 "$Name rejects the tag before downloading" }
    Assert-Equal (Get-FileHash -LiteralPath (Join-Path $Fixture.assets $installerName) -Algorithm SHA256).Hash $Fixture.originalInstallerHash "$Name preserves the approved installer"
    Assert-Equal (Get-FileHash -LiteralPath (Join-Path $Fixture.assets 'release-provenance.json') -Algorithm SHA256).Hash $Fixture.originalManifestHash "$Name preserves approved provenance"
    Write-Host "PASS $Name"
}

try {
    $stable = New-PublicationFixture -Channel stable -Tag 'v1.2.3'
    Invoke-PublicationCase -Name 'stable-success' -Fixture $stable
    $passed++
    Invoke-PublicationCase -Name 'altered-installer' -Fixture $stable -Fault altered-installer -ExpectedError 'Release provenance or channel verification failed'
    $passed++
    Invoke-PublicationCase -Name 'wrong-published-tag-sha' -Fixture $stable -Fault wrong-tag-sha -ExpectedError 'Published tag does not identify'
    $passed++
    Invoke-PublicationCase -Name 'substituted-default-download' -Fixture $stable -Fault substituted-default -ExpectedError 'public stable download serves different bytes'
    $passed++
    Invoke-PublicationCase -Name 'replaced-manifest-bytes' -Fixture $stable -Fault replaced-manifest -ExpectedError 'Published provenance bytes differ'
    $passed++
    $beta = New-PublicationFixture -Channel beta -Tag 'v1.2.3-beta.1'
    Invoke-PublicationCase -Name 'beta-explicit-route' -Fixture $beta
    $passed++
    Write-Host "Published release verification: $passed integration cases passed. Downloads and GitHub/Git are mocked; installers were not executed or signed."
} finally {
    $resolvedWorkspace = [IO.Path]::GetFullPath($workspace)
    if ([IO.Path]::GetDirectoryName($resolvedWorkspace) -ne $tempRoot -or
        [IO.Path]::GetFileName($resolvedWorkspace) -notmatch '^abigail-published-release-tests-[0-9a-f]{32}$') {
        throw 'Refusing to clean an unexpected test directory.'
    }
    if (Test-Path -LiteralPath $resolvedWorkspace) { Remove-Item -LiteralPath $resolvedWorkspace -Recurse -Force }
    if ($hadNativeExitCode) { $global:LASTEXITCODE = $savedNativeExitCode }
    else { Remove-Variable -Name LASTEXITCODE -Scope Global -ErrorAction SilentlyContinue }
}
