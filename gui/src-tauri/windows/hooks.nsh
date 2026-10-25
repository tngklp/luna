; Luna's NSIS installer hooks.
;
; The bundle ships the `luna` TUI binary as a sidecar. Installing the GUI should
; also make `luna` available in a terminal, so this adds the sidecar directory to
; the user's PATH and removes it again on uninstall.
;
; Only core NSIS string operations are used: Tauri's bundled NSIS does not ship
; the StrContains/StrRep helpers.

!macro NSIS_HOOK_POSTINSTALL
  ; $INSTDIR\bin holds the bundled TUI binary.
  ReadRegStr $0 HKCU "Environment" "Path"
  ${If} $0 == ""
    WriteRegExpandStr HKCU "Environment" "Path" "$INSTDIR\bin"
  ${Else}
    ; Search for the directory inside the existing PATH before appending.
    StrCpy $4 "$0"
    StrCpy $5 "$INSTDIR\bin"
    StrCpy $6 0
    StrLen $7 $5
    StrLen $9 $4
    ${Do}
      StrCpy $8 $4 $7 $6
      ${If} $8 == $5
        StrCpy $6 -1
        ${ExitDo}
      ${EndIf}
      IntOp $6 $6 + 1
      ${If} $6 >= $9
        ${ExitDo}
      ${EndIf}
    ${Loop}
    ${If} $6 != -1
      WriteRegExpandStr HKCU "Environment" "Path" "$0;$INSTDIR\bin"
    ${EndIf}
  ${EndIf}
  ; Tell running shells about the change without requiring a reboot.
  SendMessage ${HWND_BROADCAST} ${WM_WININICHANGE} 0 "STR:Environment" /TIMEOUT=5000
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ReadRegStr $0 HKCU "Environment" "Path"
  ${If} $0 != ""
    ; Rebuild PATH without the entry we added.
    StrCpy $1 ""
    StrCpy $2 0
    StrLen $9 $0
    ${Do}
      StrCpy $3 $0 "" $2
      StrCpy $4 $3 1
      ${If} $4 == ";"
        IntOp $2 $2 + 1
        ${Continue}
      ${EndIf}
      ; Measure the segment up to the next separator.
      StrCpy $6 0
      ${Do}
        StrCpy $7 $3 1 $6
        ${If} $7 == ";"
          ${ExitDo}
        ${EndIf}
        ${If} $7 == ""
          ${ExitDo}
        ${EndIf}
        IntOp $6 $6 + 1
      ${Loop}
      StrCpy $8 $3 $6
      IntOp $2 $2 + $6
      ${If} $8 != "$INSTDIR\bin"
        ${If} $1 == ""
          StrCpy $1 "$8"
        ${Else}
          StrCpy $1 "$1;$8"
        ${EndIf}
      ${EndIf}
      ${If} $2 >= $9
        ${ExitDo}
      ${EndIf}
    ${Loop}
    WriteRegExpandStr HKCU "Environment" "Path" "$1"
    SendMessage ${HWND_BROADCAST} ${WM_WININICHANGE} 0 "STR:Environment" /TIMEOUT=5000
  ${EndIf}
!macroend
