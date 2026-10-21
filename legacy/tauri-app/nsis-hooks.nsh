; Abigail NSIS installer hooks
; Stabilization lane defaults to clean-install behavior.

Function WriteVersionToRegistry
  WriteRegStr HKCU "Software\abigail\Abigail" "Version" "${VERSION}"
  WriteRegStr HKCU "Software\abigail\Abigail" "InstallPath" "$INSTDIR"
FunctionEnd

!macro NSIS_HOOK_PREINSTALL
  ; Intentionally empty for stabilization builds.
  ; Alpha-era backup/restore and upgrade preservation are removed so the new
  ; Hive + Entity Runtime architecture does not silently inherit stale state.
!macroend

!macro NSIS_HOOK_POSTINSTALL
  Call WriteVersionToRegistry
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  MessageBox MB_YESNO|MB_ICONQUESTION "Would you like to remove the Abigail local data directory as well?$\n$\nClick YES to remove local data.$\nClick NO to leave local data in place." IDYES RemoveData IDNO KeepData

RemoveData:
  ReadEnvStr $0 LOCALAPPDATA
  RMDir /r "$0\abigail\Abigail"
  Goto RegistryCleanup

KeepData:
  ; Leave local data in place for manual inspection or explicit import.

RegistryCleanup:
  DeleteRegKey HKCU "Software\abigail\Abigail"
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; Nothing special to do after uninstall.
!macroend
