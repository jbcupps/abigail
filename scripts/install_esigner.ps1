<# The official Windows distribution is reviewed by its complete SHA-256.
   Never run a replacement download until its contents and pin are reviewed. #>
param([string]$Destination = (Join-Path $PSScriptRoot '../.cache/signing/CodeSignTool'))
#requires -Version 7.0
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$expected = '317D429BE3AA12A5F2C1FFDD575EAB0CB0CE5E2408AB0056BCDCAAB29875F73D'
$destinationPath = [IO.Path]::GetFullPath($Destination)
$cursor = $destinationPath
while ($cursor) {
    if (Test-Path -LiteralPath $cursor) {
        if (((Get-Item -LiteralPath $cursor -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
            throw 'CodeSignTool destination must not traverse a symbolic link or junction.'
        }
    }
    $cursor = [IO.Path]::GetDirectoryName($cursor)
}
$parent = Split-Path $destinationPath
New-Item -ItemType Directory -Force -Path $parent | Out-Null
$archive = Join-Path $parent 'CodeSignTool-windows.zip'
if (-not (Test-Path -LiteralPath $archive)) {
    Invoke-WebRequest 'https://ssl.com/download/codesigntool-for-windows/' -OutFile $archive
}
if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash -ne $expected) {
    throw 'CodeSignTool download does not match the reviewed official version 1.3.3.'
}
Expand-Archive -LiteralPath $archive -DestinationPath $destinationPath -Force
Import-Module (Join-Path $PSScriptRoot 'windows-signing.psm1') -Force
$null = Get-AbigailCodeSignTool -ToolRoot $destinationPath
Write-Host 'Verified official SSL.com CodeSignTool 1.3.3.'
