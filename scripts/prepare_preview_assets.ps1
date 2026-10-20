#requires -Version 7.0
<# Prepare a NEW beta asset directory from an existing signed candidate.
   Example:
   ./scripts/prepare_preview_assets.ps1 -CandidateAssetsDirectory candidate-assets `
     -SourceCommit <40-hex SHA> -Tag v0.0.77-beta.123 `
     -CertificateThumbprint <40-hex thumbprint> -OutputDirectory beta-assets

   The tag must already be the candidate's exact beta tag. Only final provenance
   and signature evidence are regenerated. No build, signing, installer launch,
   credentials, network requests, tags, or publication occur. This is byte
   preparation, not compatibility acceptance or authorization to publish.
   A failed operation may leave an incomplete new output directory for inspection.
   It never overwrites an existing output or modifies the source candidate. #>
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$CandidateAssetsDirectory,
    [Parameter(Mandatory)][string]$SourceCommit,
    [Parameter(Mandatory)][string]$Tag,
    [Parameter(Mandatory)][string]$CertificateThumbprint,
    [Parameter(Mandatory)][string]$OutputDirectory
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Assert-PreviewPath([string]$Path, [switch]$NewDirectory) {
    $absolute = [IO.Path]::GetFullPath($Path)
    if ($absolute.StartsWith('\\') -or $absolute.StartsWith('//')) { throw 'Preview preparation requires local filesystem paths.' }
    $cursor = $absolute
    $leaf = $true
    while ($cursor) {
        if (Test-Path -LiteralPath $cursor) {
            $item = Get-Item -LiteralPath $cursor -Force
            if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw "Preview paths cannot traverse reparse points: $cursor" }
            if (-not $item.PSIsContainer) { throw "Preview directory path is not a directory: $cursor" }
            if ($leaf -and $NewDirectory) { throw 'OutputDirectory must be new; existing output is never overwritten.' }
        } elseif (-not ($leaf -and $NewDirectory)) {
            throw "Preview directory parent or source does not exist: $cursor"
        }
        $leaf = $false
        $cursor = [IO.Path]::GetDirectoryName($cursor)
    }
    return $absolute.TrimEnd([IO.Path]::DirectorySeparatorChar, [IO.Path]::AltDirectorySeparatorChar)
}

function Invoke-PreviewProvenance([string[]]$Arguments) {
    & $node $provenance @Arguments | Out-Host
    if ($LASTEXITCODE -ne 0) { throw 'Preview provenance validation failed.' }
}

function Assert-PreservedPreviewAsset($Asset) {
    $file = Get-Item -LiteralPath (Join-Path $output $Asset.name) -Force
    if ($file.PSIsContainer -or ($file.Attributes -band [IO.FileAttributes]::ReparsePoint) -or
        $file.Length -ne $Asset.size -or
        (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash -ine $Asset.sha256) {
        throw "Preview preparation changed approved bytes: $($Asset.name)"
    }
}

function Assert-PreviewOutputFiles([switch]$WithManifest) {
    $expected = @($preserved.name) + @($evidenceName)
    if ($WithManifest) { $expected += $manifestName }
    $actual = @(Get-ChildItem -LiteralPath $output -Force)
    if ($actual.Count -ne $expected.Count) { throw 'Prepared output contains missing or unapproved files.' }
    foreach ($file in $actual) {
        if ($file.PSIsContainer -or ($file.Attributes -band [IO.FileAttributes]::ReparsePoint) -or $file.Name -cnotin $expected) {
            throw 'Prepared output contains missing or unapproved files.'
        }
    }
}

if ($SourceCommit -cnotmatch '^[a-fA-F0-9]{40}$') { throw 'SourceCommit must be an exact 40-hex commit SHA.' }
if ($CertificateThumbprint -cnotmatch '^[a-fA-F0-9]{40}$') { throw 'CertificateThumbprint must be an exact 40-hex publisher thumbprint.' }
if ($Tag -cnotmatch '^v(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)-beta\.(?:0|[1-9]\d*)$') {
    throw 'Tag must already be the candidate beta tag; stable-tag conversion is forbidden.'
}
$source = Assert-PreviewPath $CandidateAssetsDirectory
$output = Assert-PreviewPath $OutputDirectory -NewDirectory
$comparison = if ([OperatingSystem]::IsWindows()) { [StringComparison]::OrdinalIgnoreCase } else { [StringComparison]::Ordinal }
if ($output.Equals($source, $comparison) -or
    $output.StartsWith($source + [IO.Path]::DirectorySeparatorChar, $comparison)) {
    throw 'OutputDirectory must be separate from the candidate directory.'
}
$node = (Get-Command node -CommandType Application -ErrorAction Stop | Select-Object -First 1).Source
$provenance = Join-Path $PSScriptRoot 'release_provenance.mjs'
$nativeVerifier = Join-Path $PSScriptRoot 'verify_release_artifacts.ps1'
$manifestName = 'release-provenance.json'
$evidenceName = 'windows-signature-evidence.json'
$manifestPath = Join-Path $source $manifestName
$locks = [Collections.Generic.List[IO.FileStream]]::new()
try {
    # Lock every source file against modification/deletion while validating and
    # copying. The source itself never receives newly generated evidence.
    foreach ($file in @(Get-ChildItem -LiteralPath $source -Force)) {
        if ($file.PSIsContainer -or ($file.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
            throw 'Candidate assets must be flat regular files without reparse points.'
        }
        $locks.Add([IO.File]::Open($file.FullName, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read))
    }
    $policy = @('--source-commit', $SourceCommit, '--tag', $Tag, '--windows-signed', 'true', '--expected-thumbprint', $CertificateThumbprint)
    Invoke-PreviewProvenance (@('verify', '--assets-dir', $source, '--channel', 'candidate') + $policy)
    $manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
    if ($manifest.verificationReport -cne $evidenceName) { throw 'Candidate must use the canonical Windows signature evidence filename.' }
    $preserved = @($manifest.assets | Where-Object { $_.name -cne $evidenceName })

    # New-Item without Force refuses a destination created since the path check.
    New-Item -ItemType Directory -Path $output -ErrorAction Stop | Out-Null
    $null = Assert-PreviewPath $output
    foreach ($asset in $preserved) {
        [IO.File]::Copy((Join-Path $source $asset.name), (Join-Path $output $asset.name), $false)
        Assert-PreservedPreviewAsset $asset
    }
    # The actual native verifier checks the copied signed bytes and writes fresh
    # beta evidence. It does not trust the candidate's recorded signature alone.
    & $nativeVerifier -AssetsDirectory $output -SourceCommit $SourceCommit -Tag $Tag `
        -Channel beta -WindowsSigned true -CertificateThumbprint $CertificateThumbprint
    foreach ($asset in $preserved) { Assert-PreservedPreviewAsset $asset }
    Assert-PreviewOutputFiles
    Invoke-PreviewProvenance (@('create', '--assets-dir', $output, '--channel', 'beta') + $policy)
    Invoke-PreviewProvenance (@('verify', '--assets-dir', $output, '--channel', 'beta') + $policy)
    Assert-PreviewOutputFiles -WithManifest
    Invoke-PreviewProvenance (@('verify', '--assets-dir', $source, '--channel', 'candidate') + $policy)
    $installer = $preserved | Where-Object name -CEQ 'Abigail-windows-x64-setup.exe'
    Write-Host "Prepared beta assets without rebuilding or publishing: $output"
    [pscustomobject]@{
        prepared = $true
        published = $false
        channel = 'beta'
        tag = $Tag
        sourceCommit = $SourceCommit.ToLowerInvariant()
        outputDirectory = $output
        installerSha256 = $installer.sha256
        installerSize = $installer.size
        preservedFiles = $preserved.Count
    }
} finally {
    foreach ($lock in $locks) { $lock.Dispose() }
}
