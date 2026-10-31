@echo off
setlocal EnableExtensions DisableDelayedExpansion
rem Native cmd entry point: no PowerShell script execution-policy dependency.
set "_kit_root=%~dp0"
set "_kit_out=%~dp0out\core-kit-bootstrap"
set "_kit_out_set="
set "_kit_verify="

:arguments
if "%~1"=="" goto configured
if "%~1"=="--help" goto help
if "%~1"=="-h" goto help
if "%~1"=="--verify-core" goto verify_option
if "%~1"=="--out" goto out_option
if defined _kit_out_set goto usage_error
set "_kit_argument=%~1"
if "%_kit_argument:~0,1%"=="-" goto usage_error
set "_kit_out=%~1"
set "_kit_out_set=1"
shift
goto arguments

:verify_option
if defined _kit_verify goto usage_error
set "_kit_verify=1"
shift
goto arguments

:out_option
if defined _kit_out_set goto usage_error
if "%~2"=="" goto usage_error
set "_kit_out=%~2"
set "_kit_out_set=1"
shift
shift
goto arguments

:configured
rem Pin installed tools and disable rustup's implicit download behavior.
set "RUSTUP_TOOLCHAIN=1.96.0"
set "RUSTUP_AUTO_INSTALL=0"
set "CARGO_NET_OFFLINE=true"
set "TOPAZ_CORE_KIT_ROOT=%_kit_root%"
set "_kit_rust_version="
set "_kit_cargo_version="
set "_kit_host="
for /f "tokens=2" %%V in ('rustc --version 2^>nul') do set "_kit_rust_version=%%V"
if not "%_kit_rust_version%"=="1.96.0" goto missing_tools
for /f "tokens=2" %%V in ('cargo --version 2^>nul') do set "_kit_cargo_version=%%V"
if not "%_kit_cargo_version%"=="1.96.0" goto missing_tools
for /f "tokens=2" %%H in ('rustc -vV 2^>nul ^| findstr /B /C:"host: "') do set "_kit_host=%%H"
if not defined _kit_host goto missing_tools
for %%I in ("%_kit_out%") do set "_kit_out=%%~fI"
if not exist "%_kit_out%\" mkdir "%_kit_out%"
if not "%ERRORLEVEL%"=="0" exit /b 1
set "_kit_target=%_kit_root%compiler\target"
set "_kit_artifacts=%_kit_target%\%_kit_host%\release"
pushd "%_kit_root%compiler"
if not "%ERRORLEVEL%"=="0" exit /b 1

if defined _kit_verify (
  echo Bootstrap mode: product build plus optional canonical core regeneration.
) else (
  echo Bootstrap mode: product build. Canonical core regeneration is disabled.
)

set "_kit_stage=public core and CLI generator build"
echo [1/3] Building the public core and CLI generator offline. The first build can take a while.
cargo build -p topaz-core-kit-native --bin topaz-core-kit-generate-cli --release --frozen --target "%_kit_host%" --target-dir "%_kit_target%"
if not "%ERRORLEVEL%"=="0" goto failed
set "_kit_stage=Topaz CLI generation"
echo [2/3] Compiling the checked-in Topaz CLI source.
"%_kit_artifacts%\topaz-core-kit-generate-cli.exe" "%_kit_out%"
if not "%ERRORLEVEL%"=="0" goto failed

set "TOPAZ_CORE_KIT_GENERATED_CLI=%_kit_out%\generated_cli.rs"
set "_kit_stage=native Core Kit build"
echo [3/3] Linking the native Core Kit.
cargo build -p topaz-core-kit-native --bin topaz-core-kit --features native-cli --message-format=short --release --frozen --target "%_kit_host%" --target-dir "%_kit_target%"
if not "%ERRORLEVEL%"=="0" goto failed
set "_kit_stage=copying the built Core Kit"
copy /b /y "%_kit_artifacts%\topaz-core-kit.exe" "%_kit_out%\topaz-core-kit.exe" >nul
if not "%ERRORLEVEL%"=="0" goto failed
if not defined _kit_verify goto success

set "_kit_stage=optional seed build"
echo Regenerating the canonical core; this optional step can take a long time.
cargo build -p topaz_seed --release --frozen --target "%_kit_host%" --target-dir "%_kit_target%"
if not "%ERRORLEVEL%"=="0" goto failed
set "_kit_stage=optional canonical core regeneration"
"%_kit_artifacts%\topaz-seed.exe" --bootstrap "%_kit_root%compiler\seed\initial" "%_kit_out%\compiler-regenerated.rs"
if not "%ERRORLEVEL%"=="0" goto failed
set "_kit_stage=optional canonical core comparison"
fc /b "%_kit_root%compiler\generated\native\compiler.rs" "%_kit_out%\compiler-regenerated.rs" >nul
if not "%ERRORLEVEL%"=="0" goto core_differs
echo Canonical core regeneration matches byte for byte.

:success
echo Core Kit output: "%_kit_out%\topaz-core-kit.exe"
echo Keep the binary inside this checkout, or set TOPAZ_CORE_KIT_ROOT to its data checkout.
echo This local build does not create a release payload or reproducibility claim.
popd
exit /b 0

:core_differs
echo Canonical core regeneration differs. 1>&2
:failed
echo Bootstrap failed during %_kit_stage%. 1>&2
popd
exit /b 1

:missing_tools
echo Rust and Cargo 1.96.0 must already be installed with the native linker. 1>&2
echo Automatic toolchain installation is disabled. 1>&2
exit /b 1

:usage_error
echo usage: bootstrap.cmd [--out DIRECTORY ^| DIRECTORY] [--verify-core] 1>&2
exit /b 2

:help
echo usage: bootstrap.cmd [--out DIRECTORY ^| DIRECTORY] [--verify-core]
echo Builds the public core, generates the Topaz CLI, and links Core Kit.
echo Requires already-installed Rust/Cargo 1.96.0 and the native linker.
echo --verify-core additionally performs expensive canonical core regeneration.
exit /b 0
