; installer.nsi
; A basic NSIS installer script with a custom icon.

;--------------------------------
; 1. General Installer Attributes
;--------------------------------

; The name of the installer's output file.
OutFile "WinDusky_x64-setup.exe"

; The default installation directory.
;InstallDir "PROGRAMFILES\WinDusky"
InstallDir "$LOCALAPPDATA\WinDusky"

; Request administrator privileges for the installer.
RequestExecutionLevel user


;--------------------------------
; 2. Modern UI (MUI2) Configuration
;--------------------------------

; Include the Modern UI 2 header file.
!include "MUI2.nsh"

; --- Set the base product name for MUI (uses the 'Name' attribute by default) ---
!define MUI_PRODUCT "WinDusky"

; --- Installer Icon ---
; This icon will be embedded into the installer .exe itself.
!define MUI_ICON "WinDusky.ico"

; --- Uninstaller Icon ---
; This icon will be used for the uninstaller.
!define MUI_UNICON "WinDusky.ico"

; --- WELCOME PAGE CUSTOMIZATION ---
; This is the large title text on the page.
!define MUI_WELCOMEPAGE_TITLE "Welcome to ${MUI_PRODUCT} Setup"

; This is the main body text. Use \r\n for line breaks.
!define MUI_WELCOMEPAGE_TEXT "This will guide you through the installation of ${MUI_PRODUCT}.$\r$\n$\r$\n$\r$\nClick Next to continue."

; Define which pages to show in the installer.
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

; Define which pages to show in the uninstaller.
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

; Set the UI language.
!insertmacro MUI_LANGUAGE "English"


;--------------------------------
; 3. Installer Sections (The actual work)
;--------------------------------

; A section defines a group of files and instructions.
Section "WinDusky" SecMyApp

  ; Set the output path to the installation directory.
  SetOutPath $INSTDIR

  ; Add the application files to the installer.
  ; The files will be extracted to the directory set by SetOutPath.
  File "target\release\WinDusky.exe"
  ; Use a wildcard to add all files from a directory.
  ; File "dist\*"

  ; --- Create Shortcuts ---
  ; Create a shortcut in the Start Menu.
  ;CreateDirectory "$SMPROGRAMS\"
  CreateShortCut "$SMPROGRAMS\WinDusky.lnk" "$INSTDIR\WinDusky.exe"

  ; --- Write Uninstaller Information to the Registry ---
  ; This is crucial for the "Add/Remove Programs" feature in Windows.
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\WinDusky" "DisplayName" "WinDusky"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\WinDusky" "DisplayIcon" '"$INSTDIR\WinDusky.exe",0'
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\WinDusky" "UninstallString" '"$INSTDIR\uninstall_WinDusky.exe"'
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\WinDusky" "NoModify" 1
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\WinDusky" "NoRepair" 1

  ; Write the uninstaller executable to the installation directory.
  WriteUninstaller "$INSTDIR\uninstall_WinDusky.exe"

SectionEnd


;--------------------------------
; 4. Uninstaller Section
;--------------------------------

Section "Uninstall"

  ; Remove files
  Delete "$INSTDIR\WinDusky.exe"
  Delete "$INSTDIR\uninstall_WinDusky.exe"
  Delete "$SMPROGRAMS\WinDusky.lnk"

  ; Remove directories
  ;RMDir "$INSTDIR"
  ;^^ rather not, as it might contain user-updated config .. which we'd rather preserve across uninstall/re-install cycles

  ; Remove registry keys
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\WinDusky"

SectionEnd
