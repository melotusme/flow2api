@echo off
setlocal
cd /d "%~dp0"
set PYTHONUTF8=1
set PYTHONIOENCODING=utf-8
where py >nul 2>nul
if not errorlevel 1 (
    py -3.11 -c "import sys; assert sys.version_info[:2] == (3,11)" >nul 2>nul
    if not errorlevel 1 (
        py -3.11 scripts\windows_launcher.py %*
        goto finished
    )
)
python -c "import sys; assert sys.version_info[:2] == (3,11)" >nul 2>nul
if not errorlevel 1 (
    python scripts\windows_launcher.py %*
    goto finished
)
echo Python 3.11 was not found. Install Python 3.11 x64, then reopen this file.
echo Download: https://www.python.org/downloads/windows/
echo Or run: winget install --exact --id Python.Python.3.11
echo Enable "Add python.exe to PATH" when installing.
:finished
echo.
echo Flow2API stopped. Review any errors above before closing this window.
pause
