; AO NSIS installer hooks
; Handles identity setup and Ollama detection/installation

Var OllamaDetected
Var OllamaUrl

!macro NSIS_HOOK_POSTINSTALL
  ; Step 1: Run identity keygen
  DetailPrint "Running AO identity setup..."
  ExecWait '"$INSTDIR\ao-keygen.exe"' $0
  DetailPrint "Identity setup completed (exit code: $0)"
  ${If} $0 != 0
    MessageBox MB_OK|MB_ICONEXCLAMATION "Identity setup did not complete. You can run it on first app launch."
  ${EndIf}

  ; Step 2: Check for Ollama
  DetailPrint "Checking for local LLM (Ollama)..."
  Call CheckOllama
!macroend

; Check if Ollama is installed and running
Function CheckOllama
  ; Try to detect Ollama via PowerShell
  nsExec::ExecToStack 'powershell -ExecutionPolicy Bypass -Command "try { $$r = Invoke-WebRequest -Uri http://localhost:11434/api/tags -TimeoutSec 3 -ErrorAction Stop; if ($$r.StatusCode -eq 200) { Write-Output OLLAMA_FOUND } } catch { Write-Output OLLAMA_NOT_FOUND }"'
  Pop $0  ; exit code
  Pop $1  ; output

  ${If} $1 == "OLLAMA_FOUND"
    StrCpy $OllamaDetected "1"
    StrCpy $OllamaUrl "http://localhost:11434"
    DetailPrint "Ollama detected at localhost:11434"
    ; Write config with Ollama URL
    Call WriteOllamaConfig
  ${Else}
    StrCpy $OllamaDetected "0"
    DetailPrint "Ollama not detected"
    ; Offer to download Ollama
    MessageBox MB_YESNO|MB_ICONQUESTION "Local LLM (Ollama) not detected.$\n$\nWould you like to download and install Ollama now?$\n$\nThis enables local AI processing without internet." IDYES DownloadOllama IDNO SkipOllama

DownloadOllama:
    DetailPrint "Downloading Ollama installer..."
    nsExec::ExecToStack 'powershell -ExecutionPolicy Bypass -Command "try { [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12; Invoke-WebRequest -Uri https://ollama.com/download/OllamaSetup.exe -OutFile $env:TEMP\OllamaSetup.exe -ErrorAction Stop; Write-Output OK } catch { Write-Output FAIL }"'
    Pop $0  ; exit code
    Pop $1  ; output (OK or FAIL)
    ${If} $1 == "OK"
      DetailPrint "Running Ollama installer..."
      ExecWait '"$TEMP\OllamaSetup.exe" /S' $0
      DetailPrint "Ollama installer completed (exit code: $0)"

      ; Wait for Ollama service to start
      DetailPrint "Waiting for Ollama service..."
      Sleep 5000

      ; Verify Ollama is now running
      nsExec::ExecToStack 'powershell -ExecutionPolicy Bypass -Command "try { $$r = Invoke-WebRequest -Uri http://localhost:11434/api/tags -TimeoutSec 5 -ErrorAction Stop; if ($$r.StatusCode -eq 200) { Write-Output OLLAMA_FOUND } } catch { Write-Output OLLAMA_NOT_FOUND }"'
      Pop $0
      Pop $1
      ${If} $1 == "OLLAMA_FOUND"
        StrCpy $OllamaDetected "1"
        StrCpy $OllamaUrl "http://localhost:11434"
        DetailPrint "Ollama installed and running"
        ; Offer to pull a model
        Call OfferModelPull
        ; Write config
        Call WriteOllamaConfig
      ${Else}
        MessageBox MB_OK|MB_ICONINFORMATION "Ollama was installed but is not yet running.$\n$\nPlease restart your computer or start Ollama manually."
      ${EndIf}
    ${Else}
      MessageBox MB_OK|MB_ICONEXCLAMATION "Failed to download Ollama. You can install it later from ollama.com"
    ${EndIf}
    Goto OllamaDone

SkipOllama:
    DetailPrint "Skipping Ollama installation"

OllamaDone:
  ${EndIf}
FunctionEnd

; Offer to pull a recommended model
Function OfferModelPull
  MessageBox MB_YESNO|MB_ICONQUESTION "Would you like to download a local AI model now?$\n$\nRecommended: phi3:mini (2.3GB)$\n$\nThis enables offline AI responses." IDYES PullModel IDNO SkipModel

PullModel:
  DetailPrint "Pulling phi3:mini model (this may take a few minutes)..."
  nsExec::ExecToStack 'ollama pull phi3:mini'
  Pop $0
  ${If} $0 == 0
    DetailPrint "Model phi3:mini downloaded successfully"
  ${Else}
    DetailPrint "Model download failed (you can run 'ollama pull phi3:mini' later)"
  ${EndIf}
  Goto ModelDone

SkipModel:
  DetailPrint "Skipping model download"

ModelDone:
FunctionEnd

; Write Ollama configuration to AO's config.json
Function WriteOllamaConfig
  ; Get AppData\Local path
  ReadEnvStr $0 LOCALAPPDATA
  StrCpy $1 "$0\ao\AO\config.json"

  ; Check if config exists
  IfFileExists $1 ConfigExists CreateConfig

ConfigExists:
  ; Update existing config - add local_llm_base_url if not present
  ; Using PowerShell for JSON manipulation
  nsExec::ExecToStack 'powershell -ExecutionPolicy Bypass -Command "$$c = Get-Content -Raw \"$1\" | ConvertFrom-Json; if (-not $$c.local_llm_base_url) { $$c | Add-Member -NotePropertyName local_llm_base_url -NotePropertyValue \"http://localhost:11434\" -Force }; $$c | ConvertTo-Json -Depth 10 | Set-Content \"$1\""'
  Pop $0
  DetailPrint "Updated config with Ollama URL"
  Goto ConfigDone

CreateConfig:
  ; Create new config with Ollama URL
  CreateDirectory "$0\ao\AO"
  FileOpen $2 $1 w
  FileWrite $2 '{"local_llm_base_url": "http://localhost:11434", "routing_mode": "ego_primary"}'
  FileClose $2
  DetailPrint "Created config with Ollama URL"

ConfigDone:
FunctionEnd
