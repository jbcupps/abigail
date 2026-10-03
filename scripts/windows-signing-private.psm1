# Private automated CodeSignTool transport. Vendor output is never emitted.
#requires -Version 7.0
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function ConvertTo-AbigailCodeSignArgument {
    param([Parameter(Mandatory)][AllowEmptyString()][string]$Value)
    '"' + $Value.Replace('\', '\\').Replace('"', '\"').Replace("`r", '\r').Replace("`n", '\n').Replace("`t", '\t') + '"'
}

function New-AbigailSigningDirectory {
    $directory = Join-Path ([IO.Path]::GetTempPath()) ('abigail-sign-' + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $directory | Out-Null
    try {
        $acl = [Security.AccessControl.DirectorySecurity]::new()
        $acl.SetAccessRuleProtection($true, $false)
        $sid = [Security.Principal.WindowsIdentity]::GetCurrent().User
        $acl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new($sid, 'FullControl', 'ContainerInherit,ObjectInherit', 'None', 'Allow'))
        Set-Acl -LiteralPath $directory -AclObject $acl
        return $directory
    } catch {
        Remove-Item -LiteralPath $directory -Force
        throw 'Could not create a protected temporary signing directory.'
    }
}

function Invoke-AbigailSigningProcess {
    param(
        [Parameter(Mandatory)][Diagnostics.ProcessStartInfo]$StartInfo,
        [ValidateRange(1, 1800)][int]$TimeoutSeconds = 600
    )
    $StartInfo.UseShellExecute = $false
    $StartInfo.CreateNoWindow = $true
    $StartInfo.RedirectStandardInput = $true
    $StartInfo.RedirectStandardOutput = $true
    $StartInfo.RedirectStandardError = $true
    # Credentials are in the ACL-protected argfile, not inherited by Java.
    foreach ($name in @('ESIGNER_USERNAME', 'ESIGNER_PASSWORD', 'ESIGNER_CREDENTIAL_ID', 'ESIGNER_TOTP_SECRET', 'WINDOWS_CERTIFICATE_THUMBPRINT', 'JAVA_TOOL_OPTIONS', '_JAVA_OPTIONS', 'JDK_JAVA_OPTIONS', 'CLASSPATH')) {
        $null = $StartInfo.Environment.Remove($name)
    }
    $process = [Diagnostics.Process]::new()
    $process.StartInfo = $StartInfo
    $started = $false
    try {
        $started = $process.Start()
        if (-not $started) { throw 'Could not start SSL.com CodeSignTool.' }
        $process.StandardInput.Close() # Automated signing must never wait for a human code.
        $stdoutBuffer = [char[]]::new(16384)
        $stderrBuffer = [char[]]::new(16384)
        $stdout = $process.StandardOutput.ReadAsync($stdoutBuffer, 0, $stdoutBuffer.Length)
        $stderr = $process.StandardError.ReadAsync($stderrBuffer, 0, $stderrBuffer.Length)
        $stdoutDone = $false
        $stderrDone = $false
        $tail = ''
        $elapsed = [Diagnostics.Stopwatch]::StartNew()
        while (-not ($stdoutDone -and $stderrDone -and $process.HasExited)) {
            $madeProgress = $false
            if ($elapsed.Elapsed.TotalSeconds -gt $TimeoutSeconds) { throw 'SSL.com signing timed out. No artifact will be published.' }
            if (-not $stdoutDone -and $stdout.IsCompleted) {
                $madeProgress = $true
                $count = $stdout.GetAwaiter().GetResult()
                if ($count -eq 0) { $stdoutDone = $true }
                else {
                    $tail += [string]::new($stdoutBuffer, 0, $count)
                    if ($tail.Length -gt 8192) { $tail = $tail.Substring($tail.Length - 8192) }
                    if ($tail.Contains('Enter the OTP') -or $tail.Contains('Press enter to continue')) {
                        throw 'SSL.com requested interactive approval. Automated signing needs the existing TOTP seed.'
                    }
                    $stdout = $process.StandardOutput.ReadAsync($stdoutBuffer, 0, $stdoutBuffer.Length)
                }
            }
            if (-not $stderrDone -and $stderr.IsCompleted) {
                $madeProgress = $true
                $count = $stderr.GetAwaiter().GetResult()
                if ($count -eq 0) { $stderrDone = $true }
                else { $stderr = $process.StandardError.ReadAsync($stderrBuffer, 0, $stderrBuffer.Length) }
            }
            if (-not $madeProgress -and -not ($stdoutDone -and $stderrDone -and $process.HasExited)) {
                $pendingReads = @()
                if (-not $stdoutDone) { $pendingReads += $stdout }
                if (-not $stderrDone) { $pendingReads += $stderr }
                if ($pendingReads.Count) { $null = [Threading.Tasks.Task]::WaitAny([Threading.Tasks.Task[]]$pendingReads, 100) }
                else { $null = $process.WaitForExit(100) }
            }
        }
        return $process.ExitCode
    } finally {
        if ($started -and -not $process.HasExited) { $process.Kill($true); $null = $process.WaitForExit(5000) }
        $process.Dispose()
    }
}

function Invoke-AbigailCodeSignTool {
    param(
        [Parameter(Mandatory)]$Tool,
        [Parameter(Mandatory)][string[]]$Arguments,
        [ValidateRange(1, 1800)][int]$TimeoutSeconds = 600
    )
    $privateDir = New-AbigailSigningDirectory
    $argumentFile = Join-Path $privateDir 'java.args'
    try {
        [IO.File]::WriteAllLines($argumentFile, @($Arguments | ForEach-Object { ConvertTo-AbigailCodeSignArgument $_ }), [Text.UTF8Encoding]::new($false))
        $start = [Diagnostics.ProcessStartInfo]::new()
        $start.FileName = $Tool.java
        $start.WorkingDirectory = $Tool.root
        foreach ($argument in @('-Dfile.encoding=UTF-8', "-Dlog4j2.configurationFile=$(Join-Path $PSScriptRoot 'esigner-log4j2.xml')", '-jar', $Tool.jar, "@$argumentFile")) { $start.ArgumentList.Add($argument) }
        return Invoke-AbigailSigningProcess -StartInfo $start -TimeoutSeconds $TimeoutSeconds
    } finally {
        if (Test-Path -LiteralPath $argumentFile) { Remove-Item -LiteralPath $argumentFile -Force }
        if (Test-Path -LiteralPath $privateDir) { Remove-Item -LiteralPath $privateDir -Force }
        $Arguments = $null
    }
}

Export-ModuleMember -Function ConvertTo-AbigailCodeSignArgument, New-AbigailSigningDirectory, Invoke-AbigailSigningProcess, Invoke-AbigailCodeSignTool
