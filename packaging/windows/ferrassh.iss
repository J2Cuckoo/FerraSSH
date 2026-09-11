#define AppName "FerraSSH"
#define AppVersion "0.1.0"
#define AppPublisher "贵州力贤网络科技有限公司"
#define AppExe "FerraSSH.exe"

[Setup]
AppId={{8F3E2C1A-9B74-4D5E-A6C1-7B2E9D4A8F01}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher={#AppPublisher}
AppCopyright=Copyright (C) 2026 {#AppPublisher}
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
OutputDir=..\..\dist\windows
OutputBaseFilename={#AppName}-{#AppVersion}-x64-Setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
PrivilegesRequired=admin
PrivilegesRequiredOverridesAllowed=dialog
AllowNoIcons=yes
UninstallDisplayIcon={app}\{#AppExe}
SetupLogging=yes
CloseApplications=yes
RestartApplications=no
UsedUserAreasWarning=no

[Languages]
Name: "chinesesimp"; MessagesFile: "ChineseSimplified.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: checkedonce

[Files]
Source: "{#AppExe}"; DestDir: "{app}"; Flags: ignoreversion
Source: "fonts\*"; DestDir: "{app}\fonts"; Flags: ignoreversion skipifsourcedoesntexist recursesubdirs createallsubdirs

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#AppExe}"; Description: "{cm:LaunchProgram,{#AppName}}"; Flags: nowait postinstall skipifsilent

[Code]
function HasCjkFont: Boolean;
var
  WinDir: String;
begin
  WinDir := GetEnv('WINDIR');
  Result := FileExists(WinDir + '\Fonts\msyh.ttc') or
            FileExists(WinDir + '\Fonts\simhei.ttf') or
            FileExists(WinDir + '\Fonts\simsun.ttc') or
            DirExists(ExpandConstant('{app}\fonts'));
end;
