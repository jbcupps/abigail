function Initialize-AbigailProcessEnvironmentInterop {
    if ($null -ne ("Abigail.Dev.ProcessEnvironmentNativeMethods" -as [type])) {
        return
    }

    $nativeSource = @'
using System;
using System.Runtime.InteropServices;
using System.Text;

namespace Abigail.Dev
{
    public sealed class ProcessEnvironmentState
    {
        public ProcessEnvironmentState(bool wasPresent, string value)
        {
            WasPresent = wasPresent;
            Value = value;
        }

        public bool WasPresent { get; private set; }
        public string Value { get; private set; }
    }

    public static class ProcessEnvironmentNativeMethods
    {
        private const int ErrorSuccess = 0;
        private const int ErrorEnvironmentVariableNotFound = 203;
        private const uint MaximumEnvironmentValueLength = 32767;

        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, ExactSpelling = true, SetLastError = true)]
        private static extern uint GetEnvironmentVariableW(
            string name,
            StringBuilder value,
            uint size
        );

        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, ExactSpelling = true, SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool SetEnvironmentVariableW(string name, string value);

        [DllImport("kernel32.dll", EntryPoint = "SetLastError", ExactSpelling = true)]
        private static extern void ResetLastError(uint errorCode);

        public static ProcessEnvironmentState Read(string name)
        {
            uint capacity = 256;
            while (true)
            {
                StringBuilder buffer = new StringBuilder((int)capacity);
                ResetLastError(ErrorSuccess);
                uint length = GetEnvironmentVariableW(name, buffer, capacity);
                if (length == 0)
                {
                    int error = Marshal.GetLastWin32Error();
                    if (error == ErrorSuccess)
                    {
                        return new ProcessEnvironmentState(true, String.Empty);
                    }
                    if (error == ErrorEnvironmentVariableNotFound)
                    {
                        return new ProcessEnvironmentState(false, null);
                    }
                    throw new InvalidOperationException();
                }

                if (length < capacity)
                {
                    return new ProcessEnvironmentState(true, buffer.ToString());
                }
                if (length > MaximumEnvironmentValueLength)
                {
                    throw new InvalidOperationException();
                }
                capacity = length;
            }
        }

        public static bool Write(string name, string value, bool present)
        {
            ResetLastError(ErrorSuccess);
            return SetEnvironmentVariableW(name, present ? (value ?? String.Empty) : null);
        }
    }
}
'@

    try {
        Add-Type -TypeDefinition $nativeSource -ErrorAction Stop | Out-Null
    } catch {
        if ($null -eq ("Abigail.Dev.ProcessEnvironmentNativeMethods" -as [type])) {
            throw "Process environment interop initialization failed."
        }
    }
}

function Test-AbigailProcessEnvironmentName {
    param([AllowNull()][AllowEmptyString()][string]$Name)

    return (
        -not [string]::IsNullOrEmpty($Name) -and
        $Name.IndexOf("=") -lt 0 -and
        $Name.IndexOf([char]0) -lt 0
    )
}

function Get-ProcessEnvironmentVariableExact {
    param(
        [Parameter(Mandatory = $true)]
        [AllowEmptyString()]
        [string]$Name
    )

    if (-not (Test-AbigailProcessEnvironmentName -Name $Name)) {
        throw "Process environment variable name is invalid."
    }

    Initialize-AbigailProcessEnvironmentInterop
    try {
        $state = [Abigail.Dev.ProcessEnvironmentNativeMethods]::Read($Name)
    } catch {
        throw "Process environment read failed for: $Name."
    }

    return [PSCustomObject]@{
        WasPresent = [bool]$state.WasPresent
        Value = $state.Value
    }
}

function Set-ProcessEnvironmentVariableExact {
    param(
        [Parameter(Mandatory = $true)]
        [AllowEmptyString()]
        [string]$Name,
        [Parameter(Mandatory = $true)]
        [bool]$Present,
        [AllowNull()]
        [AllowEmptyString()]
        [string]$Value
    )

    if (-not (Test-AbigailProcessEnvironmentName -Name $Name)) {
        throw "Process environment variable name is invalid."
    }

    $expectedValue = if ($Present -and $null -eq $Value) {
        [string]::Empty
    } else {
        $Value
    }

    Initialize-AbigailProcessEnvironmentInterop
    try {
        $succeeded = [Abigail.Dev.ProcessEnvironmentNativeMethods]::Write(
            $Name,
            $expectedValue,
            $Present
        )
    } catch {
        throw "Process environment update failed for: $Name."
    }
    if (-not $succeeded) {
        throw "Process environment update failed for: $Name."
    }

    try {
        $observed = Get-ProcessEnvironmentVariableExact -Name $Name
    } catch {
        throw "Process environment update failed for: $Name."
    }
    $matches = if ($Present) {
        $observed.WasPresent -and
        [string]::Equals(
            [string]$expectedValue,
            [string]$observed.Value,
            [StringComparison]::Ordinal
        )
    } else {
        -not $observed.WasPresent
    }
    if (-not $matches) {
        throw "Process environment update failed for: $Name."
    }
}

function Restore-ProcessEnvironmentSnapshotExact {
    param([Parameter(Mandatory = $true)][hashtable]$Snapshot)

    $failedNames = @()
    foreach ($name in @($Snapshot.Keys | Sort-Object)) {
        $previous = $Snapshot[$name]
        try {
            Set-ProcessEnvironmentVariableExact `
                -Name ([string]$name) `
                -Present ([bool]$previous.WasPresent) `
                -Value $previous.Value
        } catch {
            $failedNames += [string]$name
        }
    }

    if ($failedNames.Count -ne 0) {
        $names = [string]::Join(", ", [string[]]$failedNames)
        throw "Process environment restoration failed for: $names."
    }
}

function Start-ProcessWithEnvironmentOverlay {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][string]$FilePath,
        [Parameter(Mandatory = $true)][string]$WorkingDirectory,
        [Parameter(Mandatory = $true)][hashtable]$EnvironmentOverlay,
        [AllowEmptyCollection()][string[]]$ArgumentList,
        [switch]$Wait,
        [switch]$PassThru
    )

    $normalizedOverlay = @{}
    $invalidValueNames = @()
    $hasInvalidName = $false
    foreach ($key in $EnvironmentOverlay.Keys) {
        $name = [string]$key
        if (-not (Test-AbigailProcessEnvironmentName -Name $name)) {
            $hasInvalidName = $true
            continue
        }

        $rawValue = $EnvironmentOverlay[$key]
        if ($null -eq $rawValue) {
            $invalidValueNames += $name
            continue
        }
        $value = [string]$rawValue
        if ($value.Length -eq 0) {
            $invalidValueNames += $name
            continue
        }
        $normalizedOverlay[$name] = $value
    }

    if ($hasInvalidName) {
        throw "Environment overlay contains an invalid variable name."
    }
    if ($invalidValueNames.Count -ne 0) {
        $names = [string]::Join(", ", [string[]]@($invalidValueNames | Sort-Object))
        throw "Environment overlay requires nonempty values for: $names."
    }

    $environmentSnapshot = @{}
    foreach ($name in @($normalizedOverlay.Keys | Sort-Object)) {
        $environmentSnapshot[$name] = Get-ProcessEnvironmentVariableExact -Name $name
    }

    try {
        foreach ($name in @($normalizedOverlay.Keys | Sort-Object)) {
            Set-ProcessEnvironmentVariableExact `
                -Name $name `
                -Present $true `
                -Value $normalizedOverlay[$name]
        }

        $startParameters = @{
            FilePath = $FilePath
            WorkingDirectory = $WorkingDirectory
            ErrorAction = "Stop"
        }
        if ($PSBoundParameters.ContainsKey("ArgumentList")) {
            $startParameters["ArgumentList"] = $ArgumentList
        }
        if ($Wait) {
            $startParameters["Wait"] = $true
        }
        if ($PassThru) {
            $startParameters["PassThru"] = $true
        }
        Start-Process @startParameters
    } finally {
        Restore-ProcessEnvironmentSnapshotExact -Snapshot $environmentSnapshot
    }
}
