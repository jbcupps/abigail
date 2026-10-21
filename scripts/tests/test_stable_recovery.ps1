# Execute the actual recovery metadata guard against offline evidence fixtures.
# No GitHub calls, signing, installation, or publication.
#requires -Version 7.0
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$workflowPath = Join-Path $PSScriptRoot '../../.github/workflows/recover-stable-0.0.76.yml'
$lines = @(Get-Content -LiteralPath $workflowPath)
$heading = [Array]::IndexOf($lines, '      - name: Validate pinned build and original main CI gate')
if ($heading -lt 0) { throw 'Recovery guard step is missing.' }
$scriptStart = $heading + 3
$body = [Collections.Generic.List[string]]::new()
for ($i = $scriptStart; $i -lt $lines.Count -and $lines[$i].StartsWith('          '); $i++) {
    $body.Add($lines[$i].Substring(10))
}
$tokens = $null
$parseErrors = $null
$ast = [Management.Automation.Language.Parser]::ParseInput(($body -join "`n"), [ref]$tokens, [ref]$parseErrors)
if ($parseErrors.Count) { throw ($parseErrors.Message -join "`n") }
$guard = @($ast.FindAll({
    param($node)
    $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -ceq 'Assert-RecoveryEvidence'
}, $true))
if ($guard.Count -ne 1) { throw 'Expected one real metadata guard function.' }
. ([scriptblock]::Create($guard[0].Extent.Text))

function New-EvidenceFixture {
    $sha = 'e3ad51181da1ea11bded24ca0c39e2dcfe5fa944'
    $common = @{
        repository = @{ id = 1152494386; full_name = 'jbcupps/abigail' }
        head_repository = @{ id = 1152494386 }; head_sha = $sha; head_branch = 'main'
        status = 'completed'; run_attempt = 1; conclusion = 'failure'
    }
    $run = $common.Clone()
    $run.id = 39380331517L; $run.path = '.github/workflows/release.yml'; $run.event = 'workflow_dispatch'
    $ci = $common.Clone()
    $ci.id = 39376395634L; $ci.path = '.github/workflows/ci.yml'; $ci.event = 'push'
    $steps = @('Prepare full cloud installer without signing secrets', 'Cloud sign and verify installed full Abigail product',
        'Collect selected cloud installer validation evidence', 'Stage and verify exact release assets',
        'Upload verified release assets (Upload updater artifacts when enabled)') |
        ForEach-Object { @{ name = $_; conclusion = 'success' } }
    $publishSteps = @(@{ name = 'Refuse replacement of an existing release'; conclusion = 'failure' }) +
        @('Require successful CI gate for this commit', 'Create git tag if needed', 'Create GitHub Release' |
            ForEach-Object { @{ name = $_; conclusion = 'skipped' } })
    @{
        Run = $run; Ci = $ci
        Jobs = @{ total_count = 2; jobs = @(
            @{ name = 'build (windows-latest)'; status = 'completed'; conclusion = 'success'; steps = $steps },
            @{ name = 'publish'; status = 'completed'; conclusion = 'failure'; steps = $publishSteps }
        ) }
        CiJobs = @{ total_count = 1; jobs = @(@{ name = 'gate'; status = 'completed'; conclusion = 'success' }) }
        Artifact = @{
            id = 12246127943L; name = 'abigail-release-assets-windows'; expired = $false; size_in_bytes = 86751966L
            digest = 'sha256:942a188c532f16270f7b639ac5a222c6fbafeefbc8729df8ec83d61cc64bd1ae'
            workflow_run = @{ id = 39380331517L; repository_id = 1152494386; head_repository_id = 1152494386; head_branch = 'main'; head_sha = $sha }
        }
    } | ConvertTo-Json -Depth 20 | ConvertFrom-Json
}

$cases = @(
    @{ name = 'exact completed build and main gate accepted despite advisory audit'; allow = $true; change = {} },
    @{ name = 'different source'; change = { param($e) $e.Run.head_sha = '0000000000000000000000000000000000000000' } },
    @{ name = 'fork source'; change = { param($e) $e.Run.head_repository.id = 1 } },
    @{ name = 'different run'; change = { param($e) $e.Run.id = 39380331518L } },
    @{ name = 'rerun attempt'; change = { param($e) $e.Run.run_attempt = 2 } },
    @{ name = 'candidate branch'; change = { param($e) $e.Run.head_branch = 'dev/candidate' } },
    @{ name = 'wrong release workflow'; change = { param($e) $e.Run.path = '.github/workflows/other.yml' } },
    @{ name = 'missing installed acceptance'; change = { param($e) $e.Jobs.jobs[0].steps[1].conclusion = 'skipped' } },
    @{ name = 'failed build'; change = { param($e) $e.Jobs.jobs[0].conclusion = 'failure' } },
    @{ name = 'wrong publication failure'; change = { param($e) $e.Jobs.jobs[1].steps[0].name = 'Create GitHub Release' } },
    @{ name = 'tag already created by original run'; change = { param($e) $e.Jobs.jobs[1].steps[2].conclusion = 'success' } },
    @{ name = 'incomplete job page'; change = { param($e) $e.Jobs.total_count = 3 } },
    @{ name = 'wrong main CI source'; change = { param($e) $e.Ci.head_sha = '0000000000000000000000000000000000000000' } },
    @{ name = 'PR CI instead of main push'; change = { param($e) $e.Ci.event = 'pull_request' } },
    @{ name = 'failed main CI gate'; change = { param($e) $e.CiJobs.jobs[0].conclusion = 'failure' } },
    @{ name = 'different artifact'; change = { param($e) $e.Artifact.id = 1 } },
    @{ name = 'expired artifact'; change = { param($e) $e.Artifact.expired = $true } },
    @{ name = 'artifact digest substitution'; change = { param($e) $e.Artifact.digest = 'sha256:' + ('0' * 64) } },
    @{ name = 'artifact from another run'; change = { param($e) $e.Artifact.workflow_run.id = 1 } }
)
foreach ($case in $cases) {
    $e = New-EvidenceFixture
    & $case.change $e
    $accepted = $false
    try { Assert-RecoveryEvidence $e.Run $e.Jobs $e.Ci $e.CiJobs $e.Artifact; $accepted = $true } catch {}
    $expected = $case.ContainsKey('allow') -and $case.allow
    if ($accepted -ne $expected) { throw "Recovery guard case failed: $($case.name)" }
}
$text = $lines -join "`n"
foreach ($required in @(
    "RELEASE_MANIFEST_SHA256: c68564f76053cc0a5b3b6831d00c2fcb9146e9930454316880ca1b4dacd53037",
    "artifact-ids: '12246127943'", "run-id: '39380331517'",
    '-ReportPath recovery-evidence/native-signatures.json', '-ExpectedAssetsDirectory release-assets',
    "overwrite_files: false", "make_latest: 'true'",
    "ref: e3ad51181da1ea11bded24ca0c39e2dcfe5fa944",
    "target_commitish: e3ad51181da1ea11bded24ca0c39e2dcfe5fa944"
)) {
    if (-not $text.Contains($required)) { throw "Recovery publication constraint missing: $required" }
}
if ($text -match 'release_provenance\.mjs create|secrets\.|PENDING_|workflow_dispatch:\s+inputs:') {
    throw 'Recovery must not create provenance, consume signing secrets, or accept arbitrary inputs.'
}
Write-Host "PASS: $($cases.Count) recovery evidence cases and fixed publication constraints."
