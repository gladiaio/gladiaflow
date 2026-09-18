#Requires -Version 5.1
$ErrorActionPreference = "Stop"
Write-Host "== GladiaFlow Windows smoke ==" -ForegroundColor Cyan

function Assert-Cmd($Name) {
  if (-not (Get-Command $Name -ErrorAction SilentlyContinue)) {
    throw "Missing command: $Name"
  }
  & $Name --version 2>$null
  if ($LASTEXITCODE -ne 0 -and $Name -ne "cargo") { }
}

Write-Host "-- Toolchain"
Assert-Cmd node
Assert-Cmd npm
Assert-Cmd rustc
Assert-Cmd cargo

$root = if (Test-Path "C:\gladiaflow") { "C:\gladiaflow" }
  elseif (Test-Path "\\VBoxSvr\gladiaflow") { "\\VBoxSvr\gladiaflow" }
  else { Split-Path -Parent $PSScriptRoot }

Set-Location $root
Write-Host "Root: $root"

Write-Host "-- npm ci / build"
if (-not (Test-Path "node_modules")) { npm ci }
npm run build

Write-Host "-- cargo check (Windows)"
Push-Location src-tauri
cargo check
if ($LASTEXITCODE -ne 0) { throw "cargo check failed" }
Pop-Location

Write-Host "OK: Windows compile smoke passed." -ForegroundColor Green
Write-Host "Next: npx tauri dev  then enable on-screen vocabulary in onboarding/settings."
