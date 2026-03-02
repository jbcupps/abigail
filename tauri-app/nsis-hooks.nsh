; Abigail NSIS installer hooks
; Handles identity setup, LLM detection/installation, and upgrade detection
;
; Note: Tauri NSIS hooks don't support custom pages (nsDialogs).
; We use MessageBox dialogs and PowerShell for interactive prompts.

; ============================================================================
; VARIABLES
; ============================================================================
Var UpgradeDetected      ; "1" if existing install found, "0" otherwise
Var PreserveData         ; "1" to preserve, "0" for fresh install
Var ExistingVersion      ; Version string of existing install
Var TempResult           ; Temp variable for function results

; ============================================================================
; (LLM detection removed — Ollama is bundled with the app)
; ============================================================================

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
  ; Files: config.json, abigail_seed.db (SQLite + WAL files), external_pubkey.bin, secrets.bin, keys.bin,
  ;        docs/, and Hive files (global_settings.json, master.key, hive_secrets.bin, identities/)
  nsExec::ExecToStack 'powershell -ExecutionPolicy Bypass -Command "$$src = \"$1\"; $$dst = \"$2\"; if (Test-Path \"$$src\config.json\") { Copy-Item \"$$src\config.json\" \"$$dst\config.json\" -Force }; if (Test-Path \"$$src\abigail_seed.db\") { Copy-Item \"$$src\abigail_seed.db\" \"$$dst\abigail_seed.db\" -Force }; if (Test-Path \"$$src\abigail_seed.db-wal\") { Copy-Item \"$$src\abigail_seed.db-wal\" \"$$dst\abigail_seed.db-wal\" -Force }; if (Test-Path \"$$src\abigail_seed.db-shm\") { Copy-Item \"$$src\abigail_seed.db-shm\" \"$$dst\abigail_seed.db-shm\" -Force }; if (Test-Path \"$$src\external_pubkey.bin\") { Copy-Item \"$$src\external_pubkey.bin\" \"$$dst\external_pubkey.bin\" -Force }; if (Test-Path \"$$src\secrets.bin\") { Copy-Item \"$$src\secrets.bin\" \"$$dst\secrets.bin\" -Force }; if (Test-Path \"$$src\keys.bin\") { Copy-Item \"$$src\keys.bin\" \"$$dst\keys.bin\" -Force }; if (Test-Path \"$$src\docs\") { Copy-Item \"$$src\docs\*\" \"$$dst\docs\\" -Force -Recurse }; if (Test-Path \"$$src\global_settings.json\") { Copy-Item \"$$src\global_settings.json\" \"$$dst\global_settings.json\" -Force }; if (Test-Path \"$$src\master.key\") { Copy-Item \"$$src\master.key\" \"$$dst\master.key\" -Force }; if (Test-Path \"$$src\hive_secrets.bin\") { Copy-Item \"$$src\hive_secrets.bin\" \"$$dst\hive_secrets.bin\" -Force }; if (Test-Path \"$$src\identities\") { New-Item -ItemType Directory -Path \"$$dst\identities\" -Force | Out-Null; Copy-Item \"$$src\identities\*\" \"$$dst\identities\\" -Force -Recurse }; Write-Output OK"'
  Pop $0
  Pop $1

  DetailPrint "User data backed up for upgrade"
FunctionEnd

Function RestoreUserData
  ReadEnvStr $0 LOCALAPPDATA
  StrCpy $1 "$0\abigail\Abigail"
  StrCpy $2 "$TEMP\abigail_upgrade_backup"

  ; Restore files (using PowerShell for reliability)
  ; Files: config.json, abigail_seed.db (SQLite + WAL files), external_pubkey.bin, secrets.bin, keys.bin,
  ;        docs/, and Hive files (global_settings.json, master.key, hive_secrets.bin, identities/)
  nsExec::ExecToStack 'powershell -ExecutionPolicy Bypass -Command "$$src = \"$2\"; $$dst = \"$1\"; if (Test-Path \"$$src\config.json\") { Copy-Item \"$$src\config.json\" \"$$dst\config.json\" -Force }; if (Test-Path \"$$src\abigail_seed.db\") { Copy-Item \"$$src\abigail_seed.db\" \"$$dst\abigail_seed.db\" -Force }; if (Test-Path \"$$src\abigail_seed.db-wal\") { Copy-Item \"$$src\abigail_seed.db-wal\" \"$$dst\abigail_seed.db-wal\" -Force }; if (Test-Path \"$$src\abigail_seed.db-shm\") { Copy-Item \"$$src\abigail_seed.db-shm\" \"$$dst\abigail_seed.db-shm\" -Force }; if (Test-Path \"$$src\external_pubkey.bin\") { Copy-Item \"$$src\external_pubkey.bin\" \"$$dst\external_pubkey.bin\" -Force }; if (Test-Path \"$$src\secrets.bin\") { Copy-Item \"$$src\secrets.bin\" \"$$dst\secrets.bin\" -Force }; if (Test-Path \"$$src\keys.bin\") { Copy-Item \"$$src\keys.bin\" \"$$dst\keys.bin\" -Force }; if (Test-Path \"$$src\docs\") { New-Item -ItemType Directory -Path \"$$dst\docs\" -Force | Out-Null; Copy-Item \"$$src\docs\*\" \"$$dst\docs\\" -Force -Recurse }; if (Test-Path \"$$src\global_settings.json\") { Copy-Item \"$$src\global_settings.json\" \"$$dst\global_settings.json\" -Force }; if (Test-Path \"$$src\master.key\") { Copy-Item \"$$src\master.key\" \"$$dst\master.key\" -Force }; if (Test-Path \"$$src\hive_secrets.bin\") { Copy-Item \"$$src\hive_secrets.bin\" \"$$dst\hive_secrets.bin\" -Force }; if (Test-Path \"$$src\identities\") { New-Item -ItemType Directory -Path \"$$dst\identities\" -Force | Out-Null; Copy-Item \"$$src\identities\*\" \"$$dst\identities\\" -Force -Recurse }; Remove-Item \"$$src\" -Recurse -Force; Write-Output OK"'
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
; (LLM setup dialog removed — Ollama is bundled; no user action needed)
; ============================================================================

; ============================================================================
; UPGRADE DIALOG
; ============================================================================

Function ShowUpgradeDialog
  ${If} $UpgradeDetected == "1"
    ${If} $ExistingVersion != "unknown"
      MessageBox MB_YESNO|MB_ICONQUESTION "Upgrade Detected$\n$\nAn existing Abigail installation (v$ExistingVersion) was found.$\n$\nWould you like to preserve your existing data?$\n$\n- Config and settings$\n- Conversation history$\n- Signed documents$\n- Identity keys$\n- Hive multi-agent identities$\n$\nClick YES to preserve, NO for a fresh install." IDYES PreserveYes IDNO PreserveNo
    ${Else}
      MessageBox MB_YESNO|MB_ICONQUESTION "Upgrade Detected$\n$\nAn existing Abigail installation was found.$\n$\nWould you like to preserve your existing data?$\n$\n- Config and settings$\n- Conversation history$\n- Signed documents$\n- Identity keys$\n- Hive multi-agent identities$\n$\nClick YES to preserve, NO for a fresh install." IDYES PreserveYes IDNO PreserveNo
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
