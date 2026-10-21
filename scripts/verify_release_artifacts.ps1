#requires -Version 7.0
<# Inspect final release bytes without signing, installing, or launching them.
   Existing evidence is replaced only after every artifact passes verification. #>
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$AssetsDirectory,
    [Parameter(Mandatory)][string]$SourceCommit,
    [Parameter(Mandatory)][string]$Tag,
    [Parameter(Mandatory)][ValidateSet('stable', 'beta', 'candidate')][string]$Channel,
    [Parameter(Mandatory)][string]$WindowsSigned,
    [string]$CertificateThumbprint = $env:WINDOWS_CERTIFICATE_THUMBPRINT,
    [string]$ReportPath
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Assert-ReleaseRegularPath([string]$Path, [switch]$AllowMissingLeaf) {
    $absolute = [IO.Path]::GetFullPath($Path)
    $cursor = $absolute
    $leaf = $true
    while ($cursor) {
        if (Test-Path -LiteralPath $cursor) {
            $item = Get-Item -LiteralPath $cursor -Force
            if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) {
                throw "Release paths cannot traverse reparse points: $cursor"
            }
            if (-not $leaf -and -not $item.PSIsContainer) { throw 'Release path parents must be directories.' }
        } elseif (-not ($leaf -and $AllowMissingLeaf)) {
            throw "Release path does not exist: $cursor"
        }
        $leaf = $false
        $cursor = [IO.Path]::GetDirectoryName($cursor)
    }
    return $absolute
}

if ($env:OS -ne 'Windows_NT') { throw 'Release Authenticode verification requires Windows.' }
if ($SourceCommit -cnotmatch '^[0-9a-fA-F]{40}$') { throw 'SourceCommit must be a complete 40-character Git commit SHA.' }
if ($WindowsSigned -cnotin @('true', 'false')) { throw 'WindowsSigned must be exactly true or false.' }
$Channel = $Channel.ToLowerInvariant()
$stableTag = $Tag -cmatch '^v[0-9]+\.[0-9]+\.[0-9]+$'
$betaTag = $Tag -cmatch '^v[0-9]+\.[0-9]+\.[0-9]+-beta\.[0-9]+$'
if (-not ($stableTag -or $betaTag) -or ($Channel -eq 'stable' -and -not $stableTag) -or
    ($Channel -eq 'beta' -and -not $betaTag)) {
    throw 'Tag must be a numeric stable or beta release tag matching Channel; candidates may use either form.'
}
$signed = $WindowsSigned -ceq 'true'
if ($Channel -eq 'stable' -and -not $signed) { throw 'Stable release artifacts must have verified Windows signatures.' }

$assets = Assert-ReleaseRegularPath $AssetsDirectory
if (-not (Get-Item -LiteralPath $assets).PSIsContainer) { throw 'AssetsDirectory must be a directory.' }
if ([string]::IsNullOrWhiteSpace($ReportPath)) { $ReportPath = Join-Path $assets 'windows-signature-evidence.json' }
$report = Assert-ReleaseRegularPath $ReportPath -AllowMissingLeaf
if ([IO.Path]::GetExtension($report) -ine '.json' -or
    ((Test-Path -LiteralPath $report) -and (Get-Item -LiteralPath $report).PSIsContainer)) {
    throw 'ReportPath must be a regular JSON file in an existing directory.'
}

$canonical = @('Abigail-windows-x64-setup.exe', 'Abigail-windows-x64.msi')
$entries = @(Get-ChildItem -LiteralPath $assets -Force)
foreach ($entry in $entries) {
    if ($entry.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Release assets cannot contain reparse points.' }
    if ($entry.PSIsContainer) { throw 'Release assets must be a flat directory of regular files.' }
    if ($entry.Extension -iin @('.exe', '.msi') -and $entry.Name -cnotin $canonical) {
        throw "Unexpected release installer name: $($entry.Name)"
    }
}
$files = @($entries | Where-Object { $_.Name -cin $canonical } | Sort-Object Name)
if (@($files | Where-Object Name -CEQ $canonical[0]).Count -ne 1) {
    throw 'Exactly one canonical Abigail-windows-x64-setup.exe is required.'
}
if (@($files | Where-Object Name -CEQ $canonical[1]).Count -gt 1) { throw 'At most one canonical MSI is allowed.' }

$thumbprint = $null
if ($signed) {
    Import-Module (Join-Path $PSScriptRoot 'windows-signing.psm1') -Force
    . (Join-Path $PSScriptRoot 'windows_signing_common.ps1')
    $thumbprint = ConvertTo-AbigailThumbprint $CertificateThumbprint
}
$locks = [Collections.Generic.List[IO.FileStream]]::new()
try {
    # Keep all verified bytes immutable until their evidence is committed.
    foreach ($file in $files) {
        $null = Assert-ReleaseRegularPath $file.FullName
        $locks.Add([IO.File]::Open($file.FullName, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read))
    }
    $records = @(foreach ($file in $files) {
        if (-not $signed) {
            $signature = Get-AuthenticodeSignature -LiteralPath $file.FullName
            if ($signature.Status -ne 'NotSigned' -or $signature.SignerCertificate -or $signature.TimeStamperCertificate) {
                throw "Unsigned policy requires an actually unsigned artifact: $($file.Name) ($($signature.Status))."
            }
            [ordered]@{
                name = $file.Name; sha256 = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
                signatureStatus = 'NotSigned'; publisher = $null; certificateThumbprint = $null; timestampType = $null
            }
        } elseif ($file.Extension -ieq '.exe') {
            $verified = Assert-AbigailSignature -Path $file.FullName -Thumbprint $thumbprint
            [ordered]@{
                name = $file.Name; sha256 = $verified.sha256.ToLowerInvariant(); signatureStatus = 'Valid'
                publisher = $verified.publisher; certificateThumbprint = $verified.certificateThumbprint
                certificateExpiresUtc = $verified.certificateExpiresUtc; timestampType = 'RFC3161'
                timestampPublisher = $verified.timestampPublisher; timestampCertificateThumbprint = $verified.timestampCertificateThumbprint
                digest = $verified.digest; timestampDigest = 'SHA256'
            }
        } else {
            $verified = Assert-WindowsArtifactSignature -Path $file.FullName -Thumbprint $thumbprint -SignToolPath (Resolve-WindowsSignToolPath)
            $signature = Get-AuthenticodeSignature -LiteralPath $file.FullName
            if ($signature.SignerCertificate.Subject -eq $signature.SignerCertificate.Issuer -or
                $signature.SignerCertificate.EnhancedKeyUsageList.ObjectId -notcontains '1.3.6.1.5.5.7.3.3') {
                throw 'MSI publisher must be a non-self-signed Code Signing certificate.'
            }
            [ordered]@{
                name = $file.Name; sha256 = $verified.Sha256.ToLowerInvariant(); signatureStatus = 'Valid'
                publisher = $verified.Publisher; certificateThumbprint = $verified.Thumbprint
                timestampType = 'Authenticode'; timestampPublisher = $verified.TimestampAuthority
            }
        }
    })
    $evidence = [ordered]@{
        schemaVersion = 1; sourceCommit = $SourceCommit.ToLowerInvariant(); tag = $Tag; channel = $Channel
        windowsSigned = $signed; expectedThumbprint = $thumbprint; artifacts = $records
    }
    $null = Assert-ReleaseRegularPath $report -AllowMissingLeaf
    $json = ($evidence | ConvertTo-Json -Depth 8) + "`n"
    [IO.File]::WriteAllText($report, $json, [Text.UTF8Encoding]::new($false))
    Write-Host "Verified $($records.Count) final release artifact(s): $report"
} finally {
    foreach ($lock in $locks) { $lock.Dispose() }
}
