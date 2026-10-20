#requires -Version 7.0
<# Credential-free policy and guard tests. Only the extracted pure scope guard
   executes, with synthetic environment values. No workflow build, download,
   signing, installer, daemon, DPAPI or credential entrypoint is invoked. #>
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$workflow = [IO.File]::ReadAllText((Join-Path $root '.github/workflows/preview-candidate.yml')).Replace("`r`n", "`n")
$passed = 0

function Assert-Policy([string]$Text) {
    if ($Text -notmatch '(?m)^on:\n  workflow_dispatch:\n' -or $Text -match '(?m)^  (push|pull_request|pull_request_target|workflow_call|schedule|repository_dispatch|workflow_run):') {
        throw 'Only workflow_dispatch is allowed.'
    }
    if ($Text -notmatch '(?m)^permissions:\n  contents: read\n\n' -or
        @([regex]::Matches($Text, '(?m)^\s*permissions:')).Count -ne 1 -or $Text -match '(?m)^\s*(contents|actions|id-token|packages): write') {
        throw 'Only top-level contents:read is permitted.'
    }
    if (@([regex]::Matches($Text, '(?m)^    runs-on: windows-latest$')).Count -ne 1 -or
        @([regex]::Matches($Text, '(?m)^\s*runs-on:')).Count -ne 1 -or
        $Text -match 'self-hosted|vars\.ABIGAIL_WINDOWS_RUNNER') { throw 'One fixed hosted Windows runner is required.' }
    if ($Text -notmatch '(?m)^  group: abigail-windows-release-signing\n  queue: max\n  cancel-in-progress: false$') { throw 'Signing must share stable serialization.' }
    foreach ($inputName in @('approve_fixture_dpapi', 'approve_signing_quota')) {
        $input = [regex]::Match($Text, "(?ms)^      ${inputName}:\n(.*?)(?=^      [a-z_]+:|^permissions:)")
        if (-not $input.Success -or $input.Value -notmatch '(?m)^        default: false$' -or $input.Value -notmatch '(?m)^        type: boolean$') { throw 'Explicit approvals must default false.' }
    }
    $allowedActions = @(
        'actions/checkout@34e114876b0b11c390a56381ad16ebd13914f8d5',
        'actions/setup-node@48b55a011bda9f5d6aeb4c2d9c7362e8dae4041e',
        'dtolnay/rust-toolchain@e97e2d8cc328f1b50210efc529dca0028893a2d9',
        'actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a'
    )
    $actions = @([regex]::Matches($Text, '(?m)^        uses: ([^\s#]+)') | ForEach-Object { $_.Groups[1].Value })
    if ($actions.Count -ne 5 -or @($actions | Where-Object { $_ -cnotin $allowedActions }).Count) { throw 'Only reviewed pinned actions may run.' }
    $steps = @([regex]::Matches($Text, '(?ms)^      - name: ([^\n]+)\n(.*?)(?=^      - name:|\z)'))
    if ($steps[0].Groups[1].Value -cne 'Require reviewed hosted scope and explicit approvals') { throw 'Approval gate must run first.' }
    $sign = @($steps | Where-Object { $_.Value -match '(?m)^        id: sign$' })
    if ($sign.Count -ne 1) { throw 'Exactly one signing step is required.' }
    $preflight = @($steps | Where-Object { $_.Groups[1].Value -ceq 'Check harness policy and syntax without credentials' })
    $prepare = @($steps | Where-Object { $_.Groups[1].Value -ceq 'Prepare four preview binaries from clean hosted source' })
    if ($preflight.Count -ne 1 -or $prepare.Count -ne 1 -or $preflight[0].Index -ge $prepare[0].Index -or $preflight[0].Index -ge $sign[0].Index) { throw 'Credential-free preflight must precede building and signing.' }
    $runtimeInstallIndex = $prepare[0].Value.IndexOf('npm --prefix entity-runtime-app/src-ui ci', [StringComparison]::Ordinal)
    $prepareBuildIndex = $prepare[0].Value.IndexOf('./scripts/build_signed_installer.ps1 -Phase Prepare', [StringComparison]::Ordinal)
    if ($runtimeInstallIndex -lt 0 -or $runtimeInstallIndex -ge $prepareBuildIndex -or
        $prepare[0].Value -notmatch '(?m)^        working-directory: product$' -or
        -not $prepare[0].Value.Contains("if (`$LASTEXITCODE -ne 0) { throw 'Shared Runtime frontend dependency installation failed.' }")) {
        throw 'Install locked Runtime dependencies before Hive compiles shared Runtime source, and fail on installation errors.'
    }
    foreach ($command in @(
        './scripts/tests/test_preview_candidate_workflow.ps1',
        './scripts/tests/test_preview_candidate_runner.ps1',
        './scripts/tests/test_preview_owned_job.ps1',
        'node --check scripts/tests/preview-candidate-lifecycle.mjs',
        'node --test scripts/tests/test_preview_candidate_lifecycle.mjs',
        './scripts/run-preview-candidate.ps1 -CheckPrerequisites -HarnessSourceCommit $env:GITHUB_SHA'
    )) {
        if (-not $preflight[0].Value.Contains($command)) { throw 'Credential-free harness checks cannot be omitted.' }
    }
    $secretNames = @([regex]::Matches($sign[0].Value, '\$\{\{ secrets\.([A-Z_]+) \}\}') | ForEach-Object { $_.Groups[1].Value })
    $expectedSecrets = @('ESIGNER_USERNAME', 'ESIGNER_PASSWORD', 'ESIGNER_CREDENTIAL_ID', 'ESIGNER_TOTP_SECRET', 'WINDOWS_CERTIFICATE_THUMBPRINT')
    if ($secretNames.Count -ne 5 -or @(Compare-Object $expectedSecrets $secretNames).Count -or
        $Text.Replace($sign[0].Value, '') -match '\$\{\{\s*secrets\.') { throw 'Existing signing secrets belong only to the signing step.' }
    if ($Text -notmatch '(?m)^          ref: f7c7848d25726480b253ec1ca637c3ffa5ea3c77\n          path: product$' -or
        $Text -notmatch '(?m)^          ref: \$\{\{ github.sha \}\}\n          path: harness$' -or
        @([regex]::Matches($Text, '(?m)^          persist-credentials: false$')).Count -ne 2) { throw 'Product and harness checkouts must remain separate and immutable.' }
    $phases = @([regex]::Matches($Text, 'build_signed_installer\.ps1 -Phase (Prepare|Check|Sign) -Channel preview -Version 0\.0\.76 -TargetDir') | ForEach-Object { $_.Groups[1].Value })
    if (($phases -join ',') -cne 'Prepare,Check,Sign' -or @([regex]::Matches($Text, 'build_signed_installer\.ps1 -Phase')).Count -ne 3) { throw 'Prepare/Check/Sign must share the explicit preview version.' }
    if ($Text -notmatch 'install_esigner\.ps1' -or $Text -match '(?i)gh release|gh workflow|git push|git tag|action-gh-release|continue-on-error:\s*true|workflow_dispatch.*github\.token') { throw 'Signing tools/publication boundaries changed.' }
    $stableIndex = $Text.IndexOf('name: Verify fixed sources and download pinned public stable fixture', [StringComparison]::Ordinal)
    $signIndex = $Text.IndexOf('name: Sign exact prepared preview candidate', [StringComparison]::Ordinal)
    if ($stableIndex -lt 0 -or $signIndex -le $stableIndex -or $Text -notmatch 'Get-AuthenticodeSignature' -or $Text -notmatch 'Stable public download failed its pinned size/SHA256 check') { throw 'Stable fixture must be verified before signing.' }
    $upload = @($steps | Where-Object { $_.Value -match 'uses: actions/upload-artifact@' })
    if ($upload.Count -ne 1) { throw 'Exactly one allowlisted upload is required.' }
    $paths = @([regex]::Matches($upload[0].Value, '(?m)^            (.+)$') | ForEach-Object { $_.Groups[1].Value })
    $expectedPaths = @(
        'preview-public/Abigail-windows-x64-setup.exe', 'preview-public/Abigail Preview.exe',
        'preview-public/mvp-build.json', 'preview-public/signature-report.json',
        'preview-public/windows-signatures.json', 'preview-public/prepared-build-state.json',
        'preview-public/preview-candidate-acceptance.json', 'preview-public/hosted-provenance.json'
    )
    if ($paths.Count -ne 8 -or @(Compare-Object $expectedPaths $paths).Count -or $upload[0].Value -match '(?m)^\s*path: [^|]') { throw 'Uploads must use only the exact public allowlist.' }
    if ($Text -notmatch 'hostedPreparedStateSha256 = \$preparedHash' -or $Text -notmatch 'harnessSourceCommit = \$env:GITHUB_SHA' -or
        $Text -notmatch 'publicationEligible = \$false' -or $Text -notmatch 'realModelValidation = \$false') { throw 'Fresh hosted provenance must preserve pending publication/real-model boundaries.' }
}

function Test-PolicyMutation([string]$Label, [string]$Mutated) {
    if ($Mutated -ceq $workflow) { throw "Mutation did not change workflow: $Label" }
    $rejected = $false
    try { Assert-Policy $Mutated } catch { $rejected = $true }
    if (-not $rejected) { throw "Policy accepted invalid workflow: $Label" }
    $script:passed++
    Write-Host "PASS policy rejects $Label"
}

Assert-Policy $workflow
$passed++
Test-PolicyMutation 'automatic push' ($workflow.Replace("  workflow_dispatch:`n", "  push:`n  workflow_dispatch:`n"))
Test-PolicyMutation 'write permission' ($workflow.Replace('contents: read', 'contents: write'))
Test-PolicyMutation 'job permission override' ($workflow.Replace("  candidate:`n", "  candidate:`n    permissions:`n      contents: write`n"))
Test-PolicyMutation 'self hosted runner' ($workflow.Replace('runs-on: windows-latest', 'runs-on: self-hosted'))
Test-PolicyMutation 'approval defaults true' ($workflow.Replace('default: false', 'default: true'))
Test-PolicyMutation 'secrets in global environment' ($workflow.Replace('PRODUCT_SOURCE: f7', 'ESIGNER_PASSWORD: ${{ secrets.ESIGNER_PASSWORD }}' + "`n      PRODUCT_SOURCE: f7"))
Test-PolicyMutation 'mutable checkout pin' ($workflow.Replace('ref: f7c7848d25726480b253ec1ca637c3ffa5ea3c77', 'ref: main'))
Test-PolicyMutation 'checkout persists credentials' ($workflow.Replace('persist-credentials: false', 'persist-credentials: true'))
Test-PolicyMutation 'unreviewed action tag' ($workflow.Replace('actions/checkout@34e114876b0b11c390a56381ad16ebd13914f8d5', 'actions/checkout@v4'))
Test-PolicyMutation 'stable default Sign' ($workflow.Replace('-Phase Sign -Channel preview', '-Phase Sign'))
Test-PolicyMutation 'cancel active signing' ($workflow.Replace('cancel-in-progress: false', 'cancel-in-progress: true'))
Test-PolicyMutation 'broad upload wildcard' ($workflow.Replace('preview-public/hosted-provenance.json', 'preview-public/**'))
Test-PolicyMutation 'profile upload' ($workflow.Replace('preview-public/hosted-provenance.json', '${{ runner.temp }}/abigail-preview-fixture'))
Test-PolicyMutation 'false publication eligibility' ($workflow.Replace('publicationEligible = $false', 'publicationEligible = $true'))
Test-PolicyMutation 'omitted lifecycle unit tests' ($workflow.Replace('node --test scripts/tests/test_preview_candidate_lifecycle.mjs', 'Write-Host skipped'))
Test-PolicyMutation 'omitted WebView2 prerequisite gate' ($workflow.Replace('./scripts/run-preview-candidate.ps1 -CheckPrerequisites -HarnessSourceCommit $env:GITHUB_SHA', 'Write-Host skipped'))
Test-PolicyMutation 'omitted shared Runtime dependency install' ($workflow.Replace('npm --prefix entity-runtime-app/src-ui ci', 'Write-Host skipped'))
Test-PolicyMutation 'unlocked shared Runtime dependency install' ($workflow.Replace('npm --prefix entity-runtime-app/src-ui ci', 'npm --prefix entity-runtime-app/src-ui install'))
Test-PolicyMutation 'ignored shared Runtime install failure' ($workflow.Replace("if (`$LASTEXITCODE -ne 0) { throw 'Shared Runtime frontend dependency installation failed.' }", 'Write-Host skipped'))
$runtimeInstallLine = "          npm --prefix entity-runtime-app/src-ui ci`n"
$prepareLine = [regex]::Match($workflow, '(?m)^          \./scripts/build_signed_installer\.ps1 -Phase Prepare[^\n]+\n').Value
Test-PolicyMutation 'shared Runtime dependencies installed too late' ($workflow.Replace($runtimeInstallLine, '').Replace($prepareLine, $prepareLine + $runtimeInstallLine))

# Parse every actual inline PowerShell block, without running its body.
$blocks = @([regex]::Matches($workflow, '(?m)^        run: \|\n((?:^          .*\n|^\n)+)'))
if ($blocks.Count -lt 8) { throw 'Workflow run blocks were not fully extracted.' }
foreach ($block in $blocks) {
    $source = [regex]::Replace($block.Groups[1].Value, '(?m)^          ', '')
    $tokens = $null
    $parseErrors = $null
    $null = [Management.Automation.Language.Parser]::ParseInput($source, [ref]$tokens, [ref]$parseErrors)
    if ($parseErrors.Count) { throw 'Workflow contains invalid PowerShell.' }
    $passed++
}
$guardText = [regex]::Replace($blocks[0].Groups[1].Value, '(?m)^          ', '')
$guard = [scriptblock]::Create($guardText)
$fixture = [ordered]@{
    GITHUB_EVENT_NAME = 'workflow_dispatch'; GITHUB_REPOSITORY = 'jbcupps/abigail'
    RUNNER_ENVIRONMENT = 'github-hosted'; RUNNER_OS = 'Windows'; GITHUB_REF = 'refs/heads/dev/preview-combined-0.0.76'
    REVIEWED_HARNESS_COMMIT = ('a' * 40); GITHUB_SHA = ('a' * 40)
    APPROVE_FIXTURE_DPAPI = 'true'; APPROVE_SIGNING_QUOTA = 'true'; GITHUB_RUN_ATTEMPT = '1'
    PRODUCT_SOURCE = 'f7c7848d25726480b253ec1ca637c3ffa5ea3c77'
}
$previous = @{}
foreach ($name in $fixture.Keys) { $previous[$name] = [Environment]::GetEnvironmentVariable($name, 'Process') }
try {
    foreach ($name in $fixture.Keys) { [Environment]::SetEnvironmentVariable($name, $fixture[$name], 'Process') }
    & $guard
    $passed++
    $rejections = @(
        @('GITHUB_EVENT_NAME', 'push'), @('GITHUB_REPOSITORY', 'fork/abigail'),
        @('RUNNER_ENVIRONMENT', 'self-hosted'), @('RUNNER_OS', 'Linux'),
        @('GITHUB_REF', 'refs/heads/main'), @('GITHUB_REF', 'refs/tags/v0.0.76'),
        @('REVIEWED_HARNESS_COMMIT', ('b' * 40)), @('REVIEWED_HARNESS_COMMIT', ''), @('REVIEWED_HARNESS_COMMIT', ('A' * 40)),
        @('APPROVE_FIXTURE_DPAPI', 'false'), @('APPROVE_FIXTURE_DPAPI', ''), @('APPROVE_SIGNING_QUOTA', 'false'),
        @('GITHUB_RUN_ATTEMPT', '2'), @('PRODUCT_SOURCE', ('a' * 40))
    )
    foreach ($case in $rejections) {
        [Environment]::SetEnvironmentVariable($case[0], $case[1], 'Process')
        $rejected = $false
        try { & $guard } catch { $rejected = $true }
        if (-not $rejected) { throw "Scope guard accepted invalid synthetic $($case[0])." }
        [Environment]::SetEnvironmentVariable($case[0], $fixture[$case[0]], 'Process')
        $passed++
        Write-Host "PASS scope rejects $($case[0])"
    }
} finally {
    foreach ($name in $fixture.Keys) { [Environment]::SetEnvironmentVariable($name, $previous[$name], 'Process') }
}
Write-Host "PASS: $passed credential-free workflow policy, parser and scope checks."
