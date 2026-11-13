@echo off
setlocal
where wsl.exe >nul 2>&1
if errorlevel 1 (
    echo WSL is required. In an Administrator terminal, run: wsl --install -d Ubuntu
    echo Then finish the one-time Linux setup in README.md.
    pause
    exit /b 1
)
rem --cd accepts a Windows path; quotes preserve repository paths containing spaces.
wsl.exe --cd "%~dp0." bash -l ./build-apk.bash %*
set "BUILD_EXIT=%ERRORLEVEL%"
if not "%BUILD_EXIT%"=="0" echo Build failed. Read the error above and the README prerequisites.
pause
exit /b %BUILD_EXIT%
