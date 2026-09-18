@echo off
setlocal
call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\Common7\Tools\VsDevCmd.bat" -arch=arm64 -host_arch=arm64
if errorlevel 1 (
  echo VsDevCmd failed
  exit /b 1
)
set "CARGO_TARGET_DIR=C:\gladiaflow-target"
cd /d C:\gladiaflow
if not exist node_modules (
  echo npm ci...
  call npm ci
)
echo npm run tauri:dev ...
call npm run tauri:dev
