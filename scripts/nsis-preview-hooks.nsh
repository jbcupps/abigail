; Preview only. These checks never remove data or stop a process.
; Tauri CLI 2.11.4 calls PREINSTALL after SetOutPath (which can create INSTDIR)
; and its prerequisite/reinstall pages, but before payload/registry writes.
; PREUNINSTALL runs before payload deletion. These hooks are not an atomic
; defense against another process changing filesystem links after validation.
!ifndef ABIGAIL_PREVIEW_HOOKS_INCLUDED
!define ABIGAIL_PREVIEW_HOOKS_INCLUDED
!include LogicLib.nsh
!include FileFunc.nsh

!define ABIGAIL_STABLE_UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\Abigail"
!define ABIGAIL_STABLE_LOCATION_KEY "Software\abigail\Abigail"
!define ABIGAIL_PREVIEW_PROFILE_RELATIVE "AbigailPreview\profiles\ongoing"

Var AbigailPreviewCandidate
Var AbigailPreviewFailure

!macro AbigailPreviewAssertIdentity
  !if "${PRODUCTNAME}" != "Abigail Preview"
    !error "Preview hooks require PRODUCTNAME Abigail Preview."
  !endif
  !if "${BUNDLEID}" != "com.abigail.preview"
    !error "Preview hooks require BUNDLEID com.abigail.preview."
  !endif
  !if "${MAINBINARYNAME}" != "Abigail Preview"
    !error "Preview hooks require MAINBINARYNAME Abigail Preview."
  !endif
  !if "${INSTALLMODE}" != "currentUser"
    !error "Preview hooks require currentUser installation."
  !endif
!macroend

; Inputs are canonical volume-GUID paths without trailing separators.
; StrCmp is case-insensitive; separator boundaries avoid rejecting siblings
; such as "Abigail Preview" merely because their names start with "Abigail".
; Kept as a pure macro so the synthetic tests exercise these actual operations.
!macro AbigailPreviewCheckOverlap Candidate Protected FailureLabel
  StrCmp "${Candidate}" "${Protected}" ${FailureLabel}
  StrLen $R9 "${Protected}\"
  StrCpy $R8 "${Candidate}" $R9
  StrCmp $R8 "${Protected}\" ${FailureLabel}
  StrLen $R9 "${Candidate}\"
  StrCpy $R8 "${Protected}" $R9
  StrCmp $R8 "${Candidate}\" ${FailureLabel}
!macroend

!macro AbigailPreviewReadLocations Prefix Hive Access
  Push "${Hive}"
  Push "${Access}"
  Push "${ABIGAIL_STABLE_UNINSTALL_KEY}"
  Push "InstallLocation"
  Call ${Prefix}AbigailPreviewReadStableLocation
  Push "${Hive}"
  Push "${Access}"
  Push "${ABIGAIL_STABLE_LOCATION_KEY}"
  Push ""
  Call ${Prefix}AbigailPreviewReadStableLocation
!macroend

!macro AbigailPreviewFunctions Prefix
Function ${Prefix}AbigailPreviewFail
  DetailPrint "$AbigailPreviewFailure"
  IfSilent +2
    MessageBox MB_OK|MB_ICONSTOP "$AbigailPreviewFailure"
  SetErrorLevel 2
  Abort
FunctionEnd

; Stack input/output: absolute drive path -> canonical volume-GUID path.
; Only local fixed disks are supported. Unknown, denied, truncated, malformed,
; non-directory and reparse-point paths fail closed. Missing final components
; are appended to the native canonical path of their closest existing parent.
; Volume GUIDs and native normalization also handle DOS/8.3 drive aliases.
Function ${Prefix}AbigailPreviewCanonicalPath
  Exch $0
  Push $1
  Push $2
  Push $3
  Push $4
  Push $5
  Push $6
  Push $7
  Push $8
  Push $9
  StrCpy $AbigailPreviewFailure "Preview installation stopped: a protected or selected location cannot be verified as a local directory without filesystem links."
  StrCpy $1 $0 2 1
  StrCmp $1 ":\" 0 canonical_fail
  StrCpy $1 $0 3
  System::Call 'kernel32::GetDriveTypeW(w r1) i.r2'
  StrCmp $2 3 0 canonical_fail
  System::Call 'kernel32::GetFullPathNameW(w r0, i ${NSIS_MAX_STRLEN}, w .r1, p 0) i.r2'
  StrCmp $2 0 canonical_fail
  IntCmp $2 ${NSIS_MAX_STRLEN} canonical_fail +1 canonical_fail
  ; Exclude alternate streams, wildcards, device/UNC syntax and ambiguous
  ; trailing spaces/dots in a component. A normal drive colon is index 1.
  StrCpy $8 0
  StrCpy $9 ""
  canonical_char:
    StrCpy $2 $1 1 $8
    StrCmp $2 "" canonical_chars_done
    StrCmp $2 ":" 0 +2
      IntCmp $8 1 +1 canonical_fail canonical_fail
    StrCmp $2 '"' canonical_fail
    StrCmp $2 "*" canonical_fail
    StrCmp $2 "?" canonical_fail
    StrCmp $2 "<" canonical_fail
    StrCmp $2 ">" canonical_fail
    StrCmp $2 "|" canonical_fail
    StrCmp $2 "/" canonical_fail
    StrCmp $2 "\" 0 canonical_next_char
      StrCmp $9 "." canonical_fail
      StrCmp $9 " " canonical_fail
    canonical_next_char:
      StrCpy $9 $2
      IntOp $8 $8 + 1
      Goto canonical_char
  canonical_chars_done:
    StrCmp $9 "." canonical_fail
    StrCmp $9 " " canonical_fail
  ; Trim trailing backslashes except the drive root.
  canonical_trim:
    StrLen $2 $1
    IntCmp $2 3 canonical_trim_done canonical_fail +1
    StrCpy $2 $1 1 -1
    StrCmp $2 "\" 0 canonical_trim_done
    StrCpy $1 $1 -1
    Goto canonical_trim
  canonical_trim_done:
    StrCpy $3 ""
    StrCpy $4 ""
  canonical_walk:
    System::Call 'kernel32::GetFileAttributesW(w r1) i.r2 ?e'
    Pop $5
    StrCmp $2 -1 0 canonical_existing
    ; Only missing paths are allowed, and only before finding an ancestor.
    StrCmp $4 "" 0 canonical_fail
    StrCmp $5 2 canonical_missing
    StrCmp $5 3 canonical_missing canonical_fail
  canonical_missing:
    ${GetFileName} $1 $5
    StrCmp $5 "" canonical_fail
    StrCmp $3 "" 0 +3
      StrCpy $3 $5
      Goto canonical_parent
    StrCpy $3 "$5\$3"
    Goto canonical_parent
  canonical_existing:
    IntOp $5 $2 & 0x400
    StrCmp $5 0 0 canonical_fail
    IntOp $5 $2 & 0x10
    StrCmp $5 0 canonical_fail
    StrCmp $4 "" 0 canonical_parent
    StrCpy $4 $1
  canonical_parent:
    StrLen $2 $1
    IntCmp $2 3 canonical_resolve canonical_fail +1
    ${GetParent} $1 $5
    StrCmp $5 $1 canonical_fail
    ; FileFunc returns C: as the parent of C:\name.
    StrLen $2 $5
    IntCmp $2 2 0 +2 +2
      StrCpy $5 "$5\"
    StrCpy $1 $5
    Goto canonical_walk
  canonical_resolve:
    StrCmp $4 "" canonical_fail
    ; READ_ATTRIBUTES, shared read/write/delete, OPEN_EXISTING, directory handle.
    System::Call 'kernel32::CreateFileW(w r4, i 0x80, i 7, p 0, i 3, i 0x02000000, p 0) p.r6'
    StrCmp $6 -1 canonical_fail
    ; FILE_NAME_NORMALIZED | VOLUME_NAME_GUID. Do not compare drive aliases.
    System::Call 'kernel32::GetFinalPathNameByHandleW(p r6, w .r7, i ${NSIS_MAX_STRLEN}, i 1) i.r8'
    System::Call 'kernel32::CloseHandle(p r6)'
    StrCmp $8 0 canonical_fail
    IntCmp $8 ${NSIS_MAX_STRLEN} canonical_fail +1 canonical_fail
    StrCpy $2 $7 1 -1
    StrCmp $2 "\" 0 +2
      StrCpy $7 $7 -1
    StrCmp $3 "" 0 +3
      StrCpy $0 $7
      Goto canonical_done
    StrCpy $0 "$7\$3"
    StrLen $2 $0
    ; Reject near-capacity results rather than accept a truncated append.
    IntOp $8 ${NSIS_MAX_STRLEN} - 1
    IntCmp $2 $8 canonical_fail canonical_done canonical_fail
  canonical_fail:
    Call ${Prefix}AbigailPreviewFail
  canonical_done:
    Pop $9
    Pop $8
    Pop $7
    Pop $6
    Pop $5
    Pop $4
    Pop $3
    Pop $2
    Pop $1
    Exch $0
FunctionEnd

Function ${Prefix}AbigailPreviewProtectPath
  Call ${Prefix}AbigailPreviewCanonicalPath
  Pop $0
  !insertmacro AbigailPreviewCheckOverlap "$AbigailPreviewCandidate" "$0" overlap
  Return
  overlap:
    StrCpy $AbigailPreviewFailure "Preview installation stopped: the selected folder overlaps the stable installation or the protected preview profile. Choose a separate application folder."
    Call ${Prefix}AbigailPreviewFail
FunctionEnd

; Four arguments: hive, KEY_QUERY_VALUE|WOW64 view, key, value name.
; Opening separately distinguishes an absent key from an unreadable key.
; An existing key with an absent/malformed location is not treated as safe.
Function ${Prefix}AbigailPreviewReadStableLocation
  Pop $3
  Pop $2
  Pop $1
  Pop $0
  StrCpy $AbigailPreviewFailure "Preview installation stopped: a stable installation registry location is unreadable or incomplete."
  System::Call 'advapi32::RegOpenKeyExW(p r0, w r2, i 0, i r1, *p .r4) i.r6'
  StrCmp $6 2 registry_absent
  StrCmp $6 3 registry_absent
  StrCmp $6 0 0 registry_fail
  IntOp $7 ${NSIS_MAX_STRLEN} * 2
  ; REG_SZ or expanded REG_EXPAND_SZ, bytes including the terminating null.
  System::Call 'advapi32::RegGetValueW(p r4, p 0, w r3, i 0x6, p 0, w .r5, *i r7) i.r6'
  System::Call 'advapi32::RegCloseKey(p r4)'
  StrCmp $6 0 0 registry_fail
  StrCmp $5 "" registry_fail
  ; Tauri stores InstallLocation wrapped in quotes, remembered location raw.
  StrCpy $6 $5 1
  StrCmp $6 '"' 0 registry_check
    StrCpy $6 $5 1 -1
    StrCmp $6 '"' 0 registry_fail
    StrCpy $5 $5 -1 1
  registry_check:
    Push $5
    Call ${Prefix}AbigailPreviewProtectPath
  registry_absent:
    Return
  registry_fail:
    Call ${Prefix}AbigailPreviewFail
FunctionEnd

Function ${Prefix}AbigailPreviewGuard
  Push $0
  Push $1
  Push $2
  Push $3
  Push $4
  Push $5
  Push $6
  Push $7
  Push $8
  Push $9
  Push $R8
  Push $R9
  Push $INSTDIR
  Call ${Prefix}AbigailPreviewCanonicalPath
  Pop $AbigailPreviewCandidate
  Push "$LOCALAPPDATA\Abigail"
  Call ${Prefix}AbigailPreviewProtectPath
  Push "$LOCALAPPDATA\${ABIGAIL_PREVIEW_PROFILE_RELATIVE}"
  Call ${Prefix}AbigailPreviewProtectPath
  ; HKCU/HKLM, 64-bit and 32-bit views; no registry writes or view changes.
  !insertmacro AbigailPreviewReadLocations "${Prefix}" 0x80000001 0x0101
  !insertmacro AbigailPreviewReadLocations "${Prefix}" 0x80000001 0x0201
  !insertmacro AbigailPreviewReadLocations "${Prefix}" 0x80000002 0x0101
  !insertmacro AbigailPreviewReadLocations "${Prefix}" 0x80000002 0x0201
  Pop $R9
  Pop $R8
  Pop $9
  Pop $8
  Pop $7
  Pop $6
  Pop $5
  Pop $4
  Pop $3
  Pop $2
  Pop $1
  Pop $0
FunctionEnd
!macroend

!insertmacro AbigailPreviewFunctions ""
!insertmacro AbigailPreviewFunctions "un."

!macro NSIS_HOOK_PREINSTALL
  !insertmacro AbigailPreviewAssertIdentity
  Call AbigailPreviewGuard
!macroend
!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro AbigailPreviewAssertIdentity
  Call un.AbigailPreviewGuard
!macroend
!macro NSIS_HOOK_POSTINSTALL
!macroend
!macro NSIS_HOOK_POSTUNINSTALL
!macroend
!endif
