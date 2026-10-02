#requires -Version 7.0
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$script:AbigailCodeSignToolArchiveSha256 = '317D429BE3AA12A5F2C1FFDD575EAB0CB0CE5E2408AB0056BCDCAAB29875F73D'

function ConvertTo-AbigailThumbprint {
    param([Parameter(Mandatory)][string]$Value)
    $normalized = ($Value -replace '[\s\u200e\u200f]', '').ToUpperInvariant()
    if ($normalized -notmatch '^[0-9A-F]{40}$') { throw 'Expected a complete certificate SHA-1 thumbprint, not an eSigner credential ID.' }
    return $normalized
}

function Get-AbigailCodeSignTool {
    param([string]$ToolRoot = (Join-Path $PSScriptRoot '../.cache/signing/CodeSignTool'))
    $root = (Resolve-Path -LiteralPath $ToolRoot -ErrorAction Stop).Path
    $archive = Join-Path (Split-Path $root) 'CodeSignTool-windows.zip'
    if (-not (Test-Path -LiteralPath $archive -PathType Leaf) -or
        (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash -ne $script:AbigailCodeSignToolArchiveSha256) {
        throw 'Run scripts/install_esigner.ps1 to install the reviewed SSL.com CodeSignTool archive.'
    }
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $zip = [IO.Compression.ZipFile]::OpenRead($archive)
    try {
        foreach ($relative in @('jdk-11.0.2/bin/java.exe', 'jar/code_sign_tool-1.3.3.jar', 'conf/code_sign_tool.properties')) {
            $entry = $zip.GetEntry($relative)
            $path = Join-Path $root $relative
            if (-not $entry -or -not (Test-Path -LiteralPath $path -PathType Leaf)) {
                throw 'SSL.com CodeSignTool installation is incomplete. Run scripts/install_esigner.ps1.'
            }
            $entryStream = $entry.Open()
            $hasher = [Security.Cryptography.SHA256]::Create()
            try { $expected = [BitConverter]::ToString($hasher.ComputeHash($entryStream)).Replace('-', '') }
            finally { $entryStream.Dispose(); $hasher.Dispose() }
            if ((Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash -ne $expected) {
                throw 'SSL.com CodeSignTool files differ from the reviewed archive. Run scripts/install_esigner.ps1.'
            }
        }
    } finally { $zip.Dispose() }
    [pscustomobject]@{
        root = $root
        java = Join-Path $root 'jdk-11.0.2/bin/java.exe'
        jar = Join-Path $root 'jar/code_sign_tool-1.3.3.jar'
        archiveSha256 = $script:AbigailCodeSignToolArchiveSha256
        version = '1.3.3'
    }
}

function Read-AbigailPublicCertificates {
    param([Parameter(Mandatory)][string]$Path)
    $bytes = [IO.File]::ReadAllBytes((Resolve-Path -LiteralPath $Path).Path)
    $text = [Text.Encoding]::ASCII.GetString($bytes)
    $collection = [Security.Cryptography.X509Certificates.X509Certificate2Collection]::new()
    $pem = [regex]::Matches($text, '-----BEGIN CERTIFICATE-----\s*(?<certificate>.*?)\s*-----END CERTIFICATE-----', [Text.RegularExpressions.RegexOptions]::Singleline)
    try {
        if ($pem.Count) {
            foreach ($entry in $pem) {
                $null = $collection.Add([Security.Cryptography.X509Certificates.X509Certificate2]::new([Convert]::FromBase64String(($entry.Groups['certificate'].Value -replace '\s', ''))))
            }
        } else { $collection.Import($bytes) }
        if (-not $collection.Count -or @($collection | Where-Object { $_.HasPrivateKey }).Count) {
            throw 'Expected public certificates only.'
        }
        return ,$collection
    } catch {
        foreach ($certificate in $collection) { $certificate.Dispose() }
        throw 'The public signing certificate could not be read.'
    }
}

function Assert-AbigailPublicSigningCertificate {
    param(
        [Parameter(Mandatory)][Security.Cryptography.X509Certificates.X509Certificate2Collection]$Certificates,
        [Parameter(Mandatory)][string]$Thumbprint
    )
    $expected = ConvertTo-AbigailThumbprint $Thumbprint
    $matchingCertificates = @($Certificates | Where-Object { $_.Thumbprint -eq $expected })
    if ($matchingCertificates.Count -ne 1) { throw 'Public certificate metadata does not contain exactly the expected publisher certificate.' }
    $certificate = $matchingCertificates[0]
    $now = [DateTime]::UtcNow
    if ($certificate.NotBefore.ToUniversalTime() -gt $now -or $certificate.NotAfter.ToUniversalTime() -le $now -or $certificate.Subject -eq $certificate.Issuer) {
        throw 'The public publisher certificate is expired, future, or self-signed.'
    }
    $eku = @($certificate.Extensions | Where-Object { $_.Oid.Value -eq '2.5.29.37' })
    if ($eku.Count -ne 1 -or $eku[0].EnhancedKeyUsages.Value -notcontains '1.3.6.1.5.5.7.3.3') {
        throw 'The public publisher certificate lacks the Code Signing purpose.'
    }
    $chain = [Security.Cryptography.X509Certificates.X509Chain]::new()
    try {
        $chain.ChainPolicy.RevocationMode = [Security.Cryptography.X509Certificates.X509RevocationMode]::Online
        $chain.ChainPolicy.UrlRetrievalTimeout = [TimeSpan]::FromSeconds(15)
        $null = $chain.ChainPolicy.ApplicationPolicy.Add([Security.Cryptography.Oid]::new('1.3.6.1.5.5.7.3.3'))
        $chain.ChainPolicy.ExtraStore.AddRange($Certificates)
        if (-not $chain.Build($certificate)) { throw 'The public signing certificate chain is not trusted.' }
    } finally { $chain.Dispose() }
    [pscustomobject]@{
        publisher = $certificate.Subject
        certificateThumbprint = $certificate.Thumbprint
        certificateExpiresUtc = $certificate.NotAfter.ToUniversalTime().ToString('o')
    }
}

function Read-AbigailDigestInfo {
    param([Parameter(Mandatory)][System.Formats.Asn1.AsnReader]$Reader)
    $digest = $Reader.ReadSequence()
    $algorithm = $digest.ReadSequence()
    $oid = $algorithm.ReadObjectIdentifier()
    if ($algorithm.HasData) { $algorithm.ReadNull() }
    $algorithm.ThrowIfNotEmpty()
    $bytes = $digest.ReadOctetString()
    $digest.ThrowIfNotEmpty()
    if ($oid -ne '2.16.840.1.101.3.4.2.1' -or $bytes.Length -ne 32) {
        throw 'Release file and timestamp digests must use SHA-256.'
    }
}

function Assert-AbigailSha256PeDigest {
    param([Parameter(Mandatory)][byte[]]$Content)
    $reader = [System.Formats.Asn1.AsnReader]::new([ReadOnlyMemory[byte]]::new($Content), [System.Formats.Asn1.AsnEncodingRules]::DER)
    $sequence = $reader.ReadSequence()
    $null = $sequence.ReadEncodedValue() # SpcAttributeTypeAndOptionalValue
    Read-AbigailDigestInfo -Reader $sequence
    $sequence.ThrowIfNotEmpty()
    $reader.ThrowIfNotEmpty()
}

function Assert-AbigailSha256TimestampDigest {
    param([Parameter(Mandatory)][byte[]]$Content)
    $reader = [System.Formats.Asn1.AsnReader]::new([ReadOnlyMemory[byte]]::new($Content), [System.Formats.Asn1.AsnEncodingRules]::DER)
    $sequence = $reader.ReadSequence()
    $null = $sequence.ReadInteger() # TSTInfo version
    $null = $sequence.ReadObjectIdentifier() # Timestamp policy
    Read-AbigailDigestInfo -Reader $sequence # messageImprint
    # The timestamp also contains serial/time/optional fields; SignTool verifies them.
    $reader.ThrowIfNotEmpty()
}

function Get-AbigailSignTool {
    if ($env:TAURI_WINDOWS_SIGNTOOL_PATH) {
        return (Resolve-Path -LiteralPath $env:TAURI_WINDOWS_SIGNTOOL_PATH).Path
    }
    $found = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin" -Filter signtool.exe -Recurse |
        Where-Object { $_.FullName -match '\\x64\\' } | Sort-Object FullName -Descending | Select-Object -First 1
    if (-not $found) { throw 'Install the Windows SDK signing tools before signing.' }
    return $found.FullName
}

function Get-AbigailPeSignature {
    param([Parameter(Mandatory)][string]$Path)
    # Read the PE certificate table without loading a potentially 1+ GB installer.
    Add-Type -AssemblyName System.Security.Cryptography.Pkcs
    $stream = [IO.File]::OpenRead($Path)
    $reader = [IO.BinaryReader]::new($stream)
    try {
        if ($reader.ReadUInt16() -ne 0x5a4d) { throw 'Expected a Windows PE file.' }
        $stream.Position = 0x3c
        $pe = $reader.ReadUInt32()
        $stream.Position = $pe
        if ($reader.ReadUInt32() -ne 0x4550) { throw 'Invalid PE header.' }
        $optional = $pe + 24
        $stream.Position = $optional
        $magic = $reader.ReadUInt16()
        $directories = switch ($magic) { 0x10b { 96 }; 0x20b { 112 }; default { throw 'Unknown PE format.' } }
        $stream.Position = $optional + $directories + 32
        $offset = $reader.ReadUInt32()
        $size = $reader.ReadUInt32()
        if ($offset -eq 0 -or $size -lt 8 -or $offset + [long]$size -gt $stream.Length) { throw 'Missing or invalid embedded signature.' }
        $stream.Position = $offset
        $length = $reader.ReadUInt32()
        $revision = $reader.ReadUInt16()
        $kind = $reader.ReadUInt16()
        if ($length -gt $size -or $length -le 8 -or $length -gt 4MB -or $kind -ne 2 -or $revision -ne 0x200) { throw 'Invalid Authenticode certificate table.' }
        $cms = [Security.Cryptography.Pkcs.SignedCms]::new()
        $cms.Decode($reader.ReadBytes($length - 8))
        return $cms
    } finally { $reader.Dispose(); $stream.Dispose() }
}

function Assert-AbigailSignature {
    param([Parameter(Mandatory)][string]$Path, [Parameter(Mandatory)][string]$Thumbprint)
    $resolved = (Resolve-Path -LiteralPath $Path).Path
    $Thumbprint = ConvertTo-AbigailThumbprint $Thumbprint
    $signature = Get-AuthenticodeSignature -LiteralPath $resolved
    if ($signature.Status -ne 'Valid') { throw "Untrusted or invalid signature on $resolved ($($signature.Status))." }
    if ($signature.SignerCertificate.Thumbprint -ne $Thumbprint) { throw "Unexpected publisher certificate on $resolved." }
    if ($signature.SignerCertificate.Subject -eq $signature.SignerCertificate.Issuer) { throw 'Self-signed certificates are not accepted for release signing.' }
    if (-not $signature.TimeStamperCertificate) { throw "Missing trusted timestamp on $resolved." }
    if ($signature.SignerCertificate.EnhancedKeyUsageList.ObjectId -notcontains '1.3.6.1.5.5.7.3.3') { throw 'Certificate is not valid for code signing.' }
    $cms = Get-AbigailPeSignature -Path $resolved
    if ($cms.SignerInfos.Count -ne 1 -or $cms.SignerInfos[0].DigestAlgorithm.Value -ne '2.16.840.1.101.3.4.2.1') { throw 'Release signatures must use SHA-256.' }
    if ($cms.ContentInfo.ContentType.Value -ne '1.3.6.1.4.1.311.2.1.4') { throw 'Expected an Authenticode PE signature.' }
    Assert-AbigailSha256PeDigest -Content $cms.ContentInfo.Content
    $timestamp = @($cms.SignerInfos[0].UnsignedAttributes | Where-Object { $_.Oid.Value -eq '1.3.6.1.4.1.311.3.3.1' })
    if ($timestamp.Count -ne 1) { throw 'Release signatures require an RFC 3161 timestamp.' }
    $timestampCms = [Security.Cryptography.Pkcs.SignedCms]::new()
    $timestampCms.Decode($timestamp[0].Values[0].RawData)
    if ($timestampCms.SignerInfos.Count -ne 1 -or $timestampCms.SignerInfos[0].DigestAlgorithm.Value -ne '2.16.840.1.101.3.4.2.1') { throw 'Timestamp must use SHA-256.' }
    if ($timestampCms.ContentInfo.ContentType.Value -ne '1.2.840.113549.1.9.16.1.4') { throw 'Expected an RFC 3161 timestamp token.' }
    Assert-AbigailSha256TimestampDigest -Content $timestampCms.ContentInfo.Content
    $tool = Get-AbigailSignTool
    $verification = (& $tool verify /pa /all /v /tw $resolved 2>&1 | Out-String)
    if ($LASTEXITCODE -ne 0) { throw "SignTool rejected $resolved.`n$verification" }
    [pscustomobject]@{
        path = $resolved
        sha256 = (Get-FileHash -LiteralPath $resolved -Algorithm SHA256).Hash
        publisher = $signature.SignerCertificate.Subject
        certificateThumbprint = $signature.SignerCertificate.Thumbprint
        certificateExpiresUtc = $signature.SignerCertificate.NotAfter.ToUniversalTime().ToString('o')
        timestampPublisher = $signature.TimeStamperCertificate.Subject
        timestampCertificateThumbprint = $signature.TimeStamperCertificate.Thumbprint
        digest = 'SHA256'
        timestampType = 'RFC3161/SHA256'
        verification = $verification.Trim()
    }
}

Export-ModuleMember -Function ConvertTo-AbigailThumbprint, Get-AbigailCodeSignTool, Read-AbigailPublicCertificates, Assert-AbigailPublicSigningCertificate, Get-AbigailSignTool, Get-AbigailPeSignature, Assert-AbigailSignature, Assert-AbigailSha256PeDigest, Assert-AbigailSha256TimestampDigest
