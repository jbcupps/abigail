# Called directly for first-party payloads and by Tauri for the app/installer.
# Credentials are inherited only by the signing/bundling phase, never npm/cargo.
param([Parameter(Mandatory)][string]$Path)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'windows-signing.psm1') -Force
$resolved = (Resolve-Path -LiteralPath $Path).Path
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$bootstrapRoot = [IO.Path]::GetFullPath((Join-Path $repoRoot 'hive-app/resources/bootstrap')) + [IO.Path]::DirectorySeparatorChar
# Tauri 2.11 also invokes signCommand for unsigned resource files and NSIS DLLs.
# Preserve third-party bytes, publisher signatures, and license provenance.
if ($resolved.StartsWith($bootstrapRoot, [StringComparison]::OrdinalIgnoreCase) -or
    [IO.Path]::GetFileName($resolved) -in @('NSISdl.dll','StartMenu.dll','System.dll','nsDialogs.dll','nsis_tauri_utils.dll','WebView2Loader.dll')) {
    Write-Host "Preserving third-party file: $([IO.Path]::GetFileName($resolved))"
    exit 0
}
if ([IO.Path]::GetExtension($resolved) -ne '.exe') { throw "Unexpected signing input: $resolved" }
foreach ($name in @('WINDOWS_CERTIFICATE_THUMBPRINT','ESIGNER_USERNAME','ESIGNER_PASSWORD','ESIGNER_CREDENTIAL_ID','ESIGNER_TOTP_SECRET')) {
    if (-not [Environment]::GetEnvironmentVariable($name)) { throw "$name is required for signed builds." }
}
$thumbprint = $env:WINDOWS_CERTIFICATE_THUMBPRINT
if ($thumbprint -notmatch '^[0-9A-Fa-f]{40}$') { throw 'Invalid expected signing certificate thumbprint.' }
$existing = Get-AuthenticodeSignature -LiteralPath $resolved
if ($existing.Status -eq 'Valid' -and $existing.SignerCertificate.Thumbprint -eq $thumbprint) {
    $null = Assert-AbigailSignature -Path $resolved -Thumbprint $thumbprint
    Write-Host "Verified existing Abigail signature: $([IO.Path]::GetFileName($resolved))"
    exit 0
}
if ($existing.SignerCertificate) { throw "Refusing to replace an existing unexpected or invalid signature: $resolved" }
$toolRoot = if ($env:ESIGNER_TOOL_PATH) { $env:ESIGNER_TOOL_PATH } else { Join-Path $repoRoot '.cache/signing/CodeSignTool' }
$java = Join-Path $toolRoot 'jdk-11.0.2/bin/java.exe'
$jar = Join-Path $toolRoot 'jar/code_sign_tool-1.3.3.jar'
if (-not (Test-Path -LiteralPath $java) -or -not (Test-Path -LiteralPath $jar)) { throw 'Run scripts/install_esigner.ps1 first.' }
# CodeSignTool @argfiles keep the password and OTP seed out of process command lines.
# Restrict the temporary directory before writing anything sensitive.
$privateDir = Join-Path ([IO.Path]::GetTempPath()) ('abigail-sign-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $privateDir | Out-Null
$acl = [Security.AccessControl.DirectorySecurity]::new()
$acl.SetAccessRuleProtection($true, $false)
$sid = [Security.Principal.WindowsIdentity]::GetCurrent().User
$acl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new($sid,'FullControl','ContainerInherit,ObjectInherit','None','Allow'))
Set-Acl -LiteralPath $privateDir -AclObject $acl
$argumentFile = Join-Path $privateDir 'java.args'
function Quote-JavaArgument([string]$Value) {
    '"' + $Value.Replace('\','\\').Replace('"','\"').Replace("`r",'\r').Replace("`n",'\n').Replace("`t",'\t') + '"'
}
try {
    $arguments = @('sign', "-username=$env:ESIGNER_USERNAME", "-password=$env:ESIGNER_PASSWORD",
        "-credential_id=$env:ESIGNER_CREDENTIAL_ID", "-totp_secret=$env:ESIGNER_TOTP_SECRET", "-input_file_path=$resolved", '-override')
    [IO.File]::WriteAllLines($argumentFile, @($arguments | ForEach-Object { Quote-JavaArgument $_ }), [Text.UTF8Encoding]::new($false))
    Push-Location $toolRoot
    try {
        # Do not publish vendor output/logs: an upstream error could include credentials.
        $vendorOutput = & $java '-Dfile.encoding=UTF-8' -jar $jar "@$argumentFile" 2>&1
        $exitCode = $LASTEXITCODE
    } finally { Pop-Location }
    if ($exitCode -ne 0) { throw "SSL.com signing failed (exit $exitCode). No artifact will be published." }
    $null = Assert-AbigailSignature -Path $resolved -Thumbprint $thumbprint
    Write-Host "Signed and verified: $([IO.Path]::GetFileName($resolved))"
} finally {
    # These exact files were created above; never recursively remove a computed tree.
    if (Test-Path -LiteralPath $argumentFile) { Remove-Item -LiteralPath $argumentFile -Force }
    Remove-Item -LiteralPath $privateDir -Force
    $arguments = $null
    $vendorOutput = $null
}
