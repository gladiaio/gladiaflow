@echo off
setlocal
call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\Common7\Tools\VsDevCmd.bat" -arch=arm64 -host_arch=arm64
if errorlevel 1 (
  echo VsDevCmd failed
  exit /b 1
)
set "CARGO_TARGET_DIR=C:\gladiaflow-target"
if not exist "C:\gladiaflow\src-tauri\Cargo.toml" (
  echo Missing C:\gladiaflow - run robocopy first
  exit /b 1
)
cd /d C:\gladiaflow\src-tauri
echo PATH has link?
where link
echo Running cargo check...
cargo check
exit /b %ERRORLEVEL%
