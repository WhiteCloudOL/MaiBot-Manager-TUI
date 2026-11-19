@echo off
setlocal
chcp 65001 >nul
set "PYTHONUTF8=1"
python "%~dp0sync_release.py" %*
set "SYNC_EXIT_CODE=%ERRORLEVEL%"
if "%~1"=="" pause
exit /b %SYNC_EXIT_CODE%
