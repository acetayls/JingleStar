; Installeur Windows de CueStar (Inno Setup 6).
; Lancé par package.ps1, qui passe /DVersion=… /DStage=… /DOutDir=… :
;   Stage  : dossier préparé (cuestar.exe, libmpv-2.dll, licences\)
;   OutDir : dossier où écrire CueStar-<version>-windows-x64-setup.exe

#ifndef Version
  #define Version "0.0.0"
#endif
#ifndef Stage
  #define Stage "..\..\..\..\dist\stage-windows"
#endif
#ifndef OutDir
  #define OutDir "..\..\..\..\dist"
#endif

[Setup]
AppId={{6E2C1A0B-5D7E-4C3B-9A41-C0E5D7A1F3B2}
AppName=CueStar
AppVersion={#Version}
AppVerName=CueStar {#Version}
AppPublisher=Pierrick
AppPublisherURL=https://github.com/acetayls/mediastar
DefaultDirName={autopf}\CueStar
DefaultGroupName=CueStar
DisableProgramGroupPage=yes
; Installation pour tous (droits administrateur) ou pour l'utilisateur seul, au choix.
PrivilegesRequired=admin
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
OutputDir={#OutDir}
OutputBaseFilename=CueStar-{#Version}-windows-x64-setup
SetupIconFile=..\..\assets\icons\cuestar.ico
UninstallDisplayIcon={app}\cuestar.exe
LicenseFile={#Stage}\licences\LICENCE-CueStar.txt
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
; Fermer CueStar s'il tourne pendant une mise à jour.
CloseApplications=yes
RestartApplications=no

[Languages]
Name: "fr"; MessagesFile: "compiler:Languages\French.isl"
Name: "en"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#Stage}\cuestar.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Stage}\libmpv-2.dll"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Stage}\licences\*"; DestDir: "{app}\licences"; Flags: ignoreversion recursesubdirs

[Icons]
Name: "{autoprograms}\CueStar"; Filename: "{app}\cuestar.exe"
Name: "{autodesktop}\CueStar"; Filename: "{app}\cuestar.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\cuestar.exe"; Description: "{cm:LaunchProgram,CueStar}"; Flags: nowait postinstall skipifsilent
