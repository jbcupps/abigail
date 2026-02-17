; Abigail NSIS installer hooks
; Handles identity setup, LLM detection/installation, and upgrade detection
;
; Note: Tauri NSIS hooks don't support custom pages (nsDialogs).
; We use MessageBox dialogs and PowerShell for interactive prompts.

; ============================================================================
; VARIABLES
; ============================================================================
Var OllamaStatus         ; "running", "installed", "not_found"
Var LmStudioStatus       ; "installed", "not_found"
Var UpgradeDetected      ; "1" if existing install found, "0" otherwise
Var PreserveData         ; "1" to preserve, "0" for fresh install
Var ExistingVersion      ; Version string of existing install
Var TempResult           ; Temp variable for function results

; ============================================================================
; LLM DETECTION FUNCTIONS
; ============================================================================

; Detect Ollama via HTTP probe (running), file check (installed), registry
Function DetectOllama
  ; Method 1: HTTP probe - check if Ollama is running
  nsExec::ExecToStack 'powershell -ExecutionPolicy Bypass -Command "try { $$r = Invoke-WebRequest -Uri http://localhost:11434/api/tags -TimeoutSec 3 -ErrorAction Stop; if ($$r.StatusCode -eq 200) { Write-Output RUNNING } } catch { Write-Output NOT_RUNNING }"'
  Pop $0  ; exit code
  Pop $1  ; output

  ${If} $1 == "RUNNING"
    StrCpy $OllamaStatus "running"
    Return
  ${EndIf}

  ; Method 2: File check - common installation paths
  IfFileExists "$LOCALAPPDATA\Programs\Ollama\ollama.exe" OllamaFileFound 0
  IfFileExists "$PROGRAMFILES\Ollama\ollama.exe" OllamaFileFound 0
  IfFileExists "$PROGRAMFILES64\Ollama\ollama.exe" OllamaFileFound OllamaCheckRegistry

OllamaFileFound:
  StrCpy $OllamaStatus "installed"
  Return

OllamaCheckRegistry:
  ; Method 3: Registry check
  ReadRegStr $0 HKCU "SOFTWARE\Ollama" ""
  StrCmp $0 "" OllamaNotFound OllamaRegFound

OllamaRegFound:
  StrCpy $OllamaStatus "installed"
  Return

OllamaNotFound:
  StrCpy $OllamaStatus "not_found"
FunctionEnd

; Detect LM Studio via file check
Function DetectLmStudio
  ; Check common installation paths
  IfFileExists "$LOCALAPPDATA\Programs\LM Studio\LM Studio.exe" LmStudioFound 0
  IfFileExists "$PROGRAMFILES\LM Studio\LM Studio.exe" LmStudioFound 0
  IfFileExists "$LOCALAPPDATA\LM-Studio\LM Studio.exe" LmStudioFound LmStudioNotFound

LmStudioFound:
  StrCpy $LmStudioStatus "installed"
  Return

LmStudioNotFound:
  StrCpy $LmStudioStatus "not_found"
FunctionEnd

; ============================================================================
; UPGRADE DETECTION
; ============================================================================

Function CheckForExistingInstall
  ; Check registry for existing version
  ReadRegStr $ExistingVersion HKCU "Software\abigail\Abigail" "Version"
  StrCmp $ExistingVersion "" CheckDataDir FoundInRegistry

FoundInRegistry:
  StrCpy $UpgradeDetected "1"
  Return

CheckDataDir:
  ; Check if data directory exists with config.json
  ReadEnvStr $0 LOCALAPPDATA
  IfFileExists "$0\abigail\Abigail\config.json" FoundDataDir NoExistingInstall

FoundDataDir:
  StrCpy $UpgradeDetected "1"
  StrCpy $ExistingVersion "unknown"
  Return

NoExistingInstall:
  StrCpy $UpgradeDetected "0"
FunctionEnd

; ============================================================================
; DATA BACKUP/RESTORE (for upgrades)
; ============================================================================

Function BackupUserData
  ReadEnvStr $0 LOCALAPPDATA
  StrCpy $1 "$0\abigail\Abigail"
  StrCpy $2 "$TEMP\abigail_upgrade_backup"

  ; Create backup directory
  CreateDirectory $2
  CreateDirectory "$2\docs"

  ; Backup files (using PowerShell for reliability)
  ; Files: config.json, abigail_seed.db (SQLite), abigail_seed.db-wal, abigail_seed.db-shm (WAL files),
  ;        external_pubkey.bin, secrets.bin, keys.bin, docs/
  nsExec::ExecToStack 'powershell -ExecutionPolicy Bypass -Command "$$src = \"$1\"; $$dst = \"$2\"; if (Test-Path \"$$src\config.json\") { Copy-Item \"$$src\config.json\" \"$$dst\config.json\" -Force }; if (Test-Path \"$$src\abigail_seed.db\") { Copy-Item \"$$src\abigail_seed.db\" \"$$dst\abigail_seed.db\" -Force }; if (Test-Path \"$$src\abigail_seed.db-wal\") { Copy-Item \"$$src\abigail_seed.db-wal\" \"$$dst\abigail_seed.db-wal\" -Force }; if (Test-Path \"$$src\abigail_seed.db-shm\") { Copy-Item \"$$src\abigail_seed.db-shm\" \"$$dst\abigail_seed.db-shm\" -Force }; if (Test-Path \"$$src\external_pubkey.bin\") { Copy-Item \"$$src\external_pubkey.bin\" \"$$dst\external_pubkey.bin\" -Force }; if (Test-Path \"$$src\secrets.bin\") { Copy-Item \"$$src\secrets.bin\" \"$$dst\secrets.bin\" -Force }; if (Test-Path \"$$src\keys.bin\") { Copy-Item \"$$src\keys.bin\" \"$$dst\keys.bin\" -Force }; if (Test-Path \"$$src\docs\") { Copy-Item \"$$src\docs\*\" \"$$dst\docs\\" -Force -Recurse }; Write-Output OK"'
  Pop $0
  Pop $1

  DetailPrint "User data backed up for upgrade"
FunctionEnd

Function RestoreUserData
  ReadEnvStr $0 LOCALAPPDATA
  StrCpy $1 "$0\abigail\Abigail"
  StrCpy $2 "$TEMP\abigail_upgrade_backup"

  ; Restore files (using PowerShell for reliability)
  ; Files: config.json, abigail_seed.db (SQLite + WAL files), external_pubkey.bin, secrets.bin, keys.bin, docs/
  nsExec::ExecToStack 'powershell -ExecutionPolicy Bypass -Command "$$src = \"$2\"; $$dst = \"$1\"; if (Test-Path \"$$src\config.json\") { Copy-Item \"$$src\config.json\" \"$$dst\config.json\" -Force }; if (Test-Path \"$$src\abigail_seed.db\") { Copy-Item \"$$src\abigail_seed.db\" \"$$dst\abigail_seed.db\" -Force }; if (Test-Path \"$$src\abigail_seed.db-wal\") { Copy-Item \"$$src\abigail_seed.db-wal\" \"$$dst\abigail_seed.db-wal\" -Force }; if (Test-Path \"$$src\abigail_seed.db-shm\") { Copy-Item \"$$src\abigail_seed.db-shm\" \"$$dst\abigail_seed.db-shm\" -Force }; if (Test-Path \"$$src\external_pubkey.bin\") { Copy-Item \"$$src\external_pubkey.bin\" \"$$dst\external_pubkey.bin\" -Force }; if (Test-Path \"$$src\secrets.bin\") { Copy-Item \"$$src\secrets.bin\" \"$$dst\secrets.bin\" -Force }; if (Test-Path \"$$src\keys.bin\") { Copy-Item \"$$src\keys.bin\" \"$$dst\keys.bin\" -Force }; if (Test-Path \"$$src\docs\") { New-Item -ItemType Directory -Path \"$$dst\docs\" -Force | Out-Null; Copy-Item \"$$src\docs\*\" \"$$dst\docs\\" -Force -Recurse }; Remove-Item \"$$src\" -Recurse -Force; Write-Output OK"'
  Pop $0
  Pop $1

  DetailPrint "User data restored"
FunctionEnd

; ============================================================================
; WRITE VERSION TO REGISTRY
; ============================================================================

Function WriteVersionToRegistry
  ; Write version and install path to registry
  WriteRegStr HKCU "Software\abigail\Abigail" "Version" "${VERSION}"
  WriteRegStr HKCU "Software\abigail\Abigail" "InstallPath" "$INSTDIR"
FunctionEnd

; ============================================================================
; OLLAMA INSTALLATION
; ============================================================================

Function InstallOllama
  DetailPrint "Downloading Ollama installer..."

  ; Download using PowerShell
  nsExec::ExecToStack 'powershell -ExecutionPolicy Bypass -Command "try { [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12; Invoke-WebRequest -Uri https://ollama.com/download/OllamaSetup.exe -OutFile $env:TEMP\OllamaSetup.exe -ErrorAction Stop; Write-Output OK } catch { Write-Output FAIL }"'
  Pop $0  ; exit code
  Pop $1  ; output

  ${If} $1 == "OK"
    DetailPrint "Running Ollama installer (silent)..."
    ExecWait '"$TEMP\OllamaSetup.exe" /S' $0
    DetailPrint "Ollama installer completed (exit code: $0)"

    ; Wait for Ollama service to start
    DetailPrint "Waiting for Ollama service to start..."
    Sleep 5000

    ; Verify Ollama is now running
    nsExec::ExecToStack 'powershell -ExecutionPolicy Bypass -Command "try { $$r = Invoke-WebRequest -Uri http://localhost:11434/api/tags -TimeoutSec 5 -ErrorAction Stop; if ($$r.StatusCode -eq 200) { Write-Output RUNNING } } catch { Write-Output NOT_RUNNING }"'
    Pop $0
    Pop $1

    ${If} $1 == "RUNNING"
      StrCpy $OllamaStatus "running"
      DetailPrint "Ollama installed and running successfully"

      ; Offer to pull model
      MessageBox MB_YESNO|MB_ICONQUESTION "Ollama is installed and running!$\n$\nWould you like to download the phi3:mini model (~2.3GB)?$\n$\nThis enables offline AI responses." IDYES PullModel IDNO SkipModel

PullModel:
      DetailPrint "Downloading phi3:mini model..."
      nsExec::ExecToStack 'ollama pull phi3:mini'
      Pop $0
      ${If} $0 == 0
        DetailPrint "Model phi3:mini downloaded successfully"
      ${Else}
        DetailPrint "Model download failed - you can run 'ollama pull phi3:mini' later"
      ${EndIf}
      Goto ModelDone

SkipModel:
      DetailPrint "Skipping model download"

ModelDone:
      ; Write config with Ollama URL
      Call WriteOllamaConfig
    ${Else}
      DetailPrint "Ollama installed but not yet running"
      MessageBox MB_OK|MB_ICONINFORMATION "Ollama was installed but is not yet running.$\n$\nIt should start automatically on next login, or you can start it manually from the Start menu."
    ${EndIf}
  ${Else}
    MessageBox MB_OK|MB_ICONEXCLAMATION "Failed to download Ollama.$\n$\nYou can install it later from https://ollama.com"
  ${EndIf}
FunctionEnd

; ============================================================================
; CONFIG WRITING
; ============================================================================

Function WriteOllamaConfig
  ; Get AppData\Local path
  ReadEnvStr $0 LOCALAPPDATA
  StrCpy $1 "$0\abigail\Abigail\config.json"

  ; Check if config exists and update/create appropriately
  ; Note: schema_version=2 matches CONFIG_SCHEMA_VERSION in abigail-core/src/config.rs
  nsExec::ExecToStack 'powershell -ExecutionPolicy Bypass -Command "$$path = \"$1\"; $$dir = Split-Path $$path; if (-not (Test-Path $$dir)) { New-Item -ItemType Directory -Path $$dir -Force | Out-Null }; if (Test-Path $$path) { $$c = Get-Content -Raw $$path | ConvertFrom-Json; if (-not $$c.local_llm_base_url) { $$c | Add-Member -NotePropertyName local_llm_base_url -NotePropertyValue \"http://localhost:11434\" -Force }; if (-not $$c.PSObject.Properties[\"schema_version\"]) { $$c | Add-Member -NotePropertyName schema_version -NotePropertyValue 2 -Force }; $$c | ConvertTo-Json -Depth 10 | Set-Content $$path } else { @{local_llm_base_url=\"http://localhost:11434\"; routing_mode=\"ego_primary\"; schema_version=2} | ConvertTo-Json | Set-Content $$path }; Write-Output OK"'
  Pop $0
  Pop $1

  DetailPrint "Config updated with Ollama URL"
FunctionEnd

; ============================================================================
; LLM SETUP DIALOG (uses MessageBox since nsDialogs pages not supported in hooks)
; ============================================================================

Function ShowLlmSetupDialog
  ; Run detection
  Call DetectOllama
  Call DetectLmStudio

  ; Build status message
  StrCpy $TempResult ""

  ; Ollama status
  ${If} $OllamaStatus == "running"
    StrCpy $TempResult "Ollama: Running on port 11434$\n"
  ${ElseIf} $OllamaStatus == "installed"
    StrCpy $TempResult "Ollama: Installed (not running)$\n"
  ${Else}
    StrCpy $TempResult "Ollama: Not detected$\n"
  ${EndIf}

  ; LM Studio status
  ${If} $LmStudioStatus == "installed"
    StrCpy $TempResult "$TempResultLM Studio: Installed$\n"
  ${Else}
    StrCpy $TempResult "$TempResultLM Studio: Not detected$\n"
  ${EndIf}

  ; If both missing, offer to install Ollama
  ${If} $OllamaStatus == "not_found"
  ${AndIf} $LmStudioStatus == "not_found"
    MessageBox MB_YESNO|MB_ICONQUESTION "Local LLM Status:$\n$\n$TempResult$\nNo local LLM detected. Abigail works best with a local LLM for privacy and offline use.$\n$\nWould you like to download and install Ollama now (recommended)?" IDYES DoInstallOllama IDNO CheckLmStudio

DoInstallOllama:
    Call InstallOllama
    Goto LlmSetupDone

CheckLmStudio:
    MessageBox MB_YESNO|MB_ICONQUESTION "Would you like to open the LM Studio download page instead?$\n$\nLM Studio is a GUI-based LLM manager with a model browser." IDYES DoLmStudio IDNO LlmSetupDone

DoLmStudio:
    ExecShell "open" "https://lmstudio.ai/download"
    MessageBox MB_OK|MB_ICONINFORMATION "LM Studio download page opened in your browser.$\n$\nAfter installing LM Studio:$\n1. Launch LM Studio$\n2. Download a model (e.g., Phi-3 Mini)$\n3. Start the local server (Developer tab > Start Server)$\n4. Abigail will auto-detect it at http://localhost:1234"
    Goto LlmSetupDone
  ${EndIf}

  ; If only Ollama missing but LM Studio installed
  ${If} $OllamaStatus == "not_found"
  ${AndIf} $LmStudioStatus == "installed"
    MessageBox MB_OK|MB_ICONINFORMATION "Local LLM Status:$\n$\n$TempResult$\nLM Studio is installed. Make sure to:$\n1. Load a model$\n2. Start the local server (Developer tab > Start Server)$\n$\nYou can also install Ollama later from https://ollama.com"
    Goto LlmSetupDone
  ${EndIf}

  ; If Ollama installed but not running
  ${If} $OllamaStatus == "installed"
    MessageBox MB_OK|MB_ICONINFORMATION "Local LLM Status:$\n$\n$TempResult$\nOllama is installed but not running. It should auto-start on login, or you can start it manually from the Start menu.$\n$\nMake sure you have at least one model. Run: ollama pull phi3:mini"
    Goto LlmSetupDone
  ${EndIf}

  ; If Ollama is running, just inform
  ${If} $OllamaStatus == "running"
    ; Silently write config, don't bother user
    Call WriteOllamaConfig
  ${EndIf}

LlmSetupDone:
FunctionEnd

; ============================================================================
; UPGRADE DIALOG
; ============================================================================

Function ShowUpgradeDialog
  ${If} $UpgradeDetected == "1"
    ${If} $ExistingVersion != "unknown"
      MessageBox MB_YESNO|MB_ICONQUESTION "Upgrade Detected$\n$\nAn existing Abigail installation (v$ExistingVersion) was found.$\n$\nWould you like to preserve your existing data?$\n$\n- Config and settings$\n- Conversation history$\n- Signed documents$\n- Identity keys$\n$\nClick YES to preserve, NO for a fresh install." IDYES PreserveYes IDNO PreserveNo
    ${Else}
      MessageBox MB_YESNO|MB_ICONQUESTION "Upgrade Detected$\n$\nAn existing Abigail installation was found.$\n$\nWould you like to preserve your existing data?$\n$\n- Config and settings$\n- Conversation history$\n- Signed documents$\n- Identity keys$\n$\nClick YES to preserve, NO for a fresh install." IDYES PreserveYes IDNO PreserveNo
    ${EndIf}

PreserveYes:
    StrCpy $PreserveData "1"
    Goto UpgradeDone

PreserveNo:
    StrCpy $PreserveData "0"
    MessageBox MB_YESNO|MB_ICONEXCLAMATION "WARNING: Fresh install selected.$\n$\nThis will delete all existing Abigail data including:$\n- Your identity and trust relationship$\n- All conversation history$\n- Any customizations$\n$\nAre you sure?" IDYES ConfirmFresh IDNO PreserveYes

ConfirmFresh:
    StrCpy $PreserveData "0"

UpgradeDone:
  ${Else}
    StrCpy $PreserveData "0"
  ${EndIf}
FunctionEnd

; ============================================================================
; PRE-INSTALL HOOK
; ============================================================================

!macro NSIS_HOOK_PREINSTALL
  ; Check for existing installation
  Call CheckForExistingInstall

  ; Show upgrade dialog if needed
  Call ShowUpgradeDialog

  ; If upgrade detected and user chose preserve, backup data
  ${If} $UpgradeDetected == "1"
  ${AndIf} $PreserveData == "1"
    Call BackupUserData
  ${EndIf}
!macroend

; ============================================================================
; POST-INSTALL HOOK
; ============================================================================

!macro NSIS_HOOK_POSTINSTALL
  ; Step 1: Restore user data if upgrading
  ; Note: Identity keygen removed — the in-app birth sequence handles key generation.
  ${If} $UpgradeDetected == "1"
  ${AndIf} $PreserveData == "1"
    Call RestoreUserData
  ${EndIf}

  ; Step 2: Write version to registry
  Call WriteVersionToRegistry

  ; Step 3: LLM setup dialog — skip if bundled Ollama is present
  ; (Abigail will manage Ollama automatically at runtime)
  IfFileExists "$INSTDIR\ollama\ollama.exe" BundledOllamaFound ShowLlmDialog

ShowLlmDialog:
  Call ShowLlmSetupDialog
  Goto PostInstallDone

BundledOllamaFound:
  DetailPrint "Bundled Ollama detected — skipping LLM setup dialog"

PostInstallDone:
!macroend

; ============================================================================
; UNINSTALL HOOKS
; ============================================================================

!macro NSIS_HOOK_PREUNINSTALL
  ; Ask if user wants to keep their data
  MessageBox MB_YESNO|MB_ICONQUESTION "Would you like to keep your Abigail data (config, memories, documents)?$\n$\nClick YES to keep data for a future reinstall.$\nClick NO to remove everything." IDYES KeepData IDNO RemoveData

KeepData:
  ; Just remove the version from registry, keep data
  DeleteRegKey HKCU "Software\abigail\Abigail"
  Goto UninstallDataDone

RemoveData:
  ; Remove all data
  ReadEnvStr $0 LOCALAPPDATA
  RMDir /r "$0\abigail\Abigail"
  DeleteRegKey HKCU "Software\abigail\Abigail"

UninstallDataDone:
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; Nothing special to do after uninstall
!macroend
