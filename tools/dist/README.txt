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

PUT IT ON A REAL COMPUTER
-------------------------
1. Write hydatekos.img (from a fresh copy of this zip) to a USB stick of
   1 GB or more with balenaEtcher (https://etcher.balena.io). This erases
   the stick.
2. Plug it in, start the computer and tap its boot-menu key (often F12),
   then choose the stick. If it won't start, turn off Secure Boot in the
   computer's settings (often F2 or Del at power-on).
3. To install on the computer's own EMPTY disk: Settings > Install, pick
   the disk, Install. Then take out the stick and restart.
   HydatekOS only installs on an empty disk; it never changes a disk
   with anything on it.

SHARPER PICTURE
---------------
HydatekOS starts at 1280 x 800. For a bigger, sharper screen:
1. In HydatekOS open Settings > Display.
2. Under Resolution, pick a size that fits your screen, for example
   1600 x 900 on a laptop, or 1920 x 1080 on a big monitor.
   On a very big or high-resolution screen, pick 2560 x 1440 or larger
   and set Size to "Large (2x)" for extra-sharp text.
3. Click "Restart now".
If the picture is still blurry on Windows: in the QEMU window's View menu,
turn off "Zoom To Fit", so QEMU doesn't stretch the picture.

The UEFI firmware in "firmware" is OVMF from the EDK II project (BSD licence,
see firmware/OVMF-LICENCE.txt).
