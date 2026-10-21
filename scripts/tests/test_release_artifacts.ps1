#requires -Version 7.0
<# Uses copies of an already signed executable. Never launches an installer,
   signs a file, creates certificates, changes trust, or downloads fixtures.
   Native Windows signature verification uses the host's normal trust policy. #>
[CmdletBinding()]
param(
    [string]$TrustedProbe = (Get-Command node -ErrorAction Stop).Source,
    [string]$UnsignedProbe,
    [string]$TrustedMsi
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$script:passed = 0
$verifier = Join-Path $PSScriptRoot '../verify_release_artifacts.ps1'
Import-Module (Join-Path $PSScriptRoot '../windows-signing.psm1') -Force
$trustedSignature = Get-AuthenticodeSignature -LiteralPath $TrustedProbe
if (-not $trustedSignature.SignerCertificate) { throw 'TrustedProbe must already have a trusted SHA256/RFC3161 signature.' }
$thumbprint = $trustedSignature.SignerCertificate.Thumbprint
$null = Assert-AbigailSignature -Path $TrustedProbe -Thumbprint $thumbprint
$tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\', '/')
$workspace = Join-Path $tempRoot ('abigail-release-artifact-tests-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $workspace | Out-Null
$assets = Join-Path $workspace 'assets'
New-Item -ItemType Directory -Path $assets | Out-Null
$exe = Join-Path $assets 'Abigail-windows-x64-setup.exe'
$report = Join-Path $assets 'windows-signature-evidence.json'
$unsigned = Join-Path $workspace 'unsigned-fixture.exe'
$noTimestamp = Join-Path $workspace 'no-timestamp-fixture.exe'
$junction = Join-Path $workspace 'asset-junction'
$source = 'a' * 40

function Assert-Equal($Actual, $Expected, [string]$Description) {
    if ($Actual -cne $Expected) { throw "$Description (expected '$Expected', got '$Actual')." }
}
function Pass([string]$Name) { $script:passed++; Write-Host "PASS $Name" }
function Invoke-Verify([hashtable]$Overrides = @{}) {
    $parameters = @{
        AssetsDirectory = $assets; SourceCommit = $source; Tag = 'v0.0.76'; Channel = 'candidate'
        WindowsSigned = 'true'; CertificateThumbprint = $thumbprint
    }
    foreach ($key in $Overrides.Keys) { $parameters[$key] = $Overrides[$key] }
    & $verifier @parameters
}
function Assert-Rejected([string]$Name, [scriptblock]$Action, [string]$Pattern) {
    if (Test-Path -LiteralPath $report) { Remove-Item -LiteralPath $report -Force }
    $failed = $false
    try { & $Action }
    catch {
        $failed = $true
        if ($_.Exception.Message -notmatch $Pattern) { throw "$Name raised an unexpected error: $($_.Exception.Message)" }
    }
    if (-not $failed) { throw "$Name was not rejected." }
    if (Test-Path -LiteralPath $report) { throw "$Name created success evidence after failure." }
    Pass $Name
}
function Read-CertificateTable([IO.FileStream]$Stream) {
    $reader = [IO.BinaryReader]::new($Stream, [Text.Encoding]::UTF8, $true)
    try {
        $Stream.Position = 0x3c
        $pe = $reader.ReadUInt32()
        $Stream.Position = $pe + 24
        $magic = $reader.ReadUInt16()
        $directories = switch ($magic) { 0x10b { 96 }; 0x20b { 112 }; default { throw 'Fixture must be PE32 or PE32+.' } }
        $directory = $pe + 24 + $directories + 32
        $Stream.Position = $directory
        $offset = $reader.ReadUInt32()
        $size = $reader.ReadUInt32()
        if ($offset -eq 0 -or $size -lt 8 -or $offset + [long]$size -gt $Stream.Length) { throw 'Fixture must have a valid certificate table.' }
        return @{ directory = $directory; offset = $offset; size = $size }
    } finally { $reader.Dispose() }
}
try {
    if ($UnsignedProbe) {
        Copy-Item -LiteralPath $UnsignedProbe -Destination $unsigned
    } else {
        # Only modify an owned copy. Removing its PE certificate table creates
        # an unsigned test fixture without creating any new certificate.
        Copy-Item -LiteralPath $TrustedProbe -Destination $unsigned
        $stream = [IO.File]::Open($unsigned, 'Open', 'ReadWrite')
        try {
            $table = Read-CertificateTable $stream
            $stream.Position = $table.directory
            $stream.Write([byte[]]::new(8), 0, 8)
            $stream.SetLength($table.offset)
        } finally { $stream.Dispose() }
    }
    Assert-Equal ([string](Get-AuthenticodeSignature -LiteralPath $unsigned).Status) 'NotSigned' 'Unsigned fixture status'

    Copy-Item -LiteralPath $TrustedProbe -Destination $exe
    $originalHash = (Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash
    Invoke-Verify @{ Channel = 'stable'; SourceCommit = $source.ToUpperInvariant(); CertificateThumbprint = $thumbprint.ToLowerInvariant() }
    $evidence = Get-Content -LiteralPath $report -Raw | ConvertFrom-Json
    Assert-Equal $evidence.schemaVersion 1 'Schema version'
    Assert-Equal $evidence.sourceCommit $source 'Normalized source SHA'
    Assert-Equal $evidence.windowsSigned $true 'Signed policy'
    Assert-Equal $evidence.expectedThumbprint $thumbprint 'Normalized expected publisher'
    Assert-Equal $evidence.artifacts.Count 1 'One canonical artifact'
    Assert-Equal $evidence.artifacts[0].name 'Abigail-windows-x64-setup.exe' 'Canonical name'
    Assert-Equal $evidence.artifacts[0].sha256 $originalHash.ToLowerInvariant() 'Evidence binds exact bytes'
    Assert-Equal $evidence.artifacts[0].signatureStatus 'Valid' 'Valid signature'
    Assert-Equal $evidence.artifacts[0].timestampType 'RFC3161' 'Strict timestamp policy'
    Assert-Equal (Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash $originalHash 'Verifier never changes installer bytes'
    Pass 'signed stable evidence and unchanged installer'
    Invoke-Verify @{ Channel = 'beta'; Tag = 'v0.0.76-beta.131' }
    Pass 'signed beta'
    Invoke-Verify @{ Tag = 'v0.0.76-beta.131' }
    Pass 'candidate accepts planned beta tag'

    Assert-Rejected 'wrong expected publisher' { Invoke-Verify @{ CertificateThumbprint = '0' * 40 } } 'Unexpected publisher'
    Assert-Rejected 'signed file claimed unsigned' { Invoke-Verify @{ WindowsSigned = 'false' } } 'actually unsigned'
    Assert-Rejected 'unsigned stable policy' { Invoke-Verify @{ WindowsSigned = 'false'; Channel = 'stable' } } 'Stable release artifacts'
    Assert-Rejected 'short source SHA' { Invoke-Verify @{ SourceCommit = 'abc1234' } } '40-character'
    Assert-Rejected 'malformed tag' { Invoke-Verify @{ Tag = 'v0.0.76;anything' } } 'Tag must'
    Assert-Rejected 'stable channel with beta tag' { Invoke-Verify @{ Channel = 'stable'; Tag = 'v0.0.76-beta.131' } } 'Tag must'
    Assert-Rejected 'beta channel with stable tag' { Invoke-Verify @{ Channel = 'beta' } } 'Tag must'
    Assert-Rejected 'noncanonical boolean' { Invoke-Verify @{ WindowsSigned = 'TRUE' } } 'exactly true or false'
    Assert-Rejected 'report cannot overwrite installer' { Invoke-Verify @{ ReportPath = $exe } } 'regular JSON'

    # Preserve prior evidence when a later verification fails; consumers must
    # require a successful exit and match metadata, never accept stale files.
    [IO.File]::WriteAllText($report, 'existing evidence')
    try { Invoke-Verify @{ CertificateThumbprint = '0' * 40 }; throw 'Expected wrong publisher rejection.' }
    catch { if ($_.Exception.Message -notmatch 'Unexpected publisher') { throw } }
    Assert-Equal ([IO.File]::ReadAllText($report)) 'existing evidence' 'Prior evidence preserved'
    Remove-Item -LiteralPath $report
    Pass 'failed verification preserves existing evidence'

    $stream = [IO.File]::Open($exe, 'Open', 'ReadWrite')
    try { $stream.Position = 64; $byte = $stream.ReadByte(); $stream.Position = 64; $stream.WriteByte($byte -bxor 1) }
    finally { $stream.Dispose() }
    Assert-Rejected 'tampered signed bytes' { Invoke-Verify } 'Untrusted or invalid signature'
    Assert-Rejected 'invalid signature cannot count as unsigned' { Invoke-Verify @{ WindowsSigned = 'false' } } 'actually unsigned'

    # RFC3161 timestamps are unsigned CMS attributes. Remove them only from a
    # disposable fixture; the original publisher signature is never re-signed.
    Copy-Item -LiteralPath $TrustedProbe -Destination $noTimestamp
    $cms = Get-AbigailPeSignature -Path $noTimestamp
    $timestampAttributes = @($cms.SignerInfos[0].UnsignedAttributes | Where-Object { $_.Oid.Value -eq '1.3.6.1.4.1.311.3.3.1' })
    foreach ($attribute in $timestampAttributes) {
        foreach ($value in $attribute.Values) { $cms.SignerInfos[0].RemoveUnsignedAttribute($value) }
    }
    $encoded = $cms.Encode()
    $stream = [IO.File]::Open($noTimestamp, 'Open', 'ReadWrite')
    $writer = [IO.BinaryWriter]::new($stream, [Text.Encoding]::UTF8, $true)
    try {
        $table = Read-CertificateTable $stream
        $length = 8 + $encoded.Length
        $padded = [int]([Math]::Ceiling($length / 8.0) * 8)
        $stream.Position = $table.directory + 4; $writer.Write([uint32]$padded)
        $stream.Position = $table.offset; $writer.Write([uint32]$length); $writer.Write([uint16]0x200); $writer.Write([uint16]2)
        $writer.Write($encoded); $writer.Write([byte[]]::new($padded - $length))
        $stream.SetLength($table.offset + $padded)
    } finally { $writer.Dispose(); $stream.Dispose() }
    Copy-Item -LiteralPath $noTimestamp -Destination $exe -Force
    Assert-Rejected 'missing trusted timestamp' { Invoke-Verify } 'Missing trusted timestamp|Untrusted or invalid signature|RFC 3161 timestamp'

    Copy-Item -LiteralPath $unsigned -Destination $exe -Force
    Invoke-Verify @{ WindowsSigned = 'false' }
    $evidence = Get-Content -LiteralPath $report -Raw | ConvertFrom-Json
    Assert-Equal $evidence.windowsSigned $false 'Unsigned policy'
    Assert-Equal $evidence.expectedThumbprint $null 'No expected publisher for unsigned policy'
    Assert-Equal $evidence.artifacts[0].signatureStatus 'NotSigned' 'Actually unsigned'
    Assert-Equal $evidence.artifacts[0].publisher $null 'No invented unsigned publisher'
    Assert-Equal $evidence.artifacts[0].timestampType $null 'No invented unsigned timestamp'
    Pass 'unsigned candidate evidence'
    Invoke-Verify @{ WindowsSigned = 'false'; Channel = 'beta'; Tag = 'v0.0.76-beta.131' }
    Pass 'unsigned beta stabilization'
    Assert-Rejected 'unsigned bytes claimed signed' { Invoke-Verify } 'Untrusted or invalid signature'

    $extra = Join-Path $assets 'other.exe'
    Copy-Item -LiteralPath $unsigned -Destination $extra
    Assert-Rejected 'unexpected EXE rejected' { Invoke-Verify @{ WindowsSigned = 'false' } } 'Unexpected release installer'
    Remove-Item -LiteralPath $extra
    $extra = Join-Path $assets 'other.msi'
    [IO.File]::WriteAllText($extra, 'not an installer')
    Assert-Rejected 'unexpected MSI rejected' { Invoke-Verify @{ WindowsSigned = 'false' } } 'Unexpected release installer'
    Remove-Item -LiteralPath $extra
    Remove-Item -LiteralPath $exe
    Assert-Rejected 'missing canonical EXE' { Invoke-Verify @{ WindowsSigned = 'false' } } 'canonical Abigail'
    Copy-Item -LiteralPath $unsigned -Destination $exe
    $nested = Join-Path $assets 'nested'
    New-Item -ItemType Directory -Path $nested | Out-Null
    Assert-Rejected 'nested asset directory rejected' { Invoke-Verify @{ WindowsSigned = 'false' } } 'flat directory'
    Remove-Item -LiteralPath $nested
    New-Item -ItemType Junction -Path $junction -Target $assets | Out-Null
    Assert-Rejected 'reparse asset directory rejected' { Invoke-Verify @{ AssetsDirectory = $junction; WindowsSigned = 'false' } } 'reparse'
    Assert-Rejected 'reparse report parent rejected' { Invoke-Verify @{ ReportPath = (Join-Path $junction 'report.json'); WindowsSigned = 'false' } } 'reparse'
    Remove-Item -LiteralPath $junction -Force

    if ($TrustedMsi) {
        Copy-Item -LiteralPath $TrustedProbe -Destination $exe -Force
        Copy-Item -LiteralPath $TrustedMsi -Destination (Join-Path $assets 'Abigail-windows-x64.msi')
        Invoke-Verify @{ Channel = 'stable' }
        $evidence = Get-Content -LiteralPath $report -Raw | ConvertFrom-Json
        Assert-Equal $evidence.artifacts.Count 2 'EXE and MSI evidence'
        Assert-Equal (@($evidence.artifacts | Where-Object name -EQ 'Abigail-windows-x64.msi')[0].timestampType) 'Authenticode' 'MSI timestamp evidence'
        Pass 'optional trusted MSI with matching publisher'
    }
    Write-Host "Release artifact verification: $script:passed checks passed. No installer was executed or signed."
} finally {
    # Remove links themselves before recursively cleaning only this owned test directory.
    if (Test-Path -LiteralPath $junction) { Remove-Item -LiteralPath $junction -Force }
    $resolvedWorkspace = [IO.Path]::GetFullPath($workspace)
    if ([IO.Path]::GetDirectoryName($resolvedWorkspace) -ne $tempRoot -or
        [IO.Path]::GetFileName($resolvedWorkspace) -notmatch '^abigail-release-artifact-tests-[0-9a-f]{32}$') {
        throw 'Refusing to clean an unexpected test directory.'
    }
    if (Test-Path -LiteralPath $resolvedWorkspace) { Remove-Item -LiteralPath $resolvedWorkspace -Recurse -Force }
}
