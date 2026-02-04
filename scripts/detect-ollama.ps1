# detect-ollama.ps1
# Checks if Ollama is running at localhost:11434
# Returns: OLLAMA_FOUND (exit 0) or OLLAMA_NOT_FOUND (exit 1)

$ErrorActionPreference = "SilentlyContinue"

try {
    $response = Invoke-WebRequest -Uri "http://localhost:11434/api/tags" -TimeoutSec 3 -ErrorAction Stop
    if ($response.StatusCode -eq 200) {
        Write-Output "OLLAMA_FOUND"
        exit 0
    }
} catch {
    # Ollama not responding
}

Write-Output "OLLAMA_NOT_FOUND"
exit 1
