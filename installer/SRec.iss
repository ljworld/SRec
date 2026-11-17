; SRec installer (Inno Setup 6)
; Fully offline: everything needed is inside the setup file.
; Build:  ISCC.exe /DAppVersion=1.0.0-alpha.1 /DAppNumVersion=1.0.0 installer\SRec.iss
; Expects the files to package in ..\dist\app\

#ifndef AppVersion
  #define AppVersion "1.0.0-alpha.1"
#endif
; Windows file versions must be numeric (no "-alpha" suffix).
#ifndef AppNumVersion
  #define AppNumVersion "1.0.0"
#endif
#define AppName "SRec"
#define AppPublisher "The SRec Authors"
#define AppExe "srec.exe"

[Setup]
; Unique ID - keep it the same for every future version so upgrades replace the old install.
; (Unchanged from the "AV1 Island" days, so SRec replaces that install.)
AppId={{97B52097-0A61-424A-B7AC-EB58DA2514CD}
; Don't reuse the old "AV1 Island" folder / Start-menu group after the rename.
UsePreviousAppDir=no
UsePreviousGroup=no
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher={#AppPublisher}
AppCopyright=Copyright (c) 2026 {#AppPublisher}. MIT License.
VersionInfoVersion={#AppNumVersion}
VersionInfoDescription={#AppName} Setup
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
; Per-user install by default (no admin prompt); user may choose all-users.
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
; Windows 10 2004 or newer (needed for hiding the island from captures)
MinVersion=10.0.19041
; Agreement page: user must select "I accept" to continue.
LicenseFile=..\dist\app\TERMS-OF-USE.txt
OutputDir=..\dist
OutputBaseFilename=SRec-Setup-{#AppVersion}
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern
SetupIconFile=..\assets\srec.ico
UninstallDisplayName={#AppName}
UninstallDisplayIcon={app}\{#AppExe}
CloseApplications=yes
RestartApplications=no
SetupLogging=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "Create a &desktop shortcut"; GroupDescription: "Shortcuts:"; Flags: unchecked
Name: "autostart"; Description: "Start SRec when I sign in to Windows"; GroupDescription: "Startup:"; Flags: unchecked

[Files]
Source: "..\dist\app\{#AppExe}";                 DestDir: "{app}"; Flags: ignoreversion
Source: "..\dist\app\ffmpeg.exe";                DestDir: "{app}"; Flags: ignoreversion
Source: "..\dist\app\LICENSE";                   DestDir: "{app}"; DestName: "LICENSE.txt"
Source: "..\dist\app\TERMS-OF-USE.txt";          DestDir: "{app}"
Source: "..\dist\app\THIRD-PARTY-NOTICES.txt";   DestDir: "{app}"
Source: "..\dist\app\FFMPEG-LICENSE.txt";        DestDir: "{app}"
Source: "..\dist\app\FFMPEG-VERSION.txt";        DestDir: "{app}"
Source: "..\dist\app\RUST-LICENSES.txt";         DestDir: "{app}"
Source: "..\dist\app\USER_MANUAL.md";            DestDir: "{app}"; DestName: "User Manual.txt"
; Offline English subtitles (whisper.cpp + speech models).
Source: "..\dist\app\WHISPER-LICENSE.txt";       DestDir: "{app}"
Source: "..\dist\app\whisper\*";                 DestDir: "{app}\whisper"; Flags: ignoreversion

[InstallDelete]
; Leftovers of the previous name (AV1 Island, up to v0.3.x).
Type: filesandordirs; Name: "{autopf}\AV1 Island"
Type: filesandordirs; Name: "{autoprograms}\AV1 Island"
Type: files;          Name: "{autodesktop}\AV1 Island.lnk"
Type: files;          Name: "{app}\av1-island.exe"

[Icons]
Name: "{group}\{#AppName}";               Filename: "{app}\{#AppExe}"
Name: "{group}\User manual";              Filename: "{app}\User Manual.txt"
Name: "{group}\Licences and notices";     Filename: "{app}"
Name: "{group}\Uninstall {#AppName}";     Filename: "{uninstallexe}"
Name: "{autodesktop}\{#AppName}";         Filename: "{app}\{#AppExe}"; Tasks: desktopicon

[Registry]
; Remove the old autostart entry from before the rename.
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: none; \
  ValueName: "AV1 Island"; Flags: deletevalue
; Optional autostart (removed automatically on uninstall).
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; \
  ValueName: "SRec"; ValueData: """{app}\{#AppExe}"""; Flags: uninsdeletevalue; Tasks: autostart

[Run]
Filename: "{app}\{#AppExe}"; Description: "Launch {#AppName} now"; Flags: nowait postinstall skipifsilent

[UninstallRun]
; Close a running copy before files are removed.
Filename: "{sys}\taskkill.exe"; Parameters: "/IM {#AppExe} /F"; Flags: runhidden; RunOnceId: "StopApp"
Filename: "{sys}\taskkill.exe"; Parameters: "/IM whisper-cli.exe /F"; Flags: runhidden; RunOnceId: "StopWhisper"

[UninstallDelete]
; Settings and encoder cache. Recordings in Videos\SRec Recordings are NOT deleted.
Type: filesandordirs; Name: "{userappdata}\SRec"
Type: filesandordirs; Name: "{userappdata}\AV1Island"
Type: dirifempty;     Name: "{app}"

[Messages]
LicenseLabel3=Please read the Terms of Use. You must accept them to install {#AppName}. %n%nIn short: free MIT-licensed software, use it lawfully and with consent of people you record; provided as-is without warranty.

[Code]
// Close a running copy of the old AV1 Island app so its files can be removed.
function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  Code: Integer;
begin
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/IM av1-island.exe /F', '', SW_HIDE, ewWaitUntilTerminated, Code);
  // A subtitle job left running would lock files in {app}\whisper.
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/IM whisper-cli.exe /F', '', SW_HIDE, ewWaitUntilTerminated, Code);
  Result := '';
end;
