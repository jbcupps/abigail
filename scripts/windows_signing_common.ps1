# Shared by the opt-in Windows release lane. No credentials are stored here.
Set-StrictMode -Version Latest

function ConvertTo-WindowsSigningThumbprint {
    param([string]$Value)
    $normalized = ($Value -replace '[\s\u200e\u200f]', '').ToUpperInvariant()
    if ($normalized -notmatch '^[0-9A-F]{40}$') {
        throw 'A 40-character WINDOWS_CERTIFICATE_THUMBPRINT is required (certificate thumbprint, not eSigner credential ID).'
    }
    return $normalized
}

function Assert-WindowsSigningCertificate {
    param([Parameter(Mandatory)]$Certificate)
    if (-not $Certificate.HasPrivateKey) {
        throw 'The certificate has no private-key association. Connect SSL.com eSigner CKA or the hardware token; importing a public .cer/.pem is insufficient.'
    }
    $now = Get-Date
    if ($Certificate.NotBefore -gt $now -or $Certificate.NotAfter -le $now) {
        throw 'The signing certificate is expired or not yet valid.'
    }
    if (@($Certificate.EnhancedKeyUsageList | Where-Object { $_.ObjectId -eq '1.3.6.1.5.5.7.3.3' }).Count -eq 0) {
        throw 'The certificate must have the Code Signing EKU (1.3.6.1.5.5.7.3.3). A website TLS certificate cannot sign this release.'
    }
    if ($Certificate.Subject -eq $Certificate.Issuer) {
        throw 'A self-signed development certificate cannot be used for the public Windows release.'
    }
}

function Resolve-WindowsSigningCertificate {
    param([Parameter(Mandatory)][string]$Thumbprint)
    $normalized = ConvertTo-WindowsSigningThumbprint $Thumbprint
    foreach ($location in @('CurrentUser', 'LocalMachine')) {
        $certificates = @(Get-ChildItem "Cert:\$location\My" -ErrorAction Stop |
            Where-Object { $_.Thumbprint -eq $normalized })
        foreach ($certificate in $certificates) {
            if ($certificate.HasPrivateKey) {
                Assert-WindowsSigningCertificate $certificate
                return [pscustomobject]@{ Certificate = $certificate; StoreLocation = $location }
            }
        }
    }
    throw 'No matching certificate with a private-key association is visible to this Windows user. Configure production eSigner CKA or token middleware under the account that runs the build.'
}

function Resolve-WindowsSignToolPath {
    param([string]$Path = $env:WINDOWS_SIGNTOOL_PATH)
    if ($Path) {
        return (Get-Item -LiteralPath $Path -ErrorAction Stop).FullName
    }
    $command = Get-Command signtool.exe -ErrorAction SilentlyContinue
    if ($command) { return $command.Source }
    $sdkRoot = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
    $candidate = Get-ChildItem $sdkRoot -Recurse -Filter signtool.exe -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -match '\\x64\\' } |
        Sort-Object FullName -Descending | Select-Object -First 1
    if (-not $candidate) { throw 'Install Windows SDK Signing Tools, or set WINDOWS_SIGNTOOL_PATH to signtool.exe.' }
    return $candidate.FullName
}

function Get-WindowsSignArguments {
    param(
        [string]$Thumbprint,
        [ValidateSet('CurrentUser', 'LocalMachine')][string]$StoreLocation,
        [string]$TimestampUrl,
        [string[]]$Files
    )
    $normalized = ConvertTo-WindowsSigningThumbprint $Thumbprint
    if ($TimestampUrl -notmatch '^https?://\S+$') { throw 'An HTTP(S) RFC 3161 timestamp URL is required.' }
    if (-not $Files -or $Files.Count -eq 0) { throw 'No files were provided to sign.' }
    $signArguments = @('sign', '/s', 'My', '/sha1', $normalized, '/fd', 'sha256', '/td', 'sha256', '/tr', $TimestampUrl)
    if ($StoreLocation -eq 'LocalMachine') { $signArguments += '/sm' }
    return $signArguments + $Files
}

function Assert-WindowsArtifactSignature {
    param(
        [Parameter(Mandatory)][string]$Path,
        [Parameter(Mandatory)][string]$Thumbprint,
        [Parameter(Mandatory)][string]$SignToolPath
    )
    $normalized = ConvertTo-WindowsSigningThumbprint $Thumbprint
    $file = Get-Item -LiteralPath $Path -ErrorAction Stop
    # /pa: Authenticode policy; /all: every signature; /tw: warn without timestamp.
    & $SignToolPath verify /pa /all /v /tw $file.FullName | Out-Host
    if ($LASTEXITCODE -ne 0) { throw "SignTool verification failed for $($file.FullName) (exit $LASTEXITCODE)." }
    $signature = Get-AuthenticodeSignature -LiteralPath $file.FullName
    if ($signature.Status -ne 'Valid') { throw "Invalid Authenticode signature on $($file.FullName): $($signature.Status)." }
    if (-not $signature.SignerCertificate -or $signature.SignerCertificate.Thumbprint -ne $normalized) {
        throw "Unexpected signing certificate on $($file.FullName)."
    }
    if (-not $signature.TimeStamperCertificate) { throw "Missing trusted timestamp on $($file.FullName)." }
    return [pscustomobject]@{
        Path = $file.FullName
        Sha256 = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash
        Status = [string]$signature.Status
        Publisher = $signature.SignerCertificate.Subject
        Thumbprint = $signature.SignerCertificate.Thumbprint
        TimestampAuthority = $signature.TimeStamperCertificate.Subject
    }
}

function Set-WindowsSigningCommand {
    param(
        [Parameter(Mandatory)][string]$ConfigPath,
        [Parameter(Mandatory)][string]$Thumbprint,
        [string]$TimestampUrl = 'http://ts.ssl.com',
        [switch]$NoNotice
    )
    $normalized = ConvertTo-WindowsSigningThumbprint $Thumbprint
    $signToolPath = Resolve-WindowsSignToolPath
    $config = Get-Content -LiteralPath $ConfigPath -Raw | ConvertFrom-Json
    $commandArguments = @(
        '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File',
        (Join-Path $PSScriptRoot 'windows_interactive_sign.ps1'),
        '-SignToolPath', $signToolPath, '-CertificateThumbprint', $normalized,
        '-TimestampUrl', $TimestampUrl
    )
    if ($NoNotice) { $commandArguments += '-NoNotice' }
    $commandArguments += '%1'
    $config.bundle.windows | Add-Member -Force -NotePropertyName signCommand -NotePropertyValue @{
        cmd = 'powershell.exe'; args = $commandArguments
    }
    [IO.File]::WriteAllText($ConfigPath, ($config | ConvertTo-Json -Depth 100) + "`n", [Text.UTF8Encoding]::new($false))
}
