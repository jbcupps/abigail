#requires -Version 7.0
<# Offline staging tests. npm/cargo commands and PE version inspection are
   mocked in an isolated temporary repository. The real staging script, shared
   profile helper, and reviewed overlays are used. Synthetic .exe bytes are
   never executed. No builds, dependency installation, signing, network,
   installer execution, personal profile access, or compatibility claim. #>
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$repository = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\', '/')
$workspace = Join-Path $tempRoot ('abigail-preview-staging-tests-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $workspace | Out-Null
$savedTauri = $env:TAURI_CONFIG
$savedTarget = $env:CARGO_TARGET_DIR
$savedBuildProfile = $env:ABIGAIL_BUILD_PROFILE
$script:passed = 0

function Assert-Test([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
}
function New-Fixture([string]$Name) {
    $root = Join-Path $workspace $Name
    New-Item -ItemType Directory -Path (Join-Path $root 'scripts') -Force | Out-Null
    foreach ($name in @('stage_split_installer_resources.ps1', 'packaging_profile.ps1')) {
        Copy-Item -LiteralPath (Join-Path $repository "scripts/$name") -Destination (Join-Path $root "scripts/$name")
    }
    foreach ($app in @('hive-app', 'entity-runtime-app')) {
        $frontend = Join-Path $root "$app/src-ui/dist"
        New-Item -ItemType Directory -Path (Join-Path $frontend 'assets') -Force | Out-Null
        foreach ($name in @('tauri.conf.json', 'tauri.preview.conf.json')) {
            Copy-Item -LiteralPath (Join-Path $repository "$app/$name") -Destination (Join-Path $root "$app/$name")
        }
        [IO.File]::WriteAllText((Join-Path $root "$app/src-ui/package.json"), '{}')
        [IO.File]::WriteAllText((Join-Path $frontend 'index.html'), '<script type="module" src="./assets/app.js"></script>')
        [IO.File]::WriteAllText((Join-Path $frontend 'assets/app.js'), '// Synthetic frontend fixture.')
    }
    return $root
}
function Config-Snapshot([string]$Root) {
    return (@(foreach ($app in @('hive-app', 'entity-runtime-app')) {
        foreach ($name in @('tauri.conf.json', 'tauri.preview.conf.json')) {
            (Get-FileHash -LiteralPath (Join-Path $Root "$app/$name")).Hash
        }
    }) -join '|')
}
function Invoke-Fixture(
    [string]$Name, [string]$Channel = 'preview', [string]$Configuration = 'release',
    [string]$Version = '7.8.9', [string]$Target = '', [string]$Fault = '', [string]$ExpectedError = '',
    [string]$BuildProfile = 'synthetic-inherited-profile'
) {
    $root = New-Fixture $Name
    $originalConfig = Config-Snapshot $root
    $ambient = '{"identifier":"com.example.ambient","customStableField":"preserve-me","version":"1.0.0"}'
    $env:TAURI_CONFIG = $ambient
    $env:CARGO_TARGET_DIR = $Target
    $env:ABIGAIL_BUILD_PROFILE = $BuildProfile
    $context = @{
        root = $root; fault = $Fault; channel = $Channel; configuration = $Configuration
        calls = [Collections.Generic.List[object]]::new()
        versions = @{}
    }
    function npm {
        $context.calls.Add([pscustomobject]@{ tool = 'npm'; arguments = @($args); tauri = $env:TAURI_CONFIG; target = $env:CARGO_TARGET_DIR; buildProfile = $env:ABIGAIL_BUILD_PROFILE })
        $global:LASTEXITCODE = if ($context.fault -eq 'npm-failure') { 17 } else { 0 }
    }
    function node {
        $arguments = @($args)
        Assert-Test ($arguments.Count -eq 5 -and [IO.Path]::GetFileName($arguments[0]) -eq 'check_windows_runtime.mjs') 'Unexpected staging node invocation.'
        $context.calls.Add([pscustomobject]@{ tool = 'runtime-check'; arguments = $arguments })
        $global:LASTEXITCODE = if ($context.fault -eq 'runtime-dependency-failure') { 1 } else { 0 }
    }
    function cargo {
        $arguments = @($args)
        $context.calls.Add([pscustomobject]@{ tool = 'cargo'; arguments = $arguments; tauri = $env:TAURI_CONFIG; target = $env:CARGO_TARGET_DIR; buildProfile = $env:ABIGAIL_BUILD_PROFILE })
        $packages = @(for ($index = 0; $index -lt $arguments.Count - 1; $index++) {
            if ($arguments[$index] -ceq '-p') { $arguments[$index + 1] }
        })
        $failPackage = switch ($context.fault) {
            'daemon-failure' { 'hive-daemon' }
            'hive-failure' { 'abigail-hive-app' }
            'runtime-failure' { 'abigail-entity-runtime-app' }
            default { '' }
        }
        if ($failPackage -and $packages -contains $failPackage) { $global:LASTEXITCODE = 23; return }
        $target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $context.root 'target' }
        if (-not [IO.Path]::IsPathFullyQualified($target)) { $target = Join-Path $context.root $target }
        $binaryRoot = Join-Path $target $context.configuration
        New-Item -ItemType Directory -Path $binaryRoot -Force | Out-Null
        foreach ($package in $packages) {
            if ($context.fault -eq 'missing-resource' -and $package -eq 'entity-daemon') { continue }
            $contents = 'Synthetic nonexecutable bytes.'
            if ($package -in @('abigail-hive-app', 'abigail-entity-runtime-app')) {
                $config = $env:TAURI_CONFIG | ConvertFrom-Json
                $identifier = $config.identifier
                if ($context.fault -eq 'missing-identifier' -and $package -eq 'abigail-entity-runtime-app') { $identifier = 'com.example.wrong' }
                $entry = if ($context.fault -eq 'missing-frontend' -and $package -eq 'abigail-hive-app') { 'assets/old.js' } else { 'assets/app.js' }
                $marker = if ($context.fault -eq 'missing-marker' -and $package -eq 'abigail-entity-runtime-app') { '' } else { 'abigail-channel-profile-v1' }
                $contents = "Synthetic nonexecutable shell|$entry|$identifier|$marker"
                $context.versions[(Join-Path $binaryRoot "$package.exe")] = if ($context.fault -eq 'wrong-version') { '0.0.0' } else { $config.version }
            }
            [IO.File]::WriteAllText((Join-Path $binaryRoot "$package.exe"), $contents)
        }
        $global:LASTEXITCODE = 0
    }
    function Get-Item {
        param([string]$LiteralPath)
        $absolute = [IO.Path]::GetFullPath($LiteralPath)
        if ($context.versions.ContainsKey($absolute)) {
            return [pscustomobject]@{ VersionInfo = [pscustomobject]@{ ProductVersion = $context.versions[$absolute] } }
        }
        Microsoft.PowerShell.Management\Get-Item -LiteralPath $LiteralPath
    }
    $parameters = @{ Configuration = $Configuration; Channel = $Channel }
    if ($Version) { $parameters.Version = $Version }
    $caught = $null
    try { & (Join-Path $root 'scripts/stage_split_installer_resources.ps1') @parameters }
    catch { $caught = $_ }
    Assert-Test ($env:TAURI_CONFIG -ceq $ambient) "$Name did not restore TAURI_CONFIG."
    Assert-Test ([string]$env:CARGO_TARGET_DIR -ceq $Target) "$Name did not restore CARGO_TARGET_DIR."
    Assert-Test ([string]$env:ABIGAIL_BUILD_PROFILE -ceq $BuildProfile) "$Name did not restore ABIGAIL_BUILD_PROFILE."
    Assert-Test ((Config-Snapshot $root) -ceq $originalConfig) "$Name modified a base configuration or preview overlay."
    foreach ($call in @($context.calls | Where-Object tool -EQ 'cargo')) {
        if ($Channel -eq 'preview') {
            Assert-Test ($call.buildProfile -ceq 'preview') "$Name did not compile a preview build profile."
        } else {
            Assert-Test ([string]::IsNullOrEmpty($call.buildProfile)) "$Name compiled stable with an inherited build profile."
        }
    }
    if ($ExpectedError) {
        Assert-Test ($null -ne $caught) "$Name should have failed."
        Assert-Test ($caught.Exception.Message -match $ExpectedError) "$Name failed unexpectedly: $($caught.Exception.Message)"
    } elseif ($caught) { throw $caught }
    $script:passed++
    Write-Host "PASS $Name (environment and configuration preserved)"
    return $context
}

try {
    $preview = Invoke-Fixture 'preview-separated'
    $runtimeCheck = @($preview.calls | Where-Object tool -EQ 'runtime-check')
    Assert-Test ($runtimeCheck.Count -eq 1) 'Preview must check all four compiled runtime imports before copying.'
    foreach ($path in $runtimeCheck[0].arguments[1..4]) {
        Assert-Test ($path.StartsWith((Join-Path $preview.root 'target/preview'))) 'Runtime check escaped the isolated preview build.'
    }
    $null = Invoke-Fixture 'reject-runtime-dependency' -Fault 'runtime-dependency-failure' -ExpectedError 'failed'
    $cargo = @($preview.calls | Where-Object tool -EQ 'cargo')
    Assert-Test ($cargo.Count -eq 3) 'Preview must build daemons and both shells separately.'
    Assert-Test (-not $cargo[0].tauri) 'Daemons must not inherit Tauri configuration.'
    Assert-Test (($cargo[0].arguments -join '|') -ceq 'build|--locked|-p|hive-daemon|-p|entity-daemon|--release') 'Daemon build arguments changed.'
    $hive = $cargo[1].tauri | ConvertFrom-Json -AsHashtable
    $runtime = $cargo[2].tauri | ConvertFrom-Json -AsHashtable
    Assert-Test ($hive.identifier -ceq 'com.abigail.preview') 'Hive preview identity missing.'
    Assert-Test ($runtime.identifier -ceq 'com.abigail.preview.entity-runtime') 'Runtime inherited the Hive identity.'
    Assert-Test ($hive.version -ceq '7.8.9' -and $runtime.version -ceq '7.8.9') 'Preview version did not reach both shells.'
    Assert-Test (-not $hive.ContainsKey('customStableField') -and -not $runtime.ContainsKey('customStableField')) 'Preview inherited ambient stable overrides.'
    foreach ($config in @($hive, $runtime)) {
        Assert-Test ($config.plugins['abigail-launch'].contract -ceq 'abigail-channel-profile-v1') 'Preview shell lacks the launch contract.'
        Assert-Test ($config.plugins['abigail-launch'].compiledProfile.environment -ceq 'ABIGAIL_BUILD_PROFILE' -and
            $config.plugins['abigail-launch'].compiledProfile.value -ceq 'preview') 'Preview shell build profile contract differs from the build environment.'
    }
    foreach ($call in $cargo) {
        Assert-Test ($call.target -ceq (Join-Path $preview.root 'target/preview')) 'Preview target default is not isolated.'
        Assert-Test ($call.arguments -contains '--locked') 'Preview Cargo invocation lost --locked.'
    }
    Assert-Test (($cargo[1].arguments -join '|') -ceq 'build|--locked|-p|abigail-hive-app|--features|tauri/custom-protocol|--release') 'Hive build invocation changed.'
    Assert-Test (($cargo[2].arguments -join '|') -ceq 'build|--locked|-p|abigail-entity-runtime-app|--features|tauri/custom-protocol|--release') 'Runtime build invocation changed.'
    Assert-Test (@($preview.calls | Where-Object tool -EQ 'npm').Count -eq 4) 'Both frontends must run npm ci and build.'
    $resourceNames = @((Get-ChildItem -LiteralPath (Join-Path $preview.root 'hive-app/resources') -File).Name | Sort-Object)
    Assert-Test (($resourceNames -join '|') -ceq 'abigail-entity-runtime-app.exe|entity-daemon.exe|hive-daemon.exe') 'Preview resource names changed.'
    $script:passed++; Write-Host 'PASS preview overlays, all four compiled profiles, locked builds, isolated target, and resource contract'

    $null = Invoke-Fixture 'preview-restores-unset-build-profile' -BuildProfile ''
    $explicit = Invoke-Fixture 'preview-explicit-target' -Target 'target/custom-preview'
    foreach ($call in @($explicit.calls | Where-Object tool -EQ 'cargo')) {
        Assert-Test ($call.target -ceq 'target/custom-preview') 'Explicit target was overwritten.'
    }
    $stable = Invoke-Fixture 'stable-combined' -Channel stable -Version '' -BuildProfile ''
    $cargo = @($stable.calls | Where-Object tool -EQ 'cargo')
    Assert-Test ($cargo.Count -eq 1) 'Stable must retain its single combined Cargo invocation.'
    Assert-Test (($cargo[0].arguments -join '|') -ceq 'build|--locked|-p|hive-daemon|-p|entity-daemon|-p|abigail-hive-app|-p|abigail-entity-runtime-app|--features|tauri/custom-protocol|--release') 'Stable Cargo arguments changed.'
    $config = $cargo[0].tauri | ConvertFrom-Json
    $base = Get-Content -LiteralPath (Join-Path $stable.root 'hive-app/tauri.conf.json') -Raw | ConvertFrom-Json
    Assert-Test ($config.version -ceq $base.version -and $config.customStableField -ceq 'preserve-me' -and $config.identifier -ceq 'com.example.ambient') 'Stable ambient merge or default version changed.'
    Assert-Test (-not $cargo[0].target) 'Stable default target environment changed.'
    $script:passed++; Write-Host 'PASS stable combined invocation, version fallback, and ambient overrides preserved'
    $debug = Invoke-Fixture 'preview-debug' -Configuration debug
    foreach ($call in @($debug.calls | Where-Object tool -EQ 'cargo')) {
        Assert-Test ($call.arguments -notcontains '--release') 'Debug requested a release build.'
    }
    foreach ($failure in @('npm-failure', 'daemon-failure', 'hive-failure', 'runtime-failure')) {
        $null = Invoke-Fixture $failure -Fault $failure -ExpectedError 'failed in'
    }
    $null = Invoke-Fixture 'stable-failure' -Channel stable -Fault 'hive-failure' -ExpectedError 'failed in' -BuildProfile ''
    foreach ($inherited in @('preview', 'stable', 'unknown', ' ')) {
        $suffix = if ($inherited -eq ' ') { 'whitespace' } else { $inherited }
        $rejected = Invoke-Fixture "stable-rejects-inherited-$suffix" -Channel stable -BuildProfile $inherited -ExpectedError 'ABIGAIL_BUILD_PROFILE to be unset'
        Assert-Test ($rejected.calls.Count -eq 0) 'Stable build profile rejection must occur before any frontend or Cargo command.'
    }
    $null = Invoke-Fixture 'missing-marker' -Fault 'missing-marker' -ExpectedError 'preview identifier and launch contract marker'
    $null = Invoke-Fixture 'missing-identifier' -Fault 'missing-identifier' -ExpectedError 'preview identifier and launch contract marker'
    $null = Invoke-Fixture 'wrong-version' -Fault 'wrong-version' -ExpectedError 'embeds version'
    $null = Invoke-Fixture 'missing-frontend' -Fault 'missing-frontend' -ExpectedError 'does not embed current frontend'
    $null = Invoke-Fixture 'missing-resource' -Fault 'missing-resource' -ExpectedError 'Missing required installer resource'
    Write-Host "Preview staging: $script:passed checks passed. Commands and PE versions were mocked; no real build or app acceptance is established."
} finally {
    $env:TAURI_CONFIG = $savedTauri
    $env:CARGO_TARGET_DIR = $savedTarget
    $env:ABIGAIL_BUILD_PROFILE = $savedBuildProfile
    $resolved = [IO.Path]::GetFullPath($workspace)
    if ([IO.Path]::GetDirectoryName($resolved) -ne $tempRoot -or
        [IO.Path]::GetFileName($resolved) -notmatch '^abigail-preview-staging-tests-[0-9a-f]{32}$') {
        throw 'Refusing to clean an unexpected staging test workspace.'
    }
    if (Test-Path -LiteralPath $resolved) { Remove-Item -LiteralPath $resolved -Recurse -Force }
}
