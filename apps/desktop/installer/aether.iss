; Inno Setup script for the Aether desktop application (Windows x64).
; Built by .github/workflows/clients.yml:
;   iscc /DStageDir=<folder with aether-desktop.exe> /DAppVersion=<version> /DOutputDir=<folder> aether.iss
; Installs for the current user by default (no administrator rights); the
; installer offers "all users" through the usual Inno Setup dialog.

#ifndef StageDir
  #error StageDir is required
#endif
#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef OutputDir
  #define OutputDir "."
#endif

[Setup]
AppId={{6F1C2B7E-8D3A-4B59-9C1E-A37E00000001}
AppName=Aether
AppVersion={#AppVersion}
AppPublisher=Aether
AppPublisherURL=https://github.com/maelmoreau21/Aether
DefaultDirName={autopf}\Aether
DefaultGroupName=Aether
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
SetupIconFile=..\assets\aether.ico
UninstallDisplayIcon={app}\aether-desktop.exe
UninstallDisplayName=Aether
OutputDir={#OutputDir}
OutputBaseFilename=Aether-Setup-windows-x64
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes

[Languages]
Name: "french"; MessagesFile: "compiler:Languages\French.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[Files]
Source: "{#StageDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{autoprograms}\Aether"; Filename: "{app}\aether-desktop.exe"
Name: "{autodesktop}\Aether"; Filename: "{app}\aether-desktop.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\aether-desktop.exe"; Description: "{cm:LaunchProgram,Aether}"; Flags: nowait postinstall skipifsilent
