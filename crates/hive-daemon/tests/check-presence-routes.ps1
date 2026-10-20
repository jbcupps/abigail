<#
.SYNOPSIS
Compiles and runs current-source profile HTTP tests against a matching Rust cache.
.DESCRIPTION
Uses the real presence router, JSON extraction, profile code, and cached atomic
writer. Only registered identity lookup is synthetic. HTTP request objects go
directly through the in-process router; no sockets, daemon startup,
keyring, user Documents, model, or provider is used. This diagnostic requires the
exact coherent artifacts below and compatible rustc; it is not a clean Cargo build.
All outputs and temporary test directories stay in this worktree's target folder.
.EXAMPLE
.\crates\hive-daemon\tests\check-presence-routes.ps1 -DependencyRoot '<cached target/debug/deps>' -NativeLibraryDirectories '<cached native library directory>'
#>
param(
    [Parameter(Mandatory)][string]$DependencyRoot,
    [string[]]$NativeLibraryDirectories = @()
)
$ErrorActionPreference = 'Stop'
$worktree = (Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$DependencyRoot = (Resolve-Path -LiteralPath $DependencyRoot).Path
$output = Join-Path $worktree 'target/presence-route-check'
$originalTemp = $env:TEMP
$originalTmp = $env:TMP
Push-Location $worktree
try {
    New-Item -ItemType Directory -Path $output -Force | Out-Null
    $externMap = @{
        abigail_identity='3627762340c7e016'; hive_core='d50502afff01cf0b'
        axum='6243661a0cd11d52'
        uuid='9030ed094496c93a'; serde_json='87abf158cf5aae2f'
        tokio='ceb6ec16a2c58165'; serde='408d599948c904b8'
        abigail_core='02c84d0df20b63c8'
    }
    $testBinary = Join-Path $output 'presence-routes-tests.exe'
    $rustArgs = @('--test','--edition=2021','crates/hive-daemon/tests/presence_routes.rs',
        '-L',"dependency=$DependencyRoot",'-o',$testBinary)
    foreach ($depName in $externMap.Keys) {
        $artifact = Join-Path $DependencyRoot "lib$depName-$($externMap[$depName]).rlib"
        if (-not (Test-Path -LiteralPath $artifact -PathType Leaf)) {
            throw "Required cached artifact missing: $depName"
        }
        $rustArgs += @('--extern', "$depName=$artifact")
    }
    foreach ($nativeDirectory in $NativeLibraryDirectories) {
        $rustArgs += @('-L', "native=$((Resolve-Path -LiteralPath $nativeDirectory).Path)")
    }
    & rustc @rustArgs
    if ($LASTEXITCODE -ne 0) { throw 'Profile route test compilation failed' }
    $env:TEMP = Join-Path $output 'test-tmp'
    $env:TMP = $env:TEMP
    New-Item -ItemType Directory -Path $env:TEMP -Force | Out-Null
    & $testBinary --nocapture
    if ($LASTEXITCODE -ne 0) { throw 'Profile route tests failed' }
} finally {
    $env:TEMP = $originalTemp
    $env:TMP = $originalTmp
    Pop-Location
}
