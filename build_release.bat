@echo off
setlocal
set "MS=C:\Program Files\Microsoft Visual Studio\18\Professional\VC\Tools\MSVC\14.51.36231"
set "SDK=C:\Program Files (x86)\Windows Kits\10"
set "SDKVER=10.0.26100.0"

set "PATH=%MS%\bin\Hostx64\x64;%SDK%\bin\%SDKVER%\x64;%PATH%"
set "INCLUDE=%MS%\include;%SDK%\Include\%SDKVER%\ucrt;%SDK%\Include\%SDKVER%\shared;%SDK%\Include\%SDKVER%\um;%SDK%\Include\%SDKVER%\winrt;%SDK%\Include\%SDKVER%\cppwinrt"
set "LIB=%MS%\lib\x64;%SDK%\Lib\%SDKVER%\ucrt\x64;%SDK%\Lib\%SDKVER%\um\x64"

if not exist build md build
cmake -S . -B build -G "NMake Makefiles" -DCMAKE_BUILD_TYPE=Release
if errorlevel 1 (
  echo CONFIGURE_FAILED
  exit /b 1
)
cmake --build build
if errorlevel 1 (
  echo BUILD_FAILED
  exit /b 1
)
echo BUILD_OK
