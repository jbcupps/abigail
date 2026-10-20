#requires -Version 7.0
<# Offline configuration tests. The shared helper is evaluated against copied
   configs in a synthetic temporary repository. No application, installer,
   build, service, signing, network or personal profile is used. These tests
   validate packaging policy and identity, not runtime profile isolation. #>
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$repository = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\', '/')
$workspace = Join-Path $tempRoot ('abigail-preview-packaging-tests-' + [guid]::NewGuid().ToString('N'))
$configNames = @(
    'hive-app/tauri.conf.json', 'entity-runtime-app/tauri.conf.json',
    'hive-app/tauri.preview.conf.json', 'entity-runtime-app/tauri.preview.conf.json'
)
$script:passed = 0
$script:failures = [Collections.Generic.List[string]]::new()
New-Item -ItemType Directory -Path $workspace | Out-Null

function Assert-Test([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
}
function Get-ConfigSnapshot([string]$Root) {
    (@(foreach ($name in $configNames) {
        (Get-FileHash -LiteralPath (Join-Path $Root $name) -Algorithm SHA256).Hash
    }) -join '|')
}
function New-Fixture {
    $root = Join-Path $workspace ([guid]::NewGuid().ToString('N'))
    foreach ($app in @('hive-app', 'entity-runtime-app')) {
        New-Item -ItemType Directory -Path (Join-Path $root $app) -Force | Out-Null
    }
    foreach ($name in $configNames) {
        Copy-Item -LiteralPath (Join-Path $repository $name) -Destination (Join-Path $root $name)
    }
    $root
}
function Write-FixtureConfig([string]$Path, $Value) {
    [IO.File]::WriteAllText($Path, ($Value | ConvertTo-Json -Depth 30), [Text.UTF8Encoding]::new($false))
}
function Invoke-Case([string]$Name, [scriptblock]$Body) {
    try {
        & $Body
        $script:passed++
        Write-Host "PASS $Name"
    } catch {
        $script:failures.Add("$Name`: $($_.Exception.Message)")
        Write-Host "FAIL $Name`: $($_.Exception.Message)"
    }
}
function Assert-Rejected([scriptblock]$Body) {
    $rejected = $false
    try { $null = & $Body } catch { $rejected = $true }
    Assert-Test $rejected 'The changed packaging contract was accepted.'
}
function Test-Disjoint([string]$Left, [string]$Right) {
    $leftPath = [IO.Path]::GetFullPath($Left).TrimEnd('\', '/')
    $rightPath = [IO.Path]::GetFullPath($Right).TrimEnd('\', '/')
    $separator = [IO.Path]::DirectorySeparatorChar
    -not ($leftPath.Equals($rightPath, [StringComparison]::OrdinalIgnoreCase) -or
        $leftPath.StartsWith($rightPath + $separator, [StringComparison]::OrdinalIgnoreCase) -or
        $rightPath.StartsWith($leftPath + $separator, [StringComparison]::OrdinalIgnoreCase))
}

$originalConfigSnapshot = Get-ConfigSnapshot $repository
try {
    $helper = Join-Path $workspace 'packaging_profile.ps1'
    Copy-Item -LiteralPath (Join-Path $repository 'scripts/packaging_profile.ps1') -Destination $helper
    . $helper

    Invoke-Case 'omitted channel preserves stable identities and leaves configs unchanged' {
        $root = New-Fixture
        $before = Get-ConfigSnapshot $root
        $profile = Get-AbigailPackagingProfile -RepositoryRoot $root -Version '7.8.9'
        Assert-Test ($profile.channel -ceq 'stable') 'The default channel changed.'
        Assert-Test ($profile.productName -ceq 'Abigail' -and $profile.mainBinaryName -ceq 'Abigail' -and
            $profile.launcherFileName -ceq 'Abigail.exe') 'Stable launcher identity changed.'
        Assert-Test ($profile.hiveIdentifier -ceq 'com.abigail' -and
            $profile.runtimeIdentifier -ceq 'com.abigail.entity-runtime' -and
            $profile.runtimeProductName -ceq 'Abigail Entity Runtime') 'Stable shell identity changed.'
        Assert-Test ($profile.hiveOverlay.version -ceq '7.8.9' -and
            $profile.runtimeOverlay.version -ceq '7.8.9') 'Requested version was not applied to both overlays.'
        Assert-Test ($null -eq $profile.launchContract -and $profile.overlayHashes.Count -eq 0) 'Stable acquired a preview contract.'
        Assert-Test (($profile.hiveOverlay.Keys | Sort-Object) -join '|' -ceq 'identifier|mainBinaryName|productName|version') 'Stable Hive acquired an unexpected override.'
        Assert-Test (($profile.runtimeOverlay.Keys | Sort-Object) -join '|' -ceq 'identifier|productName|version') 'Stable Runtime acquired an unexpected override.'
        Assert-Test ((Get-ConfigSnapshot $root) -ceq $before) 'The profile helper wrote a config.'
    }
    Invoke-Case 'preview has distinct shell, launcher, window and NSIS identities' {
        $root = New-Fixture
        $before = Get-ConfigSnapshot $root
        $stable = Get-AbigailPackagingProfile -RepositoryRoot $root -Version '7.8.9'
        $profile = Get-AbigailPackagingProfile -RepositoryRoot $root -Channel preview -Version '7.8.9'
        Assert-Test ($profile.channel -ceq 'preview' -and $profile.productName -ceq 'Abigail Preview' -and
            $profile.mainBinaryName -ceq 'Abigail Preview' -and $profile.launcherFileName -ceq 'Abigail Preview.exe') 'Preview launcher identity is incorrect.'
        Assert-Test ($profile.hiveIdentifier -ceq 'com.abigail.preview' -and
            $profile.runtimeIdentifier -ceq 'com.abigail.preview.entity-runtime' -and
            $profile.runtimeProductName -ceq 'Abigail Preview Entity Runtime') 'Preview shell identities are incorrect.'
        Assert-Test ($profile.hiveIdentifier -cne $stable.hiveIdentifier -and
            $profile.runtimeIdentifier -cne $stable.runtimeIdentifier -and
            $profile.mainBinaryName -cne $stable.mainBinaryName) 'Preview overlaps stable identity.'
        Assert-Test ($profile.hiveOverlay.app.windows.Count -eq 1 -and
            $profile.hiveOverlay.app.windows[0].title -ceq 'Abigail Preview Hive' -and
            $profile.runtimeOverlay.app.windows.Count -eq 1 -and
            $profile.runtimeOverlay.app.windows[0].title -ceq 'Abigail Preview Entity Runtime') 'Preview window identity is incorrect.'
        Assert-Test ($profile.hiveOverlay.app.windows[0].visible -eq $false -and
            $profile.runtimeOverlay.app.windows[0].visible -eq $false) 'Initial window visibility changed.'
        $nsis = $profile.hiveOverlay.bundle.windows.nsis
        Assert-Test ($nsis.installMode -ceq 'currentUser' -and $nsis.startMenuFolder -ceq 'Abigail Preview' -and
            $nsis.installerHooks -ceq '../scripts/nsis-preview-hooks.nsh') 'Preview NSIS identity or hook is incorrect.'
        Assert-Test ((Get-ConfigSnapshot $root) -ceq $before) 'The profile helper wrote a config.'
    }
    Invoke-Case 'both shells carry the accepted compiled profile and persistent marker contract' {
        $root = New-Fixture
        $profile = Get-AbigailPackagingProfile -RepositoryRoot $root -Channel preview -Version '7.8.9'
        $launch = $profile.launchContract
        Assert-Test ($launch.schemaVersion -eq 1 -and $launch.contract -ceq 'abigail-channel-profile-v1' -and
            $launch.channel -ceq 'preview' -and $launch.releaseChannel -ceq 'beta') 'Preview launch identity is incorrect.'
        Assert-Test ($launch.compiledProfile.environment -ceq 'ABIGAIL_BUILD_PROFILE' -and
            $launch.compiledProfile.value -ceq 'preview') 'Preview compiled profile selector changed.'
        Assert-Test ($launch.profileRoot.windowsKnownFolder -ceq 'LocalAppData' -and
            $launch.profileRoot.relativePath -ceq 'AbigailPreview/profiles/ongoing') 'Preview profile root changed.'
        Assert-Test ($launch.profileMarker.fileName -ceq 'profile.json' -and $launch.profileMarker.schemaVersion -eq 1 -and
            $launch.profileMarker.kind -ceq 'preview' -and $launch.profileMarker.channel -ceq 'beta' -and
            $launch.profileMarker.persistentIdField -ceq 'profile_id') 'Persistent preview marker contract changed.'
        Assert-Test ($launch.runtimeInterface.status -ceq 'pending-app-owner') 'The unconfirmed runtime interface was claimed ready.'
        foreach ($withdrawnField in @('profileId', 'daemonArguments', 'roots')) {
            Assert-Test (-not $launch.Contains($withdrawnField)) "The withdrawn $withdrawnField contract is still present."
        }
        Assert-Test (($launch | ConvertTo-Json -Depth 20 -Compress) -ceq
            ($profile.runtimeOverlay.plugins['abigail-launch'] | ConvertTo-Json -Depth 20 -Compress)) 'Shell launch contracts differ.'
        # Synthetic path arithmetic only: do not resolve any current user's folders.
        $knownFolder = Join-Path $root 'synthetic-LocalAppData'
        $profilePath = Join-Path $knownFolder $launch.profileRoot.relativePath
        foreach ($installName in @('Abigail', 'Abigail Preview')) {
            Assert-Test (Test-Disjoint $profilePath (Join-Path $knownFolder $installName)) "Profile overlaps the $installName install directory."
        }
    }
    Invoke-Case 'stamps are deterministic and stable contains no preview hashes' {
        $root = New-Fixture
        $stable = Get-AbigailPackagingStamp (Get-AbigailPackagingProfile -RepositoryRoot $root -Version '7.8.9')
        Assert-Test ($stable.channel -ceq 'stable' -and $stable.launcherFileName -ceq 'Abigail.exe' -and
            $null -eq $stable.hiveOverlaySha256 -and $null -eq $stable.runtimeOverlaySha256 -and
            $null -eq $stable.launchContractSha256 -and $null -eq $stable.buildProfile) 'Stable stamp acquired preview values.'
        $profile = Get-AbigailPackagingProfile -RepositoryRoot $root -Channel preview -Version '7.8.9'
        $first = Get-AbigailPackagingStamp $profile
        Assert-Test ($first.buildProfile -ceq 'preview') 'Stamp does not record the compiled preview profile.'
        $second = Get-AbigailPackagingStamp (Get-AbigailPackagingProfile -RepositoryRoot $root -Channel preview -Version '7.8.9')
        Assert-Test (($first | ConvertTo-Json -Compress) -ceq ($second | ConvertTo-Json -Compress)) 'Unchanged inputs produced a different stamp.'
        Assert-Test ($first.hiveOverlaySha256 -ceq (Get-FileHash (Join-Path $root 'hive-app/tauri.preview.conf.json')).Hash -and
            $first.runtimeOverlaySha256 -ceq (Get-FileHash (Join-Path $root 'entity-runtime-app/tauri.preview.conf.json')).Hash) 'Stamp does not bind exact overlay bytes.'
        $contractBytes = [Text.Encoding]::UTF8.GetBytes(($profile.launchContract | ConvertTo-Json -Depth 20 -Compress))
        Assert-Test ($first.launchContractSha256 -ceq [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($contractBytes))) 'Stamp does not bind the launch contract.'
    }
    foreach ($app in @('hive-app', 'entity-runtime-app')) {
        Invoke-Case "$app whitespace-only overlay change changes its stamp" {
            $root = New-Fixture
            $beforeProfile = Get-AbigailPackagingProfile -RepositoryRoot $root -Channel preview -Version '7.8.9'
            $before = Get-AbigailPackagingStamp $beforeProfile
            [IO.File]::AppendAllText((Join-Path $root "$app/tauri.preview.conf.json"), "`n  `n")
            $afterProfile = Get-AbigailPackagingProfile -RepositoryRoot $root -Channel preview -Version '7.8.9'
            $after = Get-AbigailPackagingStamp $afterProfile
            Assert-Test (($beforeProfile.hiveOverlay | ConvertTo-Json -Depth 30 -Compress) -ceq
                ($afterProfile.hiveOverlay | ConvertTo-Json -Depth 30 -Compress)) 'Whitespace changed Hive semantics.'
            Assert-Test (($beforeProfile.runtimeOverlay | ConvertTo-Json -Depth 30 -Compress) -ceq
                ($afterProfile.runtimeOverlay | ConvertTo-Json -Depth 30 -Compress)) 'Whitespace changed Runtime semantics.'
            $changed = if ($app -eq 'hive-app') { 'hiveOverlaySha256' } else { 'runtimeOverlaySha256' }
            $unchanged = if ($app -eq 'hive-app') { 'runtimeOverlaySha256' } else { 'hiveOverlaySha256' }
            Assert-Test ($before.$changed -cne $after.$changed -and $before.$unchanged -ceq $after.$unchanged -and
                $before.launchContractSha256 -ceq $after.launchContractSha256) 'Stamp failed to distinguish exact overlay bytes from launch semantics.'
        }
    }

    $mutations = @(
        @{ name = 'Hive product'; app = 'hive-app'; change = { param($c) $c.productName = 'Abigail' } },
        @{ name = 'Hive identifier'; app = 'hive-app'; change = { param($c) $c.identifier = 'com.abigail' } },
        @{ name = 'Hive main binary'; app = 'hive-app'; change = { param($c) $c.mainBinaryName = 'Abigail' } },
        @{ name = 'Runtime product'; app = 'entity-runtime-app'; change = { param($c) $c.productName = 'Abigail Entity Runtime' } },
        @{ name = 'Runtime identifier'; app = 'entity-runtime-app'; change = { param($c) $c.identifier = 'com.abigail.entity-runtime' } },
        @{ name = 'Hive window title'; app = 'hive-app'; change = { param($c) $c.app.windows[0].title = 'Abigail' } },
        @{ name = 'Runtime window title'; app = 'entity-runtime-app'; change = { param($c) $c.app.windows[0].title = 'Abigail Entity Runtime' } },
        @{ name = 'NSIS install mode'; app = 'hive-app'; change = { param($c) $c.bundle.windows.nsis.installMode = 'perMachine' } },
        @{ name = 'NSIS menu folder'; app = 'hive-app'; change = { param($c) $c.bundle.windows.nsis.startMenuFolder = 'Abigail' } },
        @{ name = 'NSIS hook'; app = 'hive-app'; change = { param($c) $c.bundle.windows.nsis.installerHooks = '../scripts/nsis-mvp-hooks.nsh' } }
    )
    foreach ($mutation in $mutations) {
        Invoke-Case "reject changed $($mutation.name)" {
            $root = New-Fixture
            $path = Join-Path $root "$($mutation.app)/tauri.preview.conf.json"
            $config = Get-Content -LiteralPath $path -Raw | ConvertFrom-Json -AsHashtable
            & $mutation.change $config
            Write-FixtureConfig $path $config
            Assert-Rejected { Get-AbigailPackagingProfile -RepositoryRoot $root -Channel preview -Version '7.8.9' }
        }
    }
    foreach ($app in @('hive-app', 'entity-runtime-app')) {
        Invoke-Case "reject one-sided launch contract mutation in $app" {
            $root = New-Fixture
            $path = Join-Path $root "$app/tauri.preview.conf.json"
            $config = Get-Content -LiteralPath $path -Raw | ConvertFrom-Json -AsHashtable
            $config.plugins['abigail-launch'].releaseChannel = 'stable'
            Write-FixtureConfig $path $config
            Assert-Rejected { Get-AbigailPackagingProfile -RepositoryRoot $root -Channel preview -Version '7.8.9' }
        }
    }
    $contractMutations = @(
        @{ name = 'schema'; change = { param($c) $c.schemaVersion = 2 } },
        @{ name = 'marker'; change = { param($c) $c.contract = 'other' } },
        @{ name = 'channel'; change = { param($c) $c.channel = 'stable' } },
        @{ name = 'release channel'; change = { param($c) $c.releaseChannel = 'stable' } },
        @{ name = 'compiled environment name'; change = { param($c) $c.compiledProfile.environment = 'ABIGAIL_OTHER_PROFILE' } },
        @{ name = 'compiled environment value'; change = { param($c) $c.compiledProfile.value = 'stable' } },
        @{ name = 'known folder'; change = { param($c) $c.profileRoot.windowsKnownFolder = 'RoamingAppData' } },
        @{ name = 'profile under stable install'; change = { param($c) $c.profileRoot.relativePath = 'Abigail/profiles/ongoing' } },
        @{ name = 'profile under preview install'; change = { param($c) $c.profileRoot.relativePath = 'Abigail Preview/profiles/ongoing' } },
        @{ name = 'marker filename'; change = { param($c) $c.profileMarker.fileName = 'other.json' } },
        @{ name = 'marker schema'; change = { param($c) $c.profileMarker.schemaVersion = 2 } },
        @{ name = 'marker kind'; change = { param($c) $c.profileMarker.kind = 'stable' } },
        @{ name = 'marker channel'; change = { param($c) $c.profileMarker.channel = 'stable' } },
        @{ name = 'persistent ID field'; change = { param($c) $c.profileMarker.persistentIdField = 'entity_id' } },
        @{ name = 'unconfirmed runtime readiness'; change = { param($c) $c.runtimeInterface.status = 'ready' } },
        @{ name = 'withdrawn fixed profile ID'; change = { param($c) $c.profileId = 'default' } },
        @{ name = 'withdrawn daemon arguments'; change = { param($c) $c.daemonArguments = @('--channel', 'preview') } },
        @{ name = 'withdrawn child-root layout'; change = { param($c) $c.roots = @{ data = 'data' } } }
    )
    foreach ($mutation in $contractMutations) {
        Invoke-Case "reject matching mutations in both overlays: $($mutation.name)" {
            $root = New-Fixture
            foreach ($app in @('hive-app', 'entity-runtime-app')) {
                $path = Join-Path $root "$app/tauri.preview.conf.json"
                $config = Get-Content -LiteralPath $path -Raw | ConvertFrom-Json -AsHashtable
                & $mutation.change $config.plugins['abigail-launch']
                Write-FixtureConfig $path $config
            }
            Assert-Rejected { Get-AbigailPackagingProfile -RepositoryRoot $root -Channel preview -Version '7.8.9' }
        }
    }
    foreach ($app in @('hive-app', 'entity-runtime-app')) {
        foreach ($channel in @('stable', 'preview')) {
            Invoke-Case "reject changed stable base identity for $app in $channel" {
                $root = New-Fixture
                $path = Join-Path $root "$app/tauri.conf.json"
                $config = Get-Content -LiteralPath $path -Raw | ConvertFrom-Json -AsHashtable
                $config.identifier = 'com.example.changed'
                Write-FixtureConfig $path $config
                Assert-Rejected { Get-AbigailPackagingProfile -RepositoryRoot $root -Channel $channel -Version '7.8.9' }
            }
        }
    }
    Invoke-Case 'reject unsupported channel' {
        $root = New-Fixture
        Assert-Rejected { Get-AbigailPackagingProfile -RepositoryRoot $root -Channel other -Version '7.8.9' }
    }
    Invoke-Case 'reject a tag in the numeric packaging version field' {
        $root = New-Fixture
        Assert-Rejected { Get-AbigailPackagingProfile -RepositoryRoot $root -Channel preview -Version 'v7.8.9-beta.1' }
    }
    Assert-Test ((Get-ConfigSnapshot $repository) -ceq $originalConfigSnapshot) 'Source configs changed during these read-only tests.'
} finally {
    $absolute = [IO.Path]::GetFullPath($workspace)
    if ([IO.Path]::GetDirectoryName($absolute) -cne $tempRoot -or
        [IO.Path]::GetFileName($absolute) -notmatch '^abigail-preview-packaging-tests-[0-9a-f]{32}$') {
        throw 'Refusing cleanup outside the isolated test workspace.'
    }
    if (Test-Path -LiteralPath $absolute) { Remove-Item -LiteralPath $absolute -Recurse -Force }
}
if ($script:failures.Count) {
    throw "$($script:failures.Count) packaging tests failed; $script:passed passed.`n$($script:failures -join "`n")"
}
Write-Host "Preview packaging policy tests: $script:passed passed. Runtime compatibility remains untested."
