#requires -Version 7.0
<# Credential-free tests: parse definitions, compile C# declarations and call
   pure helpers/mock objects only. Never creates a native job or app process,
   opens a keyring, invokes DPAPI, installs, signs, or reads live TCP state. #>
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$script:passed = 0
$wrapper = Join-Path $PSScriptRoot 'preview-owned-job.ps1'
$native = Join-Path $PSScriptRoot 'preview-owned-job.cs'

function Assert-Test([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
}
function Test-Case([string]$Name, [scriptblock]$Body) {
    & $Body
    $script:passed++
    Write-Host "PASS $Name"
}
function Assert-Rejected([scriptblock]$Body) {
    $rejected = $false
    try { & $Body | Out-Null } catch { $rejected = $true }
    Assert-Test $rejected 'Expected rejection.'
}

$tokens = $null
$parseErrors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile($wrapper, [ref]$tokens, [ref]$parseErrors)
Test-Case 'wrapper parses' { Assert-Test ($parseErrors.Count -eq 0) 'Wrapper syntax errors.' }
# Load only function definitions; the execution branch is never evaluated.
foreach ($definition in $ast.FindAll({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] }, $false)) {
    . ([scriptblock]::Create($definition.Extent.Text))
}

Test-Case 'default invocation is plan only, even with a nonexistent config' {
    # No Execute: this must return before reading the intentionally absent path.
    & $wrapper -ConfigPath 'C:\nonexistent-abigail-fixture\control.json'
    Assert-Test (-not ('Abigail.Acceptance.OwnedStableJob' -as [type])) 'Plan loaded native class.'
}

$hostFixture = @{
    GITHUB_ACTIONS = 'true'; RUNNER_ENVIRONMENT = 'github-hosted'; RUNNER_OS = 'Windows'
    RUNNER_TEMP = 'C:\RunnerTemp'; GITHUB_REPOSITORY = 'jbcupps/abigail'
}
Test-Case 'approved hosted context passes pure guard' { Assert-OwnedJobHost $true $hostFixture $true }
Test-Case 'missing fixture approval rejected' { Assert-Rejected { Assert-OwnedJobHost $false $hostFixture $true } }
Test-Case 'non-Windows platform rejected' { Assert-Rejected { Assert-OwnedJobHost $true $hostFixture $false } }
foreach ($field in @('GITHUB_ACTIONS', 'RUNNER_ENVIRONMENT', 'RUNNER_OS', 'GITHUB_REPOSITORY', 'RUNNER_TEMP')) {
    Test-Case "invalid host $field rejected" {
        $bad = $hostFixture.Clone(); $bad[$field] = 'wrong'
        Assert-Rejected { Assert-OwnedJobHost $true $bad $true }
    }
}

function New-Config {
    $root = 'C:\RunnerTemp\abigail-preview-42-test'
    return @{
        schemaVersion = 1; fixtureRoot = $root; installRoot = "$root\install-stable"
        dataRoot = "$root\profiles\stable"; workingDirectory = "$root\profiles\stable"; port = 21001
        environment = @{
            SystemRoot = 'C:\Windows'; PATH = "$root\install-stable\resources;C:\Windows\System32"
            USERPROFILE = "$root\user\stable"; HOME = "$root\user\stable"
            APPDATA = "$root\user\stable\AppData\Roaming"; LOCALAPPDATA = "$root\user\stable\AppData\Local"
            TEMP = "$root\user\stable\Temp"; TMP = "$root\user\stable\Temp"
            ABIGAIL_DOCUMENTS_DIR = "$root\profiles\stable\Documents"
            ABIGAIL_ENTITY_DAEMON_PATH = "$root\install-stable\resources\entity-daemon.exe"
            ABIGAIL_INTERNAL_BIN_DIR = "$root\install-stable\resources"
        }
    }
}
function Test-Config($config) { Assert-OwnedJobConfig $config 'C:\RunnerTemp' 'C:\Windows' }
Test-Case 'valid config derives only exact stable executable and arguments' {
    $checked = Test-Config (New-Config)
    Assert-Test ($checked.executable -ceq 'C:\RunnerTemp\abigail-preview-42-test\install-stable\resources\hive-daemon.exe') 'Unexpected executable.'
    Assert-Test (($checked.arguments -join '|') -ceq '--port|21001|--data-dir|C:\RunnerTemp\abigail-preview-42-test\profiles\stable') 'Unexpected arguments.'
}
Test-Case 'JSON roundtrip preserves approved config' {
    $config = ConvertFrom-OwnedJobJson (New-Config | ConvertTo-Json -Depth 5)
    $null = Test-Config $config
}
$mutations = [ordered]@{
    'schema version' = { param($c) $c.schemaVersion = 2 }
    'schema string' = { param($c) $c.schemaVersion = '1' }
    'extra executable key' = { param($c) $c.executable = 'C:\Windows\cmd.exe' }
    'missing key' = { param($c) $c.Remove('port') }
    'privileged port' = { param($c) $c.port = 80 }
    'port overflow' = { param($c) $c.port = 65536 }
    'port string' = { param($c) $c.port = '21001' }
    'port fraction' = { param($c) $c.port = 21001.5 }
    'external fixture' = { param($c) $c.fixtureRoot = 'C:\RealProfile' }
    'prefix collision' = { param($c) $c.fixtureRoot = 'C:\RunnerTemp-other\abigail-preview-42-test' }
    'fixture root itself' = { param($c) $c.fixtureRoot = 'C:\RunnerTemp' }
    'nested fixture' = { param($c) $c.fixtureRoot = 'C:\RunnerTemp\outer\abigail-preview-42-test' }
    'wrong fixture prefix' = { param($c) $c.fixtureRoot = 'C:\RunnerTemp\other-42-test' }
    'wrong stable install' = { param($c) $c.installRoot = "$($c.fixtureRoot)\install-preview" }
    'wrong stable profile' = { param($c) $c.dataRoot = "$($c.fixtureRoot)\profiles\ongoing" }
    'relative cwd' = { param($c) $c.workingDirectory = '.' }
    'escaped cwd' = { param($c) $c.workingDirectory = "$($c.fixtureRoot)\..\other" }
    'network cwd' = { param($c) $c.workingDirectory = '\\host\share\test' }
    'secret environment' = { param($c) $c.environment.GITHUB_TOKEN = 'synthetic-value' }
    'preview profile environment' = { param($c) $c.environment.ABIGAIL_PROFILE = 'preview' }
    'ambient home' = { param($c) $c.environment.USERPROFILE = 'C:\Users\RealUser' }
    'ambient temp' = { param($c) $c.environment.TEMP = 'C:\Windows\Temp' }
    'missing environment entry' = { param($c) $c.environment.Remove('SystemRoot') }
    'non-string environment value' = { param($c) $c.environment.TEMP = 42 }
    'empty environment value' = { param($c) $c.environment.TEMP = '' }
    'environment newline' = { param($c) $c.environment.RUST_LOG = "info`nsecret" }
    'different Windows root' = { param($c) $c.environment.SystemRoot = 'D:\Windows' }
    'different drive' = { param($c) $c.environment.SystemDrive = 'D:' }
    'different cmd' = { param($c) $c.environment.COMSPEC = "$($c.fixtureRoot)\cmd.exe" }
    'expanded search path' = { param($c) $c.environment.PATH += ';C:\untrusted' }
    'empty search path entry' = { param($c) $c.environment.PATH += ';' }
    'reversed search path' = { param($c) $c.environment.PATH = "C:\Windows\System32;$($c.installRoot)\resources" }
    'external documents' = { param($c) $c.environment.ABIGAIL_DOCUMENTS_DIR = 'C:\Users\RealUser\Documents' }
    'different entity binary' = { param($c) $c.environment.ABIGAIL_ENTITY_DAEMON_PATH = "$($c.installRoot)\entity-daemon.exe" }
    'different internal binaries' = { param($c) $c.environment.ABIGAIL_INTERNAL_BIN_DIR = "$($c.fixtureRoot)\other" }
}
foreach ($entry in $mutations.GetEnumerator()) {
    Test-Case "config rejects $($entry.Key)" {
        $config = New-Config
        & $entry.Value $config | Out-Null
        Assert-Rejected { Test-Config $config }
    }
}
Test-Case 'duplicate JSON top-level property rejected' {
    Assert-Rejected { ConvertFrom-OwnedJobJson '{"schemaVersion":1,"SCHEMAVERSION":1,"environment":{}}' }
}
Test-Case 'duplicate JSON environment property rejected' {
    Assert-Rejected { ConvertFrom-OwnedJobJson '{"environment":{"PATH":"a","Path":"b"}}' }
}
Test-Case 'non-object JSON rejected' { Assert-Rejected { ConvertFrom-OwnedJobJson '[]' } }

function New-MockJob {
    $mock = [pscustomobject]@{
        Pid = 4321; HasExited = $false; StopCount = 0; DisposeCount = 0; FailStop = $false
    }
    $mock | Add-Member ScriptMethod Stop {
        param($timeout)
        if ($timeout -ne 30000) { throw 'Unexpected timeout.' }
        $this.StopCount++
        if ($this.FailStop) { throw 'Mock active process remains.' }
        return 0
    }
    $mock | Add-Member ScriptMethod Dispose { $this.DisposeCount++ }
    return $mock
}
Test-Case 'binding accepts only exact newly created PID on loopback' {
    Wait-OwnedJobBinding (New-MockJob) 21001 {
        [pscustomobject]@{ LocalAddress = '127.0.0.1'; OwningProcess = 4321 }
    } { throw 'Unexpected pause.' } 1
}
Test-Case 'binding refuses unrelated listener race' {
    Assert-Rejected {
        Wait-OwnedJobBinding (New-MockJob) 21001 {
            [pscustomobject]@{ LocalAddress = '127.0.0.1'; OwningProcess = 9999 }
        } {} 1
    }
}
Test-Case 'binding refuses wildcard listener' {
    Assert-Rejected {
        Wait-OwnedJobBinding (New-MockJob) 21001 {
            [pscustomobject]@{ LocalAddress = '0.0.0.0'; OwningProcess = 4321 }
        } {} 1
    }
}
Test-Case 'binding refuses ambiguous listeners' {
    Assert-Rejected {
        Wait-OwnedJobBinding (New-MockJob) 21001 {
            [pscustomobject]@{ LocalAddress = '127.0.0.1'; OwningProcess = 4321 }
            [pscustomobject]@{ LocalAddress = '::'; OwningProcess = 9999 }
        } {} 1
    }
}
Test-Case 'binding times out without adopting stale state' {
    Assert-Rejected { Wait-OwnedJobBinding (New-MockJob) 21001 {} {} 1 }
}
Test-Case 'binding notices retained process already exited' {
    $mock = New-MockJob; $mock.HasExited = $true
    Assert-Rejected { Wait-OwnedJobBinding $mock 21001 { throw 'Must not inspect listeners.' } {} 1 }
}
foreach ($command in @('stop', $null)) {
    Test-Case "control stop or EOF confirms owned-job completion ($command)" {
        $job = New-MockJob
        $events = [Collections.Generic.List[object]]::new()
        Invoke-OwnedJobSession { $job } {} { $command } { param($record) $events.Add($record) } 21001
        Assert-Test ($events.Count -eq 2 -and $events[0].event -ceq 'owned' -and
            $events[0].pid -eq 4321 -and $events[0].port -eq 21001) 'Owned event mismatch.'
        Assert-Test ($events[1].event -ceq 'stopped' -and $events[1].activeProcesses -eq 0 -and
            $events[1].mode -ceq 'forced-owned-job') 'Completion event mismatch.'
        Assert-Test ($job.StopCount -eq 1 -and $job.DisposeCount -eq 1) 'Stop/close not exactly once.'
    }
}
Test-Case 'unknown control command closes job without success claim' {
    $job = New-MockJob
    $events = [Collections.Generic.List[object]]::new()
    Assert-Rejected {
        Invoke-OwnedJobSession { $job } {} { 'kill 9999' } { param($record) $events.Add($record) } 21001
    }
    Assert-Test ($events.Count -eq 1 -and $job.DisposeCount -eq 1 -and $job.StopCount -eq 0) 'Unknown command did not fail closed.'
}
Test-Case 'binding failure closes job before any owned event' {
    $job = New-MockJob
    $events = [Collections.Generic.List[object]]::new()
    Assert-Rejected {
        Invoke-OwnedJobSession { $job } { throw 'Mock wrong owner.' } { 'stop' } { param($record) $events.Add($record) } 21001
    }
    Assert-Test ($events.Count -eq 0 -and $job.DisposeCount -eq 1) 'Readiness failure reported ownership or leaked job.'
}
Test-Case 'unconfirmed stop never emits stopped' {
    $job = New-MockJob; $job.FailStop = $true
    $events = [Collections.Generic.List[object]]::new()
    Assert-Rejected {
        Invoke-OwnedJobSession { $job } {} { 'stop' } { param($record) $events.Add($record) } 21001
    }
    Assert-Test ($events.Count -eq 1 -and $job.DisposeCount -eq 1) 'Unconfirmed stop claimed completion.'
}
Test-Case 'native declarations compile without running native APIs' {
    Add-Type -Path $native
    Assert-Test ($null -ne ('Abigail.Acceptance.OwnedStableJob' -as [type])) 'Native declarations did not compile.'
}
Test-Case 'command quoting handles empty, spaces, quotes and trailing backslashes' {
    foreach ($vector in @(
        @('', '""'), @('plain', '"plain"'), @('two words', '"two words"'),
        @('a"b', '"a\"b"'), @('C:\path\', '"C:\path\\"'), @('a\"b', '"a\\\"b"')
    )) {
        Assert-Test ([Abigail.Acceptance.OwnedStableJob]::QuoteArgument($vector[0]) -ceq $vector[1]) 'Quote vector mismatch.'
    }
    Assert-Rejected { [Abigail.Acceptance.OwnedStableJob]::QuoteArgument("bad`0value") }
}
Test-Case 'environment block is sorted and double-NUL terminated' {
    $dictionary = [Collections.Generic.Dictionary[string,string]]::new()
    $dictionary.Add('Z_LAST', 'z'); $dictionary.Add('A_FIRST', 'a')
    $block = [Abigail.Acceptance.OwnedStableJob]::BuildEnvironment($dictionary)
    Assert-Test ($block -ceq "A_FIRST=a`0Z_LAST=z`0`0") 'Environment block mismatch.'
}
Test-Case 'environment rejects duplicate case-insensitive keys and invalid separators' {
    $dictionary = [Collections.Generic.Dictionary[string,string]]::new()
    $dictionary.Add('PATH', 'a'); $dictionary.Add('Path', 'b')
    Assert-Rejected { [Abigail.Acceptance.OwnedStableJob]::BuildEnvironment($dictionary) }
    $dictionary.Clear(); $dictionary.Add('BAD=KEY', 'x')
    Assert-Rejected { [Abigail.Acceptance.OwnedStableJob]::BuildEnvironment($dictionary) }
    $dictionary.Clear(); $dictionary.Add('KEY', "bad`0value")
    Assert-Rejected { [Abigail.Acceptance.OwnedStableJob]::BuildEnvironment($dictionary) }
}
Test-Case 'native startup order and handle scope remain explicit' {
    $source = [IO.File]::ReadAllText($native)
    $create = $source.IndexOf('Check(CreateProcessW(')
    $assign = $source.IndexOf('Check(AssignProcessToJobObject(')
    $resume = $source.IndexOf('Check(ResumeThread(')
    Assert-Test ($create -lt $assign -and $assign -lt $resume) 'Suspended assignment order changed.'
    Assert-Test ($source.Contains('JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE') -and
        $source.Contains('new IntPtr(0x20002)') -and $source.Contains('accounting.ActiveProcesses == 0 && HasExited')) 'Required ownership guard missing.'
    Assert-Test (-not ($source -match 'OpenProcess|GetProcesses|Process\.Kill|CREATE_BREAKAWAY_FROM_JOB')) 'Global or reopened-process operation present.'
}
Write-Host "PASS: $script:passed owned-job checks. No native job/process, DPAPI, keyring, TCP inspection, install or signing executed."
