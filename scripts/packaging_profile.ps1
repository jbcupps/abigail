# Pure packaging configuration. Does not resolve a user's profile or launch an app.
# Dot-source from the builders and static tests.
function Get-AbigailPreviewRuntimeInterface {
    # Describes implemented Rust behavior. This is not an executable recipe;
    # concrete roots, IDs, capabilities and endpoints are resolved at launch.
    @'
{
  "status": "implemented",
  "metadataRole": "descriptive-only",
  "launcher": {
    "defaultEnvironmentWhenAbsent": ["ABIGAIL_PROFILE", "ABIGAIL_DATA_DIR"],
    "blankOrConflictingEnvironment": "reject",
    "inheritedHiveUrl": "reject",
    "markerInitialization": "new-empty-root-only",
    "childMarkerInitialization": "forbidden"
  },
  "daemonCli": {
    "commonOptions": ["--profile", "--data-dir"],
    "hiveOptions": ["--port"],
    "entityOptions": ["--entity-id", "--hive-url", "--port"],
    "contextAgreement": "required",
    "hivePort": "ephemeral-outside-43141-43150",
    "entityPort": "os-assigned"
  },
  "childEnvironment": {
    "profile": "ABIGAIL_PROFILE",
    "dataRoot": "ABIGAIL_DATA_DIR",
    "profileId": "ABIGAIL_PROFILE_ID",
    "hiveInstanceId": "ABIGAIL_HIVE_INSTANCE_ID",
    "processInstanceId": "ABIGAIL_PROCESS_INSTANCE_ID",
    "documentsOverride": "ABIGAIL_DOCUMENTS_DIR"
  },
  "runtimeSelectionEnvironment": ["ABIGAIL_ENTITY_ID", "ABIGAIL_ENTITY_URL", "ABIGAIL_HIVE_URL"],
  "packagedExecutableEnvironment": ["ABIGAIL_ENTITY_DAEMON_PATH", "ABIGAIL_ENTITY_RUNTIME_APP_PATH", "ABIGAIL_INTERNAL_BIN_DIR"],
  "processIdentity": {
    "fields": ["channel", "profile_id", "canonical_data_root", "instance_id", "hive_instance_id"],
    "verification": "all-fields-and-selected-entity",
    "hiveInstance": "fresh-equals-hive-daemon-instance",
    "entityInstance": "fresh-per-entity-daemon",
    "runtimeGuiExpectedInstance": "selected-entity-daemon-instance"
  },
  "http": {
    "pinnedHeaders": ["x-abigail-profile-id", "x-abigail-process-instance-id"],
    "unpinnedRequests": ["GET /health", "GET /v1/status", "OPTIONS"],
    "purpose": "routing-guards-not-authentication",
    "endpoint": "verified-loopback-outside-stable-ports",
    "redirects": "reject",
    "nativeFallback": "reject"
  },
  "lifecycle": {
    "entityClose": "POST /v1/entities/:id/close",
    "entityCloseInstanceField": "expected_entity_instance_id",
    "responseValidation": "api-envelope",
    "hiveShutdown": "POST /v1/shutdown",
    "hiveShutdownAcknowledgement": ["shutdown", "owned_entities_reaped"],
    "hiveReplacement": "acknowledged-drain-and-owned-child-exit",
    "stalePidTermination": "forbidden",
    "staleCreationEndpoint": "reject-preserve-exact-key"
  },
  "dynamicValueSources": {
    "dataRoot": "validated-launch-context",
    "profileId": "persistent-profile-marker",
    "processInstances": "fresh-launch-uuids",
    "endpoints": "verified-live-daemons",
    "runtimeBootstrap": "one-use-generated-capability"
  },
  "candidateAcceptance": "pending"
}
'@ | ConvertFrom-Json -AsHashtable
}

function Test-AbigailContractValue($Actual, $Expected) {
    # Compare a closed schema without PowerShell's coercive equality. Reject
    # unknown/nested dynamic values, missing fields and wrong JSON types.
    if ($Expected -is [Collections.IDictionary]) {
        if ($Actual -isnot [Collections.IDictionary] -or $Actual.Count -ne $Expected.Count) { return $false }
        foreach ($key in $Expected.Keys) {
            if (@($Actual.Keys | Where-Object { $_ -ceq $key }).Count -ne 1 -or
                -not (Test-AbigailContractValue $Actual[$key] $Expected[$key])) { return $false }
        }
        return $true
    }
    if ($Expected -is [array]) {
        if ($Actual -isnot [array] -or $Actual.Count -ne $Expected.Count) { return $false }
        for ($index = 0; $index -lt $Expected.Count; $index++) {
            if (-not (Test-AbigailContractValue $Actual[$index] $Expected[$index])) { return $false }
        }
        return $true
    }
    return ($null -ne $Actual -and $Actual.GetType() -eq $Expected.GetType() -and $Actual -ceq $Expected)
}

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
        $expectedLaunch = @{
            schemaVersion = [long]1; contract = 'abigail-channel-profile-v1'
            channel = 'preview'; releaseChannel = 'beta'
            compiledProfile = @{ environment = 'ABIGAIL_BUILD_PROFILE'; value = 'preview' }
            profileRoot = @{ windowsKnownFolder = 'LocalAppData'; relativePath = 'AbigailPreview/profiles/ongoing' }
            profileMarker = @{
                fileName = 'profile.json'; schemaVersion = [long]1
                kind = 'preview'; channel = 'beta'; persistentIdField = 'profile_id'
            }
            runtimeInterface = Get-AbigailPreviewRuntimeInterface
        }
        if (-not (Test-AbigailContractValue $launch $expectedLaunch) -or
            -not (Test-AbigailContractValue $runtimeLaunch $expectedLaunch)) {
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
