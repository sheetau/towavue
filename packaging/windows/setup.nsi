; This first lifecycle slice is deliberately restricted to harmless fixture data.
!ifndef TOWAVUE_SETUP_FIXTURE
  !error "Only the local fixture installer is implemented."
!endif
!ifndef TRIAL_ROOT
  !error "A compiler-generated trial root is required."
!endif
Unicode true
RequestExecutionLevel user
SetCompressor /FINAL zlib
SetOverwrite off
!include "MUI2.nsh"
!include "FileFunc.nsh"
!include "LogicLib.nsh"

Name "towavue Setup fixture (not the application)"
OutFile "${TRIAL_ROOT}\Setup-fixture.exe"
InstallDir "${TRIAL_ROOT}\manual-trial"
BrandingText "Local lifecycle test - no application or shared runtime"
ShowInstDetails show
ShowUninstDetails show
!define MUI_ABORTWARNING
!define MUI_WELCOMEPAGE_TITLE "towavue installer lifecycle test"
!define MUI_WELCOMEPAGE_TEXT "This installs harmless test documents only.$\r$\n$\r$\nIt does not install towavue, FFmpeg or the Visual C++ runtime, create shortcuts or change file associations.$\r$\n$\r$\nChoose an empty test folder. Existing installations cannot be updated by this fixture."
!insertmacro MUI_PAGE_WELCOME
!define MUI_PAGE_CUSTOMFUNCTION_LEAVE CheckDirectoryPage
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Var PathError

; Both paths use the same checks. Inspect all existing ancestors, not only the leaf.
; This is a safety guard, not protection against concurrent hostile filesystem changes.
!macro CheckPath Prefix
Function ${Prefix}CheckPath
  StrCpy $PathError ""
  StrCpy $0 $INSTDIR 2
  ${If} $0 == "\\"
    StrCpy $PathError "Choose a local drive, not a network or device path."
    Return
  ${EndIf}
  ; NSIS GetFullPathName can return empty for a not-yet-created directory.
  StrCpy $0 $INSTDIR
  System::Call 'kernel32::GetFullPathNameW(w r0, i ${NSIS_MAX_STRLEN}, w .r1, p 0) i .r2'
  ${If} $2 <= 0
  ${OrIf} $2 >= ${NSIS_MAX_STRLEN}
    StrCpy $PathError "The installation path cannot be resolved."
    Return
  ${EndIf}
  StrCpy $INSTDIR $1
  ${GetRoot} "$INSTDIR" $0
  ${If} $INSTDIR == $0
  ${OrIf} $INSTDIR == "$0\"
  ${OrIf} $0 == ""
    StrCpy $PathError "Choose a dedicated folder, not a drive root."
    Return
  ${EndIf}
  StrCpy $0 $INSTDIR 1 -1
  ${If} $0 == "\"
    StrCpy $INSTDIR $INSTDIR -1
  ${EndIf}
  StrCpy $0 $INSTDIR
  loop_${Prefix}:
    System::Call 'kernel32::GetFileAttributesW(w r0) i .r1'
    ${If} $1 != -1
      IntOp $2 $1 & 0x400
      ${If} $2 != 0
        StrCpy $PathError "The installation path must not contain a reparse point."
        Return
      ${EndIf}
      IntOp $2 $1 & 0x10
      ${If} $2 == 0
        StrCpy $PathError "The installation path contains a file instead of a folder."
        Return
      ${EndIf}
    ${EndIf}
    ${GetParent} "$0" $1
    ${If} $1 == ""
    ${OrIf} $1 == $0
      Return
    ${EndIf}
    StrCpy $0 $1
    Goto loop_${Prefix}
FunctionEnd
!macroend
!insertmacro CheckPath ""
!insertmacro CheckPath "un."

Function CheckEmptyDirectory
  Call CheckPath
  ${If} $PathError != ""
    Return
  ${EndIf}
  FindFirst $0 $1 "$INSTDIR\*"
  loop_empty:
    ${If} $1 == ""
      Goto done_empty
    ${EndIf}
    ${If} $1 != "."
    ${AndIf} $1 != ".."
      StrCpy $PathError "Choose an empty folder. This fixture cannot overwrite or update an existing installation."
      Goto done_empty
    ${EndIf}
    FindNext $0 $1
    Goto loop_empty
  done_empty:
  FindClose $0
FunctionEnd

Function CheckDirectoryPage
  Call CheckEmptyDirectory
  ${If} $PathError != ""
    MessageBox MB_OK|MB_ICONEXCLAMATION "$PathError"
    Abort
  ${EndIf}
FunctionEnd

Function .onInit
  ${GetParameters} $0
  ClearErrors
  ${GetOptions} $0 "/CHECKONLY" $1
  ${IfNot} ${Errors}
    ; Read-only probe for paths that must never be used by a destructive test.
    ; NSIS may replace an invalid /D path with InstallDir before .onInit.
    ${GetOptions} $0 "/CHECKPATH=" $INSTDIR
    ${If} ${Errors}
      SetErrorLevel 2
      Quit
    ${EndIf}
    Call CheckEmptyDirectory
    ${If} $PathError == ""
      SetErrorLevel 0
    ${Else}
      SetErrorLevel 2
    ${EndIf}
    Quit
  ${EndIf}
  ; Preserve an explicit destination even when NSIS's initial validation rejects it.
  ; Otherwise silent installation can unexpectedly use the default directory.
  System::Call 'kernel32::GetCommandLineW() w .r0'
  ClearErrors
  ${GetOptions} $0 "/D=" $1
  ${IfNot} ${Errors}
    StrCpy $INSTDIR $1
  ${EndIf}
FunctionEnd

Section "Fixture documents"
  ; Repeat after the page so silent mode and late directory changes cannot bypass it.
  Call CheckEmptyDirectory
  ${If} $PathError != ""
    DetailPrint "$PathError"
    SetErrorLevel 2
    Abort "$PathError"
  ${EndIf}
  ClearErrors
  SetOutPath "$INSTDIR"
  File "${TRIAL_ROOT}\payload\fixture.txt"
  SetOutPath "$INSTDIR\licenses"
  File "${TRIAL_ROOT}\payload\licenses\START-HERE.html"
  File "${TRIAL_ROOT}\payload\licenses\NSIS-COPYING.txt"
  WriteUninstaller "$INSTDIR\Uninstall-fixture.exe"
  ${If} ${Errors}
    SetErrorLevel 3
    Abort "The fixture is incomplete. No completion marker was written. Retain the folder for inspection."
  ${EndIf}
  ; Last: absence or mismatch prevents the uninstaller from deleting anything.
  ; A new Windows INI otherwise uses the ANSI code page and loses Japanese paths.
  FileOpen $0 "$INSTDIR\towavue-fixture.ini" w
  FileWriteUTF16LE /BOM $0 ""
  FileClose $0
  WriteINIStr "$INSTDIR\towavue-fixture.ini" "fixture" "id" "towavue-nsis-lifecycle-v1"
  WriteINIStr "$INSTDIR\towavue-fixture.ini" "fixture" "directory" "$INSTDIR"
  ${If} ${Errors}
    SetErrorLevel 3
    Abort "Could not record the fixture installation directory. Retain the folder for inspection."
  ${EndIf}
  SetErrorLevel 0
SectionEnd

Section "Uninstall"
  Call un.CheckPath
  ${If} $PathError != ""
    SetErrorLevel 2
    Abort "$PathError"
  ${EndIf}
  ; Check the one nested owned directory too, before any deletion.
  StrCpy $3 $INSTDIR
  StrCpy $INSTDIR "$INSTDIR\licenses"
  Call un.CheckPath
  StrCpy $INSTDIR $3
  ReadINIStr $0 "$INSTDIR\towavue-fixture.ini" "fixture" "id"
  ReadINIStr $1 "$INSTDIR\towavue-fixture.ini" "fixture" "directory"
  ${If} $PathError != ""
  ${OrIf} $0 != "towavue-nsis-lifecycle-v1"
  ${OrIf} $1 != $INSTDIR
    SetErrorLevel 2
    Abort "The fixture ownership marker or directory is invalid. No files were removed."
  ${EndIf}
  ClearErrors
  Delete "$INSTDIR\fixture.txt"
  Delete "$INSTDIR\licenses\START-HERE.html"
  Delete "$INSTDIR\licenses\NSIS-COPYING.txt"
  ${If} ${Errors}
    SetErrorLevel 3
    Abort "Some fixture files could not be removed. Close users of those files and retry; no reboot is scheduled."
  ${EndIf}
  Delete "$INSTDIR\Uninstall-fixture.exe"
  ${If} ${Errors}
    SetErrorLevel 3
    Abort "The fixture uninstaller could not be removed. Retain the ownership marker and retry."
  ${EndIf}
  Delete "$INSTDIR\towavue-fixture.ini"
  ${If} ${Errors}
    SetErrorLevel 3
    Abort "The fixture files were removed, but the ownership record could not be deleted. No reboot is scheduled."
  ${EndIf}
  ; Empty-only removal preserves extra user files, even below licenses.
  SetOutPath "$TEMP"
  RMDir "$INSTDIR\licenses"
  RMDir "$INSTDIR"
  SetErrorLevel 0
SectionEnd
