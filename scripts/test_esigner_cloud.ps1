<# Tests use a trusted pre-existing executable and fake child processes.
   No SSL.com authentication or real signing request is performed. #>
param(
    [string]$TrustedProbe = (Get-Command node).Source,
    [string]$PublicCertificate,
    [string]$PublicCertificateThumbprint
)
#requires -Version 7.0
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'windows-signing.psm1') -Force
Import-Module (Join-Path $PSScriptRoot 'windows-signing-private.psm1') -Force

function Assert-Rejected([scriptblock]$Action, [string]$Pattern) {
    try { & $Action; throw 'Expected failure was not raised.' }
    catch { if ($_.Exception.Message -notmatch $Pattern) { throw } }
}

$signature = Get-AuthenticodeSignature -LiteralPath $TrustedProbe
if (-not $signature.SignerCertificate) { throw 'The verification fixture must already carry a trusted SHA256/RFC3161 signature.' }
$null = Assert-AbigailSignature -Path $TrustedProbe -Thumbprint $signature.SignerCertificate.Thumbprint
Assert-Rejected { Assert-AbigailSignature -Path $TrustedProbe -Thumbprint ('0' * 40) } 'Unexpected publisher'
Assert-Rejected { ConvertTo-AbigailThumbprint 'not-a-thumbprint' } 'complete certificate'
if ($PublicCertificate) {
    $publicCertificates = Read-AbigailPublicCertificates -Path $PublicCertificate
    try {
        $null = Assert-AbigailPublicSigningCertificate -Certificates $publicCertificates -Thumbprint $PublicCertificateThumbprint
        Assert-Rejected { Assert-AbigailPublicSigningCertificate -Certificates $publicCertificates -Thumbprint ('0' * 40) } 'expected publisher'
    } finally { foreach ($certificate in $publicCertificates) { $certificate.Dispose() } }
}
$expiredFixture = [Security.Cryptography.X509Certificates.X509Certificate2Collection]::new()
$null = $expiredFixture.Add($signature.SignerCertificate)
if ($signature.SignerCertificate.NotAfter.ToUniversalTime() -le [DateTime]::UtcNow) {
    Assert-Rejected { Assert-AbigailPublicSigningCertificate -Certificates $expiredFixture -Thumbprint $signature.SignerCertificate.Thumbprint } 'expired'
}

# Reject weak file/message-imprint digests even when the CMS signer uses SHA256.
$cms = Get-AbigailPeSignature -Path $TrustedProbe
$weakContent = [byte[]]$cms.ContentInfo.Content.Clone()
$sha256Oid = [byte[]]@(0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01)
function Change-DigestOid([byte[]]$Content) {
    for ($offset = 0; $offset -le $Content.Length - $sha256Oid.Length; $offset++) {
        $matches = $true
        for ($index = 0; $index -lt $sha256Oid.Length; $index++) {
            if ($Content[$offset + $index] -ne $sha256Oid[$index]) { $matches = $false; break }
        }
        if ($matches) { $Content[$offset + $sha256Oid.Length - 1] = 2; return }
    }
    throw 'Trusted fixture did not contain its declared SHA256 digest OID.'
}
Change-DigestOid $weakContent
Assert-Rejected { Assert-AbigailSha256PeDigest -Content $weakContent } 'must use SHA-256'
$timestampAttribute = @($cms.SignerInfos[0].UnsignedAttributes | Where-Object { $_.Oid.Value -eq '1.3.6.1.4.1.311.3.3.1' })
$timestampCms = [Security.Cryptography.Pkcs.SignedCms]::new()
$timestampCms.Decode($timestampAttribute[0].Values[0].RawData)
$weakTimestamp = [byte[]]$timestampCms.ContentInfo.Content.Clone()
Change-DigestOid $weakTimestamp
Assert-Rejected { Assert-AbigailSha256TimestampDigest -Content $weakTimestamp } 'must use SHA-256'

$workspace = Join-Path (Join-Path $PSScriptRoot '../target') ('signing-helper-test-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $workspace | Out-Null
$fixture = Join-Path $workspace 'fake-vendor.ps1'
$pidFile = Join-Path $workspace 'fake-vendor.pid'
$tampered = Join-Path $workspace 'tampered.exe'
$launcher = Join-Path $workspace 'Abigail.exe'
$capturedLauncher = Join-Path $workspace 'captured-Abigail.exe'
$nsisName = 'nst' + [guid]::NewGuid().ToString('N') + '.tmp'
$nsisTemp = Join-Path ([IO.Path]::GetTempPath()) $nsisName
$otherTemp = Join-Path ([IO.Path]::GetTempPath()) ('other' + [guid]::NewGuid().ToString('N') + '.tmp')
$outsideTemp = Join-Path $workspace $nsisName
$badPeTemp = Join-Path ([IO.Path]::GetTempPath()) ('nst' + [guid]::NewGuid().ToString('N') + '.tmp')
$unsignedPeTemp = Join-Path ([IO.Path]::GetTempPath()) ('nst' + [guid]::NewGuid().ToString('N') + '.tmp')
$fakeCode = @'
param([string]$Mode, [string]$PidFile)
[IO.File]::WriteAllText($PidFile, [string]$PID)
if ($Mode -eq 'environment') {
    if ($env:ESIGNER_PASSWORD -or $env:ESIGNER_TOTP_SECRET -or $env:JAVA_TOOL_OPTIONS) { exit 23 }
    exit 0
}
if ($Mode -eq 'timeout') { Start-Sleep -Seconds 20; exit 19 }
if ($Mode -eq 'closed-streams') { [Console]::Out.Close(); [Console]::Error.Close(); Start-Sleep -Seconds 20; exit 19 }
if ($Mode -eq 'prompt') {
    [Console]::Out.Write('Enter the OTP - ')
    Start-Sleep -Milliseconds 30
    [Console]::Out.Write('Press enter to continue:')
    Start-Sleep -Seconds 20
    exit 19
}
for ($i = 0; $i -lt 100; $i++) {
    [Console]::Out.WriteLine(('PRIVATE-STDOUT-FIXTURE' * 1000))
    [Console]::Error.WriteLine(('PRIVATE-STDERR-FIXTURE' * 1000))
}
exit 17
'@
[IO.File]::WriteAllText($fixture, $fakeCode)
function New-FixtureStart([string]$Mode) {
    $start = [Diagnostics.ProcessStartInfo]::new()
    $start.FileName = (Get-Process -Id $PID).Path
    foreach ($argument in @('-NoProfile', '-NonInteractive', '-File', $fixture, '-Mode', $Mode, '-PidFile', $pidFile)) { $start.ArgumentList.Add($argument) }
    $start.Environment['ESIGNER_PASSWORD'] = 'PRIVATE-PASSWORD-FIXTURE'
    $start.Environment['ESIGNER_TOTP_SECRET'] = 'PRIVATE-TOTP-FIXTURE'
    $start.Environment['JAVA_TOOL_OPTIONS'] = 'PRIVATE-JAVA-OPTIONS-FIXTURE'
    return $start
}
function Assert-ChildStopped {
    if (-not (Test-Path -LiteralPath $pidFile)) { throw 'The fake vendor did not start.' }
    $childId = [int](Get-Content -LiteralPath $pidFile -Raw)
    if (Get-Process -Id $childId -ErrorAction SilentlyContinue) { throw 'The owned vendor process was left running.' }
    Remove-Item -LiteralPath $pidFile
}
function Invoke-CaptureFixture([string]$TotpSeed, [string]$InputPath = $launcher) {
    $start = [Diagnostics.ProcessStartInfo]::new()
    $start.FileName = (Get-Process -Id $PID).Path
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    foreach ($argument in @('-NoProfile', '-NonInteractive', '-File', (Join-Path $PSScriptRoot 'sign_windows_artifact.ps1'), '-Path', $InputPath)) { $start.ArgumentList.Add($argument) }
    $start.Environment['ESIGNER_USERNAME'] = 'PRIVATE-USERNAME-FIXTURE'
    $start.Environment['ESIGNER_PASSWORD'] = 'PRIVATE-PASSWORD-FIXTURE'
    $start.Environment['ESIGNER_CREDENTIAL_ID'] = 'PRIVATE-CREDENTIAL-FIXTURE'
    $start.Environment['ESIGNER_TOTP_SECRET'] = $TotpSeed
    $start.Environment['WINDOWS_CERTIFICATE_THUMBPRINT'] = $signature.SignerCertificate.Thumbprint
    $start.Environment['ABIGAIL_ESIGNER_AUTH_MODE'] = 'totp'
    $start.Environment['ABIGAIL_SIGNED_MAIN_PATH'] = $capturedLauncher
    $process = [Diagnostics.Process]::new()
    $process.StartInfo = $start
    $started = $false
    try {
        $started = $process.Start()
        $stdout = $process.StandardOutput.ReadToEndAsync()
        $stderr = $process.StandardError.ReadToEndAsync()
        if (-not $process.WaitForExit(30000)) { throw 'The already-signed capture fixture timed out.' }
        return [pscustomobject]@{ exitCode = $process.ExitCode; text = $stdout.GetAwaiter().GetResult() + $stderr.GetAwaiter().GetResult() }
    } finally {
        if ($started -and -not $process.HasExited) { $process.Kill($true); $null = $process.WaitForExit(5000) }
        $process.Dispose()
    }
}
try {
    Copy-Item -LiteralPath $TrustedProbe -Destination $tampered
    $stream = [IO.File]::Open($tampered, 'Open', 'ReadWrite')
    try { $stream.Position = 64; $original = $stream.ReadByte(); $stream.Position = 64; $stream.WriteByte($original -bxor 1) }
    finally { $stream.Dispose() }
    Assert-Rejected { Assert-AbigailSignature -Path $tampered -Thumbprint $signature.SignerCertificate.Thumbprint } 'Untrusted or invalid signature'
    Copy-Item -LiteralPath $TrustedProbe -Destination $launcher
    $captureResult = Invoke-CaptureFixture 'SYNTHETICSEED'
    if ($captureResult.exitCode -ne 0 -or -not (Test-Path -LiteralPath $capturedLauncher) -or
        (Get-FileHash -LiteralPath $launcher).Hash -ne (Get-FileHash -LiteralPath $capturedLauncher).Hash) {
        throw 'The signer failed to preserve exact already-signed Abigail.exe bytes without contacting the provider.'
    }
    if ($captureResult.text -match 'PRIVATE-(?:USERNAME|PASSWORD|CREDENTIAL)-FIXTURE|SYNTHETICSEED') { throw 'Signing diagnostics exposed synthetic credential values.' }
    $captureResult = Invoke-CaptureFixture '123456'
    if ($captureResult.exitCode -eq 0 -or $captureResult.text -notmatch 'changing six-digit code') { throw 'An OTP was accepted as the automated signing seed.' }
    foreach ($path in @($nsisTemp, $otherTemp, $outsideTemp, $unsignedPeTemp)) { Copy-Item -LiteralPath $TrustedProbe -Destination $path }
    $temporaryInput = Assert-AbigailCloudSigningInput -Path $nsisTemp
    if (-not $temporaryInput.temporaryUninstaller) { throw 'The actual NSIS temporary uninstaller input was rejected.' }
    $null = Assert-AbigailSignature -Path $nsisTemp -Thumbprint $signature.SignerCertificate.Thumbprint
    $beforeHash = (Get-FileHash -LiteralPath $nsisTemp -Algorithm SHA256).Hash
    $captureResult = Invoke-CaptureFixture 'SYNTHETICSEED' $nsisTemp
    if ($captureResult.exitCode -ne 0 -or (Get-FileHash -LiteralPath $nsisTemp -Algorithm SHA256).Hash -ne $beforeHash) {
        throw 'The signer failed to normalize and restore exact signed NSIS temporary bytes without contacting the provider.'
    }
    $null = Assert-AbigailSignature -Path $nsisTemp -Thumbprint $signature.SignerCertificate.Thumbprint
    Assert-Rejected { Assert-AbigailCloudSigningInput -Path $otherTemp } 'Only an NSIS uninstaller temporary file'
    Assert-Rejected { Assert-AbigailCloudSigningInput -Path $outsideTemp } 'Only an NSIS uninstaller temporary file'
    $fakePe = [byte[]]::new(64)
    $fakePe[0] = 0x4d; $fakePe[1] = 0x5a; $fakePe[60] = 0x40
    [IO.File]::WriteAllBytes($badPeTemp, $fakePe)
    Assert-Rejected { Assert-AbigailCloudSigningInput -Path $badPeTemp } 'valid PE executable headers'
    # Remove the existing certificate table in a synthetic copy, keeping valid PE headers.
    $stream = [IO.File]::Open($unsignedPeTemp, 'Open', 'ReadWrite')
    $reader = [IO.BinaryReader]::new($stream)
    try {
        $stream.Position = 0x3c; $peOffset = $reader.ReadUInt32()
        $stream.Position = $peOffset + 24; $magic = $reader.ReadUInt16()
        $directoryOffset = if ($magic -eq 0x20b) { 112 } else { 96 }
        $certificateDirectory = $peOffset + 24 + $directoryOffset + 32
        $stream.Position = $certificateDirectory; $certificateOffset = $reader.ReadUInt32()
        $stream.Position = $certificateDirectory; $stream.Write([byte[]]::new(8), 0, 8)
        $stream.SetLength($certificateOffset)
    } finally { $reader.Dispose(); $stream.Dispose() }
    $null = Assert-AbigailCloudSigningInput -Path $unsignedPeTemp
    Assert-Rejected { Assert-AbigailSignature -Path $unsignedPeTemp -Thumbprint $signature.SignerCertificate.Thumbprint } 'Untrusted or invalid signature'
    $result = @(Invoke-AbigailSigningProcess -StartInfo (New-FixtureStart 'output') -TimeoutSeconds 10)
    if ($result.Count -ne 1 -or $result[0] -ne 17) { throw 'Vendor output escaped the private transport, or exit status was lost.' }
    Assert-ChildStopped
    $result = @(Invoke-AbigailSigningProcess -StartInfo (New-FixtureStart 'environment') -TimeoutSeconds 10)
    if ($result.Count -ne 1 -or $result[0] -ne 0) { throw 'Signing secrets or Java overrides were inherited by the child.' }
    Assert-ChildStopped
    Assert-Rejected { Invoke-AbigailSigningProcess -StartInfo (New-FixtureStart 'prompt') -TimeoutSeconds 10 } 'interactive approval'
    Assert-ChildStopped
    Assert-Rejected { Invoke-AbigailSigningProcess -StartInfo (New-FixtureStart 'timeout') -TimeoutSeconds 3 } 'timed out'
    Assert-ChildStopped
    Assert-Rejected { Invoke-AbigailSigningProcess -StartInfo (New-FixtureStart 'closed-streams') -TimeoutSeconds 3 } 'timed out'
    Assert-ChildStopped
    $quoted = ConvertTo-AbigailCodeSignArgument 'C:\path with spaces\"quoted"'
    if ($quoted -ne '"C:\\path with spaces\\\"quoted\""') { throw 'CodeSignTool argfile quoting changed.' }
    Write-Host 'PASS: trusted signatures, publisher/tamper/digest rejection, exact launcher and NSIS temp-byte preservation, constrained valid PE inputs, unsigned temp refusal, seed refusal, private output, isolated environment and bounded process cleanup.'
} finally {
    # Only remove the exact files allocated by this test; no recursive deletion.
    foreach ($path in @($tampered, $launcher, $capturedLauncher, $fixture, $pidFile, $nsisTemp, $otherTemp, $outsideTemp, $badPeTemp, $unsignedPeTemp)) {
        if (Test-Path -LiteralPath $path) { Remove-Item -LiteralPath $path -Force }
    }
    Remove-Item -LiteralPath $workspace -Force
}
