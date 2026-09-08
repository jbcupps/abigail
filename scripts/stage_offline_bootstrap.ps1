<# Build-time provisioning only. The installed app never downloads a runtime/model.
   Keep the model's upstream manifest and content-addressed blobs byte-for-byte.
   CPU runtime avoids mandatory GPU drivers and keeps the installer below NSIS's size limit.
#>
param(
    [string]$Destination = (Join-Path $PSScriptRoot '../hive-app/resources/bootstrap'),
    [string]$Cache = (Join-Path $PSScriptRoot '../.cache/bootstrap')
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
New-Item -ItemType Directory -Force -Path $Destination, $Cache | Out-Null
$Destination = (Resolve-Path -LiteralPath $Destination).Path
$Cache = (Resolve-Path -LiteralPath $Cache).Path
$lock = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'offline-bootstrap.lock.json') -Raw | ConvertFrom-Json

function Get-VerifiedFile([string]$Url, [string]$Path, [string]$Sha256) {
    if ((Test-Path -LiteralPath $Path) -and (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash -eq $Sha256) { return }
    $partial = "$Path.partial"
    & curl.exe --fail --location --retry 3 --output $partial $Url
    if ($LASTEXITCODE -ne 0) { throw "Download failed: $Url. Re-run to retry." }
    if ((Get-FileHash -LiteralPath $partial -Algorithm SHA256).Hash -ne $Sha256) { throw "Checksum mismatch: $Url" }
    Move-Item -LiteralPath $partial -Destination $Path -Force
}

$archive = Join-Path $Cache 'ollama-windows-amd64.zip'
Get-VerifiedFile $lock.runtime_url $archive $lock.runtime_sha256
$runtimeDir = Join-Path $Destination 'ollama'
New-Item -ItemType Directory -Force -Path $runtimeDir | Out-Null
Add-Type -AssemblyName System.IO.Compression.FileSystem
$zip = [IO.Compression.ZipFile]::OpenRead($archive)
try {
    foreach ($entry in $zip.Entries) {
        $name = $entry.FullName.Replace('\', '/')
        # Keep root licenses and executable, plus common/CPU libraries. GPU folders
        # are deliberately omitted; no changes are made to the upstream binaries.
        if ($name -match '(^|/)(cuda[^/]*|rocm[^/]*|vulkan[^/]*|mlx[^/]*)/') { continue }
        if ($name.EndsWith('/')) { continue }
        $path = [IO.Path]::GetFullPath((Join-Path $runtimeDir $name))
        if (-not $path.StartsWith($runtimeDir + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw 'Unsafe archive path' }
        New-Item -ItemType Directory -Force -Path ([IO.Path]::GetDirectoryName($path)) | Out-Null
        [IO.Compression.ZipFileExtensions]::ExtractToFile($entry, $path, $true)
    }
} finally { $zip.Dispose() }
if (-not (Test-Path -LiteralPath (Join-Path $runtimeDir 'ollama.exe'))) { throw 'Runtime archive layout changed' }

Get-VerifiedFile "https://raw.githubusercontent.com/ollama/ollama/$($lock.runtime_version)/LICENSE" (Join-Path $runtimeDir "LICENSE.txt") $lock.runtime_license_sha256
$models = Join-Path $Destination 'models'
$manifestDir = Join-Path $models "manifests/registry.ollama.ai/library/$($lock.model_name)"
$blobs = Join-Path $models 'blobs'
New-Item -ItemType Directory -Force -Path $manifestDir, $blobs | Out-Null
$manifestFile = Join-Path $manifestDir $lock.model_tag
Get-VerifiedFile "https://registry.ollama.ai/v2/library/$($lock.model_name)/manifests/$($lock.model_tag)" $manifestFile ($lock.model_digest -replace '^sha256:', '')
$manifest = Get-Content -LiteralPath $manifestFile -Raw | ConvertFrom-Json
foreach ($layer in (@($manifest.config) + @($manifest.layers))) {
    if ($layer.digest -notmatch '^sha256:[a-f0-9]{64}$') { throw 'Invalid model layer digest' }
    $path = Join-Path $blobs ($layer.digest -replace ':', '-')
    Get-VerifiedFile "https://registry.ollama.ai/v2/library/$($lock.model_name)/blobs/$($layer.digest)" $path ($layer.digest -replace '^sha256:', '')
    if ((Get-Item -LiteralPath $path).Length -ne $layer.size) { throw 'Model layer size mismatch' }
    if ($layer.mediaType -eq 'application/vnd.ollama.image.license') { Copy-Item -LiteralPath $path -Destination (Join-Path $Destination 'MODEL-LICENSE.txt') -Force }
}
@{
    model = "$($lock.model_name):$($lock.model_tag)"; runtime = 'ollama/ollama.exe'; models = 'models'
    runtime_version = $lock.runtime_version; model_digest = $lock.model_digest
} | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $Destination 'bundle.json') -Encoding utf8NoBOM
@"
Offline Abigail bootstrap: Ollama $($lock.runtime_version) (MIT), Qwen3.5-0.8B (Apache-2.0).
Model source: https://ollama.com/library/qwen3.5:0.8b
Runtime source: https://github.com/ollama/ollama/releases/tag/$($lock.runtime_version)
Unmodified model weights, manifest, and CPU runtime. GPU libraries omitted.
See MODEL-LICENSE.txt and ollama/LICENSE* for the accompanying license terms/notices.
"@ | Set-Content -LiteralPath (Join-Path $Destination 'NOTICE.txt') -Encoding utf8NoBOM
$size = (Get-ChildItem -LiteralPath $Destination -File -Recurse | Measure-Object -Property Length -Sum).Sum
if ($size -gt 1700000000) { throw "Offline payload is $size bytes; leave space in the NSIS installer for the app and WebView2." }
Write-Host "Offline payload ready: $size bytes at $Destination"
