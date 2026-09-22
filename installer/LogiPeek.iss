#define AppVersion GetEnv("LOGIPEEK_VERSION")
#if AppVersion == ""
  #error Set LOGIPEEK_VERSION before compiling
#endif

[Setup]
AppId={{E2C4B70D-9D10-4C8E-A507-67E7852F69F4}
AppName=LogiPeek
AppVersion={#AppVersion}
AppPublisher=LogiPeek Contributors
AppPublisherURL=https://github.com/1TcC/LogiPeek
DefaultDirName={localappdata}\Programs\LogiPeek
DefaultGroupName=LogiPeek
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
SetupArchitecture=x64
ArchitecturesAllowed=x64os
ArchitecturesInstallIn64BitMode=x64os
CloseApplications=yes
RestartApplications=no
SetupIconFile=..\assets\logipeek.ico
UninstallDisplayIcon={app}\logipeek.ico
OutputDir=..\dist
OutputBaseFilename=LogiPeek-{#AppVersion}-x64-Setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern

[Languages]
Name: "en"; MessagesFile: "compiler:Default.isl"
Name: "zhcn"; MessagesFile: "compiler:Languages\ChineseSimplified.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "..\target\release\logipeek.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\assets\logipeek.ico"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\LogiPeek"; Filename: "{app}\logipeek.exe"; IconFilename: "{app}\logipeek.ico"
Name: "{autodesktop}\LogiPeek"; Filename: "{app}\logipeek.exe"; IconFilename: "{app}\logipeek.ico"; Tasks: desktopicon

[Run]
Filename: "{app}\logipeek.exe"; Description: "{cm:LaunchProgram,LogiPeek}"; Flags: nowait postinstall skipifsilent

[Code]
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usPostUninstall then
    RegDeleteValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Run', 'LogiPeek');
end;
