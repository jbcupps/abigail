# Private human input and CodeSignTool transport. Never emit vendor output or secrets.
Set-StrictMode -Version Latest

function Read-AbigailSigningSecret {
    param(
        [Parameter(Mandatory)][string]$Title,
        [Parameter(Mandatory)][string]$Prompt,
        [switch]$OneTimeCode
    )
    if ($env:GITHUB_ACTIONS -eq 'true' -or -not [Environment]::UserInteractive) {
        throw 'Manual signing requires an interactive Windows desktop. GitHub requires the encrypted TOTP secret.'
    }
    Add-Type -AssemblyName System.Windows.Forms
    Add-Type -AssemblyName System.Drawing
    $form = [Windows.Forms.Form]::new()
    try {
        $form.Text = $Title
        $form.ClientSize = [Drawing.Size]::new(540, 225)
        $form.StartPosition = 'CenterScreen'
        $form.FormBorderStyle = 'FixedDialog'
        $form.MaximizeBox = $false
        $form.MinimizeBox = $false
        $form.TopMost = $true
        $form.Font = [Drawing.Font]::new('Segoe UI', 10)
        $label = [Windows.Forms.Label]::new()
        $label.Location = [Drawing.Point]::new(20, 18)
        $label.Size = [Drawing.Size]::new(500, 80)
        $label.Text = $Prompt
        $inputBox = [Windows.Forms.TextBox]::new()
        $inputBox.Location = [Drawing.Point]::new(20, 102)
        $inputBox.Size = [Drawing.Size]::new(500, 28)
        $inputBox.UseSystemPasswordChar = $true
        if ($OneTimeCode) { $inputBox.MaxLength = 6 }
        $errorLabel = [Windows.Forms.Label]::new()
        $errorLabel.Location = [Drawing.Point]::new(20, 140)
        $errorLabel.Size = [Drawing.Size]::new(500, 25)
        $errorLabel.ForeColor = [Drawing.Color]::Firebrick
        $ok = [Windows.Forms.Button]::new()
        $ok.Text = 'Continue'
        $ok.Location = [Drawing.Point]::new(305, 178)
        $ok.Size = [Drawing.Size]::new(105, 30)
        $ok.Add_Click({
            if ([string]::IsNullOrWhiteSpace($inputBox.Text)) {
                $errorLabel.Text = 'Enter the requested value, or cancel.'
            } elseif ($OneTimeCode -and $inputBox.Text -notmatch '^\d{6}$') {
                $errorLabel.Text = 'Enter the current six-digit signing code.'
            } else {
                $form.DialogResult = [Windows.Forms.DialogResult]::OK
                $form.Close()
            }
        }.GetNewClosure())
        $cancel = [Windows.Forms.Button]::new()
        $cancel.Text = 'Cancel'
        $cancel.Location = [Drawing.Point]::new(420, 178)
        $cancel.Size = [Drawing.Size]::new(100, 30)
        $cancel.DialogResult = [Windows.Forms.DialogResult]::Cancel
        $form.Controls.AddRange(@($label, $inputBox, $errorLabel, $ok, $cancel))
        $form.AcceptButton = $ok
        $form.CancelButton = $cancel
        $form.Add_Shown({ $null = $inputBox.Focus() }.GetNewClosure())
        if ($form.ShowDialog() -ne [Windows.Forms.DialogResult]::OK) { throw 'Signing input cancelled. No further signing operation will be started.' }
        $secret = ConvertTo-SecureString -String $inputBox.Text -AsPlainText -Force
        $inputBox.Clear()
        return $secret
    } finally { $form.Dispose() }
}

function Invoke-AbigailSigningProcess {
    param(
        [Parameter(Mandatory)][Diagnostics.ProcessStartInfo]$StartInfo,
        [switch]$Manual,
        [scriptblock]$RequestOtp,
        [int]$TimeoutSeconds = 600
    )
    $StartInfo.UseShellExecute = $false
    $StartInfo.CreateNoWindow = $true
    $StartInfo.RedirectStandardInput = $true
    $StartInfo.RedirectStandardOutput = $true
    $StartInfo.RedirectStandardError = $true
    $process = [Diagnostics.Process]::new()
    $process.StartInfo = $StartInfo
    $started = $false
    $otp = $null
    $otpText = $null
    try {
        $started = $process.Start()
        if (-not $started) { throw 'Could not start SSL.com CodeSignTool.' }
        $stderr = $process.StandardError.ReadToEndAsync()
        $buffer = [char[]]::new(1024)
        $pending = $process.StandardOutput.ReadAsync($buffer, 0, $buffer.Length)
        $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
        $tail = ''
        $otpSent = $false
        while ($true) {
            if ([DateTime]::UtcNow -gt $deadline) { throw 'SSL.com signing timed out. No artifact will be published.' }
            if (-not $pending.Wait(100)) {
                continue
            }
            $count = $pending.GetAwaiter().GetResult()
            if ($count -eq 0) { break }
            $tail += [string]::new($buffer, 0, $count)
            if ($tail.Length -gt 8192) { $tail = $tail.Substring($tail.Length - 8192) }
            if ($tail.Contains('Enter the OTP - Press enter to continue:')) {
                if (-not $Manual -or -not $RequestOtp -or $otpSent) { throw 'Unexpected interactive request from SSL.com. Signing stopped.' }
                # CodeSignTool 1.3.3 reads manual OTP from System.in. The code is
                # requested only once the vendor is waiting, never in arguments.
                $delivery = if ($tail.Contains('OTP sent on your registered mobile number')) { 'sms' } else { 'authenticator' }
                $otp = & $RequestOtp $delivery
                if ($otp -isnot [Security.SecureString]) { throw 'The signing prompt did not return a protected code.' }
                $otpText = [Net.NetworkCredential]::new('', $otp).Password
                if ($otpText -notmatch '^\d{6}$') { throw 'A six-digit signing code is required.' }
                $process.StandardInput.WriteLine($otpText)
                $process.StandardInput.Flush()
                $process.StandardInput.Close()
                $otp.Dispose(); $otp = $null; $otpText = $null
                $otpSent = $true
                $tail = ''
                $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
            }
            $pending = $process.StandardOutput.ReadAsync($buffer, 0, $buffer.Length)
        }
        if (-not $process.WaitForExit($TimeoutSeconds * 1000)) { throw 'SSL.com signing did not finish.' }
        $null = $stderr.GetAwaiter().GetResult()
        return $process.ExitCode
    } finally {
        if ($otp) { $otp.Dispose() }
        $otpText = $null
        if ($started -and -not $process.HasExited) { $process.Kill($true); $process.WaitForExit() }
        $process.Dispose()
    }
}

Export-ModuleMember -Function Read-AbigailSigningSecret, Invoke-AbigailSigningProcess
