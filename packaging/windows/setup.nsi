; Fixture and local application builds share path and deletion safety checks.
!ifndef TOWAVUE_SETUP_FIXTURE
  !ifndef TOWAVUE_SETUP_APPLICATION
    !error "Select an explicit fixture or local application build."
  !endif
!else
  !ifdef TOWAVUE_SETUP_APPLICATION
    !error "Fixture and application modes are mutually exclusive."
  !endif
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

!ifdef TOWAVUE_SETUP_APPLICATION
!include "x64.nsh"
!include "${TRIAL_ROOT}\payload.nsh"
Name "towavue (local evaluation)"
OutFile "${TRIAL_ROOT}\Setup-local.exe"
InstallDir "$LOCALAPPDATA\Programs\towavue-evaluation"
BrandingText "Local evaluation - not a published release"
!define MARKER_FILE "towavue-install.ini"
!define MARKER_SECTION "installation"
!define UNINSTALL_FILE "Uninstall.exe"
!define MUI_WELCOMEPAGE_TITLE "towavue local evaluation Setup"
!define MUI_WELCOMEPAGE_TEXT "This installs towavue and its adjacent media runtime into an empty folder, with a Start menu shortcut and uninstall entry for the current user.$\r$\n$\r$\nIf required, the original Microsoft Visual C++ installer will ask you to review its terms. No automatic restart or application launch follows.$\r$\n$\r$\nThis evaluation cannot update an existing installation. Source archives are supplied separately; see the installed licenses guide."
; Never let the Finish page request a system restart.
!define MUI_FINISHPAGE_NOREBOOTSUPPORT
!else
Name "towavue Setup fixture (not the application)"
OutFile "${TRIAL_ROOT}\Setup-fixture.exe"
InstallDir "${TRIAL_ROOT}\manual-trial"
BrandingText "Local lifecycle test - no application or shared runtime"
!define MARKER_FILE "towavue-fixture.ini"
!define MARKER_SECTION "fixture"
!define OWNERSHIP_ID "towavue-nsis-lifecycle-v1"
!define UNINSTALL_FILE "Uninstall-fixture.exe"
!define MUI_WELCOMEPAGE_TITLE "towavue installer lifecycle test"
!define MUI_WELCOMEPAGE_TEXT "This installs harmless test documents only.$\r$\n$\r$\nIt does not install towavue, FFmpeg or the Visual C++ runtime, create shortcuts or change file associations.$\r$\n$\r$\nChoose an empty test folder. Existing installations cannot be updated by this fixture."
!endif
ShowInstDetails show
ShowUninstDetails show
!define MUI_ABORTWARNING
!insertmacro MUI_PAGE_WELCOME
!define MUI_PAGE_CUSTOMFUNCTION_LEAVE CheckDirectoryPage
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Var PathError
Var OperationHandle
!ifdef TOWAVUE_SETUP_APPLICATION
Var PrerequisiteResult
Var RegistrationMode
Var RegistrationResult
!endif

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
  ; NSIS strips the trailing separator when assigning a drive root to $INSTDIR.
  ; Reject C: and drive-relative paths before Windows resolves them against cwd.
  ${GetRoot} "$INSTDIR" $0
  StrCpy $1 $INSTDIR 3
  ${If} $0 == ""
  ${OrIf} $INSTDIR == $0
  ${OrIf} $1 != "$0\"
    StrCpy $PathError "Choose an absolute dedicated folder, not a drive root or relative path."
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

!macro OperationFunctions Prefix
Function ${Prefix}AcquireOperation
  InitPluginsDir
  ClearErrors
  SetOutPath "$PLUGINSDIR"
  File "${TRIAL_ROOT}\operation-lock.ps1"
  ${If} ${Errors}
    SetErrorLevel 4
    Abort "Could not prepare installation coordination. No application files were changed."
  ${EndIf}
  ; Helpers use Windows PowerShell, even when Setup was launched from PowerShell
  ; 7. Let that child initialize its own modules; only this process is changed.
  System::Call 'kernel32::SetEnvironmentVariableW(w "PSModulePath", p 0)'
!ifdef TOWAVUE_SETUP_FIXTURE
  nsExec::ExecToStack '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PLUGINSDIR\operation-lock.ps1" -EmitName -RegistrySubKey "towavue-setup-fixture-v1"'
!else
  nsExec::ExecToStack '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PLUGINSDIR\operation-lock.ps1" -EmitName'
!endif
  Pop $1
  Pop $0
  ${If} $1 != 0
  ${OrIf} $0 == ""
    SetErrorLevel 4
    Abort "Could not resolve installation coordination. No application files were changed."
  ${EndIf}
  ; Object existence is the lease. Do not acquire thread ownership: the section
  ; worker and GUI cleanup may run on different threads. Never inherit the handle.
  System::Call 'kernel32::CreateMutexW(p 0, i 0, w r0) p .r1 ?e'
  Pop $2
  ${If} $1 == 0
    SetErrorLevel 4
    Abort "Could not acquire installation coordination. No application files were changed."
  ${EndIf}
  ${If} $2 == 183
    System::Call 'kernel32::CloseHandle(p r1)'
    SetErrorLevel 4
    Abort "Another installation, update or removal is active. Close it and retry. No application files were changed."
  ${EndIf}
  StrCpy $OperationHandle $1
FunctionEnd

Function ${Prefix}ReleaseOperation
  ${If} $OperationHandle != ""
  ${AndIf} $OperationHandle != 0
    System::Call 'kernel32::CloseHandle(p $OperationHandle)'
    StrCpy $OperationHandle ""
  ${EndIf}
FunctionEnd

!macroend
!insertmacro OperationFunctions ""
!insertmacro OperationFunctions "un."

; Abort/cancel also release; silent process exit closes any remaining handle.
Function .onGUIEnd
  Call ReleaseOperation
FunctionEnd
Function un.onGUIEnd
  Call un.ReleaseOperation
FunctionEnd

Function CheckEmptyDirectory
  Call CheckPath
  ${If} $PathError != ""
    Return
  ${EndIf}
!ifdef TOWAVUE_SETUP_APPLICATION
  StrLen $0 $INSTDIR
  IntOp $0 $0 + ${PAYLOAD_MAX_PATH}
  ${If} $0 >= 260
    StrCpy $PathError "Choose a shorter folder path so all installed files fit Windows path limits."
    Return
  ${EndIf}
!endif
  FindFirst $0 $1 "$INSTDIR\*"
  loop_empty:
    ${If} $1 == ""
      Goto done_empty
    ${EndIf}
    ${If} $1 != "."
    ${AndIf} $1 != ".."
      StrCpy $PathError "Choose an empty folder. This Setup cannot overwrite or update an existing installation."
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
!ifdef TOWAVUE_SETUP_APPLICATION
  ${If} ${Silent}
    SetErrorLevel 2
    Quit
  ${EndIf}
  ${IfNot} ${IsNativeAMD64}
    MessageBox MB_OK|MB_ICONSTOP "This evaluation requires native x86-64 Windows."
    SetErrorLevel 2
    Quit
  ${EndIf}
  GetWinVer $0 Build
  ${If} $0 < 19045
    MessageBox MB_OK|MB_ICONSTOP "Windows 10 22H2 or later is required."
    SetErrorLevel 2
    Quit
  ${EndIf}
!endif
FunctionEnd

!ifdef TOWAVUE_SETUP_APPLICATION
!macro RegistrationFunction Prefix
Function ${Prefix}Registration
  InitPluginsDir
  ClearErrors
  SetOutPath "$PLUGINSDIR\registration"
  File "${TRIAL_ROOT}\registration\registration.ps1"
  File "${TRIAL_ROOT}\registration\registration-state.ps1"
  File "${TRIAL_ROOT}\registration\UnicodeShellLink.cs"
  ${If} ${Errors}
    StrCpy $RegistrationResult 20
    Return
  ${EndIf}
  nsExec::ExecToStack '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -STA -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PLUGINSDIR\registration\registration.ps1" -Mode $RegistrationMode -InstallDirectory "$INSTDIR" -OwnershipId "${OWNERSHIP_ID}" -SizeKiB ${PAYLOAD_SIZE_KIB}'
  Pop $RegistrationResult
  Pop $0
  DetailPrint "$0"
FunctionEnd
!macroend
!insertmacro RegistrationFunction ""
!insertmacro RegistrationFunction "un."

Function CheckPrerequisite
  InitPluginsDir
  ClearErrors
  SetOutPath "$PLUGINSDIR\scripts"
  File "${TRIAL_ROOT}\prerequisite\scripts\prerequisite.ps1"
  File "${TRIAL_ROOT}\prerequisite\scripts\get-vc-redist-status.ps1"
  File "${TRIAL_ROOT}\prerequisite\scripts\vc-redist-state.ps1"
  SetOutPath "$PLUGINSDIR\docs"
  File "${TRIAL_ROOT}\prerequisite\docs\vc-redist-inputs.json"
  SetOutPath "$PLUGINSDIR"
  File "${TRIAL_ROOT}\prerequisite\vc_redist.x64.exe"
  ${If} ${Errors}
    SetErrorLevel 3
    Abort "Could not extract the prerequisite checker. No application files were installed."
  ${EndIf}
  ; Absolute Windows PowerShell path; process policy only. The reader uses Registry64
  ; even under the 32-bit NSIS process. No host policy or shared runtime is removed.
  nsExec::ExecToStack '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PLUGINSDIR\scripts\prerequisite.ps1" -Mode Inspect -PackagePath "$PLUGINSDIR\vc_redist.x64.exe"'
  Pop $PrerequisiteResult
  Pop $0
  DetailPrint "$0"
  ${If} $PrerequisiteResult == 10
    MessageBox MB_OKCANCEL|MB_ICONINFORMATION "The Microsoft Visual C++ x64 runtime is required. Open its original installer to review the terms and choose whether to install? Cancelling stops towavue Setup without installing the application." IDOK install_prerequisite
    SetErrorLevel 2
    Abort "Prerequisite cancelled. No application files were installed."
    install_prerequisite:
    nsExec::ExecToStack '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PLUGINSDIR\scripts\prerequisite.ps1" -Mode Install -PackagePath "$PLUGINSDIR\vc_redist.x64.exe"'
    Pop $PrerequisiteResult
    Pop $0
    DetailPrint "$0"
  ${EndIf}
  ${If} $PrerequisiteResult == 3010
    DetailPrint "The prerequisite requests a restart. Restart manually before launching towavue. Setup will not restart Windows."
    MessageBox MB_OK|MB_ICONINFORMATION "The prerequisite requests a restart. Restart Windows manually before launching towavue. Setup will not restart Windows or launch the application."
  ${ElseIf} $PrerequisiteResult != 0
    SetErrorLevel 3
    Abort "Prerequisite did not pass (checker result $PrerequisiteResult). See the details. No application files were installed."
  ${EndIf}
FunctionEnd
!endif

Section "Files"
  ; Repeat after the page so silent mode and late directory changes cannot bypass it.
  Call CheckEmptyDirectory
  ${If} $PathError != ""
    DetailPrint "$PathError"
    SetErrorLevel 2
    Abort "$PathError"
  ${EndIf}
  Call AcquireOperation
!ifdef TOWAVUE_SETUP_APPLICATION
  StrCpy $RegistrationMode "Inspect"
  Call Registration
  ${If} $RegistrationResult != 0
    SetErrorLevel 3
    Abort "Application registration is unavailable. No application files were installed. See details."
  ${EndIf}
  Call CheckPrerequisite
  ; The user may spend time in the prerequisite UI. Recheck before target writes.
  Call CheckEmptyDirectory
  ${If} $PathError != ""
    SetErrorLevel 2
    Abort "$PathError"
  ${EndIf}
!endif
  ClearErrors
!ifdef TOWAVUE_SETUP_APPLICATION
  !insertmacro InstallApplicationFiles
!else
  SetOutPath "$INSTDIR"
  File "${TRIAL_ROOT}\payload\fixture.txt"
  SetOutPath "$INSTDIR\licenses"
  File "${TRIAL_ROOT}\payload\licenses\START-HERE.html"
  File "${TRIAL_ROOT}\payload\licenses\NSIS-COPYING.txt"
!endif
  WriteUninstaller "$INSTDIR\${UNINSTALL_FILE}"
  ${If} ${Errors}
    SetErrorLevel 3
    Abort "The installation is incomplete. No completion marker was written. Retain the folder for inspection."
  ${EndIf}
  ; After payload, before registration: a failed registration can still be uninstalled.
  ; Absence or mismatch prevents the uninstaller from deleting anything.
  ; A new Windows INI otherwise uses the ANSI code page and loses Japanese paths.
  FileOpen $0 "$INSTDIR\${MARKER_FILE}" w
  FileWriteUTF16LE /BOM $0 ""
  FileClose $0
  WriteINIStr "$INSTDIR\${MARKER_FILE}" "${MARKER_SECTION}" "id" "${OWNERSHIP_ID}"
  WriteINIStr "$INSTDIR\${MARKER_FILE}" "${MARKER_SECTION}" "directory" "$INSTDIR"
  ${If} ${Errors}
    SetErrorLevel 3
    Abort "Could not record the installation directory. Retain the folder for inspection."
  ${EndIf}
  SetErrorLevel 0
!ifdef TOWAVUE_SETUP_APPLICATION
  StrCpy $RegistrationMode "Install"
  Call Registration
  ${If} $RegistrationResult != 0
    SetErrorLevel 3
    Abort "Files were installed but registration failed. See details; retain the folder or use its Uninstall.exe."
  ${EndIf}
  ${If} $PrerequisiteResult == 3010
    SetErrorLevel 3010
  ${EndIf}
!endif
  Call ReleaseOperation
SectionEnd

Section "Uninstall"
  Call un.CheckPath
  ${If} $PathError != ""
    SetErrorLevel 2
    Abort "$PathError"
  ${EndIf}
  Call un.AcquireOperation
  ; Check every owned directory before any deletion.
  StrCpy $3 $INSTDIR
!ifdef TOWAVUE_SETUP_APPLICATION
  !insertmacro CheckApplicationDirectories
!else
  StrCpy $INSTDIR "$INSTDIR\licenses"
  Call un.CheckPath
!endif
  StrCpy $INSTDIR $3
  ReadINIStr $0 "$INSTDIR\${MARKER_FILE}" "${MARKER_SECTION}" "id"
  ReadINIStr $1 "$INSTDIR\${MARKER_FILE}" "${MARKER_SECTION}" "directory"
  ${If} $PathError != ""
  ${OrIf} $0 != "${OWNERSHIP_ID}"
  ${OrIf} $1 != $INSTDIR
    SetErrorLevel 2
    Abort "The ownership marker or directory is invalid. No files were removed."
  ${EndIf}
!ifdef TOWAVUE_SETUP_APPLICATION
  StrCpy $RegistrationMode "VerifyRemoval"
  Call un.Registration
  ${If} $RegistrationResult != 0
    SetErrorLevel 3
    Abort "Registration ownership could not be verified. No application files were removed. See details."
  ${EndIf}
!endif
  ClearErrors
!ifdef TOWAVUE_SETUP_APPLICATION
  !insertmacro RemoveApplicationFiles
!else
  Delete "$INSTDIR\fixture.txt"
  Delete "$INSTDIR\licenses\START-HERE.html"
  Delete "$INSTDIR\licenses\NSIS-COPYING.txt"
!endif
  ${If} ${Errors}
    SetErrorLevel 3
    Abort "Some files could not be removed. Close users of those files and retry; no reboot is scheduled."
  ${EndIf}
!ifdef TOWAVUE_SETUP_APPLICATION
  StrCpy $RegistrationMode "Remove"
  Call un.Registration
  ${If} $RegistrationResult != 0
    SetErrorLevel 3
    Abort "Some application registration could not be removed. Retain the uninstaller and marker to retry; see details."
  ${EndIf}
!endif
  Delete "$INSTDIR\${UNINSTALL_FILE}"
  ${If} ${Errors}
    SetErrorLevel 3
    Abort "The uninstaller could not be removed. Retain the ownership marker and retry."
  ${EndIf}
  Delete "$INSTDIR\${MARKER_FILE}"
  ${If} ${Errors}
    SetErrorLevel 3
    Abort "The files were removed, but the ownership record could not be deleted. No reboot is scheduled."
  ${EndIf}
  ; Empty-only removal preserves extra user files, even below licenses.
  SetOutPath "$TEMP"
!ifdef TOWAVUE_SETUP_APPLICATION
  !insertmacro RemoveApplicationDirectories
!else
  RMDir "$INSTDIR\licenses"
!endif
  RMDir "$INSTDIR"
  SetErrorLevel 0
  Call un.ReleaseOperation
SectionEnd
