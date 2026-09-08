Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

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
    if ($Thumbprint -notmatch '^[0-9A-Fa-f]{40}$') { throw 'Expected a complete certificate SHA-1 thumbprint.' }
    $signature = Get-AuthenticodeSignature -LiteralPath $resolved
    if ($signature.Status -ne 'Valid') { throw "Untrusted or invalid signature on $resolved ($($signature.Status))." }
    if ($signature.SignerCertificate.Thumbprint -ne $Thumbprint) { throw "Unexpected publisher certificate on $resolved." }
    if ($signature.SignerCertificate.Subject -eq $signature.SignerCertificate.Issuer) { throw 'Self-signed certificates are not accepted for release signing.' }
    if (-not $signature.TimeStamperCertificate) { throw "Missing trusted timestamp on $resolved." }
    if ($signature.SignerCertificate.EnhancedKeyUsageList.ObjectId -notcontains '1.3.6.1.5.5.7.3.3') { throw 'Certificate is not valid for code signing.' }
    $cms = Get-AbigailPeSignature -Path $resolved
    if ($cms.SignerInfos.Count -ne 1 -or $cms.SignerInfos[0].DigestAlgorithm.Value -ne '2.16.840.1.101.3.4.2.1') { throw 'Release signatures must use SHA-256.' }
    $timestamp = @($cms.SignerInfos[0].UnsignedAttributes | Where-Object { $_.Oid.Value -eq '1.3.6.1.4.1.311.3.3.1' })
    if ($timestamp.Count -ne 1) { throw 'Release signatures require an RFC 3161 timestamp.' }
    $timestampCms = [Security.Cryptography.Pkcs.SignedCms]::new()
    $timestampCms.Decode($timestamp[0].Values[0].RawData)
    if ($timestampCms.SignerInfos[0].DigestAlgorithm.Value -ne '2.16.840.1.101.3.4.2.1') { throw 'Timestamp must use SHA-256.' }
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

Export-ModuleMember -Function Get-AbigailSignTool, Get-AbigailPeSignature, Assert-AbigailSignature
