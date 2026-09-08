<#
Run interactively yourself. Password and OTP seed are entered as SecureStrings.
Local mode keeps secrets only in this process and invokes the prepared signing phase.
GitHub mode sends them on stdin to gh's encrypted repository secret store.
No secret is written into source, console output, or a PowerShell command line.
#>
param(
    [Parameter(Mandatory)][ValidateSet('Local','GitHub')][string]$Destination,
    [string]$Repository = 'jbcupps/abigail',
    [string]$Username,
    [string]$CredentialId,
    [string]$Thumbprint,
    [string]$Version = '0.0.75',
    [string]$AccountProfile = (Join-Path $PSScriptRoot '../.cache/signing/account.json'),
    [ValidateSet('TOTP','Manual')][string]$Authentication = 'TOTP',
    [switch]$UseDialog,
    [switch]$StartBuild
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'windows-signing.psm1') -Force
Import-Module (Join-Path $PSScriptRoot 'windows-signing-private.psm1') -Force
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$Host.UI.RawUI.WindowTitle = 'Abigail - Private Signing Setup'
if (-not [Environment]::UserInteractive) { throw 'This script requires a private interactive terminal.' }
if (Test-Path -LiteralPath $AccountProfile) {
    # This profile contains only public account/certificate metadata.
    $profile = Get-Content -LiteralPath $AccountProfile -Raw | ConvertFrom-Json
    if (-not $Username) { $Username = $profile.username }
    if (-not $CredentialId) { $CredentialId = $profile.credentialId }
    if (-not $Thumbprint) { $Thumbprint = $profile.thumbprint }
}
if (-not $Username -or -not $CredentialId -or -not $Thumbprint) { throw 'Supply the public username, credential ID and thumbprint, or a non-secret account profile.' }
$Thumbprint = ConvertTo-AbigailThumbprint $Thumbprint
if ($Destination -eq 'GitHub' -and $Authentication -eq 'Manual') { throw 'GitHub unattended signing needs the existing TOTP secret. Select Local for manual authenticator codes.' }
$branch = (git -C $repoRoot branch --show-current).Trim()
if ($StartBuild -and -not $branch) { throw 'Check out the reviewed branch before starting its GitHub build.' }
if ($Destination -eq 'GitHub') {
    Write-Host "This sends your SSL.com signing password and existing eSigner OTP seed to encrypted Actions secrets in $Repository."
    Write-Host 'Workflows with repository secret access can use this certificate to sign code.'
} else {
    & (Join-Path $PSScriptRoot 'build_signed_installer.ps1') -Phase Check -Version $Version
    Write-Host 'Credentials will be used only for this prepared local signing run; they will not be saved for later.'
}
$password = $null
$totp = $null
$values = @{}
$previous = @{}
$promptMutex = [Threading.Mutex]::new($false, 'Local\Abigail.PrivateSigning')
if (-not $promptMutex.WaitOne(0)) { $promptMutex.Dispose(); throw 'Another Abigail private signing flow is open. Finish or cancel that flow before starting another.' }
try {
    $password = if ($UseDialog) {
        Read-AbigailSigningSecret -Title 'Abigail SSL.com password' -Prompt "Enter the existing SSL.com password for $Username.`nDestination: $Destination. The value stays hidden and is never sent to chat."
    } else { Read-Host 'SSL.com password (hidden)' -AsSecureString }
    if ($Authentication -eq 'TOTP') {
        $totp = if ($UseDialog) {
            Read-AbigailSigningSecret -Title 'Abigail SSL.com signing secret' -Prompt "Enter the existing eSigner secret code for this certificate, not a six-digit OTP.`nOn SSL.com: certificate details, existing 4-digit PIN, Show QR Code. Use the Secret Code value."
        } else { Read-Host 'Existing eSigner signing secret code (hidden; not a six-digit OTP)' -AsSecureString }
    }
    $values = @{
        ESIGNER_USERNAME = $Username
        ESIGNER_CREDENTIAL_ID = $CredentialId
        WINDOWS_CERTIFICATE_THUMBPRINT = $Thumbprint
        ESIGNER_PASSWORD = [Net.NetworkCredential]::new('', $password).Password
    }
    if ($totp) {
        $values.ESIGNER_TOTP_SECRET = [Net.NetworkCredential]::new('', $totp).Password.Trim()
        if ($values.ESIGNER_TOTP_SECRET -match '^\d{6}$') { throw 'That is a changing six-digit code. Use -Destination Local -Authentication Manual, or supply the existing eSigner secret code for GitHub automation.' }
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
        if ($StartBuild) {
            gh workflow run release.yml --repo $Repository --ref $branch -f signed_artifact_only=true -f "release_version=$Version"
            if ($LASTEXITCODE -ne 0) { throw 'Secrets were configured, but artifact-only dispatch failed.' }
        }
    } else {
        foreach ($name in @('ESIGNER_USERNAME','ESIGNER_CREDENTIAL_ID','WINDOWS_CERTIFICATE_THUMBPRINT','ESIGNER_PASSWORD','ESIGNER_TOTP_SECRET','ABIGAIL_ESIGNER_AUTH_MODE')) {
            $previous[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
            [Environment]::SetEnvironmentVariable($name, $null, 'Process')
        }
        foreach ($entry in $values.GetEnumerator()) { [Environment]::SetEnvironmentVariable($entry.Key, $entry.Value, 'Process') }
        $env:ABIGAIL_ESIGNER_AUTH_MODE = if ($Authentication -eq 'Manual') { 'manual' } else { 'totp' }
        & (Join-Path $PSScriptRoot 'build_signed_installer.ps1') -Phase Sign -Version $Version
        foreach ($name in $previous.Keys) { [Environment]::SetEnvironmentVariable($name, $null, 'Process') }
        $target = if ($env:CARGO_TARGET_DIR) { [IO.Path]::GetFullPath($env:CARGO_TARGET_DIR, $repoRoot) } else { Join-Path $repoRoot 'target' }
        $success = Get-Content -LiteralPath (Join-Path $target 'signed-release/latest-success.json') -Raw | ConvertFrom-Json
        python (Join-Path $PSScriptRoot 'test_initial_setup.py') --binary-dir "$($success.extractedPayload)/resources" --bundle-dir "$($success.extractedPayload)/resources/bootstrap"
        if ($LASTEXITCODE -ne 0) { throw 'Signed payload verification passed, but packaged startup acceptance failed.' }
        Write-Host "Signed package and startup verified: $($success.artifactDirectory)"
    }
} finally {
    foreach ($name in $previous.Keys) { [Environment]::SetEnvironmentVariable($name, $previous[$name], 'Process') }
    $values.Clear()
    if ($password) { $password.Dispose() }
    if ($totp) { $totp.Dispose() }
    $promptMutex.ReleaseMutex()
    $promptMutex.Dispose()
}
