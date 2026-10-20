#requires -Version 7.0
<# Credential-free policy, orchestration and injected boundary checks.
   Never invokes an installer, normal product binary, native signature verifier,
   process inventory, registry/shortcut API or OS credential operation. #>
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
. (Join-Path $repo 'scripts/preview_candidate_policy.ps1')
$runner = Join-Path $repo 'scripts/run-preview-candidate.ps1'
$tokens = $null
$errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile($runner, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Runner PowerShell syntax errors.' }
# Extract definitions only; Execute entry point is never invoked by these tests.
foreach ($definition in $ast.FindAll({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] }, $false)) {
    # Rebind only the source-location automatic variable, which is empty in a
    # dynamically extracted definition. Its real runner value is scripts/.
    $definitionText = $definition.Extent.Text.Replace('$PSScriptRoot', ("'{0}'" -f (Join-Path $repo 'scripts').Replace("'", "''")))
    . ([scriptblock]::Create($definitionText))
}
$script:checks = 0
function Assert-Test([bool]$Condition, [string]$Name) {
    if (-not $Condition) { throw "FAIL $Name" }
    $script:checks++
}
function Assert-Rejected([string]$Name, [scriptblock]$Action, [string]$Pattern) {
    $caught = $false
    try { & $Action } catch { if ($_.Exception.Message -notmatch $Pattern) { throw }; $caught = $true }
    Assert-Test $caught $Name
}
function Copy-TestObject($Value) { ConvertTo-Json $Value -Depth 20 | ConvertFrom-Json }
$baseline = Get-Content -LiteralPath (Join-Path $repo 'scripts/preview-candidate-baseline.json') -Raw | ConvertFrom-Json
$product = $baseline.productSourceCommit
$harness = 'a' * 40
$context = @{
    GITHUB_ACTIONS = 'true'; RUNNER_ENVIRONMENT = 'github-hosted'; RUNNER_OS = 'Windows'
    GITHUB_REPOSITORY = 'jbcupps/abigail'; GITHUB_RUN_ID = '123'; GITHUB_RUN_ATTEMPT = '1'
    GITHUB_SHA = $harness; RUNNER_TEMP = 'D:\a\_temp'; GITHUB_WORKSPACE = 'D:\a\abigail\abigail'
}
Assert-CandidateHostedContext $context $true $harness
Assert-Test $true 'reviewed hosted context accepted'
Assert-Rejected 'approval required' { Assert-CandidateHostedContext $context $false $harness } 'approval'
foreach ($field in @('GITHUB_ACTIONS', 'RUNNER_ENVIRONMENT', 'RUNNER_OS', 'GITHUB_REPOSITORY', 'GITHUB_RUN_ID', 'GITHUB_RUN_ATTEMPT', 'GITHUB_SHA', 'RUNNER_TEMP', 'GITHUB_WORKSPACE')) {
    $bad = $context.Clone(); $bad[$field] = ''
    Assert-Rejected "missing $field" { Assert-CandidateHostedContext $bad $true $harness } 'hosted|path|run identity'
}
$bad = $context.Clone(); $bad.RUNNER_ENVIRONMENT = 'self-hosted'
Assert-Rejected 'self-hosted rejected' { Assert-CandidateHostedContext $bad $true $harness } 'hosted'
$bad = $context.Clone(); $bad.RUNNER_TEMP = 'D:\'
Assert-Rejected 'volume-root temp rejected' { Assert-CandidateHostedContext $bad $true $harness } 'volume root'
Assert-Test (Test-CandidateWithin 'D:\a\_temp\fixture\install' 'D:\a\_temp') 'owned child path'
Assert-Test (-not (Test-CandidateWithin 'D:\a\_temp-other\fixture' 'D:\a\_temp')) 'prefix sibling is not contained'
Assert-Test (-not (Test-CandidateWithin 'D:\a\_temp' 'D:\a\_temp')) 'root is not own child'
foreach ($path in @('\\server\share\x', 'D:\a\file:stream', 'D:\a\trailing.', 'D:\a\trailing ', 'D:\a\*\x', 'relative')) {
    Assert-Rejected "unsafe path $path" { Get-CandidateFullPath $path } 'path'
}
Assert-Rejected 'overlap rejected' { Assert-CandidateSeparatePaths 'D:\a\one' 'D:\a\one\child' } 'nonoverlapping'
Assert-CandidateSeparatePaths 'D:\a\one' 'D:\a\two'
Assert-Test $true 'separate roots accepted'

$script:webviewReads = [Collections.Generic.List[string]]::new()
$script:machineWebview = '152.0.1234.5'
$script:userWebview = $null
$readWebview = {
    param($hive, $view, $key, $name)
    $script:webviewReads.Add("$hive/$view")
    Assert-Test ($key -ceq 'SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}' -and $name -ceq 'pv') 'exact NSIS WebView2 key'
    if ($hive -ceq 'LocalMachine') { return $script:machineWebview }
    return $script:userWebview
}
$webview = Assert-CandidateExistingWebView $true $readWebview
Assert-Test ($webview.existingWebView2 -eq $true -and ($script:webviewReads -join ',') -ceq 'LocalMachine/Registry32') 'x64 NSIS machine WebView2 branch'
$script:machineWebview = ''; $script:userWebview = '152.0.1234.6'; $script:webviewReads.Clear()
$null = Assert-CandidateExistingWebView $true $readWebview
Assert-Test (($script:webviewReads -join ',') -ceq 'LocalMachine/Registry32,CurrentUser/Registry64') 'x64 NSIS user fallback view'
$script:webviewReads.Clear()
$null = Assert-CandidateExistingWebView $false $readWebview
Assert-Test (($script:webviewReads -join ',') -ceq 'LocalMachine/Registry32,CurrentUser/Registry32') 'x86 NSIS fallback view'
foreach ($version in @('', '0.0.0.0', 'unparseable', 123)) {
    $script:userWebview = $version
    Assert-Rejected 'missing/invalid WebView blocks bootstrap' { Assert-CandidateExistingWebView $true $readWebview } 'existing WebView2'
}

# Model actual f7 signing JSON: PowerShell Get-FileHash emits uppercase.
$stamp = [pscustomobject]@{
    channel = 'preview'; buildProfile = 'preview'; productName = 'Abigail Preview'
    launcherFileName = 'Abigail Preview.exe'; hiveIdentifier = 'com.abigail.preview'
    runtimeIdentifier = 'com.abigail.preview.entity-runtime'
    hiveOverlaySha256 = 'B' * 64; runtimeOverlaySha256 = 'C' * 64; launchContractSha256 = 'D' * 64
}
$payload = @('Abigail Preview.exe', 'resources/abigail-entity-runtime-app.exe', 'resources/hive-daemon.exe', 'resources/entity-daemon.exe') |
    ForEach-Object { [pscustomobject]@{ relative_path = $_; sha256 = 'E' * 64 } }
$manifest = [pscustomobject]@{
    signed = $true; configuration = 'release'; version = $baseline.version; source_commit = $product
    source_state_sha256 = 'F' * 64; packaging = $stamp; installer_sha256 = 'A' * 64; payload = $payload
}
$signatures = [pscustomobject]@{
    sourceCommit = $product; sourceStateSha256 = 'F' * 64; packaging = $stamp
    expectedThumbprint = $baseline.certificateThumbprint; publicationEligible = $false
}
$prepared = [pscustomobject]@{ sourceCommit = $product; source = @{ sha256 = 'F' * 64 }; packaging = $stamp }
Assert-CandidateMetadata $manifest $signatures $prepared $baseline $product $baseline.certificateThumbprint
Assert-Test $true 'actual signer uppercase SHA256 shape accepted'
foreach ($field in @('source_commit', 'source_state_sha256', 'installer_sha256', 'signed', 'configuration', 'version')) {
    $bad = Copy-TestObject $manifest; $bad.$field = 'wrong'
    Assert-Rejected "mutated manifest $field" { Assert-CandidateMetadata $bad $signatures $prepared $baseline $product $baseline.certificateThumbprint } 'source|SHA256|signed release'
}
foreach ($field in @('channel', 'buildProfile', 'productName', 'launcherFileName', 'hiveIdentifier', 'runtimeIdentifier', 'hiveOverlaySha256', 'runtimeOverlaySha256', 'launchContractSha256')) {
    $bad = Copy-TestObject $manifest; $bad.packaging.$field = 'wrong'
    Assert-Rejected "mutated packaging $field" { Assert-CandidateMetadata $bad $signatures $prepared $baseline $product $baseline.certificateThumbprint } 'packaging|hashes'
}
$bad = Copy-TestObject $signatures; $bad.publicationEligible = $true
Assert-Rejected 'publication eligibility rejected' { Assert-CandidateMetadata $manifest $bad $prepared $baseline $product $baseline.certificateThumbprint } 'nonpublication'
$bad = Copy-TestObject $prepared; $bad.source.sha256 = '0' * 64
Assert-Rejected 'prepared hash mismatch' { Assert-CandidateMetadata $manifest $signatures $bad $baseline $product $baseline.certificateThumbprint } 'hashes'
$badPayload = Copy-TestObject $payload; $badPayload[1].relative_path = '../other.exe'
Assert-Rejected 'payload traversal rejected' { Assert-CandidatePayloadPolicy $badPayload preview } 'four reviewed'
$badPayload = Copy-TestObject $payload; $badPayload[1].relative_path = $badPayload[0].relative_path
Assert-Rejected 'duplicate payload rejected' { Assert-CandidatePayloadPolicy $badPayload preview } 'four reviewed'
$badPayload = Copy-TestObject $payload; $badPayload[0].relative_path = 'abigail preview.exe'
Assert-Rejected 'payload casing rejected' { Assert-CandidatePayloadPolicy $badPayload preview } 'four reviewed'
Assert-Rejected 'missing payload rejected' { Assert-CandidatePayloadPolicy @($payload | Select-Object -First 3) preview } 'Exactly four'
Assert-CandidatePayloadPolicy $baseline.stable.payload stable
Assert-Test $true 'reviewed stable payload accepted'

$owned = [pscustomobject]@{ hive = 'CurrentUser'; kind = 'uninstall'; values = @{ InstallLocation = '"D:\a\fixture\install"'; UninstallString = '"D:\a\fixture\install\uninstall.exe"' } }
Assert-CandidateRegistrationOwnership @($owned) 'D:\a\fixture\install' $true
Assert-Test $true 'owned registration accepted'
$bad = Copy-TestObject $owned; $bad.values.InstallLocation = 'C:\Users\real\Abigail'
Assert-Rejected 'foreign install registration rejected' { Assert-CandidateRegistrationOwnership @($bad) 'D:\a\fixture\install' $true } 'outside'
$bad = Copy-TestObject $owned; $bad.hive = 'LocalMachine'
Assert-Rejected 'machine registration rejected' { Assert-CandidateRegistrationOwnership @($bad) 'D:\a\fixture\install' $true } 'current-user'
$bad = Copy-TestObject $owned; $bad.values.UninstallString += ' /extra'
Assert-Rejected 'uninstaller arguments rejected' { Assert-CandidateRegistrationOwnership @($bad) 'D:\a\fixture\install' $true } 'exact owned'
Assert-Rejected 'missing registration rejected' { Assert-CandidateRegistrationOwnership @() 'D:\a\fixture\install' $true } 'not created'

$seen = [Collections.Generic.List[string]]::new()
Invoke-CandidateAcceptanceSequence { param($step) $seen.Add($step) }
Assert-Test ($seen.Count -eq 20) 'complete sequence visited'
Assert-Test ($seen.IndexOf('lifecycle-seed') -lt $seen.IndexOf('reinstall-preview')) 'seed stopped before reinstall'
Assert-Test ($seen.IndexOf('compare-after-reinstall') -lt $seen.IndexOf('lifecycle-verify')) 'stable preserved before reboot'
Assert-Test ($seen.IndexOf('uninstall-preview') -lt $seen.IndexOf('lifecycle-stable-return')) 'stable return after preview removal'
Assert-Test ($seen.IndexOf('lifecycle-stable-return') -lt $seen.IndexOf('uninstall-stable')) 'stable return before stable removal'
foreach ($failure in @('cold-process-guard', 'reinstall-preview', 'compare-after-preview-uninstall')) {
    $seen = [Collections.Generic.List[string]]::new()
    Assert-Rejected "failure stops sequence at $failure" {
        Invoke-CandidateAcceptanceSequence { param($step) $seen.Add($step); if ($step -eq $failure) { throw 'injected failure' } }
    } 'injected'
    Assert-Test ($seen[-1] -ceq $failure -and -not $seen.Contains('cleanup-owned-fixture')) 'no cleanup after uncertain failure'
}
$lifecycle = [pscustomobject]@{
    schema_version = 1; kind = 'hosted-preview-candidate-lifecycle'
    phase = 'seed'; passed = $true; executed = $true; processes_quiesced = $true
    product_source = $product; stable_source = $baseline.stable.sourceCommit
    assertions = @(@('stable-first-start', 'preview-second-start', 'stable-synthetic-family',
        'preview-synthetic-family-and-routing-guards', 'preview-acknowledged-shutdown', 'stable-owned-job-stop') |
        ForEach-Object { [pscustomobject]@{ name = $_; passed = $true; private_log = 'must never escape' } })
    private_log = 'must never escape'
}
$public = ConvertTo-CandidatePublicLifecycle $lifecycle seed
Assert-Test ((ConvertTo-Json $public -Compress) -notmatch 'must never escape|private_log') 'private lifecycle data cannot escape'
$bad = Copy-TestObject $lifecycle; $bad.processes_quiesced = $false
Assert-Rejected 'live lifecycle report rejected' { ConvertTo-CandidatePublicLifecycle $bad seed } 'shutdown'
$bad = Copy-TestObject $lifecycle; $bad.assertions[0].passed = $false
Assert-Rejected 'failed phase assertion rejected' { ConvertTo-CandidatePublicLifecycle $bad seed } 'failed assertion'
Assert-Rejected 'wrong phase rejected' { ConvertTo-CandidatePublicLifecycle $lifecycle verify } 'phase mismatch'
foreach ($field in @('product_source', 'stable_source')) {
    $bad = Copy-TestObject $lifecycle; $bad.$field = '0' * 40
    Assert-Rejected "stale lifecycle $field" { ConvertTo-CandidatePublicLifecycle $bad seed } 'source pins'
}
$bad = Copy-TestObject $lifecycle; $bad.assertions[0].name = 'unknown-sensitive-name'
Assert-Rejected 'unreviewed assertion name rejected' { ConvertTo-CandidatePublicLifecycle $bad seed } 'name or order'
$bad = Copy-TestObject $lifecycle; $bad.assertions = @($bad.assertions | Select-Object -First 5)
Assert-Rejected 'missing phase assertion rejected' { ConvertTo-CandidatePublicLifecycle $bad seed } 'coverage'
$bad = Copy-TestObject $lifecycle; $bad.schema_version = 2
Assert-Rejected 'unknown report schema rejected' { ConvertTo-CandidatePublicLifecycle $bad seed } 'schema'
Assert-Test (@($public.assertions).Count -eq 6 -and $public.assertions[0].name -ceq 'stable-first-start') 'reviewed assertions preserved publicly'
$bad = Copy-TestObject $lifecycle; $bad.passed = $false; $bad.processes_quiesced = $false
$bad.assertions = @($bad.assertions | Select-Object -First 2); $bad.assertions[1].passed = $false
$failurePublic = ConvertTo-CandidatePublicLifecycle $bad seed -AllowFailed
Assert-Test ($failurePublic.passed -eq $false -and $failurePublic.processesQuiesced -eq $false -and
    $failurePublic.failurePoint -ceq 'preview-second-start' -and $failurePublic.assertions.Count -eq 2) 'failed known assertion prefix retained'
Assert-Test ((ConvertTo-Json $failurePublic -Depth 5) -notmatch 'must never escape|private_log') 'failed evidence remains sanitized'
$bad.assertions[0].passed = $false
Assert-Rejected 'nonterminal failed assertion rejected' { ConvertTo-CandidatePublicLifecycle $bad seed -AllowFailed } 'failed assertion'

# Files here are text fixtures, never executables. Native/process functions below
# are injected mocks before exercising real installer-boundary functions.
$testRoot = Join-Path $repo ('target/manual-test/preview-runner-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $testRoot | Out-Null
try {
    $synthetic = Join-Path $testRoot 'synthetic-installer.txt'
    [IO.File]::WriteAllText($synthetic, 'not executable; boundary fixture')
    $hash = (Get-FileHash -LiteralPath $synthetic -Algorithm SHA256).Hash
    Assert-CandidateHash $synthetic $hash
    Assert-Test $true 'exact synthetic file hash accepted'
    Assert-Rejected 'changed bytes rejected' { Assert-CandidateHash $synthetic ('0' * 64) } 'does not match'
    $tree = Get-CandidateTreeFingerprint $testRoot
    [IO.File]::WriteAllText($synthetic, 'changed fixture')
    Assert-Rejected 'profile changes detected' { Assert-CandidateFingerprintSame $tree (Get-CandidateTreeFingerprint $testRoot) } 'fingerprint'
    $hash = (Get-FileHash -LiteralPath $synthetic -Algorithm SHA256).Hash
    $script:calls = [Collections.Generic.List[string]]::new()
    $script:processBlocked = $false
    $script:installerWaits = $true
    $script:registered = @()
    function Assert-CandidateNoProductProcesses { $script:calls.Add('process-guard'); if ($script:processBlocked) { throw 'injected running product' } }
    function Assert-CandidateHostPrerequisites { $script:calls.Add('prerequisites'); @{ existingWebView2 = $true } }
    function Assert-CandidateNativeSignature { param($Path) $script:calls.Add('signature'); @{ sha256 = $hash } }
    function Get-CandidateRegistrations { param($Product) return $script:registered }
    function Start-Process {
        param($FilePath, $ArgumentList, [switch]$PassThru, $WindowStyle)
        $script:calls.Add('start-mock')
        Assert-Test ($FilePath -ceq $synthetic -and $ArgumentList -ceq "/S /D=$testRoot\install") 'exact silent NSIS command with /D last'
        $fake = [pscustomobject]@{ ExitCode = 0 }
        $fake | Add-Member ScriptMethod WaitForExit { param($timeout) return $script:installerWaits }
        $fake | Add-Member ScriptMethod Dispose {}
        return $fake
    }
    Invoke-CandidateInstaller $synthetic $hash (Join-Path $testRoot 'install') 'Abigail Preview'
    Assert-Test (($script:calls -join ',') -ceq 'process-guard,prerequisites,signature,start-mock,process-guard') 'process and prerequisite guards surround mocked installer'
    $script:calls.Clear(); $script:processBlocked = $true
    Assert-Rejected 'live process prevents installer' { Invoke-CandidateInstaller $synthetic $hash (Join-Path $testRoot 'install') 'Abigail Preview' } 'running product'
    Assert-Test (-not $script:calls.Contains('start-mock')) 'no installer after process refusal'
    $script:processBlocked = $false; $script:calls.Clear()
    $script:registered = @($owned)
    Assert-Rejected 'existing registration prevents initial install' { Invoke-CandidateInstaller $synthetic $hash (Join-Path $testRoot 'install') 'Abigail Preview' } 'fresh path'
    Assert-Test (-not $script:calls.Contains('start-mock')) 'no installer after registration refusal'
    $script:registered = @(); $script:calls.Clear(); $script:installerWaits = $false
    Assert-Rejected 'uncertain installer wait stops operation' { Invoke-CandidateInstaller $synthetic $hash (Join-Path $testRoot 'install') 'Abigail Preview' } 'outcome is uncertain'
    Assert-Test (($script:calls -join ',') -ceq 'process-guard,prerequisites,signature,start-mock') 'uncertain installer never advances to cleanup'

    $script:fixtureRoot = $testRoot
    $script:ownedRecoveryCli = $synthetic
    $script:previewInstall = Join-Path $testRoot 'install-preview'
    $script:stableInstall = Join-Path $testRoot 'install-stable'
    $RecoveryCliSha256 = $hash
    $ProductSourceCommit = $product
    $script:report = [ordered]@{ lifecycle = @() }
    New-Item -ItemType Directory -Path (Join-Path $testRoot 'control') | Out-Null
    $script:failedPhase = Copy-TestObject $lifecycle
    $script:failedPhase.passed = $false
    $script:failedPhase.processes_quiesced = $false
    $script:failedPhase.assertions = @($script:failedPhase.assertions | Select-Object -First 2)
    $script:failedPhase.assertions[1].passed = $false
    function node {
        # Inject an already sanitized failed child report; never launch Node/app.
        $reportIndex = [Array]::IndexOf($args, '--report') + 1
        $script:failedPhase | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $args[$reportIndex]
        $global:LASTEXITCODE = 1
    }
    try {
        Assert-Rejected 'nonzero lifecycle exit fails after evidence capture' { Invoke-CandidateLifecycle seed } 'reviewed assertion prefix'
        Assert-Test ($script:report.lifecycle.Count -eq 1 -and
            $script:report.lifecycle[0].failurePoint -ceq 'preview-second-start') 'actual lifecycle boundary retains failed known assertion'
    } finally { $global:LASTEXITCODE = 0 }
} finally {
    $resolved = [IO.Path]::GetFullPath($testRoot)
    if (-not (Test-CandidateWithin $resolved (Join-Path $repo 'target/manual-test'))) { throw 'Test cleanup containment failure.' }
    Assert-CandidateNoLinks $resolved
    Remove-Item -LiteralPath $resolved -Recurse -Force
}

# Running only the ordinary default path is safe and proves it needs no artifacts.
$plan = & $runner -CandidateArtifactDir 'this path does not exist' | ConvertFrom-Json
Assert-Test ($plan.mode -ceq 'plan' -and $plan.executed -eq $false) 'default is read-only plan'
Write-Host "PASS $script:checks credential-free preview candidate runner checks."
