param([string]$Bundle = (Join-Path $PSScriptRoot '../hive-app/resources/bootstrap'))
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$Bundle = (Resolve-Path -LiteralPath $Bundle).Path
$config = Get-Content -LiteralPath (Join-Path $Bundle 'bundle.json') -Raw | ConvertFrom-Json
$profile = Join-Path ([IO.Path]::GetTempPath()) ('abigail-offline-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $profile | Out-Null
$socket = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 0)
$socket.Start(); $port = $socket.LocalEndpoint.Port; $socket.Stop()
$start = [Diagnostics.ProcessStartInfo]::new((Join-Path $Bundle $config.runtime), 'serve')
$start.UseShellExecute = $false
$start.CreateNoWindow = $true
$start.RedirectStandardOutput = $true
$start.RedirectStandardError = $true
$start.Environment['OLLAMA_HOST'] = "127.0.0.1:$port"
$start.Environment['OLLAMA_MODELS'] = Join-Path $Bundle $config.models
$start.Environment['OLLAMA_NO_CLOUD'] = '1'
$start.Environment['OLLAMA_ORIGINS'] = ''
$start.Environment['USERPROFILE'] = $profile
$start.Environment['HOME'] = $profile
# Disable ordinary outbound HTTP for this test child only; loopback remains usable.
# This is a network-dependency probe, not an OS firewall/network-isolation claim.
$start.Environment['HTTP_PROXY'] = 'http://127.0.0.1:9'
$start.Environment['HTTPS_PROXY'] = 'http://127.0.0.1:9'
$start.Environment['NO_PROXY'] = '127.0.0.1,localhost'
$process = [Diagnostics.Process]::Start($start)
$stdout = $process.StandardOutput.ReadToEndAsync()
$stderr = $process.StandardError.ReadToEndAsync()
try {
    $ready = $false
    for ($attempt = 0; $attempt -lt 100; $attempt++) {
        if ($process.HasExited) { throw "Packaged Ollama exited: $($stderr.Result)" }
        try { $null = Invoke-RestMethod "http://127.0.0.1:$port/api/tags" -TimeoutSec 1 -NoProxy; $ready = $true; break } catch { Start-Sleep -Milliseconds 200 }
    }
    if (-not $ready) { throw 'Packaged Ollama never listened' }
    $body = @{ model = $config.model; stream = $false; think = $false; options = @{num_ctx = 4096; num_predict = 160}; messages = @(@{role='system';content='You are Abigail, a concise local setup assistant. Explain API setup. Never ask for API keys in chat; direct the person to the secure Connect a model form.'}, @{role='user';content='Hello Abigail. Can you help me connect Claude later, while we talk offline now?'}) } | ConvertTo-Json -Depth 8
    $reply = Invoke-RestMethod "http://127.0.0.1:$port/api/chat" -Method Post -ContentType 'application/json' -Body $body -TimeoutSec 180 -NoProxy
    if (-not $reply.done -or [string]::IsNullOrWhiteSpace($reply.message.content)) { throw 'No real generated completion' }
    Write-Host "Isolated profile: $profile"
    Write-Host "Packaged model: $($config.model)"
    Write-Host "Completion: $($reply.message.content)"
} finally {
    if (-not $process.HasExited) { $process.Kill($true); $process.WaitForExit() }
    $stderr.Result | Set-Content -LiteralPath (Join-Path $profile 'ollama.log') -Encoding utf8NoBOM
    $process.Dispose()
}
