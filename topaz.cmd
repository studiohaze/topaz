@echo off
setlocal EnableExtensions DisableDelayedExpansion
rem Resolve an ASCII relative path only. Preserve original native argv with %%*.
set "_topaz_relative="
rem Keep checkout path characters out of the nested command parsed by for /f.
pushd "%~dp0" >nul
if errorlevel 1 exit /b 1
for /f "delims=" %%P in ('powershell.exe -NoLogo -NoProfile -NonInteractive -File "core-kit\launch.ps1"') do set "_topaz_relative=%%P"
popd
if not defined _topaz_relative exit /b 1
set "TOPAZ_CORE_KIT_ROOT=%~dp0"
"%~dp0%_topaz_relative%" %*
exit /b %ERRORLEVEL%
