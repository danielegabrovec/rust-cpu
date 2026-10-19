Unicode True
!include "MUI2.nsh"
!include "x64.nsh"
!ifndef VERSION
  !define VERSION "0.2.0"
!endif
Name "RUST-CPU"
OutFile "..\artifacts\rust-cpu-${VERSION}-windows-x64-setup.exe"
InstallDir "$LOCALAPPDATA\Programs\RUST-CPU"
InstallDirRegKey HKCU "Software\RUST-CPU" "InstallDir"
RequestExecutionLevel user
SetCompressor /SOLID lzma
BrandingText "RUST-CPU | Hardware & Local AI Workbench"
VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "RUST-CPU"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "FileDescription" "RUST-CPU installer per Windows x64"
VIAddVersionKey "LegalCopyright" "2026 Daniele Gabrovec - MIT"
!define MUI_ABORTWARNING
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_LICENSE "..\LICENSE"
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "Italian"
!insertmacro MUI_LANGUAGE "English"

Function .onInit
  ${IfNot} ${RunningX64}
    MessageBox MB_ICONSTOP "RUST-CPU richiede Windows x64."
    Abort
  ${EndIf}
FunctionEnd

Section "RUST-CPU" Main
  SetOutPath "$INSTDIR"
  File "..\target\release\rust-cpu.exe"
  File "..\LICENSE"
  File "..\README.md"
  File "..\CHANGELOG.md"
  File "..\THIRD-PARTY-NOTICES.txt"
  SetOutPath "$INSTDIR\docs"
  File "..\docs\*.md"
  SetOutPath "$INSTDIR\docs\screenshots"
  File "..\docs\screenshots\*.png"
  WriteUninstaller "$INSTDIR\Uninstall.exe"
  CreateDirectory "$SMPROGRAMS\RUST-CPU"
  CreateShortcut "$SMPROGRAMS\RUST-CPU\RUST-CPU.lnk" "$INSTDIR\rust-cpu.exe"
  CreateShortcut "$SMPROGRAMS\RUST-CPU\Documentazione.lnk" "$INSTDIR\README.md"
  CreateShortcut "$SMPROGRAMS\RUST-CPU\Disinstalla.lnk" "$INSTDIR\Uninstall.exe"
  WriteRegStr HKCU "Software\RUST-CPU" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\RUST-CPU" "DisplayName" "RUST-CPU"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\RUST-CPU" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\RUST-CPU" "Publisher" "Daniele Gabrovec"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\RUST-CPU" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\RUST-CPU" "QuietUninstallString" '"$INSTDIR\Uninstall.exe" /S'
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\RUST-CPU" "InstallLocation" "$INSTDIR"
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\RUST-CPU" "NoModify" 1
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\RUST-CPU" "NoRepair" 1
SectionEnd

Section "Uninstall"
  Delete "$INSTDIR\rust-cpu.exe"
  Delete "$INSTDIR\LICENSE"
  Delete "$INSTDIR\README.md"
  Delete "$INSTDIR\CHANGELOG.md"
  Delete "$INSTDIR\THIRD-PARTY-NOTICES.txt"
  Delete "$INSTDIR\docs\ARCHITETTURA.md"
  Delete "$INSTDIR\docs\BUILD.md"
  Delete "$INSTDIR\docs\FONTI.md"
  Delete "$INSTDIR\docs\MANUALE.md"
  Delete "$INSTDIR\docs\PROGETTO.md"
  Delete "$INSTDIR\docs\VERIFICHE.md"
  Delete "$INSTDIR\docs\HARDWARE.md"
  Delete "$INSTDIR\docs\OBSERVER.md"
  Delete "$INSTDIR\docs\SESSIONI.md"
  Delete "$INSTDIR\docs\screenshots\overview.png"
  Delete "$INSTDIR\docs\screenshots\processi.png"
  Delete "$INSTDIR\docs\screenshots\llm.png"
  Delete "$INSTDIR\docs\screenshots\capacity.png"
  Delete "$INSTDIR\docs\screenshots\sessioni.png"
  Delete "$INSTDIR\docs\screenshots\confronto.png"
  Delete "$INSTDIR\docs\screenshots\impostazioni.png"
  RMDir "$INSTDIR\docs\screenshots"
  RMDir "$INSTDIR\docs"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\RUST-CPU\RUST-CPU.lnk"
  Delete "$SMPROGRAMS\RUST-CPU\Documentazione.lnk"
  Delete "$SMPROGRAMS\RUST-CPU\Disinstalla.lnk"
  RMDir "$SMPROGRAMS\RUST-CPU"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\RUST-CPU"
  DeleteRegKey HKCU "Software\RUST-CPU"
SectionEnd
