#requires -Version 7.0
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Stop-AbigailPreparation([string]$Reason) {
    if ($Reason.EndsWith('Prepare must be rerun.', [StringComparison]::Ordinal)) { throw $Reason }
    throw "$Reason Prepare must be rerun."
}

function Get-AbigailStateProperty($InputObject, [string]$Name) {
    if ($null -eq $InputObject) { throw "Prepared state is missing '$Name'." }
    if ($InputObject -is [Collections.IDictionary]) {
        if (-not $InputObject.Contains($Name)) { throw "Prepared state is missing '$Name'." }
        return ,$InputObject[$Name]
    }
    $property = $InputObject.PSObject.Properties[$Name]
    if ($null -eq $property) { throw "Prepared state is missing '$Name'." }
    return ,$property.Value
}

function Assert-AbigailHashValue($Value, [string]$Label) {
    if ($Value -isnot [string] -or $Value -cnotmatch '^[0-9a-fA-F]{64}$') {
        throw "Invalid SHA-256 for $Label."
    }
}

function ConvertTo-AbigailAbsolutePath([string]$Path) {
    if ([string]::IsNullOrWhiteSpace($Path) -or -not [IO.Path]::IsPathFullyQualified($Path)) {
        throw "Expected an absolute filesystem path: '$Path'."
    }
    if ($Path -match '^[\\/]{2}[?.][\\/]') { throw "Device paths are not accepted: '$Path'." }
    return [IO.Path]::GetFullPath($Path)
}

function Assert-AbigailNoReparsePoint([string]$Path) {
    # Check ancestors too: the supplied root itself may sit below a junction.
    $cursor = $Path
    while (-not [string]::IsNullOrEmpty($cursor)) {
        $item = Get-Item -LiteralPath $cursor -Force -ErrorAction Stop
        if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
            throw "Reparse point or symbolic link is not accepted: '$cursor'."
        }
        $cursor = [IO.Path]::GetDirectoryName($cursor)
    }
}

function Assert-AbigailManifestPath($Path) {
    # Only canonical portable relative paths are allowed. In particular, reject
    # Windows aliases (ADS, device names, trailing dots/spaces) before any IO.
    if ($Path -isnot [string] -or [string]::IsNullOrEmpty($Path) -or
        $Path -match '[<>:"\\|?*\x00-\x1f\x7f]' -or [IO.Path]::IsPathRooted($Path)) {
        throw "Unsafe manifest path: '$Path'."
    }
    foreach ($segment in $Path.Split('/')) {
        if ($segment -in @('', '.', '..') -or $segment.EndsWith('.') -or $segment.EndsWith(' ') -or
            $segment -match '^(?i:CON|PRN|AUX|NUL|CONIN\$|CONOUT\$|COM[1-9\u00b9\u00b2\u00b3]|LPT[1-9\u00b9\u00b2\u00b3])(?:\.|$)') {
            throw "Unsafe manifest path: '$Path'."
        }
    }
}

function Get-AbigailInputHash([string]$Path) {
    $item = Get-Item -LiteralPath $Path -Force -ErrorAction Stop
    if ($item.PSIsContainer) { throw "Expected a file: '$Path'." }
    Assert-AbigailNoReparsePoint -Path $item.FullName
    return (Get-FileHash -LiteralPath $item.FullName -Algorithm SHA256 -ErrorAction Stop).Hash
}

function Get-AbigailFileManifest {
    <# Returns file records in ordinal path order, using '/' path separators.
       Hidden/system files and licenses count; empty directories do not.
       Enumerate one directory at a time so no reparse point is traversed. #>
    [CmdletBinding()]
    param([Parameter(Mandatory)][AllowEmptyString()][string]$Root)
    try {
        $absoluteRoot = ConvertTo-AbigailAbsolutePath -Path $Root
        Assert-AbigailNoReparsePoint -Path $absoluteRoot
        $rootItem = Get-Item -LiteralPath $absoluteRoot -Force -ErrorAction Stop
        if (-not $rootItem.PSIsContainer) { throw "Manifest root is not a directory: '$Root'." }
        $pending = [Collections.Generic.Stack[string]]::new()
        $pending.Push($rootItem.FullName)
        $files = [Collections.Generic.Dictionary[string, string]]::new([StringComparer]::OrdinalIgnoreCase)
        while ($pending.Count -gt 0) {
            $directory = $pending.Pop()
            Assert-AbigailNoReparsePoint -Path $directory
            foreach ($item in Get-ChildItem -LiteralPath $directory -Force -ErrorAction Stop) {
                if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
                    throw "Reparse point or symbolic link is not accepted: '$($item.FullName)'."
                }
                if ($item.PSIsContainer) {
                    $pending.Push($item.FullName)
                } else {
                    $relative = [IO.Path]::GetRelativePath($rootItem.FullName, $item.FullName).Replace('\', '/')
                    Assert-AbigailManifestPath -Path $relative
                    if (-not $files.TryAdd($relative, $item.FullName)) {
                        throw "Duplicate file path under Windows comparison: '$relative'."
                    }
                }
            }
        }
        $paths = [string[]]@($files.Keys)
        [Array]::Sort($paths, [StringComparer]::Ordinal)
        # Hash before emitting anything, so a failed enumeration/hash cannot
        # leave a partial manifest in the caller's success pipeline.
        $records = [Collections.Generic.List[object]]::new()
        foreach ($path in $paths) {
            $records.Add([pscustomobject]@{ path = $path; sha256 = Get-AbigailInputHash -Path $files[$path] })
        }
        return $records.ToArray()
    } catch { Stop-AbigailPreparation -Reason $_.Exception.Message }
}

function Assert-AbigailFileManifest {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][AllowEmptyString()][string]$Root,
        [Parameter(Mandatory)][AllowNull()][AllowEmptyCollection()][object[]]$Manifest
    )
    try {
        if ($null -eq $Manifest) { throw 'Prepared bootstrap manifest is missing.' }
        $expected = [Collections.Generic.Dictionary[string, string]]::new([StringComparer]::OrdinalIgnoreCase)
        foreach ($entry in $Manifest) {
            $path = Get-AbigailStateProperty -InputObject $entry -Name 'path'
            Assert-AbigailManifestPath -Path $path
            $hash = Get-AbigailStateProperty -InputObject $entry -Name 'sha256'
            Assert-AbigailHashValue -Value $hash -Label "bootstrap '$path'"
            if (-not $expected.TryAdd($path, $hash)) { throw "Duplicate manifest path: '$path'." }
        }
        $actual = @(Get-AbigailFileManifest -Root $Root)
        foreach ($entry in $actual) {
            if (-not $expected.ContainsKey($entry.path)) { throw "Bootstrap file added: '$($entry.path)'." }
            if (-not [string]::Equals($expected[$entry.path], $entry.sha256, [StringComparison]::OrdinalIgnoreCase)) {
                throw "Bootstrap file changed: '$($entry.path)'."
            }
            $null = $expected.Remove($entry.path)
        }
        if ($expected.Count -gt 0) { throw "Bootstrap file missing: '$(@($expected.Keys)[0])'." }
    } catch { Stop-AbigailPreparation -Reason $_.Exception.Message }
}

function Assert-AbigailPreparedInputs {
    <# ExpectedBinaryPaths must come from the canonical build script. State
       paths are compared first; only caller-supplied paths are opened. #>
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][AllowNull()]$State,
        [Parameter(Mandatory)][AllowEmptyString()][string]$ExpectedCommit,
        [Parameter(Mandatory)][AllowEmptyString()][string]$ExpectedVersion,
        [Parameter(Mandatory)][AllowNull()][AllowEmptyCollection()][object[]]$ExpectedBinaryPaths,
        [Parameter(Mandatory)][AllowEmptyString()][string]$ConfigPath,
        [Parameter(Mandatory)][AllowEmptyString()][string]$BootstrapRoot
    )
    try {
        $commit = Get-AbigailStateProperty -InputObject $State -Name 'sourceCommit'
        $version = Get-AbigailStateProperty -InputObject $State -Name 'version'
        if ($commit -isnot [string] -or [string]::IsNullOrWhiteSpace($ExpectedCommit) -or
            -not [string]::Equals($commit, $ExpectedCommit, [StringComparison]::Ordinal)) {
            throw 'Prepared source commit does not match this signing run.'
        }
        if ($version -isnot [string] -or [string]::IsNullOrWhiteSpace($ExpectedVersion) -or
            -not [string]::Equals($version, $ExpectedVersion, [StringComparison]::Ordinal)) {
            throw 'Prepared version does not match this signing run.'
        }
        $configHash = Get-AbigailStateProperty -InputObject $State -Name 'configSha256'
        Assert-AbigailHashValue -Value $configHash -Label 'generated config'
        if (-not [string]::Equals($configHash, (Get-AbigailInputHash -Path $ConfigPath), [StringComparison]::OrdinalIgnoreCase)) {
            throw 'Prepared generated config changed.'
        }
        $binaries = Get-AbigailStateProperty -InputObject $State -Name 'binaries'
        if ($null -eq $ExpectedBinaryPaths -or $ExpectedBinaryPaths.Count -ne 4 -or
            $binaries -isnot [array] -or $binaries.Count -ne 4) {
            throw 'Exactly four ordered binary paths and prepared binary records are required.'
        }
        $canonicalPaths = [Collections.Generic.List[string]]::new()
        $binaryHashes = [Collections.Generic.List[string]]::new()
        $seen = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
        for ($index = 0; $index -lt 4; $index++) {
            if ($ExpectedBinaryPaths[$index] -isnot [string]) { throw 'Expected binary paths must be strings.' }
            $expectedPath = ConvertTo-AbigailAbsolutePath -Path $ExpectedBinaryPaths[$index]
            if (-not $seen.Add($expectedPath)) { throw "Duplicate expected binary path: '$expectedPath'." }
            $statePath = Get-AbigailStateProperty -InputObject $binaries[$index] -Name 'path'
            if ($statePath -isnot [string]) { throw "Prepared binary path at index $index must be a string." }
            $preparedPath = ConvertTo-AbigailAbsolutePath -Path $statePath
            if (-not [string]::Equals($expectedPath, $preparedPath, [StringComparison]::OrdinalIgnoreCase)) {
                throw "Prepared binary path/order mismatch at index ${index}: expected '$expectedPath'."
            }
            $hash = Get-AbigailStateProperty -InputObject $binaries[$index] -Name 'sha256'
            Assert-AbigailHashValue -Value $hash -Label "binary '$expectedPath'"
            $canonicalPaths.Add($expectedPath)
            $binaryHashes.Add($hash)
        }
        for ($index = 0; $index -lt 4; $index++) {
            if (-not [string]::Equals($binaryHashes[$index], (Get-AbigailInputHash -Path $canonicalPaths[$index]), [StringComparison]::OrdinalIgnoreCase)) {
                throw "Prepared binary changed: '$($canonicalPaths[$index])'."
            }
        }
        $bootstrap = Get-AbigailStateProperty -InputObject $State -Name 'bootstrap'
        if ($bootstrap -isnot [array]) { throw 'Prepared bootstrap manifest must be an array.' }
        Assert-AbigailFileManifest -Root $BootstrapRoot -Manifest $bootstrap
    } catch { Stop-AbigailPreparation -Reason $_.Exception.Message }
}

Export-ModuleMember -Function Get-AbigailFileManifest, Assert-AbigailFileManifest, Assert-AbigailPreparedInputs
