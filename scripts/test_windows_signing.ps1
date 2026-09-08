param([string]$TrustedProbe = (Get-Command node).Source)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'windows-signing.psm1') -Force
$signature = Get-AuthenticodeSignature -LiteralPath $TrustedProbe
$thumbprint = $signature.SignerCertificate.Thumbprint
$null = Assert-AbigailSignature -Path $TrustedProbe -Thumbprint $thumbprint
Write-Host 'Trusted SHA256/RFC3161 fixture accepted.'
try {
    $null = Assert-AbigailSignature -Path $TrustedProbe -Thumbprint ('0' * 40)
    throw 'Wrong publisher was accepted.'
} catch {
    if ($_.Exception.Message -notmatch 'Unexpected publisher') { throw }
}
Write-Host 'Wrong publisher rejected.'
$work = Join-Path $PSScriptRoot ('../target/signing-test-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $work | Out-Null
$tampered = Join-Path $work 'tampered.exe'
try {
    Copy-Item -LiteralPath $TrustedProbe -Destination $tampered
    $stream = [IO.File]::Open($tampered, 'Open', 'ReadWrite')
    try { $stream.Position = 64; $original = $stream.ReadByte(); $stream.Position = 64; $stream.WriteByte($original -bxor 1) }
    finally { $stream.Dispose() }
    try {
        $null = Assert-AbigailSignature -Path $tampered -Thumbprint $thumbprint
        throw 'Tampered executable was accepted.'
    } catch {
        if ($_.Exception.Message -notmatch 'Untrusted or invalid signature') { throw }
    }
    Write-Host 'Tampered executable rejected.'
} finally {
    Remove-Item -LiteralPath $tampered -Force
    Remove-Item -LiteralPath $work -Force
}
