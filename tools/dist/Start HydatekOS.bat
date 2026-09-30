@echo off
rem HydatekOS: double-click this file to start HydatekOS in QEMU.
setlocal
cd /d "%~dp0"
set "QEMU="
if exist "%ProgramFiles%\qemu\qemu-system-x86_64.exe" set "QEMU=%ProgramFiles%\qemu\qemu-system-x86_64.exe"
if not defined QEMU (
  where qemu-system-x86_64 >nul 2>nul && set "QEMU=qemu-system-x86_64"
)
if not defined QEMU (
  echo.
  echo  QEMU isn't installed yet.
  echo  Download it from https://qemu.weilnetz.de/w64/ and install it,
  echo  then double-click this file again.
  echo.
  pause
  exit /b 1
)
if not exist "firmware\vars.fd" copy "firmware\OVMF_VARS.fd" "firmware\vars.fd" >nul
echo Starting HydatekOS. Close the QEMU window, or use Shut Down in HydatekOS, to stop.
"%QEMU%" -name HydatekOS -machine q35 -m 1G ^
  -drive if=pflash,format=raw,readonly=on,file=firmware\OVMF_CODE.fd ^
  -drive if=pflash,format=raw,file=firmware\vars.fd ^
  -drive format=raw,file=hydatekos.img ^
  -device qemu-xhci -device usb-kbd -device usb-tablet ^
  -netdev user,id=n0 -device virtio-net-pci,netdev=n0 ^
  -device virtio-rng-pci ^
  -serial file:hydatekos-log.txt
if errorlevel 1 (
  echo.
  echo  QEMU stopped with an error. The message above says why.
  pause
)
