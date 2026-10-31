@echo off
setlocal EnableExtensions DisableDelayedExpansion
rem Respect host execution policy. No persistent or process policy override.
powershell.exe -NoLogo -NoProfile -NonInteractive -File "%~dp0core-kit\install.ps1" %*
exit /b %ERRORLEVEL%
