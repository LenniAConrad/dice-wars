@echo off
rem Starts install-windows.ps1 without changing the PowerShell script policy.
rem Double-click this file, or run it in a terminal; options go straight to the script:
rem   install-windows.cmd -Jobs 4     install-windows.cmd -Uninstall     install-windows.cmd -Help
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0install-windows.ps1" %*
set "DICE_WARS_EXIT=%ERRORLEVEL%"
rem Keep the window open when it was started by double-clicking.
echo %cmdcmdline% | find /i "%~nx0" >nul && pause
exit /b %DICE_WARS_EXIT%
