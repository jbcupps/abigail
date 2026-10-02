; Unsigned MVP installers remove app files only. Family data is never deleted
; by an installer/uninstaller, including during silent acceptance installs.
; No upgrade backup/restore, updater, or signing behavior belongs in this lane.

!macro NSIS_HOOK_PREINSTALL
!macroend

!macro NSIS_HOOK_POSTINSTALL
!macroend

!macro NSIS_HOOK_PREUNINSTALL
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
!macroend
