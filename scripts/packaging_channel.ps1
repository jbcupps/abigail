# Channel identity for the Windows packaging lanes. Pure configuration: reads
# the reviewed Tauri configs and never resolves or opens a user's profile.
# Dot-source from builders, verifiers, and their tests.

function Read-AbigailJson([string]$Path) {
    Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json
}

function Get-AbigailPackagingChannel {
    param(
        [Parameter(Mandatory)][string]$RepositoryRoot,
        [ValidateSet('stable', 'preview')][string]$Channel = 'stable'
    )
    $hiveBase = Read-AbigailJson (Join-Path $RepositoryRoot 'hive-app/tauri.conf.json')
    $runtimeBase = Read-AbigailJson (Join-Path $RepositoryRoot 'entity-runtime-app/tauri.conf.json')
    if ($hiveBase.identifier -cne 'com.abigail' -or $hiveBase.productName -cne 'Abigail' -or
        $runtimeBase.identifier -cne 'com.abigail.entity-runtime') {
        throw 'Stable application identities changed; existing installs and data depend on them.'
    }

    if ($Channel -eq 'preview') {
        $hive = Read-AbigailJson (Join-Path $RepositoryRoot 'hive-app/tauri.preview.conf.json')
        $runtime = Read-AbigailJson (Join-Path $RepositoryRoot 'entity-runtime-app/tauri.preview.conf.json')
        foreach ($pair in @(@($hive, $hiveBase), @($runtime, $runtimeBase))) {
            $overlay, $base = $pair
            if (-not $overlay.identifier.StartsWith('com.abigail.preview', [StringComparison]::Ordinal) -or
                $overlay.identifier -ceq $base.identifier -or $overlay.productName -ceq $base.productName) {
                throw "Preview overlay must give $($base.productName) its own identifier and product name."
            }
        }
        if ([string]::IsNullOrWhiteSpace($hive.mainBinaryName) -or $hive.mainBinaryName -ceq 'Abigail') {
            throw 'Preview overlay must give the Hive launcher its own executable name.'
        }
        $productName = $hive.productName
        $mainBinaryName = $hive.mainBinaryName
        $hiveIdentifier = $hive.identifier
        $runtimeIdentifier = $runtime.identifier
        # Embedded into each shell at cargo build time. Bundle-only keys are
        # applied by the bundler configuration instead.
        $hiveBuildConfig = [ordered]@{
            productName = $hive.productName; identifier = $hive.identifier
            mainBinaryName = $hive.mainBinaryName; app = [ordered]@{ windows = @($hive.app.windows) }
        }
        $runtimeBuildConfig = [ordered]@{
            productName = $runtime.productName; identifier = $runtime.identifier
            app = [ordered]@{ windows = @($runtime.app.windows) }
        }
        $hooks = 'nsis-preview-hooks.nsh'
        $installerFileName = 'AbigailPreview-windows-x64-setup.exe'
        $defaultOutputDir = 'dist/windows-preview'
        $targetSubdir = 'preview'
        $foreign = 'stable'
    } else {
        $productName = $hiveBase.productName
        $mainBinaryName = 'Abigail'
        $hiveIdentifier = $hiveBase.identifier
        $runtimeIdentifier = $runtimeBase.identifier
        $hiveBuildConfig = [ordered]@{}
        $runtimeBuildConfig = [ordered]@{}
        $hooks = 'nsis-mvp-hooks.nsh'
        $installerFileName = 'Abigail-windows-x64-setup.exe'
        $defaultOutputDir = 'dist/windows-mvp'
        $targetSubdir = ''
        $foreign = 'preview'
    }

    [pscustomobject][ordered]@{
        Channel = $Channel
        ProductName = $productName
        MainBinaryName = $mainBinaryName
        LauncherFileName = "$mainBinaryName.exe"
        InstallerFileName = $installerFileName
        HiveIdentifier = $hiveIdentifier
        RuntimeIdentifier = $runtimeIdentifier
        HiveBuildConfig = $hiveBuildConfig
        RuntimeBuildConfig = $runtimeBuildConfig
        InstallerHooks = (Join-Path $RepositoryRoot "scripts/$hooks")
        # Matches crates/abigail-channel/build.rs.
        BuildMarker = "abigail-build-channel=$Channel;"
        ForeignBuildMarker = "abigail-build-channel=$foreign;"
        DefaultOutputDir = $defaultOutputDir
        TargetSubdir = $targetSubdir
    }
}

# Proves an executable was compiled for the requested channel. Hive, both
# daemons, and anything else linking abigail-channel carry exactly one marker.
# The Entity Runtime shell does not link it, so its Tauri identifier is checked.
function Assert-AbigailChannelBinary {
    param(
        [Parameter(Mandatory)][string]$Path,
        [Parameter(Mandatory)]$ChannelProfile,
        [ValidateSet('hive', 'runtime', 'daemon')][string]$Kind
    )
    $text = [Text.Encoding]::ASCII.GetString([IO.File]::ReadAllBytes($Path))
    $previewIdentity = $text.Contains('com.abigail.preview')
    if ($Kind -ne 'runtime') {
        if (-not $text.Contains($ChannelProfile.BuildMarker) -or $text.Contains($ChannelProfile.ForeignBuildMarker)) {
            throw "$Path was not compiled for the $($ChannelProfile.Channel) channel. Rebuild it with -Channel $($ChannelProfile.Channel)."
        }
    }
    if ($Kind -ne 'daemon') {
        $identifier = if ($Kind -eq 'hive') { $ChannelProfile.HiveIdentifier } else { $ChannelProfile.RuntimeIdentifier }
        $expectPreview = $ChannelProfile.Channel -eq 'preview'
        if ($previewIdentity -ne $expectPreview -or ($expectPreview -and -not $text.Contains($identifier))) {
            throw "$Path does not embed the $($ChannelProfile.Channel) app identity $identifier. Rebuild it with -Channel $($ChannelProfile.Channel)."
        }
    }
}
