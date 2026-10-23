function Test-AbigailEnvironmentVariableName {
    param([AllowNull()][AllowEmptyString()][string]$Name)

    return (
        -not [string]::IsNullOrEmpty($Name) -and
        $Name.IndexOf("=") -lt 0 -and
        $Name.IndexOf([char]0) -lt 0
    )
}

function Start-ProcessWithEnvironmentOverlay {
    param(
        [Parameter(Mandatory = $true)][string]$FilePath,
        [Parameter(Mandatory = $true)][string]$WorkingDirectory,
        [Parameter(Mandatory = $true)][hashtable]$EnvironmentOverlay,
        [string]$Arguments
    )

    $normalizedOverlay = @{}
    $invalidValueNames = @()
    foreach ($key in $EnvironmentOverlay.Keys) {
        $name = [string]$key
        if (-not (Test-AbigailEnvironmentVariableName -Name $name)) {
            throw "Environment overlay contains an invalid variable name."
        }

        $rawValue = $EnvironmentOverlay[$key]
        if ($null -eq $rawValue -or ([string]$rawValue).Length -eq 0) {
            $invalidValueNames += $name
            continue
        }
        $normalizedOverlay[$name] = [string]$rawValue
    }

    if ($invalidValueNames.Count -ne 0) {
        $names = [string]::Join(", ", [string[]]@($invalidValueNames | Sort-Object))
        throw "Environment overlay requires nonempty values for: $names."
    }

    $startInfo = New-Object System.Diagnostics.ProcessStartInfo
    $startInfo.FileName = $FilePath
    $startInfo.WorkingDirectory = $WorkingDirectory
    $startInfo.UseShellExecute = $false
    if (-not [string]::IsNullOrEmpty($Arguments)) {
        $startInfo.Arguments = $Arguments
    }
    foreach ($name in @($normalizedOverlay.Keys | Sort-Object)) {
        $startInfo.EnvironmentVariables[$name] = $normalizedOverlay[$name]
    }

    $process = [System.Diagnostics.Process]::Start($startInfo)
    if ($null -eq $process) {
        throw "Child process did not start."
    }
    return $process
}
