@echo off
setlocal EnableExtensions
REM Build GladiaFlow on the Windows VM and install it locally (not from X:\ share).

call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\Common7\Tools\VsDevCmd.bat" -arch=arm64 -host_arch=arm64
if errorlevel 1 (
  echo ERROR: VsDevCmd failed. Install VS Build Tools C++ ARM64 first.
  exit /b 1
)

echo === Sync sources X:\ -^> C:\gladiaflow ===
if not exist X:\package.json (
  echo ERROR: X:\gladiaflow share not mounted. Enable Host Shared Folders in Parallels.
  exit /b 1
)
robocopy X:\ C:\gladiaflow /MIR /XD node_modules target .git .models dist src-tauri\target C:\gladiaflow-target /NFL /NDL /NJH /NJS /nc /ns /np
REM robocopy codes 0-7 are success
if errorlevel 8 (
  echo ERROR: robocopy failed
  exit /b 1
)

set "CARGO_TARGET_DIR=C:\gladiaflow-target"
cd /d C:\gladiaflow

echo === npm ci ===
call npm ci
if errorlevel 1 exit /b 1

echo === tauri build (NSIS installer) ===
call npm run tauri:build
if errorlevel 1 exit /b 1

set "NSIS="
for %%F in ("C:\gladiaflow-target\release\bundle\nsis\*.exe") do set "NSIS=%%~fF"
if not defined NSIS (
  for %%F in ("C:\gladiaflow\src-tauri\target\release\bundle\nsis\*.exe") do set "NSIS=%%~fF"
)

if not defined NSIS (
  echo ERROR: NSIS installer not found under gladiaflow-target\release\bundle\nsis
  dir /s /b C:\gladiaflow-target\release\bundle 2>nul
  exit /b 1
)

echo === Installing %NSIS% ===
"%NSIS%" /S
if errorlevel 1 (
  echo Silent install failed — launching interactive installer...
  start "" "%NSIS%"
)

echo.
echo Done. Launch GladiaFlow from the Start menu.
echo Installer path: %NSIS%
exit /b 0
