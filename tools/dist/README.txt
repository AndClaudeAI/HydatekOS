HydatekOS 0.1 - ready to run
============================

This folder has everything HydatekOS needs except QEMU, the program that
runs it as a virtual computer. Nothing here changes your own computer.


WINDOWS
-------
1. Unzip this folder: right-click the .zip file > Extract All > Extract.
   (It won't start from inside the .zip.)
2. Install QEMU:
   a. Go to https://qemu.weilnetz.de/w64/
   b. Download the newest file that starts with "qemu-w64-setup".
   c. Open it and click Next until it finishes. Keep the settings as they are.
3. In the unzipped folder, double-click "Start HydatekOS.bat".
   If Windows shows "Windows protected your PC", click More info > Run anyway.
4. A window opens and HydatekOS starts.


MAC
---
1. Double-click the .zip file to unzip it.
2. Install QEMU. Open Terminal (Applications > Utilities) and type:
       brew install qemu
   (If "brew" isn't found, install Homebrew first from https://brew.sh)
3. In the unzipped folder, double-click "start-hydatekos.command".
   If the Mac says it can't be opened, right-click it > Open > Open.
4. A window opens and HydatekOS starts.
   On Apple Silicon Macs it runs, but slowly: the Mac has to pretend to be an
   Intel PC.


LINUX
-----
1. Unzip:              unzip HydatekOS-0.1-x86_64.zip
2. Install QEMU:       sudo apt install qemu-system-x86     (Ubuntu/Debian)
3. Start it:           cd HydatekOS-0.1 && ./start-hydatekos.command


USING IT
--------
- The first time, HydatekOS asks your name, a picture and a password.
- Click inside the window to use the mouse there.
- To stop: HydatekOS menu (the logo, top left) > Shut Down.
  Closing the window also works, but may lose unsaved work.
- Your files and settings are kept in hydatekos.img, so they're still there
  next time. To start over, unzip a fresh copy.
- Something went wrong? hydatekos-log.txt in this folder says what
  HydatekOS was doing. Send it along with a description.

The UEFI firmware in "firmware" is OVMF from the EDK II project (BSD licence,
see firmware/OVMF-LICENCE.txt).
