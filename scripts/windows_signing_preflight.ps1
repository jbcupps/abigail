$ErrorActionPreference = "Stop"

function Coalesce-String {
  param([AllowNull()][string]$Value)

  if ($null -eq $Value) {
    return ""
  }

  return $Value
}

function Normalize-Thumbprint {
  param([string]$Value)

  return ((Coalesce-String $Value).Trim().ToUpperInvariant() -replace "[^0-9A-F]", "")
}

function Write-GitHubOutput {
  param(
    [string]$Name,
    [string]$Value
  )

  if ($env:GITHUB_OUTPUT) {
    Add-Content -Path $env:GITHUB_OUTPUT -Value "$Name=$Value"
  }
}

function Get-CertificateMatches {
  param([string]$Thumbprint)

  $stores = @("Cert:\CurrentUser\My", "Cert:\LocalMachine\My")
  $matches = foreach ($store in $stores) {
    Get-ChildItem $store -ErrorAction SilentlyContinue |
      Where-Object { $_.Thumbprint -eq $Thumbprint } |
      ForEach-Object {
        [pscustomobject]@{
          Certificate = $_
          Store       = $store
        }
      }
  }

  return @($matches)
}

function Resolve-Certificate {
  param([string]$Thumbprint)

  $matches = Get-CertificateMatches -Thumbprint $Thumbprint
  if ($matches.Count -eq 0) {
    throw "Windows signing certificate with thumbprint $Thumbprint was not found in Cert:\CurrentUser\My or Cert:\LocalMachine\My."
  }

  $selected = $matches |
    Sort-Object @{ Expression = { if ($_.Certificate.HasPrivateKey) { 0 } else { 1 } } } |
    Select-Object -First 1

  if (-not $selected.Certificate.HasPrivateKey) {
    throw "Windows signing certificate $Thumbprint was found in $($selected.Store) but does not have an accessible private key. For SSL.com hardware-token signing, install the SSL.com/YubiKey middleware on the self-hosted runner and unlock the token before starting the job."
  }

  return $selected
}

function Import-PublicCertificateIfNeeded {
  param([string]$Thumbprint)

  if ([string]::IsNullOrWhiteSpace($env:WINDOWS_SIGNING_CERT_PEM)) {
    return
  }

  if ((Get-CertificateMatches -Thumbprint $Thumbprint).Count -gt 0) {
    return
  }

  $tempRoot = if ($env:RUNNER_TEMP) { $env:RUNNER_TEMP } else { [IO.Path]::GetTempPath() }
  $pemPath = Join-Path $tempRoot "abigail-windows-signing-cert.pem"
  Set-Content -Path $pemPath -Value ($env:WINDOWS_SIGNING_CERT_PEM.Trim() + "`n") -Encoding Ascii
  Import-Certificate -FilePath $pemPath -CertStoreLocation "Cert:\CurrentUser\My" | Out-Null
}

$thumbprint = Normalize-Thumbprint -Value $env:WINDOWS_CERTIFICATE_THUMBPRINT
if (-not $thumbprint) {
  throw "WINDOWS_CERTIFICATE_THUMBPRINT is required."
}

$mode = (Coalesce-String $env:ABIGAIL_WINDOWS_SIGNING_MODE).Trim().ToLowerInvariant()
if (-not $mode) {
  if (-not [string]::IsNullOrWhiteSpace($env:WINDOWS_SIGNING_CERT_BASE64) -or
      -not [string]::IsNullOrWhiteSpace($env:WINDOWS_SIGNING_CERT_PASSWORD)) {
    $mode = "pfx"
  } else {
    $mode = "off"
  }
}

switch ($mode) {
  "off" {
    Write-Host "Windows signing preflight skipped."
    Write-GitHubOutput -Name "mode" -Value $mode
    exit 0
  }

  "pfx" {
    if ([string]::IsNullOrWhiteSpace($env:WINDOWS_SIGNING_CERT_BASE64)) {
      throw "WINDOWS_SIGNING_CERT_BASE64 is required for pfx signing mode."
    }
    if ([string]::IsNullOrWhiteSpace($env:WINDOWS_SIGNING_CERT_PASSWORD)) {
      throw "WINDOWS_SIGNING_CERT_PASSWORD is required for pfx signing mode."
    }

    $tempRoot = if ($env:RUNNER_TEMP) { $env:RUNNER_TEMP } else { [IO.Path]::GetTempPath() }
    $certPath = Join-Path $tempRoot "abigail-signing-cert.pfx"
    [IO.File]::WriteAllBytes($certPath, [Convert]::FromBase64String($env:WINDOWS_SIGNING_CERT_BASE64))
    $password = ConvertTo-SecureString -String $env:WINDOWS_SIGNING_CERT_PASSWORD -AsPlainText -Force
    Import-PfxCertificate -FilePath $certPath -Password $password -CertStoreLocation "Cert:\CurrentUser\My" | Out-Null
  }

  "store" {
    Import-PublicCertificateIfNeeded -Thumbprint $thumbprint
  }

  default {
    throw "Unsupported ABIGAIL_WINDOWS_SIGNING_MODE '$mode'. Expected one of: off, pfx, store."
  }
}

$resolved = Resolve-Certificate -Thumbprint $thumbprint
$cert = $resolved.Certificate

Write-Host "Windows signing certificate ready."
$cert | Select-Object Thumbprint, Subject, HasPrivateKey | Format-Table -AutoSize

Write-GitHubOutput -Name "mode" -Value $mode
Write-GitHubOutput -Name "store" -Value $resolved.Store
Write-GitHubOutput -Name "subject" -Value $cert.Subject
