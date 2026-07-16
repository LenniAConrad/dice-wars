@echo off
rem Build Dice Wars from scratch on Windows - installs Rust if missing.
setlocal
cd /d "%~dp0.."

where cargo >nul 2>nul
if errorlevel 1 (
    echo Rust toolchain not found - downloading rustup...
    powershell -Command "Invoke-WebRequest https://win.rustup.rs/x86_64 -OutFile $env:TEMP\rustup-init.exe"
    "%TEMP%\rustup-init.exe" -y
    set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
)

cargo build --release
if errorlevel 1 exit /b 1
echo Built: target\release\dicegame.exe
