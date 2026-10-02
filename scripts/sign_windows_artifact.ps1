<# Signs only Abigail's first-party executables through SSL.com eSigner.
   Tauri invokes this after stamping the main executable and for NSIS/uninstall.
   The build phase must finish before credentials enter the environment. #>
param([Parameter(Mandatory)][string]$Path)
#requires -Version 7.0
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'windows-signing.psm1') -Force
Import-Module (Join-Path $PSScriptRoot 'windows-signing-private.psm1') -Force
$resolved = (Resolve-Path -LiteralPath $Path -ErrorAction Stop).Path
$name = [IO.Path]::GetFileName($resolved)
# Tauri also invokes the callback for NSIS/WebView helper DLLs. Preserve vendor bytes.
if ($name -in @('NSISdl.dll', 'StartMenu.dll', 'System.dll', 'nsDialogs.dll', 'nsis_tauri_utils.dll', 'WebView2Loader.dll')) {
    Write-Host "Preserving third-party helper: $name"
    exit 0
}
$inputRecord = Assert-AbigailCloudSigningInput -Path $resolved
if ($env:ABIGAIL_ESIGNER_AUTH_MODE -and $env:ABIGAIL_ESIGNER_AUTH_MODE -ne 'totp') {
    throw 'Cloud signing requires automated TOTP authorization.'
}
foreach ($variable in @('WINDOWS_CERTIFICATE_THUMBPRINT', 'ESIGNER_USERNAME', 'ESIGNER_PASSWORD', 'ESIGNER_CREDENTIAL_ID', 'ESIGNER_TOTP_SECRET')) {
    if ([string]::IsNullOrWhiteSpace([Environment]::GetEnvironmentVariable($variable))) { throw "$variable is required for cloud signing." }
}
if ($env:ESIGNER_TOTP_SECRET.Trim() -match '^\d{6}$') { throw 'ESIGNER_TOTP_SECRET must be the existing signing seed, not a changing six-digit code.' }
$thumbprint = ConvertTo-AbigailThumbprint $env:WINDOWS_CERTIFICATE_THUMBPRINT
function Save-SignedMain {
    if ($name -eq 'Abigail.exe' -and $env:ABIGAIL_SIGNED_MAIN_PATH) {
        if (-not [IO.Path]::IsPathFullyQualified($env:ABIGAIL_SIGNED_MAIN_PATH)) { throw 'Signed launcher capture requires an absolute path.' }
        $capture = [IO.Path]::GetFullPath($env:ABIGAIL_SIGNED_MAIN_PATH)
        if ([string]::Equals($capture, $resolved, [StringComparison]::OrdinalIgnoreCase)) { throw 'Signed launcher capture must be separate from the build executable.' }
        Copy-Item -LiteralPath $resolved -Destination $capture -Force
    }
}
function Copy-VerifiedTemporaryUninstaller {
    if ($inputRecord.temporaryUninstaller) {
        $record = Assert-AbigailSignature -Path $signingInput -Thumbprint $thumbprint
        Copy-Item -LiteralPath $signingInput -Destination $resolved -Force
        if ((Get-FileHash -LiteralPath $resolved -Algorithm SHA256).Hash -ne $record.sha256) {
            throw 'NSIS temporary uninstaller differs from its verified signed executable.'
        }
        $null = Assert-AbigailSignature -Path $resolved -Thumbprint $thumbprint
    }
}
$temporaryDirectory = $null
$signingInput = $resolved
$arguments = $null
try {
    if ($inputRecord.temporaryUninstaller) {
        # Give CodeSignTool an explicit executable name; NSIS passes nst*.tmp.
        $temporaryDirectory = New-AbigailSigningDirectory
        $signingInput = Join-Path $temporaryDirectory 'uninstaller.exe'
        Copy-Item -LiteralPath $resolved -Destination $signingInput
    }
    $existing = Get-AuthenticodeSignature -LiteralPath $signingInput
    if ($existing.Status -eq 'Valid' -and $existing.SignerCertificate.Thumbprint -eq $thumbprint) {
        $null = Assert-AbigailSignature -Path $signingInput -Thumbprint $thumbprint
        Copy-VerifiedTemporaryUninstaller
        Save-SignedMain
        Write-Host "Verified existing Abigail signature: $name"
        exit 0
    }
    if ($existing.SignerCertificate) { throw 'Refusing to replace an unexpected or invalid existing signature.' }
    $toolRoot = if ($env:ESIGNER_TOOL_PATH) { $env:ESIGNER_TOOL_PATH } else { Join-Path $PSScriptRoot '../.cache/signing/CodeSignTool' }
    $tool = Get-AbigailCodeSignTool -ToolRoot $toolRoot
    $arguments = @('sign', "-username=$env:ESIGNER_USERNAME", "-password=$env:ESIGNER_PASSWORD", "-credential_id=$env:ESIGNER_CREDENTIAL_ID", "-totp_secret=$env:ESIGNER_TOTP_SECRET", "-input_file_path=$signingInput", '-override')
    $exitCode = Invoke-AbigailCodeSignTool -Tool $tool -Arguments $arguments
    if ($exitCode -ne 0) {
        $reason = switch ($exitCode) {
            7 { 'The provider malware check blocked this file.' }
            8 { 'The provider did not receive automated signing authorization.' }
            10 { 'SSL.com account authentication failed.' }
            11 { 'SSL.com rejected the certificate authorization or signing code.' }
            default { 'Check the prepared input and existing signing authorization.' }
        }
        throw "SSL.com signing failed (exit $exitCode). $reason No artifact will be published."
    }
    $null = Assert-AbigailSignature -Path $signingInput -Thumbprint $thumbprint
    Copy-VerifiedTemporaryUninstaller
    Save-SignedMain
    Write-Host "Signed and verified: $name"
} finally {
    if ($temporaryDirectory) {
        if (Test-Path -LiteralPath $signingInput) { Remove-Item -LiteralPath $signingInput -Force }
        Remove-Item -LiteralPath $temporaryDirectory -Force
    }
    $arguments = $null
}
