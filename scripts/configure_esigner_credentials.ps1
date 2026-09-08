<#
Run interactively yourself. Password and OTP seed are entered as SecureStrings.
Local mode keeps secrets only in this process and invokes the prepared signing phase.
GitHub mode sends them on stdin to gh's encrypted repository secret store.
No secret is written into source, console output, or a PowerShell command line.
#>
param(
    [Parameter(Mandatory)][ValidateSet('Local','GitHub')][string]$Destination,
    [string]$Repository = 'jbcupps/abigail',
    [Parameter(Mandatory)][string]$Username,
    [Parameter(Mandatory)][string]$CredentialId,
    [Parameter(Mandatory)][string]$Thumbprint,
    [string]$Version = '0.0.75'
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if (-not [Environment]::UserInteractive) { throw 'This script requires a private interactive terminal.' }
if ($Destination -eq 'GitHub') {
    Write-Host "This sends your SSL.com signing password and existing eSigner OTP seed to encrypted Actions secrets in $Repository."
    Write-Host 'Workflows with repository secret access can use this certificate to sign code.'
    if ((Read-Host 'Type the repository name to confirm this destination') -ne $Repository) { throw 'No secrets were changed.' }
} else {
    Write-Host 'Credentials will be used only for this prepared local signing run; they will not be saved for later.'
}
$password = Read-Host 'SSL.com password (hidden)' -AsSecureString
$totp = Read-Host 'Existing eSigner signing TOTP secret (hidden; not the six-digit code)' -AsSecureString
$values = @{}
try {
    $values = @{
        ESIGNER_USERNAME = $Username
        ESIGNER_CREDENTIAL_ID = $CredentialId
        WINDOWS_CERTIFICATE_THUMBPRINT = $Thumbprint
        ESIGNER_PASSWORD = [Net.NetworkCredential]::new('', $password).Password
        ESIGNER_TOTP_SECRET = [Net.NetworkCredential]::new('', $totp).Password
    }
    foreach ($entry in $values.GetEnumerator()) {
        if ([string]::IsNullOrWhiteSpace($entry.Value)) { throw "Missing $($entry.Key)." }
    }
    if ($Destination -eq 'GitHub') {
        foreach ($entry in $values.GetEnumerator()) {
            $entry.Value | & gh secret set $entry.Key --repo $Repository
            if ($LASTEXITCODE -ne 0) { throw "Failed to set $($entry.Key)." }
        }
        Write-Host 'Encrypted GitHub signing secrets configured. No release was started.'
    } else {
        foreach ($entry in $values.GetEnumerator()) { [Environment]::SetEnvironmentVariable($entry.Key, $entry.Value, 'Process') }
        & (Join-Path $PSScriptRoot 'build_signed_installer.ps1') -Phase Sign -Version $Version
    }
} finally {
    foreach ($name in $values.Keys) { [Environment]::SetEnvironmentVariable($name, $null, 'Process') }
    $values.Clear()
    $password.Dispose()
    $totp.Dispose()
}
