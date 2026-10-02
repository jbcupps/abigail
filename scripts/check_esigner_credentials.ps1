<# Authenticate only to retrieve the selected certificate's public chain.
   No signing request, TOTP submission, or raw vendor output is permitted. #>
param([string]$ToolRoot = $(if ($env:ESIGNER_TOOL_PATH) { $env:ESIGNER_TOOL_PATH } else { Join-Path $PSScriptRoot '../.cache/signing/CodeSignTool' }))
#requires -Version 7.0
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'windows-signing.psm1') -Force
Import-Module (Join-Path $PSScriptRoot 'windows-signing-private.psm1') -Force
foreach ($variable in @('ESIGNER_USERNAME', 'ESIGNER_PASSWORD', 'ESIGNER_CREDENTIAL_ID', 'WINDOWS_CERTIFICATE_THUMBPRINT')) {
    if ([string]::IsNullOrWhiteSpace([Environment]::GetEnvironmentVariable($variable))) { throw "$variable is required for the SSL.com credential check." }
}
if ($env:ESIGNER_TOTP_SECRET -and $env:ESIGNER_TOTP_SECRET.Trim() -match '^\d{6}$') {
    throw 'ESIGNER_TOTP_SECRET must be the existing signing seed, not a changing six-digit code.'
}
$thumbprint = ConvertTo-AbigailThumbprint $env:WINDOWS_CERTIFICATE_THUMBPRINT
$tool = Get-AbigailCodeSignTool -ToolRoot $ToolRoot
$output = Join-Path ([IO.Path]::GetTempPath()) ('abigail-cert-check-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $output | Out-Null
$arguments = $null
$certificates = [Security.Cryptography.X509Certificates.X509Certificate2Collection]::new()
try {
    $arguments = @('get_certs', "-username=$env:ESIGNER_USERNAME", "-password=$env:ESIGNER_PASSWORD", "-credential_id=$env:ESIGNER_CREDENTIAL_ID", "-output_dir_path=$output")
    $exitCode = Invoke-AbigailCodeSignTool -Tool $tool -Arguments $arguments -TimeoutSeconds 120
    if ($exitCode -ne 0) { throw "SSL.com credential check failed (exit $exitCode). Check the existing account credentials and certificate authorization privately." }
    foreach ($file in Get-ChildItem -LiteralPath $output -File -Force) {
        $certificates.AddRange((Read-AbigailPublicCertificates -Path $file.FullName))
    }
    Assert-AbigailPublicSigningCertificate -Certificates $certificates -Thumbprint $thumbprint | ConvertTo-Json -Compress
    Write-Host 'SSL.com account access and expected public signing certificate verified. No signing request was made.'
} finally {
    foreach ($certificate in $certificates) { $certificate.Dispose() }
    $arguments = $null
    # Public certificate files only; remove exact entries in this newly allocated directory.
    foreach ($file in Get-ChildItem -LiteralPath $output -File -Force -ErrorAction SilentlyContinue) { Remove-Item -LiteralPath $file.FullName -Force }
    Remove-Item -LiteralPath $output -Force
}
