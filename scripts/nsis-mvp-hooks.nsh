; Unsigned MVP installers remove app files only. Family data is never deleted
; by an installer/uninstaller, including during silent acceptance installs.
; No upgrade backup/restore, updater, or signing behavior belongs in this lane.

!ifndef ABIGAIL_INSTALL_LOCATION_REGISTRY_KEY
  !define ABIGAIL_INSTALL_LOCATION_REGISTRY_KEY "Software\abigail\Abigail"
!endif
!ifndef ABIGAIL_UNINSTALL_REGISTRY_ROOT
  !define ABIGAIL_UNINSTALL_REGISTRY_ROOT "Software\Microsoft\Windows\CurrentVersion\Uninstall"
!endif

; The hook is included before Tauri declares its pages and sections. This
; skipped page runs before the old installer's uninstaller can be launched.
; Silent installers skip pages, so the first hidden section repeats the check.
Page custom AbigailGuardEarlyPage
Section "-Abigail executable preflight"
  SectionIn RO
  Call AbigailGuardRegisteredInstall
SectionEnd

; Only probe existing executable bytes. GENERIC_WRITE | DELETE tests both
; extraction and removal access; sharing all access does not lock other users
; out. OPEN_EXISTING neither creates nor truncates a file. Missing paths alone
; are safe for a fresh installation; permission and sharing failures must stop.
!macro AbigailDefinePayloadGuard Prefix
Function ${Prefix}AbigailGuardExecutable
  Exch $0
  Push $1
  Push $2
  System::Call 'kernel32::CreateFileW(w r0, i 0x40010000, i 7, p 0, i 3, i 0, p 0) p .r1 ?e'
  Pop $2
  StrCmp $1 -1 abigail_guard_failed
  System::Call 'kernel32::CloseHandle(p r1) i'
  Goto abigail_guard_done

  abigail_guard_failed:
    StrCmp $2 2 abigail_guard_done ; ERROR_FILE_NOT_FOUND
    StrCmp $2 3 abigail_guard_done ; ERROR_PATH_NOT_FOUND
    DetailPrint "Abigail setup stopped before changing files: $0 (Windows error $2)."
    IfSilent abigail_guard_quit
    MessageBox MB_OK|MB_ICONSTOP "Abigail setup cannot replace or remove this file:$\r$\n$0$\r$\n$\r$\nClose Abigail and all Entity windows, then run setup again. If they are already closed, restart Windows and run setup again.$\r$\n$\r$\nWindows error: $2"
  abigail_guard_quit:
    Pop $2
    Pop $1
    Pop $0
    SetErrorLevel 32
    Quit

  abigail_guard_done:
    Pop $2
    Pop $1
    Pop $0
FunctionEnd

Function ${Prefix}AbigailGuardPayload
  Exch $0
  Push $1
  StrCmp $0 "" abigail_payload_done
  ; NSIS records InstallLocation with surrounding quotes; registry values are
  ; paths, not commands. Remove only their outer quotes before probing.
  StrCpy $1 $0 1
  StrCmp $1 '"' 0 +2
    StrCpy $0 $0 "" 1
  StrCpy $1 $0 1 -1
  StrCmp $1 '"' 0 +2
    StrCpy $0 $0 -1
  StrCmp $0 "" abigail_payload_done
  Push "$0\Abigail.exe"
  Call ${Prefix}AbigailGuardExecutable
  Push "$0\resources\abigail-entity-runtime-app.exe"
  Call ${Prefix}AbigailGuardExecutable
  Push "$0\resources\hive-daemon.exe"
  Call ${Prefix}AbigailGuardExecutable
  Push "$0\resources\entity-daemon.exe"
  Call ${Prefix}AbigailGuardExecutable
  abigail_payload_done:
    Pop $1
    Pop $0
FunctionEnd
!macroend
!insertmacro AbigailDefinePayloadGuard ""
!insertmacro AbigailDefinePayloadGuard "un."

; Probe only Abigail's registered locations. This also covers an MSI location
; the existing Tauri maintenance page may remove before extracting any files.
; Keep the caller's registry view; Tauri selects it during .onInit.
!macro AbigailGuardRegistryLocations Hive Label
  ReadRegStr $R0 ${Hive} "${ABIGAIL_INSTALL_LOCATION_REGISTRY_KEY}" ""
  Push $R0
  Call AbigailGuardPayload
  ReadRegStr $R0 ${Hive} "${ABIGAIL_UNINSTALL_REGISTRY_ROOT}\Abigail" "InstallLocation"
  Push $R0
  Call AbigailGuardPayload
  StrCpy $R1 0
  abigail_registry_${Label}_next:
    EnumRegKey $R2 ${Hive} "${ABIGAIL_UNINSTALL_REGISTRY_ROOT}" $R1
    StrCmp $R2 "" abigail_registry_${Label}_done
    IntOp $R1 $R1 + 1
    ReadRegStr $R3 ${Hive} "${ABIGAIL_UNINSTALL_REGISTRY_ROOT}\$R2" "DisplayName"
    StrCmp $R3 "Abigail" 0 abigail_registry_${Label}_next
    ReadRegStr $R3 ${Hive} "${ABIGAIL_UNINSTALL_REGISTRY_ROOT}\$R2" "Publisher"
    StrCmp $R3 "abigail" 0 abigail_registry_${Label}_next
    ReadRegStr $R0 ${Hive} "${ABIGAIL_UNINSTALL_REGISTRY_ROOT}\$R2" "InstallLocation"
    Push $R0
    Call AbigailGuardPayload
    Goto abigail_registry_${Label}_next
  abigail_registry_${Label}_done:
!macroend

Function AbigailGuardRegisteredInstall
  Push $R0
  Push $R1
  Push $R2
  Push $R3
  Push $INSTDIR
  Call AbigailGuardPayload
  !insertmacro AbigailGuardRegistryLocations HKCU current
  !insertmacro AbigailGuardRegistryLocations HKLM machine
  Pop $R3
  Pop $R2
  Pop $R1
  Pop $R0
FunctionEnd

Function AbigailGuardEarlyPage
  Call AbigailGuardRegisteredInstall
  Abort ; Success skips this invisible page and continues to Welcome.
FunctionEnd

!macro NSIS_HOOK_PREINSTALL
  Push $INSTDIR
  Call AbigailGuardPayload
!macroend

!macro NSIS_HOOK_POSTINSTALL
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  Push $INSTDIR
  Call un.AbigailGuardPayload
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
!macroend
