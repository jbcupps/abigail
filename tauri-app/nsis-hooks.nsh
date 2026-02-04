; Abby NSIS installer hooks
; Runs the identity keygen tool after installation completes

!macro NSIS_HOOK_POSTINSTALL
  DetailPrint "Running Abby identity setup..."
  ExecWait '"$INSTDIR\abby-keygen.exe"' $0
  DetailPrint "Identity setup completed (exit code: $0)"
  ${If} $0 != 0
    MessageBox MB_OK|MB_ICONEXCLAMATION "Identity setup did not complete. You can run it on first app launch."
  ${EndIf}
!macroend
