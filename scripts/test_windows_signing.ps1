# Offline release-gate regression tests. No real keys, signing, or network calls.
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'windows_signing_common.ps1')
$testRoot = Join-Path (Get-Item $PSScriptRoot).Parent.FullName ('artifacts\signing-tests\' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $testRoot -Force | Out-Null
$thumbprint = 'A' * 40
$script:passed = 0
function Assert-Test {
    param([bool]$Condition, [string]$Message)
    if (-not $Condition) { throw "FAIL: $Message" }
    $script:passed++
}
function Assert-Rejected {
    param([scriptblock]$Action, [string]$Pattern)
    $caught = $null
    try { & $Action | Out-Null } catch { $caught = $_.Exception.Message }
    Assert-Test ($null -ne $caught -and $caught -match $Pattern) "Expected rejection matching '$Pattern'; got '$caught'."
}
function New-TestCertificate {
    return [pscustomobject]@{
        Subject = 'CN=Test Publisher'; Issuer = 'CN=Test CA'; Thumbprint = $thumbprint
        HasPrivateKey = $true; NotBefore = (Get-Date).AddDays(-1); NotAfter = (Get-Date).AddDays(30)
        EnhancedKeyUsageList = @([pscustomobject]@{ ObjectId = '1.3.6.1.5.5.7.3.3' })
    }
}
Assert-Test ((ConvertTo-WindowsSigningThumbprint (('aa ' * 20).Trim())) -eq $thumbprint) 'Normalize spaced thumbprints.'
foreach ($invalid in @('', 'credential-id', ('A' * 39), (('A' * 40) + '!'))) {
    Assert-Rejected { ConvertTo-WindowsSigningThumbprint $invalid } '40-character'
}
Assert-WindowsSigningCertificate (New-TestCertificate)
$certificate = New-TestCertificate
$certificate.HasPrivateKey = $false
Assert-Rejected { Assert-WindowsSigningCertificate $certificate } 'private-key'
$certificate = New-TestCertificate
$certificate.NotAfter = (Get-Date).AddSeconds(-1)
Assert-Rejected { Assert-WindowsSigningCertificate $certificate } 'expired'
$certificate = New-TestCertificate
$certificate.NotBefore = (Get-Date).AddDays(1)
Assert-Rejected { Assert-WindowsSigningCertificate $certificate } 'not yet valid'
$certificate = New-TestCertificate
$certificate.EnhancedKeyUsageList = @([pscustomobject]@{ ObjectId = '1.3.6.1.5.5.7.3.1' })
Assert-Rejected { Assert-WindowsSigningCertificate $certificate } 'Code Signing EKU'
$certificate = New-TestCertificate
$certificate.Issuer = $certificate.Subject
Assert-Rejected { Assert-WindowsSigningCertificate $certificate } 'self-signed'

$filePath = Join-Path $testRoot 'an installer with spaces.exe'
Set-Content -LiteralPath $filePath -Value 'fixture - never executed'
$signArguments = @(Get-WindowsSignArguments -Thumbprint $thumbprint -StoreLocation LocalMachine -TimestampUrl 'http://ts.ssl.com' -Files @($filePath))
Assert-Test ($signArguments -contains '/sm') 'Machine-store signing must use /sm.'
Assert-Test ($signArguments[-1] -eq $filePath) 'Keep paths with spaces as one native argument.'
Assert-Test (($signArguments -contains '/tr') -and (@($signArguments | Where-Object { $_ -eq 'sha256' }).Count -eq 2)) 'Require RFC 3161 and SHA-256 for both digests.'
$userArguments = @(Get-WindowsSignArguments -Thumbprint $thumbprint -StoreLocation CurrentUser -TimestampUrl 'http://ts.ssl.com' -Files @($filePath))
Assert-Test ($userArguments -notcontains '/sm') 'Current-user signing must not select the machine store.'
Assert-Rejected { Get-WindowsSignArguments -Thumbprint $thumbprint -StoreLocation CurrentUser -TimestampUrl '' -Files @($filePath) } 'timestamp'
Assert-Rejected { Get-WindowsSignArguments -Thumbprint $thumbprint -StoreLocation CurrentUser -TimestampUrl 'http://ts.ssl.com' -Files @() } 'No files'

# Substitute only the native signer and certificate lookup, preserving the gate
# and real file/hash handling. A native warning must fail just like an error.
$script:fakeSignToolExit = 0
function Invoke-TestSignTool { $global:LASTEXITCODE = $script:fakeSignToolExit }
$script:signature = [pscustomobject]@{
    Status = 'Valid'; SignerCertificate = New-TestCertificate
    TimeStamperCertificate = [pscustomobject]@{ Subject = 'CN=Timestamp CA' }
}
function Get-AuthenticodeSignature { param([string]$LiteralPath); return $script:signature }
$record = Assert-WindowsArtifactSignature -Path $filePath -Thumbprint $thumbprint -SignToolPath Invoke-TestSignTool
Assert-Test ($record.Sha256 -eq (Get-FileHash $filePath -Algorithm SHA256).Hash) 'Record the exact signed file hash.'
foreach ($exitCode in @(1, 2)) {
    $script:fakeSignToolExit = $exitCode
    Assert-Rejected { Assert-WindowsArtifactSignature -Path $filePath -Thumbprint $thumbprint -SignToolPath Invoke-TestSignTool } 'SignTool verification failed'
}
$script:fakeSignToolExit = 0
foreach ($status in @('NotSigned', 'HashMismatch', 'NotTrusted', 'UnknownError')) {
    $script:signature.Status = $status
    Assert-Rejected { Assert-WindowsArtifactSignature -Path $filePath -Thumbprint $thumbprint -SignToolPath Invoke-TestSignTool } 'Invalid Authenticode'
}
$script:signature.Status = 'Valid'
$script:signature.SignerCertificate.Thumbprint = 'B' * 40
Assert-Rejected { Assert-WindowsArtifactSignature -Path $filePath -Thumbprint $thumbprint -SignToolPath Invoke-TestSignTool } 'Unexpected signing certificate'
$script:signature.SignerCertificate.Thumbprint = $thumbprint
$script:signature.TimeStamperCertificate = $null
Assert-Rejected { Assert-WindowsArtifactSignature -Path $filePath -Thumbprint $thumbprint -SignToolPath Invoke-TestSignTool } 'Missing trusted timestamp'
Assert-Rejected { Assert-WindowsArtifactSignature -Path (Join-Path $testRoot 'missing.exe') -Thumbprint $thumbprint -SignToolPath Invoke-TestSignTool } 'does not exist'

$configPath = Join-Path $testRoot 'tauri config.json'
Set-Content -LiteralPath $configPath -Value '{"bundle":{"windows":{},"resources":["resources/hive-daemon.exe"]}}'
$previousSignTool = $env:WINDOWS_SIGNTOOL_PATH
try {
    $env:WINDOWS_SIGNTOOL_PATH = $filePath
    Set-WindowsSigningCommand -ConfigPath $configPath -Thumbprint $thumbprint -NoNotice
    $config = Get-Content -LiteralPath $configPath -Raw | ConvertFrom-Json
    Assert-Test ($config.bundle.windows.signCommand.args[-1] -eq '%1') 'Tauri substitutes the artifact path.'
    Assert-Test ($config.bundle.windows.signCommand.args -contains '-NoNotice') 'CI cannot wait on the local signing notice.'
    Assert-Test ($config.bundle.windows.signCommand.args -contains (Join-Path $PSScriptRoot 'windows_interactive_sign.ps1')) 'Resolve signing hook relative to the shared script.'
    Assert-Test ($config.bundle.resources[0] -eq 'resources/hive-daemon.exe') 'Preserve installer payload when adding the signing hook.'
} finally { $env:WINDOWS_SIGNTOOL_PATH = $previousSignTool }
Write-Host "Windows signing regression checks passed: $script:passed"
