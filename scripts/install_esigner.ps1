# Vendor download is hash-pinned. If SSL.com replaces it, review and update the pin.
param([string]$Destination = (Join-Path $PSScriptRoot '../.cache/signing/CodeSignTool'))
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$expected = '317D429BE3AA12A5F2C1FFDD575EAB0CB0CE5E2408AB0056BCDCAAB29875F73D'
$parent = Split-Path ([IO.Path]::GetFullPath($Destination))
New-Item -ItemType Directory -Force -Path $parent | Out-Null
$archive = Join-Path $parent 'CodeSignTool-windows.zip'
if (-not (Test-Path -LiteralPath $archive)) {
    Invoke-WebRequest 'https://ssl.com/download/codesigntool-for-windows/' -OutFile $archive
}
if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash -ne $expected) { throw 'CodeSignTool download hash does not match the reviewed version 1.3.3.' }
Expand-Archive -LiteralPath $archive -DestinationPath $Destination -Force
if (-not (Test-Path (Join-Path $Destination 'jar/code_sign_tool-1.3.3.jar'))) { throw 'CodeSignTool archive layout changed.' }
Write-Host 'Verified SSL.com CodeSignTool 1.3.3.'
