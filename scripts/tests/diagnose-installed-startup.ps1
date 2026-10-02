#requires -Version 7.0
<# Temporary hosted diagnostic. Reuses an existing signed installer; no build or signing. #>
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$DownloadDir,
    [Parameter(Mandatory)][string]$ReportDir
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if ($env:OS -ne 'Windows_NT') { throw 'This diagnostic requires Windows.' }
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$expectedSource = '93d96f11adb8f1094208e281ad46755b877c5a03'
$expectedThumbprint = '69BE78FD201A59A29C46F5D37097E0C6DDA04A41'
$DownloadDir = [IO.Path]::GetFullPath($DownloadDir)
$ReportDir = [IO.Path]::GetFullPath($ReportDir)
$targetPrefix = [IO.Path]::GetFullPath((Join-Path $repoRoot 'target/manual-test')).TrimEnd('\') + '\'
$reportPrefix = [IO.Path]::GetFullPath((Join-Path $repoRoot 'artifacts')).TrimEnd('\') + '\'
if (-not $DownloadDir.StartsWith($targetPrefix, [StringComparison]::OrdinalIgnoreCase) -or
    -not $ReportDir.StartsWith($reportPrefix, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'Diagnostic downloads and selected reports must stay in their designated checkout directories.'
}
New-Item -ItemType Directory -Force -Path $ReportDir | Out-Null

# Load precisely the existing bounded exporter functions, never the builder's phase body.
$tokens = $null
$parseErrors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile((Join-Path $repoRoot 'scripts/build_signed_installer.ps1'), [ref]$tokens, [ref]$parseErrors)
if ($parseErrors.Count) { throw 'The existing diagnostic exporter contains syntax errors.' }
foreach ($name in @('Write-BuildJson', 'Assert-DiagnosticPath', 'Copy-ContractDiagnostic', 'Export-ContractDiagnostics')) {
    $definitions = @($ast.FindAll({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name }, $false))
    if ($definitions.Count -ne 1) { throw 'The required existing diagnostic exporter function is missing or ambiguous.' }
    . ([scriptblock]::Create($definitions[0].Extent.Text))
}
Import-Module (Join-Path $repoRoot 'scripts/windows-signing.psm1') -Force
$summary = [ordered]@{
    kind = 'same installed signed bytes; deep/short/deep synthetic startup comparison'
    source_run = 37075547249; artifact_source_commit = $expectedSource
    diagnostic_source_commit = [string](@(& git -C $repoRoot rev-parse HEAD)[0])
    started_utc = [DateTime]::UtcNow.ToString('o'); passed = $false
    setup_verified = $false; signing_or_real_model_requests = $false; runs = @()
}
$phase = 'candidate metadata'
try {
    $run = Get-Content -LiteralPath (Join-Path $DownloadDir 'run.json') -Raw | ConvertFrom-Json
    if ($run.id -ne 37075547249 -or $run.head_sha -ne $expectedSource -or $run.conclusion -ne 'success') {
        throw 'Candidate run metadata does not match the pinned successful source.'
    }
    $validationRoot = Join-Path $DownloadDir 'abigail-installer-validation-windows-latest'
    $manifestPath = Join-Path $validationRoot 'mvp-build.json'
    $manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
    $signatures = Get-Content -LiteralPath (Join-Path $DownloadDir 'abigail-signatures-windows-latest/windows-signatures.json') -Raw | ConvertFrom-Json
    $expectedPaths = @('Abigail.exe', 'resources/abigail-entity-runtime-app.exe', 'resources/hive-daemon.exe', 'resources/entity-daemon.exe')
    if ($manifest.source_commit -ne $expectedSource -or $manifest.version -ne '0.0.76' -or -not $manifest.signed -or
        @($manifest.payload).Count -ne 4 -or
        (@($manifest.payload.relative_path | Sort-Object) -join '|') -ne (@($expectedPaths | Sort-Object) -join '|') -or
        $signatures.sourceCommit -ne $expectedSource -or $signatures.expectedThumbprint -ne $expectedThumbprint -or
        @($signatures.artifacts | Where-Object { $_.certificateThumbprint -ne $expectedThumbprint }).Count -or
        @($signatures.artifacts.sha256 | Sort-Object -Unique).Count -ne 6) {
        throw 'Candidate manifest and signature evidence are inconsistent.'
    }
    $sourceLatest = Get-Content -LiteralPath (Join-Path $validationRoot 'contract/latest.json') -Raw | ConvertFrom-Json
    $sourceRunName = [IO.Path]::GetFileName($sourceLatest.run_dir)
    if ($sourceRunName -notmatch '^\d{4}-\d{2}-\d{2}T[0-9TZ-]+-[0-9a-f]{8}$') { throw 'Candidate contract index is invalid.' }
    $sourceContract = Get-Content -LiteralPath (Join-Path $validationRoot "contract/$sourceRunName/result.json") -Raw | ConvertFrom-Json
    if (-not $sourceContract.passed -or @($sourceContract.stages).Count -ne 12 -or
        @($sourceContract.stages | Where-Object { -not $_.passed }).Count) {
        throw 'The downloaded candidate did not pass its original twelve-stage contract.'
    }
    $installers = @(Get-ChildItem -LiteralPath (Join-Path $DownloadDir 'abigail-installer-windows-latest') -File -Filter '*.exe')
    if ($installers.Count -ne 1) { throw 'Expected exactly one downloaded signed installer.' }
    $installer = $installers[0].FullName
    $phase = 'installer signature'
    $installerSignature = Assert-AbigailSignature -Path $installer -Thumbprint $expectedThumbprint
    if ($installerSignature.sha256 -ne $manifest.installer_sha256 -or
        $installerSignature.sha256 -notin $signatures.artifacts.sha256) { throw 'Installer bytes do not match the verified candidate.' }
    $installId = [DateTime]::UtcNow.ToString('yyyyMMddTHHmmssZ') + '-' + [guid]::NewGuid().ToString('N').Substring(0, 8)
    $installDir = Join-Path $repoRoot "target/manual-test/startup-installed-$installId"
    $phase = 'installed payload and daemon loader verification'
    & (Join-Path $repoRoot 'scripts/verify-mvp-windows.ps1') -InstallerPath $installer -ManifestPath $manifestPath -InstallDir $installDir
    $phase = 'six installed and outer signatures'
    $signatureChecks = @($installerSignature)
    foreach ($relative in $expectedPaths + @('uninstall.exe')) {
        $record = Assert-AbigailSignature -Path (Join-Path $installDir $relative) -Thumbprint $expectedThumbprint
        if ($record.sha256 -notin $signatures.artifacts.sha256) { throw 'Installed signed bytes differ from the candidate signature evidence.' }
        $signatureChecks += $record
    }
    $summary.setup_verified = $true
    $summary.install_dir = $installDir
    $summary.installer_sha256 = $installerSignature.sha256
    $summary.six_signature_checks = @($signatureChecks | ForEach-Object {
        [ordered]@{ name = [IO.Path]::GetFileName($_.path); sha256 = $_.sha256; certificate_thumbprint = $_.certificateThumbprint; digest = $_.digest; timestamp = $_.timestampType }
    })
    $installedReport = Get-Content -LiteralPath "$installDir-verification/installed-payload.json" -Raw | ConvertFrom-Json
    Write-BuildJson $installedReport (Join-Path $ReportDir 'installed-payload.json')
    Write-BuildJson $summary (Join-Path $ReportDir 'comparison.json')
    $phase = 'isolated synthetic contracts'
    $layouts = @('validation/contract', 'contract-work', 'validation/contract')
    for ($index = 0; $index -lt $layouts.Count; $index++) {
        # Match the builder's actual 25-character artifact ID and CI checkout prefix.
        $runId = [DateTime]::UtcNow.ToString('yyyyMMddTHHmmssZ') + '-' + [guid]::NewGuid().ToString('N').Substring(0, 8)
        $privateRoot = Join-Path $repoRoot "target/signed-release/0.0.76/artifacts/$runId"
        $contractOutput = Join-Path $privateRoot $layouts[$index]
        $selectedRoot = Join-Path $ReportDir ('run-' + ($index + 1))
        New-Item -ItemType Directory -Force -Path $privateRoot, $selectedRoot | Out-Null
        $consolePath = Join-Path $privateRoot 'contract-console.txt'
        $watch = [Diagnostics.Stopwatch]::StartNew()
        $contractFailed = $false
        try { & (Join-Path $PSScriptRoot 'run-mvp-acceptance.ps1') -BinaryDir $installDir -OutputDir $contractOutput *> $consolePath }
        catch { $contractFailed = $true }
        $watch.Stop()
        $entry = [ordered]@{
            iteration = $index + 1; layout = $layouts[$index]; output_path_length = $contractOutput.Length
            artifact_id_length = $runId.Length; passed = $false; duration_ms = $watch.ElapsedMilliseconds
            stages = @(); diagnostics_exported = $false; cleanup_error = $false
        }
        try {
            $export = Export-ContractDiagnostics $contractOutput $selectedRoot
            $consoleBudget = @{ files = 0; bytes = 0; omitted = @(); destinationRoot = $selectedRoot }
            Copy-ContractDiagnostic $consolePath $privateRoot (Join-Path $selectedRoot 'contract-console.txt') $consoleBudget
            $export.console_files = $consoleBudget.files
            $export.console_bytes = $consoleBudget.bytes
            Write-BuildJson $export (Join-Path $selectedRoot 'diagnostic-export.json')
            $entry.diagnostics_exported = $true
        } catch { $entry.export_failed = $true }
        try {
            $latest = Get-Content -LiteralPath (Join-Path $contractOutput 'latest.json') -Raw | ConvertFrom-Json
            $resultPath = Assert-DiagnosticPath (Join-Path $latest.run_dir 'result.json') $contractOutput
            $result = Get-Content -LiteralPath $resultPath -Raw | ConvertFrom-Json
            $entry.stages = @($result.stages | ForEach-Object { [ordered]@{ name = $_.name; passed = $_.passed; duration_ms = $_.duration_ms } })
            $entry.run_path_length = ([string]$latest.run_dir).Length
            $entry.cleanup_error = [bool]($result.PSObject.Properties['cleanup_error'])
            $entry.passed = [bool](-not $contractFailed -and $result.passed -and @($result.stages).Count -eq 12 -and
                @($result.stages | Where-Object { -not $_.passed }).Count -eq 0 -and $entry.diagnostics_exported)
        } catch { $entry.result_unavailable = $true }
        $summary.runs += $entry
        Write-BuildJson $summary (Join-Path $ReportDir 'comparison.json')
        Write-Host "Synthetic layout $($index + 1): passed=$($entry.passed), output_length=$($entry.output_path_length), duration_ms=$($entry.duration_ms)"
        # Each script owns and reaps only its isolated daemons. Continue after
        # failures so this diagnostic can compare both layouts, never mark a failed run successful.
    }
    $summary.passed = $summary.runs.Count -eq 3 -and @($summary.runs | Where-Object { -not $_.passed }).Count -eq 0
} catch {
    $summary.setup_failure_phase = $phase
    Write-Warning "Startup comparison stopped during $phase; selected reports retain the failure status."
} finally {
    $summary.finished_utc = [DateTime]::UtcNow.ToString('o')
    Write-BuildJson $summary (Join-Path $ReportDir 'comparison.json')
    $selectedFiles = @(Get-ChildItem -LiteralPath $ReportDir -File -Recurse)
    if ($selectedFiles.Count -gt 205 -or @($selectedFiles | Where-Object { $_.Length -gt 2MB -or ($_.Attributes -band [IO.FileAttributes]::ReparsePoint) }).Count -or
        ($selectedFiles | Measure-Object Length -Sum).Sum -gt 64MB) {
        throw 'Selected diagnostics exceed the bounded upload policy.'
    }
}
if (-not $summary.passed) { throw 'At least one signed startup diagnostic did not pass; inspect the selected comparison report and redacted logs.' }
Write-Host 'All three signed installed twelve-stage synthetic contracts passed.'
