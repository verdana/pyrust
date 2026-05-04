# pyrust - Personal Pinyin IME (dev console mode)
Set-Location "$PSScriptRoot\.."

Write-Host "=== pyrust - Personal Pinyin IME ===" -ForegroundColor Cyan
Write-Host "Type pinyin then Enter, press 1-9 to select" -ForegroundColor Gray
Write-Host "quit to exit" -ForegroundColor Gray
Write-Host ""

if (-not (Test-Path "pyrust.exe")) {
    Write-Host "pyrust.exe not found. Run scripts\build.ps1 first." -ForegroundColor Red
    exit 1
}

& .\pyrust.exe
