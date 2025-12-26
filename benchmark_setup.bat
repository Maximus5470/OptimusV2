@echo off
REM OptimusV2 Benchmark Setup for Windows
REM This script sets up and runs the benchmark test

echo ============================================
echo   OptimusV2 Benchmark Setup
echo ============================================
echo.

REM Check if Python is installed
python --version >nul 2>&1
if errorlevel 1 (
    echo [ERROR] Python is not installed or not in PATH
    echo Please install Python from https://www.python.org/downloads/
    pause
    exit /b 1
)

echo [OK] Python found
python --version

REM Install required package
echo.
echo [*] Installing requests package...
pip install requests --quiet

REM Default settings
set API_URL=http://172.16.7.253:80
set LANGUAGE=python
set CONCURRENCY=20
set REQUESTS=50

REM Parse arguments
:parse_args
if "%~1"=="" goto run_benchmark
if /i "%~1"=="--url" set API_URL=%~2& shift & shift & goto parse_args
if /i "%~1"=="-l" set LANGUAGE=%~2& shift & shift & goto parse_args
if /i "%~1"=="--language" set LANGUAGE=%~2& shift & shift & goto parse_args
if /i "%~1"=="-c" set CONCURRENCY=%~2& shift & shift & goto parse_args
if /i "%~1"=="-n" set REQUESTS=%~2& shift & shift & goto parse_args
shift
goto parse_args

:run_benchmark
echo.
echo ============================================
echo   Running Benchmark
echo ============================================
echo   API URL:     %API_URL%
echo   Language:    %LANGUAGE%
echo   Concurrency: %CONCURRENCY%
echo   Requests:    %REQUESTS%
echo ============================================
echo.

python benchmark.py --url %API_URL% --language %LANGUAGE% --concurrency %CONCURRENCY% --requests %REQUESTS%

echo.
echo ============================================
echo   Benchmark Complete
echo ============================================
pause
