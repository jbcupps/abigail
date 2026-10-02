param(
    [Parameter(Mandatory)][string[]]$Files,
    [string]$CertificateThumbprint = $env:WINDOWS_CERTIFICATE_THUMBPRINT,
    [string]$ReportPath
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'windows_signing_common.ps1')
$thumbprint = ConvertTo-WindowsSigningThumbprint $CertificateThumbprint
$signTool = Resolve-WindowsSignToolPath
if ($Files.Count -eq 0) { throw 'At least one explicit file is required for signature verification.' }
$records = @(foreach ($file in $Files) {
    Assert-WindowsArtifactSignature -Path $file -Thumbprint $thumbprint -SignToolPath $signTool
})
if ($ReportPath) {
    $absoluteReport = [IO.Path]::GetFullPath($ReportPath)
    New-Item -ItemType Directory -Force -Path (Split-Path $absoluteReport -Parent) | Out-Null
    $report = [ordered]@{ VerifiedAtUtc = [DateTime]::UtcNow.ToString('o'); Files = $records }
    [IO.File]::WriteAllText($absoluteReport, ($report | ConvertTo-Json -Depth 8) + "`n", [Text.UTF8Encoding]::new($false))
}
$records | Format-Table Path, Status, Publisher -AutoSize
