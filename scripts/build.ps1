# pyrust Windows 构建脚本
param(
    [switch]$Release
)

$ErrorActionPreference = "Stop"
Set-Location "$PSScriptRoot\.."

Write-Host "=== pyrust Windows Build ===" -ForegroundColor Cyan

# Check Rust
if (-not (Get-Command rustc -ErrorAction SilentlyContinue)) {
    Write-Host "Rust not installed. Installing..." -ForegroundColor Yellow
    $rustupUrl = "https://win.rustup.rs/x86_64"
    $rustupPath = "$env:TEMP\rustup-init.exe"
    Invoke-WebRequest -Uri $rustupUrl -OutFile $rustupPath
    Write-Host "Follow the prompts to install Rust (default options are fine)" -ForegroundColor Yellow
    Start-Process -FilePath $rustupPath -Wait
    $env:Path = [System.Environment]::GetEnvironmentVariable("Path", "Machine") + ";" + [System.Environment]::GetEnvironmentVariable("Path", "User")
    Write-Host "Rust installed!" -ForegroundColor Green
}

$profile = if ($Release) { "--release" } else { "" }
Write-Host "Building pyrust ($($Release ? 'release' : 'debug'))..." -ForegroundColor Cyan
cargo build $profile
if ($LASTEXITCODE -ne 0) {
    Write-Host "Build FAILED" -ForegroundColor Red
    exit 1
}

$targetDir = if ($Release) { "target\release" } else { "target\debug" }
$exe = "$targetDir\pyrust.exe"
if (Test-Path $exe) {
    Copy-Item $exe "$PSScriptRoot\.." -Force
    Write-Host "Build OK -> pyrust.exe" -ForegroundColor Green
} else {
    Write-Host "Build OK (binary in $targetDir)" -ForegroundColor Green
}
