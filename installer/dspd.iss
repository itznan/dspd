; SPDX-License-Identifier: MIT
; Copyright (c) 2026 itznan.
; Script for Inno Setup 6 - dspd Disk Benchmark Engine for Windows
#ifndef MyAppVersion
  #define MyAppVersion "0.2.0"
#endif

#ifndef MyAppTarget
  #define MyAppTarget "x86_64"
#endif

#ifndef ArchitectureMode
  #define ArchitectureMode "x64compatible"
#endif

#ifndef SourceExePath
  #define SourceExePath "..\target\release\dspd.exe"
#endif

#define MyAppName "dspd"
#define MyAppPublisher "itznan"
#define MyAppURL "https://github.com/itznan/dspd"
#define MyAppExeName "dspd.exe"

[Setup]
AppId={{9144B27C-6A38-4F2E-99A0-3277C6B61622}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}
AppUpdatesURL={#MyAppURL}/releases
DefaultDirName={autopf}\dspd
DefaultGroupName=dspd
AllowNoIcons=yes
LicenseFile=..\LICENSE
OutputDir=Output
OutputBaseFilename=dspd-Setup-v{#MyAppVersion}-{#MyAppTarget}
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern
ArchitecturesInstallIn64BitMode={#ArchitectureMode}
PrivilegesRequired=admin
PrivilegesRequiredOverridesAllowed=dialog
ChangesEnvironment=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "addtopath"; Description: "Add dspd to system PATH environment variable (recommended)"; GroupDescription: "System Integration:"; Flags: checkedonce
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#SourceExePath}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\dspd.ps1"; DestDir: "{app}"; DestName: "dspd-interactive.ps1"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\dspd Interactive Benchmark"; Filename: "powershell.exe"; Parameters: "-ExecutionPolicy Bypass -NoExit -File ""{app}\dspd-interactive.ps1"""; WorkingDir: "{app}"; Comment: "Interactive drive selector and benchmark launcher"
Name: "{group}\dspd Command Prompt"; Filename: "cmd.exe"; Parameters: "/K ""cd /d ""{app}"" && title dspd Benchmark && dspd.exe --help"""; WorkingDir: "{app}"; Comment: "Open command prompt ready for dspd commands"
Name: "{group}\{cm:UninstallProgram,{#MyAppName}}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\dspd Interactive Benchmark"; Filename: "powershell.exe"; Parameters: "-ExecutionPolicy Bypass -NoExit -File ""{app}\dspd-interactive.ps1"""; Tasks: desktopicon

[Run]
Filename: "powershell.exe"; Parameters: "-ExecutionPolicy Bypass -NoExit -File ""{app}\dspd-interactive.ps1"""; Description: "Launch dspd Interactive Benchmark"; Flags: postinstall skipifsilent nowait unchecked
Filename: "{app}\{#MyAppExeName}"; Parameters: "--help"; Description: "View dspd CLI usage"; Flags: postinstall skipifsilent nowait unchecked

[Code]
const
    EnvironmentKey = 'SYSTEM\CurrentControlSet\Control\Session Manager\Environment';

procedure AddPathToEnv();
var
    Paths: string;
    AppDir: string;
begin
    AppDir := ExpandConstant('{app}');
    if RegQueryStringValue(HKEY_LOCAL_MACHINE, EnvironmentKey, 'Path', Paths) then
    begin
        if Pos(';' + Uppercase(AppDir) + ';', ';' + Uppercase(Paths) + ';') = 0 then
        begin
            if (Length(Paths) > 0) and (Paths[Length(Paths)] <> ';') then
                Paths := Paths + ';';
            Paths := Paths + AppDir;
            RegWriteStringValue(HKEY_LOCAL_MACHINE, EnvironmentKey, 'Path', Paths);
        end;
    end;
end;

procedure RemovePathFromEnv();
var
    Paths: string;
    AppDir: string;
    P: Integer;
    CleanPaths: string;
begin
    AppDir := ExpandConstant('{app}');
    if RegQueryStringValue(HKEY_LOCAL_MACHINE, EnvironmentKey, 'Path', Paths) then
    begin
        CleanPaths := ';' + Paths + ';';
        P := Pos(';' + Uppercase(AppDir) + ';', Uppercase(CleanPaths));
        if P > 0 then
        begin
            Delete(CleanPaths, P, Length(AppDir) + 1);
            if (Length(CleanPaths) > 0) and (CleanPaths[1] = ';') then
                Delete(CleanPaths, 1, 1);
            if (Length(CleanPaths) > 0) and (CleanPaths[Length(CleanPaths)] = ';') then
                Delete(CleanPaths, Length(CleanPaths), 1);
            RegWriteStringValue(HKEY_LOCAL_MACHINE, EnvironmentKey, 'Path', CleanPaths);
        end;
    end;
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
    if CurStep = ssPostInstall then
    begin
        if WizardIsTaskSelected('addtopath') then
            AddPathToEnv();
    end;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
    if CurUninstallStep = usPostUninstall then
        RemovePathFromEnv();
end;
