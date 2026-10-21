# Verify distribution, not just the build job. Never launches or signs downloads.
#requires -Version 7.0
param(
    [Parameter(Mandatory)][string]$Repository,
    [Parameter(Mandatory)][string]$Tag,
    [Parameter(Mandatory)][string]$SourceCommit,
    [Parameter(Mandatory)][ValidateSet('stable', 'beta')][string]$Channel,
    [Parameter(Mandatory)][ValidateSet('true', 'false')][string]$WindowsSigned,
    [string]$CertificateThumbprint = $env:WINDOWS_CERTIFICATE_THUMBPRINT,
    [string]$ExpectedAssetsDirectory = 'release-assets',
    [string]$OutputDirectory = 'published-verification'
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if ($Repository -notmatch '^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$' -or $Tag -notmatch '^v\d+\.\d+\.\d+(?:-beta\.\d+)?$' -or $SourceCommit -notmatch '^[a-fA-F0-9]{40}$') { throw 'Invalid release identity.' }
if (Test-Path -LiteralPath $OutputDirectory) { throw 'Published verification directory must be new.' }
New-Item -ItemType Directory -Path $OutputDirectory | Out-Null
$result = [ordered]@{ verified = $false; repository = $Repository; tag = $Tag; sourceCommit = $SourceCommit; channel = $Channel }
$nodeScript = Join-Path $PSScriptRoot 'release_provenance.mjs'
$expectedManifest = Join-Path $ExpectedAssetsDirectory 'release-provenance.json'
$policy = @('--source-commit', $SourceCommit, '--tag', $Tag, '--channel', $Channel, '--windows-signed', $WindowsSigned)
if ($WindowsSigned -eq 'true') { $policy += @('--expected-thumbprint', $CertificateThumbprint) }
function Invoke-Provenance([string[]]$Arguments) {
    & node $nodeScript @Arguments
    if ($LASTEXITCODE -ne 0) { throw 'Release provenance or channel verification failed.' }
}
function Save-GitHubMetadata([string]$Endpoint, [string]$Path) {
    $json = & gh api $Endpoint
    if ($LASTEXITCODE -ne 0) { throw "Could not read release metadata: $Endpoint" }
    [IO.File]::WriteAllText([IO.Path]::GetFullPath($Path), ($json -join "`n"), [Text.UTF8Encoding]::new($false))
}
try {
    Invoke-Provenance (@('verify', '--assets-dir', $ExpectedAssetsDirectory) + $policy)
    $metadata = Join-Path $OutputDirectory 'release.json'
    $latest = Join-Path $OutputDirectory 'latest-release.json'
    Save-GitHubMetadata "repos/$Repository/releases/tags/$Tag" $metadata
    Save-GitHubMetadata "repos/$Repository/releases/latest" $latest
    Invoke-Provenance @('verify-metadata', '--manifest', $expectedManifest, '--release', $metadata, '--latest-release', $latest, '--source-commit', $SourceCommit)

    # Resolve the tag itself; target_commitish alone is not source provenance.
    $refs = @(& git ls-remote "https://github.com/$Repository.git" "refs/tags/$Tag" "refs/tags/$Tag^{}")
    if ($LASTEXITCODE -ne 0 -or -not $refs.Count) { throw 'Could not resolve published release tag.' }
    $peeled = @($refs | Where-Object { $_ -match '\^\{\}$' })
    $resolved = if ($peeled.Count -eq 1) { ($peeled[0] -split '\s+')[0] } elseif ($refs.Count -eq 1) { ($refs[0] -split '\s+')[0] } else { throw 'Ambiguous published tag.' }
    if ($resolved -ne $SourceCommit) { throw 'Published tag does not identify the verified source commit.' }

    $manifest = Get-Content -LiteralPath $expectedManifest -Raw | ConvertFrom-Json
    $downloadDir = Join-Path $OutputDirectory 'downloaded'
    New-Item -ItemType Directory -Path $downloadDir | Out-Null
    $names = @($manifest.assets.name) + @('release-provenance.json')
    foreach ($name in $names) {
        $url = "https://github.com/$Repository/releases/download/$([Uri]::EscapeDataString($Tag))/$([Uri]::EscapeDataString($name))"
        Invoke-WebRequest -Uri $url -OutFile (Join-Path $downloadDir $name) -TimeoutSec 180 -MaximumRetryCount 2 -RetryIntervalSec 3
    }
    if ((Get-FileHash -LiteralPath (Join-Path $downloadDir 'release-provenance.json')).Hash -ne (Get-FileHash -LiteralPath $expectedManifest).Hash) { throw 'Published provenance bytes differ from the approved build artifact.' }
    Invoke-Provenance (@('verify', '--assets-dir', $downloadDir) + $policy)
    & (Join-Path $PSScriptRoot 'verify_release_artifacts.ps1') -AssetsDirectory $downloadDir -SourceCommit $SourceCommit -Tag $Tag -Channel $Channel -WindowsSigned $WindowsSigned -CertificateThumbprint $CertificateThumbprint -ReportPath (Join-Path $OutputDirectory 'downloaded-signatures.json')

    $installerName = 'Abigail-windows-x64-setup.exe'
    $installerHash = (Get-FileHash -LiteralPath (Join-Path $downloadDir $installerName)).Hash
    $downloadUrl = "https://github.com/$Repository/releases/download/$Tag/$installerName"
    if ($Channel -eq 'stable') {
        # This is the exact route used by docs/index.html, not the tag API URL.
        $downloadUrl = "https://github.com/$Repository/releases/latest/download/$installerName"
        $defaultDownload = Join-Path $OutputDirectory 'default-download.exe'
        Invoke-WebRequest -Uri $downloadUrl -OutFile $defaultDownload -TimeoutSec 180 -MaximumRetryCount 2 -RetryIntervalSec 3
        if ((Get-FileHash -LiteralPath $defaultDownload).Hash -ne $installerHash) { throw 'The public stable download serves different bytes from the verified release.' }
    }
    $result.verified = $true
    $result['downloadUrl'] = $downloadUrl
    $result['installerSha256'] = $installerHash
    $result['verifiedUtc'] = [DateTime]::UtcNow.ToString('o')
    Write-Host "Published $Channel download verified: $downloadUrl ($installerHash)"
} catch {
    $result['error'] = $_.Exception.Message
    throw
} finally {
    [IO.File]::WriteAllText([IO.Path]::GetFullPath((Join-Path $OutputDirectory 'result.json')), ($result | ConvertTo-Json -Depth 5), [Text.UTF8Encoding]::new($false))
}
