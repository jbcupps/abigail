#requires -Version 7.0
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'windows-signing-state.psm1') -Force

$repoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$workspaceName = 'signing state tests ' + [guid]::NewGuid().ToString('N')
$testWorkspace = [IO.Path]::GetFullPath((Join-Path $repoRoot $workspaceName))
$createdLinks = [Collections.Generic.List[string]]::new()
$passed = 0
$caseNumber = 0

function Assert-Test($Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
}

function Assert-TestPath([string]$Path) {
    $absolute = [IO.Path]::GetFullPath($Path)
    $prefix = $testWorkspace + [IO.Path]::DirectorySeparatorChar
    if (-not $absolute.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Test path escaped the allocated workspace: '$absolute'."
    }
    return $absolute
}

function Write-FixtureFile([string]$Path, [string]$Content) {
    $absolute = Assert-TestPath -Path $Path
    $null = [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($absolute))
    [IO.File]::WriteAllText($absolute, $Content, [Text.UTF8Encoding]::new($false))
}

function New-Fixture {
    $script:caseNumber++
    $root = Join-Path $testWorkspace "case $script:caseNumber"
    $bootstrap = Join-Path $root 'bootstrap payload'
    $binaryPaths = @(
        (Join-Path $root 'release/abigail-hive-app.exe'),
        (Join-Path $root 'resources/abigail-entity-runtime-app.exe'),
        (Join-Path $root 'resources/hive-daemon.exe'),
        (Join-Path $root 'resources/entity-daemon.exe')
    )
    for ($index = 0; $index -lt 4; $index++) {
        Write-FixtureFile $binaryPaths[$index] "Synthetic unsigned binary $index"
    }
    foreach ($relative in @('runtime/engine.bin', 'models/tiny.bin', 'licenses/LICENSE.txt', 'NOTICE', '.metadata', 'literal [1].txt')) {
        Write-FixtureFile (Join-Path $bootstrap $relative) "Synthetic bootstrap: $relative"
    }
    if ($IsWindows) {
        $hiddenPath = Join-Path $bootstrap '.metadata'
        [IO.File]::SetAttributes($hiddenPath, [IO.FileAttributes]::Hidden -bor [IO.FileAttributes]::System)
    }
    $configPath = Join-Path $root 'bundle config.json'
    Write-FixtureFile $configPath '{"version":"1.2.3","bundle":{"windows":{"signCommand":null}}}'
    $state = [ordered]@{
        sourceCommit = 'a' * 40
        version = '1.2.3'
        configSha256 = (Get-FileHash -LiteralPath $configPath -Algorithm SHA256).Hash
        binaries = @($binaryPaths | ForEach-Object {
            [pscustomobject]@{ path = $_; sha256 = (Get-FileHash -LiteralPath $_ -Algorithm SHA256).Hash }
        })
        bootstrap = @(Get-AbigailFileManifest -Root $bootstrap)
    }
    [pscustomobject]@{
        Root = $root
        Bootstrap = $bootstrap
        ConfigPath = $configPath
        BinaryPaths = $binaryPaths
        State = ($state | ConvertTo-Json -Depth 12 | ConvertFrom-Json)
        Arguments = @{
            ExpectedCommit = 'a' * 40
            ExpectedVersion = '1.2.3'
            ExpectedBinaryPaths = $binaryPaths
            ConfigPath = $configPath
            BootstrapRoot = $bootstrap
        }
    }
}

function Assert-Fixture($Fixture, $State = $Fixture.State) {
    $arguments = $Fixture.Arguments
    Assert-AbigailPreparedInputs -State $State @arguments
}

function Assert-NoOutput([scriptblock]$Action) {
    $output = @(& $Action)
    Assert-Test ($output.Count -eq 0) 'Successful assertion wrote to the pipeline.'
}

function Assert-Rejected([scriptblock]$Action, [string]$Pattern) {
    $failure = $null
    try { $null = & $Action } catch { $failure = $_.Exception.Message }
    Assert-Test ($null -ne $failure) 'Invalid prepared input was accepted.'
    Assert-Test ($failure -like '*Prepare must be rerun.') "Missing Prepare guidance: $failure"
    Assert-Test ($failure -match $Pattern) "Expected '$Pattern'; received: $failure"
}

function Invoke-RegressionCheck([string]$Name, [scriptblock]$Action) {
    $fixture = New-Fixture
    try { & $Action $fixture } catch { throw "FAILED ${Name}: $($_.Exception.Message)" }
    $script:passed++
    Write-Host "PASS $Name"
}

function New-TestJunction([string]$Path, [string]$Target) {
    $absolute = Assert-TestPath $Path
    $destination = Assert-TestPath $Target
    $null = New-Item -ItemType Junction -Path $absolute -Target $destination -ErrorAction Stop
    $createdLinks.Add($absolute)
}

# Never reuse an existing directory, including any other agent's fixtures.
if (Test-Path -LiteralPath $testWorkspace) { throw 'Unique test workspace already exists.' }
$null = New-Item -ItemType Directory -Path $testWorkspace
try {
    Invoke-RegressionCheck 'JSON roundtrip, sorted portable complete manifest, silent assertions' {
        param($f)
        $manifest = @($f.State.bootstrap)
        Assert-Test ($manifest.Count -eq 6) 'Manifest omitted a fixture file.'
        Assert-Test (($manifest.path -join '|') -ceq '.metadata|NOTICE|licenses/LICENSE.txt|literal [1].txt|models/tiny.bin|runtime/engine.bin') 'Manifest was not sorted with portable paths.'
        Assert-NoOutput { Assert-Fixture $f }
        Assert-NoOutput { Assert-AbigailFileManifest -Root $f.Bootstrap -Manifest $manifest }
        $again = @(Get-AbigailFileManifest -Root $f.Bootstrap)
        Assert-Test (($again | ConvertTo-Json -Compress) -ceq ($manifest | ConvertTo-Json -Compress)) 'Validation changed the bootstrap files.'
        $hashState = $f.State | ConvertTo-Json -Depth 12 | ConvertFrom-Json -AsHashtable
        Assert-NoOutput { Assert-Fixture $f $hashState }
    }
    Invoke-RegressionCheck 'empty and single-entry manifests survive JSON' {
        param($f)
        $emptyRoot = Join-Path $f.Root 'empty bootstrap'
        $null = [IO.Directory]::CreateDirectory($emptyRoot)
        $f.Arguments.BootstrapRoot = $emptyRoot
        $f.State.bootstrap = @(Get-AbigailFileManifest -Root $emptyRoot)
        $roundtrip = $f.State | ConvertTo-Json -Depth 12 | ConvertFrom-Json
        Assert-NoOutput { Assert-Fixture $f $roundtrip }
        Write-FixtureFile (Join-Path $emptyRoot 'LICENSE') 'Only license'
        $f.State.bootstrap = @(Get-AbigailFileManifest -Root $emptyRoot)
        $roundtrip = $f.State | ConvertTo-Json -Depth 12 | ConvertFrom-Json
        Assert-NoOutput { Assert-Fixture $f $roundtrip }
    }
    Invoke-RegressionCheck 'case-insensitive Windows binary comparison and hexadecimal hashes' {
        param($f)
        foreach ($entry in $f.State.binaries) {
            $entry.path = $entry.path.ToUpperInvariant()
            $entry.sha256 = $entry.sha256.ToLowerInvariant()
        }
        $f.State.configSha256 = $f.State.configSha256.ToLowerInvariant()
        Assert-NoOutput { Assert-Fixture $f }
    }
    Invoke-RegressionCheck 'config drift' {
        param($f)
        Write-FixtureFile $f.ConfigPath '{"version":"9.9.9"}'
        Assert-Rejected { Assert-Fixture $f } 'config changed'
    }
    Invoke-RegressionCheck 'missing config hash in old state' {
        param($f)
        $f.State.PSObject.Properties.Remove('configSha256')
        Assert-Rejected { Assert-Fixture $f } "missing 'configSha256'"
    }
    Invoke-RegressionCheck 'added bootstrap file' {
        param($f)
        Write-FixtureFile (Join-Path $f.Bootstrap 'licenses/NEW-LICENSE.txt') 'Added license'
        Assert-Rejected { Assert-Fixture $f } 'Bootstrap file added'
    }
    Invoke-RegressionCheck 'removed bootstrap file' {
        param($f)
        $removed = Assert-TestPath (Join-Path $f.Bootstrap 'licenses/LICENSE.txt')
        Remove-Item -LiteralPath $removed
        Assert-Rejected { Assert-Fixture $f } 'Bootstrap file missing'
    }
    Invoke-RegressionCheck 'changed bootstrap license' {
        param($f)
        Write-FixtureFile (Join-Path $f.Bootstrap 'licenses/LICENSE.txt') 'Changed license'
        Assert-Rejected { Assert-Fixture $f } 'Bootstrap file changed'
    }
    Invoke-RegressionCheck 'unsafe manifest paths' {
        param($f)
        $unsafePaths = @(
            '../outside.txt', 'licenses/../../outside.txt', '/absolute.txt',
            'C:/outside.txt', 'C:relative.txt', '\\server\share\file.txt',
            '\\?\C:\file.txt', 'licenses\LICENSE.txt', 'licenses//LICENSE.txt',
            'licenses/./LICENSE.txt', 'licenses/../LICENSE.txt', 'NOTICE:stream',
            'NOTICE.', 'NOTICE ', 'NUL', 'CON.txt', 'COM1.bin', 'licenses/',
            'wild*.txt', 'wild?.txt', "bad`nname", '', $null
        )
        foreach ($path in $unsafePaths) {
            $badManifest = @([pscustomobject]@{ path = $path; sha256 = '0' * 64 })
            Assert-Rejected { Assert-AbigailFileManifest -Root $f.Bootstrap -Manifest $badManifest } 'Unsafe manifest path'
        }
    }
    Invoke-RegressionCheck 'exact duplicate manifest entries' {
        param($f)
        $f.State.bootstrap += $f.State.bootstrap[0]
        Assert-Rejected { Assert-Fixture $f } 'Duplicate manifest path'
    }
    Invoke-RegressionCheck 'case-variant duplicate manifest entries' {
        param($f)
        $first = $f.State.bootstrap[0]
        $f.State.bootstrap += [pscustomobject]@{ path = $first.path.ToUpperInvariant(); sha256 = $first.sha256 }
        Assert-Rejected { Assert-Fixture $f } 'Duplicate manifest path'
    }
    Invoke-RegressionCheck 'wrong binary order' {
        param($f)
        $f.State.binaries = @($f.State.binaries[1], $f.State.binaries[0], $f.State.binaries[2], $f.State.binaries[3])
        Assert-Rejected { Assert-Fixture $f } 'binary path/order mismatch'
    }
    Invoke-RegressionCheck 'arbitrary binary path with a matching hash' {
        param($f)
        $other = Join-Path $f.Root 'unapproved.exe'
        Write-FixtureFile $other 'Synthetic unsigned binary 0'
        $f.State.binaries[0].path = $other
        Assert-Rejected { Assert-Fixture $f } 'binary path/order mismatch'
    }
    Invoke-RegressionCheck 'reject all state paths before opening any binary' {
        param($f)
        Remove-Item -LiteralPath (Assert-TestPath $f.BinaryPaths[0])
        $f.State.binaries[3].path = Join-Path $f.Root 'does not exist.exe'
        Assert-Rejected { Assert-Fixture $f } 'binary path/order mismatch at index 3'
    }
    Invoke-RegressionCheck 'relative state binary path' {
        param($f)
        $f.State.binaries[0].path = 'release/abigail-hive-app.exe'
        Assert-Rejected { Assert-Fixture $f } 'absolute filesystem path'
    }
    Invoke-RegressionCheck 'wrong binary count' {
        param($f)
        $f.State.binaries = @($f.State.binaries[0..2])
        Assert-Rejected { Assert-Fixture $f } 'Exactly four'
    }
    Invoke-RegressionCheck 'wrong canonical binary count' {
        param($f)
        $f.Arguments.ExpectedBinaryPaths = @($f.BinaryPaths[0..2])
        Assert-Rejected { Assert-Fixture $f } 'Exactly four'
    }
    Invoke-RegressionCheck 'wrong commit' {
        param($f)
        $f.State.sourceCommit = 'b' * 40
        Assert-Rejected { Assert-Fixture $f } 'source commit does not match'
    }
    Invoke-RegressionCheck 'wrong version' {
        param($f)
        $f.State.version = '1.2.4'
        Assert-Rejected { Assert-Fixture $f } 'version does not match'
    }
    Invoke-RegressionCheck 'tampered binary' {
        param($f)
        Write-FixtureFile $f.BinaryPaths[2] 'Tampered synthetic executable'
        Assert-Rejected { Assert-Fixture $f } 'Prepared binary changed'
    }
    Invoke-RegressionCheck 'malformed binary hash' {
        param($f)
        $f.State.binaries[0].sha256 = 'not a hash'
        Assert-Rejected { Assert-Fixture $f } 'Invalid SHA-256'
    }
    Invoke-RegressionCheck 'missing and malformed bootstrap state' {
        param($f)
        $f.State.bootstrap = $null
        Assert-Rejected { Assert-Fixture $f } 'bootstrap manifest must be an array'
        Assert-Rejected { Assert-AbigailFileManifest -Root $f.Bootstrap -Manifest $null } 'manifest is missing'
        $f.State.bootstrap = @([pscustomobject]@{ path = 'NOTICE'; sha256 = 'bad hash' })
        Assert-Rejected { Assert-Fixture $f } 'Invalid SHA-256'
        $f.State.PSObject.Properties.Remove('bootstrap')
        Assert-Rejected { Assert-Fixture $f } "missing 'bootstrap'"
    }
    if ($IsWindows) {
        Invoke-RegressionCheck 'junction bootstrap entry is rejected without traversal' {
            param($f)
            $outside = Join-Path $f.Root 'outside bootstrap'
            Write-FixtureFile (Join-Path $outside 'outside.txt') 'Must never be included'
            New-TestJunction (Join-Path $f.Bootstrap 'linked directory') $outside
            Assert-Rejected { Get-AbigailFileManifest -Root $f.Bootstrap } 'Reparse point or symbolic link'
            Assert-Rejected { Assert-Fixture $f } 'Reparse point or symbolic link'
        }
        Invoke-RegressionCheck 'junction root is rejected' {
            param($f)
            $linkedRoot = Join-Path $f.Root 'linked bootstrap'
            New-TestJunction $linkedRoot $f.Bootstrap
            Assert-Rejected { Get-AbigailFileManifest -Root $linkedRoot } 'Reparse point or symbolic link'
        }
        Invoke-RegressionCheck 'junction ancestor of root is rejected' {
            param($f)
            $linkedRoot = Join-Path $f.Root 'linked bootstrap'
            New-TestJunction $linkedRoot $f.Bootstrap
            Assert-Rejected { Get-AbigailFileManifest -Root (Join-Path $linkedRoot 'licenses') } 'Reparse point or symbolic link'
        }
    } else {
        Write-Host 'SKIP Windows junction checks (requires Windows).'
    }
    Write-Host "Passed $passed offline signing-state regression checks."
} finally {
    # Verify the one recursive-delete target is our exact allocated directory,
    # inside this repository. Unlink only junctions that this test created.
    $expectedWorkspace = [IO.Path]::GetFullPath((Join-Path $repoRoot $workspaceName))
    $cleanupTarget = (Resolve-Path -LiteralPath $testWorkspace -ErrorAction Stop).ProviderPath
    $repoPrefix = $repoRoot + [IO.Path]::DirectorySeparatorChar
    if (-not [string]::Equals($cleanupTarget, $expectedWorkspace, [StringComparison]::OrdinalIgnoreCase) -or
        -not $cleanupTarget.StartsWith($repoPrefix, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Refusing cleanup outside the exact test workspace: '$cleanupTarget'."
    }
    foreach ($link in $createdLinks) {
        $absoluteLink = Assert-TestPath $link
        $item = Get-Item -LiteralPath $absoluteLink -Force -ErrorAction Stop
        if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -eq 0) {
            throw "Refusing to unlink a replaced test junction: '$absoluteLink'."
        }
        [IO.Directory]::Delete($absoluteLink, $false)
    }
    # Check every directory without following links before recursive cleanup.
    $pending = [Collections.Generic.Stack[string]]::new()
    $pending.Push($cleanupTarget)
    while ($pending.Count -gt 0) {
        $directory = $pending.Pop()
        $item = Get-Item -LiteralPath $directory -Force -ErrorAction Stop
        if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
            throw "Refusing recursive cleanup through a reparse point: '$directory'."
        }
        foreach ($child in Get-ChildItem -LiteralPath $directory -Force -ErrorAction Stop) {
            if (($child.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
                throw "Refusing recursive cleanup containing a reparse point: '$($child.FullName)'."
            }
            if ($child.PSIsContainer) { $pending.Push($child.FullName) }
        }
    }
    Remove-Item -LiteralPath $cleanupTarget -Recurse -Force -ErrorAction Stop
}
