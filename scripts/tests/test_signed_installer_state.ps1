#requires -Version 7.0
<# Validates prepared-input rejection without builds, installs, or cloud signing. #>
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$builder = Join-Path $repoRoot 'scripts/build_signed_installer.ps1'
$tokens = $null
$errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile($builder, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Signed installer builder contains PowerShell syntax errors.' }
# Load only pure function definitions. The actual phase body cannot execute.
foreach ($definition in $ast.FindAll({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] }, $false)) {
    . ([scriptblock]::Create($definition.Extent.Text))
}
$actualSource = Get-SourceState
$actualCommit = (@(& git -C $repoRoot rev-parse HEAD))[0]
if ($actualSource.commit -ne $actualCommit -or $actualSource.commit.Length -ne 40) {
    throw 'Preparation must retain the entire source commit, not its first character.'
}
Write-Host 'PASS full source commit and source-state hash'
$testSource = $actualSource
function Get-SourceState { $testSource }
$Version = '0.0.75'
$Compression = 'zlib'
$resources = @('resources/abigail-entity-runtime-app.exe', 'resources/hive-daemon.exe', 'resources/entity-daemon.exe')
$testRoot = Join-Path $repoRoot ('target/manual-test/signed-builder-state-' + [guid]::NewGuid().ToString('N').Substring(0, 8))
New-Item -ItemType Directory -Path $testRoot | Out-Null
$configPath = Join-Path $testRoot 'config.json'
$config = @{
    version = $Version; mainBinaryName = 'Abigail'
    bundle = @{ targets = @('nsis'); createUpdaterArtifacts = $false; resources = $resources }
}
Write-BuildJson $config $configPath
$binaryInputs = @(foreach ($index in 0..3) {
    $source = Join-Path $testRoot "source-$index.exe"
    $prepared = Join-Path $testRoot "prepared-$index.exe"
    [IO.File]::WriteAllText($source, "synthetic prepared bytes $index")
    Copy-Item -LiteralPath $source -Destination $prepared
    [pscustomobject]@{ source_path = $source; prepared_path = $prepared; relative_path = "payload-$index.exe" }
})
$state = [pscustomobject]@{
    sourceCommit = $testSource.commit; source = $testSource; version = $Version; compression = $Compression
    configSha256 = (Get-FileHash -LiteralPath $configPath -Algorithm SHA256).Hash
    binaries = @($binaryInputs | ForEach-Object {
        [pscustomobject]@{
            source_path = $_.source_path; prepared_path = $_.prepared_path; relative_path = $_.relative_path
            sha256 = (Get-FileHash -LiteralPath $_.source_path -Algorithm SHA256).Hash
        }
    })
}
function Copy-TestState { $state | ConvertTo-Json -Depth 20 | ConvertFrom-Json }
function Assert-Rejected([string]$Name, [scriptblock]$Action, [string]$Message) {
    $rejected = $false
    try { & $Action } catch {
        if ($_.Exception.Message -notmatch $Message) { throw }
        $rejected = $true
    }
    if (-not $rejected) { throw "$Name was incorrectly accepted." }
    Write-Host "PASS rejected $Name"
}
Assert-PreparedState $state
Write-Host 'PASS unchanged prepared inputs'
$mutated = Copy-TestState
$mutated.sourceCommit = '0' * 40
Assert-Rejected 'different source commit' { Assert-PreparedState $mutated } 'Source changed'
$mutated = Copy-TestState
$mutated.source.sha256 = '0' * 64
Assert-Rejected 'changed source bytes' { Assert-PreparedState $mutated } 'Source changed'
$mutated = Copy-TestState
$mutated.version = '0.0.76'
Assert-Rejected 'different version' { Assert-PreparedState $mutated } 'version or compression'
$mutated = Copy-TestState
$mutated.compression = 'lzma'
Assert-Rejected 'different compression' { Assert-PreparedState $mutated } 'version or compression'
$mutated = Copy-TestState
$mutated.binaries = @($mutated.binaries | Select-Object -First 3)
Assert-Rejected 'missing executable' { Assert-PreparedState $mutated } 'Exactly four'
$mutated = Copy-TestState
$mutated.binaries[1].source_path = $mutated.binaries[0].source_path
Assert-Rejected 'substituted executable path' { Assert-PreparedState $mutated } 'paths/order'
$mutated = Copy-TestState
$mutated.binaries[0].sha256 = 'not-a-hash'
Assert-Rejected 'invalid recorded hash' { Assert-PreparedState $mutated } 'paths/order'
foreach ($path in @($binaryInputs[0].source_path, $binaryInputs[0].prepared_path)) {
    $original = [IO.File]::ReadAllBytes($path)
    try {
        [IO.File]::WriteAllText($path, 'changed synthetic bytes')
        Assert-Rejected 'changed executable bytes' { Assert-PreparedState $state } 'bytes changed'
    } finally { [IO.File]::WriteAllBytes($path, $original) }
}
$originalConfig = [IO.File]::ReadAllBytes($configPath)
try {
    Add-Content -LiteralPath $configPath ' '
    Assert-Rejected 'changed bundle configuration' { Assert-PreparedState $state } 'configuration changed'
} finally { [IO.File]::WriteAllBytes($configPath, $originalConfig) }
Assert-PreparedState $state
Write-Host 'PASS restored prepared inputs'
Write-Host 'Signed installer state tests passed without building, installing, or signing.'
