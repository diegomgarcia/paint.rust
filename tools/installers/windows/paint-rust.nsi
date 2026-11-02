; Paint.Rust Windows x86_64 development-preview installer.
;
; build.ps1 supplies these required defines from the command line:
;   VERSION      product version (for example 0.1.1 or 0.1.1-preview.1)
;   VI_VERSION   four-part numeric version for VIProductVersion (for example 0.1.1.0)
;   BINARY       absolute path to the x86_64 paint-ui.exe
;   OUTFILE      absolute path to the setup executable
;   INSTALL_TEXT absolute path to this lane's INSTALL-WINDOWS.txt
;   LICENSE_FILE optional absolute path to the repository MIT LICENSE
;
; This installer is intentionally unsigned. It installs per-machine, installs no
; third-party or proprietary payload assets, and preserves user documents and
; settings.

Unicode true

!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "x64.nsh"

!ifndef VERSION
  !error "VERSION must be defined (pass /DVERSION=<version>)."
!endif
!ifndef VI_VERSION
  !error "VI_VERSION must be defined (pass /DVI_VERSION=<a.b.c.d>)."
!endif
!ifndef BINARY
  !error "BINARY must be defined (pass /DBINARY=<path to paint-ui.exe>)."
!endif
!ifndef OUTFILE
  !error "OUTFILE must be defined (pass /DOUTFILE=<path to setup.exe>)."
!endif
!ifndef INSTALL_TEXT
  !error "INSTALL_TEXT must be defined (pass /DINSTALL_TEXT=<path to INSTALL-WINDOWS.txt>)."
!endif

!define APP_NAME "Paint.Rust"
!define APP_PUBLISHER "Diego Garcia"
!define APP_URL "https://github.com/diegomgarcia/paint.rust"
!define APP_EXE "paint-ui.exe"
!define INSTALL_TEXT_FILE "INSTALL-WINDOWS.txt"
!define APP_REG_KEY "Software\Paint.Rust"
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\Paint.Rust"

Name "${APP_NAME}"
OutFile "${OUTFILE}"
InstallDir "$PROGRAMFILES64\Paint.Rust"
InstallDirRegKey HKLM "${APP_REG_KEY}" "InstallDir"
RequestExecutionLevel admin
BrandingText "${APP_NAME} ${VERSION} - unsigned development preview"
SetCompressor /SOLID lzma

VIProductVersion "${VI_VERSION}"
VIAddVersionKey "ProductName" "${APP_NAME}"
VIAddVersionKey "CompanyName" "${APP_PUBLISHER}"
VIAddVersionKey "FileDescription" "${APP_NAME} development-preview installer (unsigned)"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "LegalCopyright" "Paint.Rust contributors - MIT License"
VIAddVersionKey "Comments" "Unsigned development preview; no code-signing identity is used."

!define MUI_ABORTWARNING
!define MUI_WELCOMEPAGE_TITLE "${APP_NAME} ${VERSION} installer"
!define MUI_WELCOMEPAGE_TEXT "This installs the ${APP_NAME} development preview for 64-bit Windows.$\r$\n$\r$\nThe application and this installer are unsigned. Windows SmartScreen or your organization's policy may warn or block them; no code-signing certificate is used.$\r$\n$\r$\nInstallation is per-machine and requires administrator approval."
!define MUI_FINISHPAGE_RUN "$INSTDIR\${APP_EXE}"
!define MUI_FINISHPAGE_RUN_TEXT "Launch ${APP_NAME}"
!define MUI_FINISHPAGE_SHOWREADME "$INSTDIR\${INSTALL_TEXT_FILE}"
!define MUI_FINISHPAGE_SHOWREADME_TEXT "Show install notes"

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_UNPAGE_FINISH

!insertmacro MUI_LANGUAGE "English"

LangString DESC_SECTION_APP ${LANG_ENGLISH} "Paint.Rust application files, install notes, MIT license and Start Menu shortcuts."
LangString DESC_SECTION_DESKTOP ${LANG_ENGLISH} "Create a Paint.Rust shortcut on the desktop (optional)."

Function .onInit
  ${IfNot} ${RunningX64}
    MessageBox MB_ICONSTOP|MB_OK "${APP_NAME} requires 64-bit Windows (x86_64)."
    Abort
  ${EndIf}
FunctionEnd

Section "Paint.Rust (required)" SEC_APP
  SectionIn RO
  SetShellVarContext all

  SetOutPath "$INSTDIR"
  File /oname=${APP_EXE} "${BINARY}"
  File /oname=${INSTALL_TEXT_FILE} "${INSTALL_TEXT}"
!ifdef LICENSE_FILE
  File /oname=LICENSE "${LICENSE_FILE}"
!endif
  WriteUninstaller "$INSTDIR\uninstall.exe"

  CreateDirectory "$SMPROGRAMS\Paint.Rust"
  CreateShortCut "$SMPROGRAMS\Paint.Rust\Paint.Rust.lnk" "$INSTDIR\${APP_EXE}"
  CreateShortCut "$SMPROGRAMS\Paint.Rust\Uninstall Paint.Rust.lnk" "$INSTDIR\uninstall.exe"

  WriteRegStr HKLM "${APP_REG_KEY}" "InstallDir" "$INSTDIR"
  WriteRegStr HKLM "${APP_REG_KEY}" "Version" "${VERSION}"
  WriteRegStr HKLM "${APP_REG_KEY}" "ProductURL" "${APP_URL}"

  WriteRegStr HKLM "${UNINSTALL_KEY}" "DisplayName" "${APP_NAME}"
  WriteRegStr HKLM "${UNINSTALL_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKLM "${UNINSTALL_KEY}" "Publisher" "${APP_PUBLISHER}"
  WriteRegStr HKLM "${UNINSTALL_KEY}" "ProductURL" "${APP_URL}"
  WriteRegStr HKLM "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKLM "${UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\${APP_EXE},0"
  WriteRegStr HKLM "${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKLM "${UNINSTALL_KEY}" "QuietUninstallString" '"$INSTDIR\uninstall.exe" /S'
  WriteRegDWORD HKLM "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKLM "${UNINSTALL_KEY}" "NoRepair" 1
SectionEnd

Section /o "Desktop shortcut" SEC_DESKTOP
  SetShellVarContext all
  CreateShortCut "$DESKTOP\Paint.Rust.lnk" "$INSTDIR\${APP_EXE}"
SectionEnd

Section "Uninstall"
  SetShellVarContext all

  Delete "$SMPROGRAMS\Paint.Rust\Paint.Rust.lnk"
  Delete "$SMPROGRAMS\Paint.Rust\Uninstall Paint.Rust.lnk"
  RMDir "$SMPROGRAMS\Paint.Rust"
  Delete "$DESKTOP\Paint.Rust.lnk"

  ; Remove only files created by this installer. User documents and per-user
  ; settings are never touched.
  Delete "$INSTDIR\${APP_EXE}"
  Delete "$INSTDIR\${INSTALL_TEXT_FILE}"
  Delete "$INSTDIR\LICENSE"
  Delete "$INSTDIR\uninstall.exe"

  ; Non-recursive: keeps the directory if the user placed files in it.
  RMDir "$INSTDIR"

  DeleteRegKey HKLM "${UNINSTALL_KEY}"
  DeleteRegKey HKLM "${APP_REG_KEY}"
SectionEnd

!insertmacro MUI_FUNCTION_DESCRIPTION_BEGIN
  !insertmacro MUI_DESCRIPTION_TEXT ${SEC_APP} $(DESC_SECTION_APP)
  !insertmacro MUI_DESCRIPTION_TEXT ${SEC_DESKTOP} $(DESC_SECTION_DESKTOP)
!insertmacro MUI_FUNCTION_DESCRIPTION_END
