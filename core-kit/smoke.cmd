@echo off
setlocal EnableExtensions DisableDelayedExpansion
rem Explicit execution gate. No Core Kit rebuild or core self-compilation.
for %%I in ("%~dp0..") do set "_smoke_root=%%~fI"
set "_smoke_binary=%_smoke_root%\out\core-kit-bootstrap\topaz-core-kit.exe"
set "_smoke_binary_set="
set "_smoke_build="

:arguments
if "%~1"=="" goto configured
if "%~1"=="--native-build" goto build_option
if defined _smoke_binary_set goto usage_error
set "_smoke_binary=%~f1"
set "_smoke_binary_set=1"
shift
goto arguments
:build_option
set "_smoke_build=1"
shift
goto arguments

:configured
if not exist "%_smoke_binary%" (
  echo Build Core Kit first or provide its executable path. 1>&2
  exit /b 1
)
set "TOPAZ_CORE_KIT_ROOT=%_smoke_root%"
set "RUSTUP_TOOLCHAIN=1.96.0"
set "RUSTUP_AUTO_INSTALL=0"
set "CARGO_NET_OFFLINE=true"
:choose_output
set "_smoke_out=%_smoke_root%\out\core-kit smoke-%RANDOM%-%RANDOM%"
if exist "%_smoke_out%" goto choose_output
mkdir "%_smoke_out%"
if not "%ERRORLEVEL%"=="0" exit /b 1
pushd "%_smoke_out%"
if not "%ERRORLEVEL%"=="0" exit /b 1

set "_smoke_stage=version and help"
echo Smoke: %_smoke_stage%
"%_smoke_binary%" --version >version.txt 2>&1
if not "%ERRORLEVEL%"=="0" goto failed
findstr /B /C:"topaz-core-kit " version.txt >nul
if not "%ERRORLEVEL%"=="0" goto failed
"%_smoke_binary%" help >help.txt 2>&1
if not "%ERRORLEVEL%"=="0" goto failed
findstr /C:"Topaz Core Kit" help.txt >nul
if not "%ERRORLEVEL%"=="0" goto failed

set "_smoke_stage=file hashing"
echo Smoke: %_smoke_stage%
"%_smoke_binary%" hash "%_smoke_root%\core-kit\cli\data\input.txt" >hash.txt 2>&1
if not "%ERRORLEVEL%"=="0" goto failed
rem NativeHost uses LF on every platform; compare the complete recorded bytes.
fc /b "%_smoke_root%\core-kit\fixtures\hash.stdout" hash.txt >nul
if not "%ERRORLEVEL%"=="0" goto failed

set "_smoke_stage=linked CLI identity"
echo Smoke: %_smoke_stage%
"%_smoke_binary%" kit build-id >linked-cli.sha256
if errorlevel 1 goto failed
for %%I in ("%_smoke_binary%") do set "_smoke_source=%%~dpIgenerated_cli.rs"
if not exist "%_smoke_source%" goto linked_cli_checked
"%_smoke_binary%" hash "%_smoke_source%" >generated-cli.sha256
if errorlevel 1 goto failed
fc /b generated-cli.sha256 linked-cli.sha256 >nul
if errorlevel 1 goto failed
:linked_cli_checked

set "_smoke_stage=platform executable inspection"
echo Smoke: %_smoke_stage%
"%_smoke_binary%" kit binary-info --file "%_smoke_binary%" >binary-info.json 2>binary-info.stderr.txt
if not "%ERRORLEVEL%"=="0" goto failed
for %%I in (binary-info.json) do if %%~zI==0 goto failed

set "_smoke_stage=check, request and Rust output"
echo Smoke: %_smoke_stage%
"%_smoke_binary%" check --source "%_smoke_root%\examples\core-kit\hello.tpz" >check.txt 2>&1
if not "%ERRORLEVEL%"=="0" goto failed
"%_smoke_binary%" request --source "%_smoke_root%\examples\core-kit\hello.tpz" --out hello.request.json >request.txt 2>&1
if not "%ERRORLEVEL%"=="0" goto failed
"%_smoke_binary%" check --request hello.request.json >check-request.txt 2>&1
if not "%ERRORLEVEL%"=="0" goto failed
"%_smoke_binary%" emit-rust --source "%_smoke_root%\examples\core-kit\hello.tpz" --out hello.rs >emit.txt 2>&1
if not "%ERRORLEVEL%"=="0" goto failed
if not exist hello.rs goto failed
for %%I in (hello.rs) do if %%~zI==0 goto failed

set "_smoke_stage=project resolution and saved input replay"
echo Smoke: %_smoke_stage%
"%_smoke_binary%" check --root "%_smoke_root%\examples\core-kit\project" >project-check.txt 2>&1
if not "%ERRORLEVEL%"=="0" goto failed
"%_smoke_binary%" request --root "%_smoke_root%\examples\core-kit\project" --out "project inputs\snapshot.json" >project-request.txt 2>&1
if not "%ERRORLEVEL%"=="0" goto failed
"%_smoke_binary%" check --request "project inputs\snapshot.json" >project-replay.txt 2>&1
if not "%ERRORLEVEL%"=="0" goto failed
"%_smoke_binary%" emit-rust --root "%_smoke_root%\examples\core-kit\project" --out "project output\generated.rs" >project-emit.txt 2>&1
if not "%ERRORLEVEL%"=="0" goto failed
if not exist "project output\generated.rs" goto failed
for %%I in ("project output\generated.rs") do if %%~zI==0 goto failed

set "_smoke_stage=negative type-check and output preservation"
echo Smoke: %_smoke_stage%
"%_smoke_binary%" check --source "%_smoke_root%\examples\core-kit\type-error.tpz" >rejected-check.txt 2>&1
if not "%ERRORLEVEL%"=="1" goto failed
findstr /C:"TPZ5001:" rejected-check.txt >nul
if not "%ERRORLEVEL%"=="0" goto failed
"%_smoke_binary%" emit-rust --source "%_smoke_root%\examples\core-kit\type-error.tpz" --out rejected.rs >rejected-emit.txt 2>&1
if not "%ERRORLEVEL%"=="1" goto failed
findstr /C:"TPZ5001:" rejected-emit.txt >nul
if not "%ERRORLEVEL%"=="0" goto failed
if exist rejected.rs goto failed
"%_smoke_binary%" check --root "%_smoke_root%\examples\core-kit" >rejected-root.txt 2>&1
if not "%ERRORLEVEL%"=="1" goto failed
findstr /C:"project topaz.toml must be a readable file inside the project root" rejected-root.txt >nul
if not "%ERRORLEVEL%"=="0" goto failed

set "_smoke_stage=recorded input verification"
echo Smoke: %_smoke_stage%
"%_smoke_binary%" kit verify >verify.txt 2>&1
if not "%ERRORLEVEL%"=="0" goto failed

set "_smoke_stage=CLI snapshot and callable contract check"
"%_smoke_binary%" kit refresh --check >refresh-check.txt 2>&1
if not "%ERRORLEVEL%"=="0" goto failed

set "_smoke_stage=payload inventory and extra-file rejection"
echo Smoke: %_smoke_stage%
set "_smoke_arch=%PROCESSOR_ARCHITECTURE%"
if defined PROCESSOR_ARCHITEW6432 set "_smoke_arch=%PROCESSOR_ARCHITEW6432%"
set "_smoke_payload_target="
if /I "%_smoke_arch%"=="AMD64" set "_smoke_payload_target=x86_64-pc-windows-msvc"
if /I "%_smoke_arch%"=="ARM64" set "_smoke_payload_target=aarch64-pc-windows-msvc"
if not defined _smoke_payload_target goto failed
mkdir "payload files"
if not "%ERRORLEVEL%"=="0" goto failed
copy /b "%_smoke_binary%" "payload files\topaz-core-kit.exe" >payload-copy.txt 2>&1
if not "%ERRORLEVEL%"=="0" goto failed
"%_smoke_binary%" kit payload-record --target "%_smoke_payload_target%" --payload-dir "payload files" >payload-record.txt 2>&1
if not "%ERRORLEVEL%"=="0" goto failed
"%_smoke_binary%" kit payload-verify --payload-dir "payload files" >payload-verify.txt 2>&1
if not "%ERRORLEVEL%"=="0" goto failed
echo unrecorded smoke fixture>"payload files\extra.txt"
if not "%ERRORLEVEL%"=="0" goto failed
"%_smoke_binary%" kit payload-verify --payload-dir "payload files" >rejected-payload.txt 2>&1
if not "%ERRORLEVEL%"=="1" goto failed
findstr /C:"payload file count differs from payload.json" rejected-payload.txt >nul
if not "%ERRORLEVEL%"=="0" goto failed
del "payload files\extra.txt"
if not "%ERRORLEVEL%"=="0" goto failed
if not defined _smoke_build goto passed

set "_smoke_stage=native sample build and execution"
echo Smoke: %_smoke_stage%
"%_smoke_binary%" build --source "%_smoke_root%\examples\core-kit\hello.tpz" --out-dir "native output" >native-build.txt 2>&1
if not "%ERRORLEVEL%"=="0" goto failed
set "_smoke_host="
for /f "tokens=2" %%H in ('rustc -vV 2^>nul ^| findstr /B /C:"host: "') do set "_smoke_host=%%H"
if not defined _smoke_host goto failed
"native output\target\%_smoke_host%\release\topaz-core-kit-program.exe" <nul >program.txt 2>&1
if not "%ERRORLEVEL%"=="0" goto failed
fc /b "%_smoke_root%\core-kit\fixtures\hello.stdout" program.txt >nul
if not "%ERRORLEVEL%"=="0" goto failed

set "_smoke_stage=native project build and execution"
echo Smoke: %_smoke_stage%
"%_smoke_binary%" build --request "project inputs\snapshot.json" --out-dir "project native" >project-native-build.txt 2>&1
if not "%ERRORLEVEL%"=="0" goto failed
"project native\target\%_smoke_host%\release\topaz-core-kit-program.exe" <nul >project-program.txt 2>&1
if not "%ERRORLEVEL%"=="0" goto failed
fc /b "%_smoke_root%\core-kit\fixtures\project.stdout" project-program.txt >nul
if not "%ERRORLEVEL%"=="0" goto failed

:passed
echo Core Kit smoke checks passed.
echo Logs retained: "%_smoke_out%"
popd
exit /b 0
:failed
echo Smoke failed during %_smoke_stage%. 1>&2
echo Logs retained: "%_smoke_out%" 1>&2
popd
exit /b 1
:usage_error
echo usage: smoke.cmd [core-kit-executable] [--native-build] 1>&2
exit /b 2
