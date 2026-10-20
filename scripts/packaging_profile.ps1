# Pure packaging configuration. Does not resolve a user's profile or launch an app.
# Dot-source from the builders and static tests.
function Get-AbigailPackagingProfile {
    param(
        [Parameter(Mandatory)][string]$RepositoryRoot,
        [ValidateSet('stable', 'preview')][string]$Channel = 'stable',
        [Parameter(Mandatory)][ValidatePattern('^\d+\.\d+\.\d+$')][string]$Version
    )
    $hiveBase = Get-Content -LiteralPath (Join-Path $RepositoryRoot 'hive-app/tauri.conf.json') -Raw | ConvertFrom-Json -AsHashtable
    $runtimeBase = Get-Content -LiteralPath (Join-Path $RepositoryRoot 'entity-runtime-app/tauri.conf.json') -Raw | ConvertFrom-Json -AsHashtable
    if ($hiveBase.identifier -cne 'com.abigail' -or $hiveBase.productName -cne 'Abigail' -or
        $runtimeBase.identifier -cne 'com.abigail.entity-runtime' -or $runtimeBase.productName -cne 'Abigail Entity Runtime') {
        throw 'Stable base application identities differ from the reviewed packaging contract.'
    }
    $overlayHashes = [ordered]@{}
    if ($Channel -eq 'preview') {
        $hivePath = Join-Path $RepositoryRoot 'hive-app/tauri.preview.conf.json'
        $runtimePath = Join-Path $RepositoryRoot 'entity-runtime-app/tauri.preview.conf.json'
        $hive = Get-Content -LiteralPath $hivePath -Raw | ConvertFrom-Json -AsHashtable
        $runtime = Get-Content -LiteralPath $runtimePath -Raw | ConvertFrom-Json -AsHashtable
        $overlayHashes.hive = (Get-FileHash -LiteralPath $hivePath -Algorithm SHA256).Hash
        $overlayHashes.runtime = (Get-FileHash -LiteralPath $runtimePath -Algorithm SHA256).Hash
        if ($hive.productName -cne 'Abigail Preview' -or $hive.identifier -cne 'com.abigail.preview' -or
            $hive.mainBinaryName -cne 'Abigail Preview' -or $runtime.productName -cne 'Abigail Preview Entity Runtime' -or
            $runtime.identifier -cne 'com.abigail.preview.entity-runtime' -or
            @($hive.app.windows).Count -ne 1 -or $hive.app.windows[0].title -cne 'Abigail Preview Hive' -or
            @($runtime.app.windows).Count -ne 1 -or $runtime.app.windows[0].title -cne 'Abigail Preview Entity Runtime' -or
            $hive.bundle.windows.nsis.installMode -cne 'currentUser' -or
            $hive.bundle.windows.nsis.startMenuFolder -cne 'Abigail Preview' -or
            $hive.bundle.windows.nsis.installerHooks -cne '../scripts/nsis-preview-hooks.nsh') {
            throw 'Preview product, executable, compiled identities or NSIS identity changed.'
        }
        $launch = $hive.plugins['abigail-launch']
        $runtimeLaunch = $runtime.plugins['abigail-launch']
        if (($launch | ConvertTo-Json -Depth 20 -Compress) -cne ($runtimeLaunch | ConvertTo-Json -Depth 20 -Compress) -or
            $launch.schemaVersion -ne 1 -or $launch.contract -cne 'abigail-channel-profile-v1' -or
            $launch.channel -cne 'preview' -or $launch.releaseChannel -cne 'beta' -or
            $launch.compiledProfile.environment -cne 'ABIGAIL_BUILD_PROFILE' -or $launch.compiledProfile.value -cne 'preview' -or
            $launch.profileRoot.windowsKnownFolder -cne 'LocalAppData' -or
            $launch.profileRoot.relativePath -cne 'AbigailPreview/profiles/ongoing' -or
            $launch.profileMarker.fileName -cne 'profile.json' -or $launch.profileMarker.schemaVersion -ne 1 -or
            $launch.profileMarker.kind -cne 'preview' -or $launch.profileMarker.channel -cne 'beta' -or
            $launch.profileMarker.persistentIdField -cne 'profile_id' -or
            $launch.runtimeInterface.status -cne 'pending-app-owner' -or $launch.runtimeInterface.Count -ne 1 -or
            $launch.Contains('daemonArguments') -or $launch.Contains('profileId') -or $launch.Contains('roots')) {
            throw 'Preview launch contract differs between shells or from the reviewed profile contract.'
        }
    } else {
        $hive = @{ productName = $hiveBase.productName; identifier = $hiveBase.identifier; mainBinaryName = 'Abigail' }
        $runtime = @{ productName = $runtimeBase.productName; identifier = $runtimeBase.identifier }
        $launch = $null
    }
    $hive.version = $Version
    $runtime.version = $Version
    [pscustomobject][ordered]@{
        channel = $Channel
        productName = $hive.productName
        mainBinaryName = $hive.mainBinaryName
        launcherFileName = "$($hive.mainBinaryName).exe"
        hiveIdentifier = $hive.identifier
        runtimeIdentifier = $runtime.identifier
        runtimeProductName = $runtime.productName
        hiveOverlay = $hive
        runtimeOverlay = $runtime
        overlayHashes = $overlayHashes
        launchContract = $launch
    }
}

# This small ordered record is frozen into preparation and compared before any
# signing. It binds the chosen channel to both compiled overlays and their bytes.
function Get-AbigailPackagingStamp {
    param([Parameter(Mandatory)]$Profile)
    $contractHash = if ($null -ne $Profile.launchContract) {
        $json = $Profile.launchContract | ConvertTo-Json -Depth 20 -Compress
        [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($json)))
    } else { $null }
    [pscustomobject][ordered]@{
        channel = $Profile.channel
        productName = $Profile.productName
        launcherFileName = $Profile.launcherFileName
        hiveIdentifier = $Profile.hiveIdentifier
        runtimeIdentifier = $Profile.runtimeIdentifier
        buildProfile = if ($Profile.channel -eq 'preview') { 'preview' } else { $null }
        hiveOverlaySha256 = if ($Profile.channel -eq 'preview') { $Profile.overlayHashes.hive } else { $null }
        runtimeOverlaySha256 = if ($Profile.channel -eq 'preview') { $Profile.overlayHashes.runtime } else { $null }
        launchContractSha256 = $contractHash
    }
}
