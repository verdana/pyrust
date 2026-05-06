# pyrust TSF DLL Build + Register
$ErrorActionPreference = "Stop"
Push-Location "$PSScriptRoot\..\crates\tsf"

try {
Write-Host "=== pyrust TSF DLL Build ===" -ForegroundColor Cyan

# Check Rust
if (-not (Get-Command rustc -ErrorAction SilentlyContinue)) {
    Write-Host "Rust not installed. Run scripts\build.ps1 first." -ForegroundColor Red
    exit 1
}

# Check admin
$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {
    Write-Host "WARNING: Not running as Administrator. DLL registration will be skipped." -ForegroundColor Yellow
}

# Function: Find and kill processes locking a DLL using PowerShell's openfiles or WMI
function Stop-DllLockers {
    param([string]$DllPath)

    if (-not (Test-Path $DllPath)) { return }

    $fullPath = (Resolve-Path $DllPath).Path
    Write-Host "      Checking locks on: $fullPath" -ForegroundColor Gray

    # Try using handle.exe if available (Sysinternals)
    if (Get-Command handle.exe -ErrorAction SilentlyContinue) {
        $output = & handle.exe -accepteula $fullPath 2>$null
        foreach ($line in $output) {
            if ($line -match '^\s*(\S+)\s+pid:\s*(\d+)') {
                $procName = $Matches[1]
                $pid = [int]$Matches[2]
                Write-Host "      Killing: $procName (PID: $pid)" -ForegroundColor Yellow
                Stop-Process -Id $pid -Force -ErrorAction SilentlyContinue
            }
        }
    }
}

# Phase 1: Kill processes that lock TSF DLLs
Write-Host "[1/3] Killing text-input processes..." -ForegroundColor Cyan

# Standard IME-related processes
$procs = @("notepad", "TextInputHost", "ctfmon", "ApplicationFrameHost", "SearchHost", "ShellExperienceHost")
foreach ($p in $procs) {
    Stop-Process -Name $p -Force -ErrorAction SilentlyContinue
}

# Also try to find and kill any process locking our specific DLLs
$dllPaths = @(
    ".\target\release\tsf.dll"
    ".\target\release\deps\tsf.dll"
)
foreach ($dll in $dllPaths) {
    Stop-DllLockers -DllPath $dll
}

Write-Host "      Waiting for file handles to release..." -ForegroundColor Gray
Start-Sleep -Seconds 2

# Second pass: try again after wait
foreach ($dll in $dllPaths) {
    Stop-DllLockers -DllPath $dll
}

# Clean old build artifacts
$dll = "target\release\tsf.dll"
if (Test-Path $dll) { Remove-Item $dll -Force -ErrorAction SilentlyContinue }
if (Test-Path "target\release\deps\tsf.dll") { Remove-Item "target\release\deps\tsf.dll" -Force -ErrorAction SilentlyContinue }

# If still locked, try cargo clean
if (Test-Path "target\release\deps\tsf.dll") {
    cargo clean 2>$null
    if (Test-Path "target\release\deps\tsf.dll") {
        Rename-Item "target\release\deps\tsf.dll" "tsf.locked.old" -ErrorAction SilentlyContinue
    }
}

# Phase 2: Build
Write-Host ""
Write-Host "[2/3] Building TSF DLL..." -ForegroundColor Cyan
cargo build --release
if ($LASTEXITCODE -ne 0) {
    Write-Host ""
    Write-Host "Build FAILED - DLL may be locked by the system." -ForegroundColor Red
    Write-Host "Fix: Reboot, then run this script immediately after login." -ForegroundColor Yellow
    exit 1
}
Write-Host "      Build OK" -ForegroundColor Green

# Clean up renamed file
if (Test-Path "target\release\deps\tsf.locked.old") { Remove-Item "target\release\deps\tsf.locked.old" -Force -ErrorAction SilentlyContinue }

# Phase 3: Register
Write-Host ""
if ($isAdmin) {
    Write-Host "[3/3] Registering TSF DLL..." -ForegroundColor Cyan
    $dllPath = (Resolve-Path $dll).Path
    regsvr32 /s $dllPath
    if ($LASTEXITCODE -eq 0) {
        Write-Host "      Register OK" -ForegroundColor Green
    } else {
        Write-Host "      Register FAILED - check C:\Users\Verdana\pyrust_tsf.log" -ForegroundColor Red
    }
} else {
    Write-Host "[3/3] Skipped registration (not admin)" -ForegroundColor Yellow
    Write-Host "      Manual: regsvr32 $(Resolve-Path $dll)" -ForegroundColor Gray
}

Write-Host ""
Write-Host "======================================" -ForegroundColor Cyan
Write-Host "DLL: $(Resolve-Path $dll)" -ForegroundColor Green
Write-Host "Log: C:\Users\Verdana\pyrust_tsf.log" -ForegroundColor Gray
Write-Host "======================================" -ForegroundColor Cyan

} finally {
    Pop-Location
}
