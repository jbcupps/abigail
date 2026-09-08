# Tiny fake-process tests: no account, network, certificate, or real signing.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'windows-signing-private.psm1') -Force
$work = Join-Path (Join-Path $PSScriptRoot '../target') ('signing-private-test-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $work | Out-Null
$fixture = Join-Path $work 'fake-vendor.ps1'
$pidFile = Join-Path $work 'fake-vendor.pid'
$fakeCode = @'
param([string]$Mode, [string]$PidFile)
[IO.File]::WriteAllText($PidFile, [string]$PID)
[Console]::Error.WriteLine('PRIVATE-STDERR-FIXTURE')
if ($Mode -eq 'timeout') { Start-Sleep -Seconds 20; exit 19 }
if ($Mode -eq 'auto') {
    [Console]::Out.WriteLine(('PRIVATE-STDOUT-FIXTURE' * 1000))
    exit 17
}
[Console]::Out.Write('Enter the OTP - ')
Start-Sleep -Milliseconds 30
[Console]::Out.Write('Press enter to continue:')
$value = [Console]::ReadLine()
if ($value -ne '123456') { exit 9 }
[Console]::Out.WriteLine('PRIVATE-OTP-FIXTURE-123456')
exit 0
'@
[IO.File]::WriteAllText($fixture, $fakeCode)
function New-FixtureStart([string]$Mode) {
    $start = [Diagnostics.ProcessStartInfo]::new()
    $start.FileName = (Get-Process -Id $PID).Path
    foreach ($arg in @('-NoProfile','-NonInteractive','-File',$fixture,'-Mode',$Mode,'-PidFile',$pidFile)) { $start.ArgumentList.Add($arg) }
    return $start
}
function Assert-Failure([scriptblock]$Action, [string]$Pattern) {
    try { & $Action; throw 'Expected failure was not raised.' }
    catch { if ($_.Exception.Message -notmatch $Pattern) { throw } }
    $childId = [int](Get-Content -LiteralPath $pidFile -Raw)
    if (Get-Process -Id $childId -ErrorAction SilentlyContinue) { throw 'The owned vendor process was left running.' }
}
try {
    $probe = @{ count = 0 }
    $request = { $probe.count++; ConvertTo-SecureString '123456' -AsPlainText -Force }.GetNewClosure()
    $result = @(Invoke-AbigailSigningProcess -StartInfo (New-FixtureStart 'manual') -Manual -RequestOtp $request -TimeoutSeconds 10)
    if ($result.Count -ne 1 -or $result[0] -ne 0 -or $probe.count -ne 1) { throw 'Manual code transport or output suppression failed.' }
    $result = @(Invoke-AbigailSigningProcess -StartInfo (New-FixtureStart 'auto') -TimeoutSeconds 10)
    if ($result.Count -ne 1 -or $result[0] -ne 17) { throw 'Vendor exit status or output suppression failed.' }
    Assert-Failure { Invoke-AbigailSigningProcess -StartInfo (New-FixtureStart 'manual') -TimeoutSeconds 10 } 'Unexpected interactive request'
    Assert-Failure { Invoke-AbigailSigningProcess -StartInfo (New-FixtureStart 'manual') -Manual -RequestOtp { throw 'Cancelled fixture input' } -TimeoutSeconds 10 } 'Cancelled fixture input'
    Assert-Failure { Invoke-AbigailSigningProcess -StartInfo (New-FixtureStart 'manual') -Manual -RequestOtp { ConvertTo-SecureString 'bad-code' -AsPlainText -Force } -TimeoutSeconds 10 } 'six-digit signing code'
    Assert-Failure { Invoke-AbigailSigningProcess -StartInfo (New-FixtureStart 'timeout') -TimeoutSeconds 1 } 'timed out'
    Write-Host 'PASS: manual code input, hidden output, exit codes, unexpected prompts, cancellation, invalid codes and timeout cleanup.'
} finally {
    if (Test-Path -LiteralPath $pidFile) { Remove-Item -LiteralPath $pidFile -Force }
    Remove-Item -LiteralPath $fixture -Force
    Remove-Item -LiteralPath $work -Force
}
