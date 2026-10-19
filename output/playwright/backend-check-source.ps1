<#
.SYNOPSIS
Runs focused source checks against an explicitly supplied cached Rust graph.
.DESCRIPTION
This is a cached-graph diagnostic, not a clean Cargo build or a live service
validation. The dependency hashes below deliberately identify the original
coherent cache; the supplied directory must contain those exact artifacts and
rustc must be compatible with them. No dependencies are downloaded or rebuilt.
Outputs and storage-test temporary files stay in this script's directory.
.PARAMETER Check
Storage compiles and runs the focused tests. Metadata checks daemon source only.
.PARAMETER DependencyRoot
Required path to the matching cached Rust target/debug/deps directory.
.PARAMETER NativeLibraryDirectories
Native library directories required by the cached storage graph. At least one
must be supplied for storage; metadata does not require native library paths.
.EXAMPLE
.\output\playwright\backend-check-source.ps1 -Check storage -DependencyRoot '<cached Rust dependency directory>' -NativeLibraryDirectories '<native library directory 1>', '<native library directory 2>'
.EXAMPLE
.\output\playwright\backend-check-source.ps1 -Check metadata -DependencyRoot '<cached Rust dependency directory>'
#>
param(
    [ValidateSet('storage', 'metadata')][string]$Check = 'storage',
    [Parameter(Mandatory)][ValidateNotNullOrEmpty()][string]$DependencyRoot,
    [string[]]$NativeLibraryDirectories = @()
)
$ErrorActionPreference = 'Stop'
$worktree = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
if (-not (Test-Path -LiteralPath $DependencyRoot -PathType Container)) {
    throw 'DependencyRoot must name an existing cached Rust dependency directory.'
}
$DependencyRoot = (Resolve-Path -LiteralPath $DependencyRoot).Path
if ($Check -eq 'storage') {
    if ($NativeLibraryDirectories.Count -eq 0) {
        throw 'Storage checks require explicit NativeLibraryDirectories for the cached graph.'
    }
    $NativeLibraryDirectories = @($NativeLibraryDirectories | ForEach-Object {
        if (-not (Test-Path -LiteralPath $_ -PathType Container)) {
            throw 'Each NativeLibraryDirectories entry must name an existing directory.'
        }
        (Resolve-Path -LiteralPath $_).Path
    })
}
$originalTemp = $env:TEMP
$originalTmp = $env:TMP
Push-Location $worktree
try {
    if ($Check -eq 'storage') {
        $testBinary = Join-Path $PSScriptRoot 'presence-profile-tests.exe'
        $rustArgs = @('--test', '--edition=2021', 'crates/hive-daemon/src/presence_profile.rs',
            '-L', "dependency=$DependencyRoot",
            '--extern', "serde=$DependencyRoot\libserde-2841a0cae7e5a796.rlib",
            '--extern', "serde_json=$DependencyRoot\libserde_json-0a4bff3c6ff7b427.rlib",
            '--extern', "uuid=$DependencyRoot\libuuid-f936de7fbdbb0053.rlib",
            '--extern', "abigail_core=$DependencyRoot\libabigail_core-874e7a257357a91c.rlib",
            '-o', $testBinary)
        foreach ($nativeDirectory in $NativeLibraryDirectories) {
            $rustArgs += @('-L', "native=$nativeDirectory")
        }
        & rustc @rustArgs
        if ($LASTEXITCODE -ne 0) { throw 'Storage test compilation failed' }
        $env:TEMP = Join-Path $PSScriptRoot 'test-tmp'
        $env:TMP = $env:TEMP
        New-Item -ItemType Directory -Path $env:TEMP -Force | Out-Null
        & $testBinary --nocapture
        if ($LASTEXITCODE -ne 0) { throw 'Storage tests failed' }
    } else {
        # Exact coherent cached metadata graph from fingerprint ec5fccde04a9f4cd.
        $externMap = @{
            abigail_hive='ec67ceb60616f243'; abigail_identity='7a0f2c760d9b27db'
            tokio='0c2bcb524e2d2641'; hive_core='30bf51c4edb13e0a'
            chrono='fe6ffa91f3e20425'; sha2='09ec8d2df750a63d'
            axum='073ffd74791277a0'; reqwest='778c9fd91eb42e32'
            abigail_skills='f75dce72a312fe7b'; uuid='d1388e39878e3b80'
            abigail_memory='8bd5d95c44731c35'; tower_http='d51590cdd6115744'
            serde_json='59a35750849aa43b'; anyhow='40b62db00cfc6fcc'
            tracing_subscriber='46adeb71d22525b9'; base64='99826fbb775b45ed'
            serde='3b5aaf29c8ffcad3'; tracing='c70347d6285cdcb3'
            abigail_runtime='1e427a6e72b2c2c5'; clap='33de83e4e66fae8d'
            abigail_core='dee48e810fd74a7f'; abigail_diag='4b57850949778ad6'
            abigail_capabilities='1a817132cbaad2d6'; soul_forge='19d7ae9a0869064a'
            abigail_persistence='d488753b846f373c'
        }
        $rustArgs = @('--edition=2021','--crate-name','hive_daemon','--crate-type','bin',
            '--emit','metadata','crates/hive-daemon/src/main.rs','-C','target-feature=+crt-static',
            '-L',"dependency=$dependencyRoot",'-o',(Join-Path $PSScriptRoot 'hive_daemon.rmeta'))
        foreach ($depName in $externMap.Keys) {
            $rustArgs += @('--extern', "$depName=$dependencyRoot\lib$depName-$($externMap[$depName]).rmeta")
        }
        & rustc @rustArgs
        if ($LASTEXITCODE -ne 0) { throw 'Daemon source metadata check failed' }
    }
} finally {
    $env:TEMP = $originalTemp
    $env:TMP = $originalTmp
    Pop-Location
}
