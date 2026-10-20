#requires -Version 7.0
<#
Test-only stable daemon supervisor for an explicitly approved ephemeral Windows
hosted run. No Execute means plan only: no config read, native calls or writes.
Child output goes to NUL. This is a forced owned-job stop, not graceful shutdown.
#>
[CmdletBinding()]
param([switch]$Execute, [switch]$ApproveDpapiFixtures, [string]$ConfigPath)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Assert-OwnedJobHost([bool]$Approved, [hashtable]$HostEnvironment, [bool]$WindowsPlatform) {
    if (-not $Approved) { throw 'Disposable DPAPI fixture approval is required.' }
    if (-not $WindowsPlatform -or $HostEnvironment.GITHUB_ACTIONS -cne 'true' -or
        $HostEnvironment.RUNNER_ENVIRONMENT -cne 'github-hosted' -or $HostEnvironment.RUNNER_OS -cne 'Windows' -or
        $HostEnvironment.GITHUB_REPOSITORY -cne 'jbcupps/abigail') {
        throw 'Owned stable jobs require the approved GitHub-hosted Windows repository run.'
    }
    if (-not [IO.Path]::IsPathFullyQualified($HostEnvironment.RUNNER_TEMP)) { throw 'RUNNER_TEMP must be absolute.' }
}

function Get-OwnedFullPath([string]$Path) {
    if ([string]::IsNullOrWhiteSpace($Path) -or -not [IO.Path]::IsPathFullyQualified($Path) -or
        $Path.IndexOfAny([char[]]"`0`r`n") -ge 0 -or $Path.StartsWith('\\')) {
        throw 'Fixture paths must be absolute local paths.'
    }
    return [IO.Path]::GetFullPath($Path).TrimEnd('\', '/')
}

function Assert-OwnedChildPath([string]$Path, [string]$Parent) {
    $full = Get-OwnedFullPath $Path
    $root = Get-OwnedFullPath $Parent
    if (-not $full.StartsWith($root + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'Path escapes its owned fixture boundary.'
    }
    return $full
}

function Assert-OwnedJobConfig([Collections.IDictionary]$Config, [string]$RunnerTemp, [string]$SystemRoot) {
    $keys = @('schemaVersion', 'fixtureRoot', 'installRoot', 'dataRoot', 'workingDirectory', 'port', 'environment')
    if ($Config.Count -ne $keys.Count -or @($Config.Keys | Where-Object { $_ -cnotin $keys }).Count) {
        throw 'Unexpected owned-job config shape.'
    }
    if ($Config.schemaVersion -isnot [long] -and $Config.schemaVersion -isnot [int]) { throw 'Invalid job schema type.' }
    if ($Config.schemaVersion -ne 1) { throw 'Unsupported job schema.' }
    if (($Config.port -isnot [long] -and $Config.port -isnot [int]) -or $Config.port -lt 1024 -or $Config.port -gt 65535) {
        throw 'Invalid fixture port.'
    }
    foreach ($key in @('fixtureRoot', 'installRoot', 'dataRoot', 'workingDirectory')) {
        if ($Config[$key] -isnot [string]) { throw 'Invalid fixture path type.' }
    }
    $fixture = Assert-OwnedChildPath $Config.fixtureRoot $RunnerTemp
    if ([IO.Path]::GetDirectoryName($fixture) -ine (Get-OwnedFullPath $RunnerTemp) -or
        [IO.Path]::GetFileName($fixture) -cnotmatch '^abigail-preview-[A-Za-z0-9-]+$') {
        throw 'Invalid hosted fixture root.'
    }
    $install = Assert-OwnedChildPath $Config.installRoot $fixture
    $data = Assert-OwnedChildPath $Config.dataRoot $fixture
    $cwd = Assert-OwnedChildPath $Config.workingDirectory $fixture
    if ($install -ine (Join-Path $fixture 'install-stable') -or $data -ine (Join-Path $fixture 'profiles/stable')) {
        throw 'Stable install or profile does not match the owned fixture layout.'
    }
    $resources = Join-Path $install 'resources'
    $allowed = @('SystemRoot', 'WINDIR', 'SystemDrive', 'COMSPEC', 'PATHEXT', 'NUMBER_OF_PROCESSORS',
        'PROCESSOR_ARCHITECTURE', 'PATH', 'USERPROFILE', 'HOME', 'APPDATA', 'LOCALAPPDATA', 'TEMP', 'TMP',
        'RUST_LOG', 'ABIGAIL_DOCUMENTS_DIR', 'ABIGAIL_ENTITY_DAEMON_PATH', 'ABIGAIL_INTERNAL_BIN_DIR')
    $required = @('SystemRoot', 'PATH', 'USERPROFILE', 'APPDATA', 'LOCALAPPDATA', 'TEMP', 'TMP',
        'ABIGAIL_DOCUMENTS_DIR', 'ABIGAIL_ENTITY_DAEMON_PATH')
    if ($Config.environment -isnot [Collections.IDictionary]) { throw 'Invalid fixture environment.' }
    $environment = [Collections.Generic.Dictionary[string,string]]::new([StringComparer]::OrdinalIgnoreCase)
    foreach ($key in $Config.environment.Keys) {
        $value = $Config.environment[$key]
        if ($key -notin $allowed -or $environment.ContainsKey($key) -or $value -isnot [string] -or
            [string]::IsNullOrWhiteSpace($value) -or $value.IndexOfAny([char[]]"`0`r`n") -ge 0) {
            throw 'Fixture environment has an invalid or unapproved entry.'
        }
        $environment.Add($key, $value)
    }
    foreach ($key in $required) {
        if (-not $environment.ContainsKey($key)) { throw 'Required fixture environment entry is missing.' }
    }
    foreach ($key in @('USERPROFILE', 'HOME', 'APPDATA', 'LOCALAPPDATA', 'TEMP', 'TMP')) {
        if ($environment.ContainsKey($key)) { $null = Assert-OwnedChildPath $environment[$key] $fixture }
    }
    $windows = Get-OwnedFullPath $SystemRoot
    foreach ($key in @('SystemRoot', 'WINDIR')) {
        if ($environment.ContainsKey($key) -and (Get-OwnedFullPath $environment[$key]) -ine $windows) {
            throw 'Windows environment path mismatch.'
        }
    }
    if ($environment.ContainsKey('SystemDrive') -and $environment['SystemDrive'] -ine ([IO.Path]::GetPathRoot($windows).TrimEnd('\'))) {
        throw 'Windows drive mismatch.'
    }
    if ($environment.ContainsKey('COMSPEC') -and (Get-OwnedFullPath $environment['COMSPEC']) -ine (Join-Path $windows 'System32/cmd.exe')) {
        throw 'Windows command interpreter mismatch.'
    }
    $search = @($environment['PATH'].Split(';'))
    if ($search.Count -ne 2 -or $search[0] -ine $resources -or $search[1] -ine (Join-Path $windows 'System32')) {
        throw 'Fixture executable search path must contain only installed resources and Windows System32.'
    }
    if ((Get-OwnedFullPath $environment['ABIGAIL_DOCUMENTS_DIR']) -ine (Join-Path $data 'Documents') -or
        (Get-OwnedFullPath $environment['ABIGAIL_ENTITY_DAEMON_PATH']) -ine (Join-Path $resources 'entity-daemon.exe') -or
        ($environment.ContainsKey('ABIGAIL_INTERNAL_BIN_DIR') -and
            (Get-OwnedFullPath $environment['ABIGAIL_INTERNAL_BIN_DIR']) -ine $resources)) {
        throw 'Stable application fixture path mismatch.'
    }
    return @{
        fixtureRoot = $fixture; installRoot = $install; dataRoot = $data; workingDirectory = $cwd
        executable = Join-Path $resources 'hive-daemon.exe'; environment = $environment; port = [int]$Config.port
        arguments = [string[]]@('--port', [string]$Config.port, '--data-dir', $data)
    }
}

function Assert-OwnedExistingPath([string]$Path, [bool]$File) {
    $full = Get-OwnedFullPath $Path
    $item = Get-Item -LiteralPath $full -Force -ErrorAction Stop
    if ($File -eq [bool]$item.PSIsContainer) { throw 'Fixture path has an unexpected type.' }
    $cursor = $full
    while ($cursor) {
        $ancestor = Get-Item -LiteralPath $cursor -Force -ErrorAction Stop
        if (($ancestor.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
            throw 'Fixture paths cannot traverse reparse points.'
        }
        $cursor = [IO.Path]::GetDirectoryName($cursor)
    }
}

function ConvertFrom-OwnedJobJson([string]$Json) {
    $document = [Text.Json.JsonDocument]::Parse($Json)
    try {
        if ($document.RootElement.ValueKind -ne [Text.Json.JsonValueKind]::Object) { throw 'Config must be an object.' }
        $objects = [Collections.Generic.List[Text.Json.JsonElement]]::new()
        $objects.Add($document.RootElement)
        $environmentElement = [Text.Json.JsonElement]::new()
        if ($document.RootElement.TryGetProperty('environment', [ref]$environmentElement)) { $objects.Add($environmentElement) }
        foreach ($element in $objects) {
            if ($element.ValueKind -ne [Text.Json.JsonValueKind]::Object) { throw 'Environment must be an object.' }
            $names = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
            foreach ($property in $element.EnumerateObject()) {
                if (-not $names.Add($property.Name)) { throw 'Duplicate config property.' }
            }
        }
    } finally { $document.Dispose() }
    return ConvertFrom-Json -InputObject $Json -AsHashtable -Depth 8
}

function Wait-OwnedJobBinding($Job, [int]$Port, [scriptblock]$ReadListeners, [scriptblock]$Pause, [int]$Attempts = 240) {
    for ($attempt = 0; $attempt -lt $Attempts; $attempt++) {
        if ($Job.HasExited) { throw 'Owned stable process exited before binding.' }
        $listeners = @(& $ReadListeners $Port)
        if ($listeners.Count -gt 0) {
            if ($listeners.Count -ne 1 -or $listeners[0].LocalAddress -cne '127.0.0.1' -or
                $listeners[0].OwningProcess -ne $Job.Pid) {
                throw 'Stable fixture port belongs to a different listener.'
            }
            return
        }
        & $Pause
    }
    throw 'Owned stable listener did not become ready.'
}

function Invoke-OwnedJobSession([scriptblock]$StartJob, [scriptblock]$WaitBinding,
    [scriptblock]$ReadCommand, [scriptblock]$Emit, [int]$Port) {
    $job = & $StartJob
    try {
        & $WaitBinding $job $Port
        & $Emit @{ event = 'owned'; pid = $job.Pid; port = $Port }
        $command = & $ReadCommand
        if ($null -ne $command -and $command -cne 'stop') { throw 'Unrecognized owned-job control command.' }
        $exitCode = $job.Stop(30000)
        & $Emit @{ event = 'stopped'; exitCode = $exitCode; activeProcesses = 0; mode = 'forced-owned-job' }
    } finally { $job.Dispose() }
}

if (-not $Execute) {
    [Console]::Out.WriteLine('ABIGAIL_JOB:{"event":"plan","execute":false,"scope":"hosted-disposable-stable-job","nativeOperations":false}')
    return
}

try {
    Assert-OwnedJobHost ([bool]$ApproveDpapiFixtures) @{
        GITHUB_ACTIONS = $env:GITHUB_ACTIONS; RUNNER_ENVIRONMENT = $env:RUNNER_ENVIRONMENT
        RUNNER_OS = $env:RUNNER_OS; RUNNER_TEMP = $env:RUNNER_TEMP; GITHUB_REPOSITORY = $env:GITHUB_REPOSITORY
    } $IsWindows
    $configFull = Assert-OwnedChildPath $ConfigPath $env:RUNNER_TEMP
    Assert-OwnedExistingPath $configFull $true
    if ((Get-Item -LiteralPath $configFull).Length -gt 65536) { throw 'Owned-job config exceeds size limit.' }
    $config = ConvertFrom-OwnedJobJson ([IO.File]::ReadAllText($configFull))
    $checked = Assert-OwnedJobConfig $config $env:RUNNER_TEMP $env:SystemRoot
    $null = Assert-OwnedChildPath $configFull (Join-Path $checked.fixtureRoot 'control')
    foreach ($path in @($checked.fixtureRoot, $checked.installRoot, $checked.dataRoot, $checked.workingDirectory)) {
        Assert-OwnedExistingPath $path $false
    }
    foreach ($path in @($checked.executable, $checked.environment['ABIGAIL_ENTITY_DAEMON_PATH'])) {
        Assert-OwnedExistingPath $path $true
    }
    foreach ($key in @('USERPROFILE', 'HOME', 'APPDATA', 'LOCALAPPDATA', 'TEMP', 'TMP', 'ABIGAIL_DOCUMENTS_DIR')) {
        if ($checked.environment.ContainsKey($key)) { Assert-OwnedExistingPath $checked.environment[$key] $false }
    }
    Add-Type -Path (Join-Path $PSScriptRoot 'preview-owned-job.cs')
    $start = {
        [Abigail.Acceptance.OwnedStableJob]::Start(
            $checked.executable, $checked.arguments, $checked.workingDirectory, $checked.environment)
    }.GetNewClosure()
    $binding = {
        param($job, $port)
        Wait-OwnedJobBinding $job $port {
            param($selectedPort)
            # Read only, scoped by endpoint. Never used for process termination.
            Get-NetTCPConnection -State Listen -ErrorAction Stop |
                Where-Object { $_.LocalPort -eq $selectedPort -and $_.LocalAddress -in @('127.0.0.1', '0.0.0.0', '::', '::1') }
        } { Start-Sleep -Milliseconds 250 }
    }
    Invoke-OwnedJobSession $start $binding { [Console]::In.ReadLine() } {
        param($record)
        [Console]::Out.WriteLine('ABIGAIL_JOB:' + (ConvertTo-Json -InputObject $record -Compress))
        [Console]::Out.Flush()
    } $checked.port
} catch {
    # Exception details can contain fixture configuration. Emit no values.
    [Console]::Error.WriteLine('Owned stable job failed; completion is unconfirmed. Private job handles were closed where available.')
    exit 1
}
