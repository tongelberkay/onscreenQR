@echo off
title QR Frame — Rust Build
echo.
echo  Building QR Frame Scanner (Rust)...
echo.

where cargo >nul 2>&1
if errorlevel 1 (
    echo  [!] Rust not found. Install from https://rustup.rs
    pause & exit /b 1
)

cargo build --release

if errorlevel 1 (
    echo.
    echo  [!] Build failed. See errors above.
    pause & exit /b 1
)

echo.
echo  [+] Done! Executable at:
echo      target\release\qr-frame.exe
echo.
pause
