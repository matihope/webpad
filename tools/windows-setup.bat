@echo off
:: webpad setup: installs the C++ Build Tools (linker for Rust) and the ViGEmBus driver.
:: Double-click it; it asks for administrator rights by itself. Results go to %USERPROFILE%\webpad-setup.log.

net session >nul 2>&1
if %errorlevel% neq 0 (
    echo Requesting administrator rights...
    powershell -NoProfile -Command "Start-Process -FilePath '%~f0' -Verb RunAs"
    exit /b
)

set LOG=%USERPROFILE%\webpad-setup.log
echo webpad setup started %date% %time% > "%LOG%"

echo.
echo [1/2] Installing Visual Studio Build Tools (C++ workload). This takes a while...
winget install --id Microsoft.VisualStudio.BuildTools --exact --source winget --accept-package-agreements --accept-source-agreements --override "--passive --wait --norestart --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
echo BuildTools exit code: %errorlevel% >> "%LOG%"

echo.
echo [2/2] Installing ViGEmBus driver...
winget install --id ViGEm.ViGEmBus --exact --source winget --accept-package-agreements --accept-source-agreements
echo ViGEmBus exit code: %errorlevel% >> "%LOG%"

sc query ViGEmBus >> "%LOG%" 2>&1
echo webpad setup finished %date% %time% >> "%LOG%"

echo.
echo Done. Results were saved to %LOG%
echo If the ViGEmBus installer asked for a restart, restart the PC.
pause
