@echo off
title Steam Downgrader (dev server)
cd /d "%~dp0"
echo Starting Steam Downgrader dev server... (logging to devlog.txt)
echo Keep this window open. Close it to stop the app.
echo.
call npm run tauri dev > devlog.txt 2>&1
echo.
echo === dev server exited. Press a key to close. ===
pause >nul
