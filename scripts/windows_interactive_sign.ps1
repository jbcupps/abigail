param(
    [Parameter(Mandatory = $true)]
    [string]$SignToolPath,
    [Parameter(Mandatory = $true)]
    [string]$CertificateThumbprint,
    [Parameter(Mandatory = $true)]
    [string]$TimestampUrl,
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$Files
)

$ErrorActionPreference = "Stop"

function Get-NoticeMarkerPath {
    if (-not [string]::IsNullOrWhiteSpace($env:ABIGAIL_WINDOWS_SIGNING_NOTICE_FILE)) {
        return $env:ABIGAIL_WINDOWS_SIGNING_NOTICE_FILE
    }

    return Join-Path ([IO.Path]::GetTempPath()) "abigail-windows-signing-notified.marker"
}

function Get-AdapterPath {
    if (-not [string]::IsNullOrWhiteSpace($env:ABIGAIL_WINDOWS_ESIGNER_PATH) -and
        (Test-Path $env:ABIGAIL_WINDOWS_ESIGNER_PATH)) {
        return $env:ABIGAIL_WINDOWS_ESIGNER_PATH
    }

    $defaultPath = "C:\Program Files (x86)\SSL Corp eSigner CKA\eSigner CKA.exe"
    if (Test-Path $defaultPath) {
        return $defaultPath
    }

    return $null
}

function Show-InteractiveSigningNotice {
    param([string[]]$ArtifactPaths)

    $artifactSummary = if ($ArtifactPaths.Count -eq 1) {
        $ArtifactPaths[0]
    }
    else {
        $ArtifactPaths -join ", "
    }

    $message = @"
Abigail is ready for the final Windows signing step.

Artifact:
$artifactSummary

Approve the malware blocker and complete any SSL.com OTP verification now.
Click OK when you are ready for signing to continue.
"@

    $adapterPath = Get-AdapterPath
    if ($adapterPath) {
        try {
            Start-Process -FilePath $adapterPath | Out-Null
        }
        catch {
            Write-Warning "Could not auto-open the SSL.com eSigner adapter. You can open it manually if needed."
        }
    }

    try {
        [console]::Beep(1200, 250)
        Start-Sleep -Milliseconds 120
        [console]::Beep(1400, 350)
    }
    catch {
        # Ignore consoles that do not support beeps.
    }

    $shown = $false

    try {
        Add-Type -AssemblyName PresentationFramework -ErrorAction Stop
        [System.Windows.MessageBox]::Show(
            $message,
            "Abigail Windows Signing",
            [System.Windows.MessageBoxButton]::OK,
            [System.Windows.MessageBoxImage]::Warning
        ) | Out-Null
        $shown = $true
    }
    catch {
        $shown = $false
    }

    if (-not $shown) {
        try {
            $shell = New-Object -ComObject WScript.Shell
            $shell.Popup($message, 0, "Abigail Windows Signing", 48) | Out-Null
            $shown = $true
        }
        catch {
            $shown = $false
        }
    }

    if (-not $shown) {
        Write-Warning $message
        Read-Host "Press Enter when you are ready to continue signing" | Out-Null
    }
}

if (-not (Test-Path $SignToolPath)) {
    throw "SignTool path does not exist: $SignToolPath"
}

if ($null -eq $Files -or $Files.Count -eq 0) {
    throw "No files were provided to sign."
}

$noticeMarkerPath = Get-NoticeMarkerPath
if (-not (Test-Path $noticeMarkerPath)) {
    Show-InteractiveSigningNotice -ArtifactPaths $Files
    $markerDir = Split-Path $noticeMarkerPath -Parent
    if (-not [string]::IsNullOrWhiteSpace($markerDir) -and -not (Test-Path $markerDir)) {
        New-Item -ItemType Directory -Path $markerDir | Out-Null
    }
    Set-Content -Path $noticeMarkerPath -Value (Get-Date).ToString("o") -Encoding Ascii
}

$arguments = @(
    "sign",
    "/sha1",
    $CertificateThumbprint,
    "/fd",
    "sha256",
    "/td",
    "sha256",
    "/tr",
    $TimestampUrl
) + $Files

& $SignToolPath @arguments
if ($LASTEXITCODE -ne 0) {
    throw "SignTool failed with exit code $LASTEXITCODE."
}
