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
$work = $testRoot
$fixtureModule = Join-Path $PSScriptRoot 'windows_runtime_fixtures.mjs'
function Write-TestPe([string]$Path, [string]$Import = 'KERNEL32.dll') {
    $fixtureWriter = @'
import { writeFileSync } from 'node:fs';
import { pathToFileURL } from 'node:url';
const { createPeFixture } = await import(pathToFileURL(process.argv[2]).href);
writeFileSync(process.argv[3], createPeFixture({ imports: [process.argv[4]], delayImports: [] }));
'@
    & node --input-type=module -e $fixtureWriter -- signed-state-fixture-writer $fixtureModule $Path $Import
    if ($LASTEXITCODE -ne 0) { throw 'Could not write the synthetic Windows executable fixture.' }
}
$configPath = Join-Path $testRoot 'config.json'
$config = @{
    version = $Version; mainBinaryName = 'Abigail'
    bundle = @{ targets = @('nsis'); createUpdaterArtifacts = $false; resources = $resources }
}
Write-BuildJson $config $configPath
$binaryInputs = @(foreach ($index in 0..3) {
    $source = Join-Path $testRoot "source-$index.exe"
    $prepared = Join-Path $testRoot "prepared-$index.exe"
    Write-TestPe $source
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
$originalSource = [IO.File]::ReadAllBytes($binaryInputs[0].source_path)
$originalPrepared = [IO.File]::ReadAllBytes($binaryInputs[0].prepared_path)
try {
    # Matching file hashes must not make an undeployed CRT dependency safe.
    Write-TestPe $binaryInputs[0].source_path 'VCRUNTIME140.dll'
    Copy-Item -LiteralPath $binaryInputs[0].source_path -Destination $binaryInputs[0].prepared_path -Force
    $mutated = Copy-TestState
    $mutated.binaries[0].sha256 = (Get-FileHash -LiteralPath $binaryInputs[0].source_path -Algorithm SHA256).Hash
    Assert-Rejected 'undeployed runtime import with matching hashes' { Assert-PreparedState $mutated } 'node failed with exit code'
    $runtimeReport = Get-Content -LiteralPath (Join-Path $work 'windows-runtime.json') -Raw | ConvertFrom-Json
    $changedExecutable = @($runtimeReport.executables | Where-Object { $_.path -eq $binaryInputs[0].prepared_path })
    if ($runtimeReport.passed -ne $false -or $changedExecutable.Count -ne 1 -or
        $changedExecutable[0].sha256 -ne $mutated.binaries[0].sha256 -or
        $changedExecutable[0].requires_external_vc_runtime -ne $true -or
        'VCRUNTIME140.dll' -notin @($changedExecutable[0].external_vc_runtime) -or
        @($runtimeReport.errors | Where-Object { $_.code -eq 'EXTERNAL_VC_RUNTIME' }).Count -ne 1) {
        throw 'The real import checker did not reject the exact synthetic CRT-dependent bytes.'
    }
} finally {
    [IO.File]::WriteAllBytes($binaryInputs[0].source_path, $originalSource)
    [IO.File]::WriteAllBytes($binaryInputs[0].prepared_path, $originalPrepared)
}
$originalConfig = [IO.File]::ReadAllBytes($configPath)
try {
    Add-Content -LiteralPath $configPath ' '
    Assert-Rejected 'changed bundle configuration' { Assert-PreparedState $state } 'configuration changed'
} finally { [IO.File]::WriteAllBytes($configPath, $originalConfig) }
foreach ($variant in @('updater', 'other-target', 'other-launcher', 'missing-resource')) {
    try {
        $invalidConfig = $config | ConvertTo-Json -Depth 8 | ConvertFrom-Json -AsHashtable
        switch ($variant) {
            'updater' { $invalidConfig.bundle.createUpdaterArtifacts = $true }
            'other-target' { $invalidConfig.bundle.targets = @('msi') }
            'other-launcher' { $invalidConfig.mainBinaryName = 'Other' }
            'missing-resource' { $invalidConfig.bundle.resources = @($resources | Select-Object -First 2) }
        }
        Write-BuildJson $invalidConfig $configPath
        $mutated = Copy-TestState
        $mutated.configSha256 = (Get-FileHash -LiteralPath $configPath -Algorithm SHA256).Hash
        Assert-Rejected "semantically invalid $variant configuration" { Assert-PreparedState $mutated } 'one-app NSIS release'
    } finally { [IO.File]::WriteAllBytes($configPath, $originalConfig) }
}
Assert-PreparedState $state
Write-Host 'PASS restored prepared inputs'
# Exercise the real phase entrypoint with synthetic secrets. It must stop before
# discovering tools or touching build files, and must not print secret values.
$blockedWork = Join-Path $testRoot 'blocked-secret-prepare'
$passwordSentinel = 'synthetic-password-must-not-be-printed'
$seedSentinel = 'synthetic-otp-seed-must-not-be-printed'
$start = [Diagnostics.ProcessStartInfo]::new()
$start.FileName = (Get-Command pwsh).Source
$start.UseShellExecute = $false
$start.RedirectStandardOutput = $true
$start.RedirectStandardError = $true
foreach ($argument in @('-NoProfile', '-NonInteractive', '-File', $builder, '-Phase', 'Prepare', '-WorkDir', $blockedWork)) {
    $start.ArgumentList.Add($argument)
}
foreach ($name in @('ESIGNER_USERNAME', 'ESIGNER_PASSWORD', 'ESIGNER_CREDENTIAL_ID', 'ESIGNER_TOTP_SECRET', 'WINDOWS_CERTIFICATE_THUMBPRINT')) {
    $null = $start.Environment.Remove($name)
}
$start.Environment['ESIGNER_PASSWORD'] = $passwordSentinel
$start.Environment['ESIGNER_TOTP_SECRET'] = $seedSentinel
$process = [Diagnostics.Process]::Start($start)
try {
    if (-not $process.WaitForExit(15000)) {
        $process.Kill($true)
        throw 'Credential-free preparation guard did not stop promptly.'
    }
    $output = $process.StandardOutput.ReadToEnd() + $process.StandardError.ReadToEnd()
    if ($process.ExitCode -eq 0 -or $output -notmatch 'Prepare must run without signing passwords or TOTP secrets' -or
        $output.Contains($passwordSentinel) -or $output.Contains($seedSentinel) -or (Test-Path -LiteralPath $blockedWork)) {
        throw 'Preparation did not reject signing secrets before builds without exposing their values.'
    }
} finally { $process.Dispose() }
Write-Host 'PASS real Prepare entrypoint rejects synthetic signing secrets before building'
Write-Host 'Signed installer state tests passed without building, installing, or signing.'
