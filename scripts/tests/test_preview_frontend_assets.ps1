#requires -Version 7.0
<# Run after both frontend production builds. Inspect their emitted files only;
   do not execute Prepare, an application, an installer or signing. #>
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$repoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))

# Exercise the actual packaging guard without running the build script's phases.
$tokens = $null
$parseErrors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile(
    (Join-Path $repoRoot 'scripts/build_signed_installer.ps1'),
    [ref]$tokens,
    [ref]$parseErrors
)
if ($parseErrors.Count -ne 0) { throw 'Could not parse the packaging script.' }
$guards = @($ast.FindAll({
    param($node)
    $node -is [Management.Automation.Language.FunctionDefinitionAst] -and
        $node.Name -eq 'Read-FrontendAssets'
}, $false))
if ($guards.Count -ne 1) { throw 'Expected exactly one Read-FrontendAssets guard.' }
. ([scriptblock]::Create($guards[0].Extent.Text))

$passed = 0
foreach ($app in @('hive-app', 'entity-runtime-app')) {
    $frontend = Read-FrontendAssets $app
    Write-Host ($frontend | ConvertTo-Json -Compress)
    Write-Host "PASS $app packaging frontend guard"
    $passed++

    $dist = Join-Path $repoRoot "$app/src-ui/dist"
    $stylesheets = @(Get-ChildItem -LiteralPath $dist -Filter '*.css' -File -Recurse)
    if ($stylesheets.Count -ne 0) { throw "$app emitted an external CSS asset." }
    Write-Host "PASS $app has no emitted external CSS"
    $passed++

    $entrySource = @($frontend.javascript_entries | ForEach-Object {
        Get-Content -LiteralPath (Join-Path $dist $_) -Raw
    }) -join "`n"
    foreach ($marker in @('.first-signal-shell', '.presence-art', '.presence-runtime', 'presence-breathe', 'prefers-reduced-motion')) {
        if (-not $entrySource.Contains($marker)) {
            throw "$app does not embed presence stylesheet content: $marker"
        }
    }
    Write-Host "PASS $app entry embeds presence layout, animation and reduced-motion CSS"
    $passed++
}
Write-Host "$passed frontend artifact checks passed."
