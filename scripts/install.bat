@echo off
rem Build and install Dice Wars on Windows: binary in %LOCALAPPDATA%\Programs
rem plus a Start Menu shortcut. Installs the Rust toolchain if missing.
setlocal
cd /d "%~dp0.."

call scripts\build.bat
if errorlevel 1 exit /b 1

set "DEST=%LOCALAPPDATA%\Programs\DiceWars"
if not exist "%DEST%" mkdir "%DEST%"
copy /Y target\release\dicegame.exe "%DEST%\DiceWars.exe" >nul
copy /Y assets\icon.ico "%DEST%\DiceWars.ico" >nul

powershell -Command "$ws = New-Object -ComObject WScript.Shell; $s = $ws.CreateShortcut([Environment]::GetFolderPath('Programs') + '\Dice Wars.lnk'); $s.TargetPath = '%DEST%\DiceWars.exe'; $s.WorkingDirectory = '%DEST%'; $s.IconLocation = '%DEST%\DiceWars.ico'; $s.Save()"
echo Installed to %DEST% with a Start Menu shortcut.
