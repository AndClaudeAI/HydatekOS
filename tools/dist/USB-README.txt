HydatekOS 0.1 - copy to a flash drive and start a PC from it
============================================================

No special tools: you copy two folders onto a flash drive, and the PC
starts from it. Files already on the drive are left alone.

WHAT YOU NEED
- A USB flash drive formatted FAT32. Most drives of 32 GB or less come
  that way. (Larger ones often come as exFAT, which PCs can't start from;
  see "FORMATTING THE DRIVE" below.)
- A PC with UEFI firmware: almost every PC made since about 2012.
  Intel/AMD PCs and ARM (Snapdragon) laptops both work.

STEP 1 - COPY
1. Unzip this file.
2. Open the unzipped folder. Inside are two folders: EFI and HYDATEK.
3. Copy BOTH folders to the top of the flash drive (not into another
   folder on it). The drive should then have:
       EFI\BOOT\BOOTX64.EFI
       EFI\BOOT\BOOTAA64.EFI
       HYDATEK\...
4. Eject the drive safely (Windows: right-click it > Eject).

STEP 2 - START THE PC FROM IT
1. Plug the drive into the PC and turn it on.
2. Straight away, keep tapping the boot-menu key: usually F12
   (sometimes F11, F10, F9, F8 or Esc; the logo screen often says).
3. Choose the flash drive ("UEFI: <drive name>").
4. HydatekOS starts with its setup screen.

IF IT DOESN'T START
- Turn off Secure Boot: open the PC's settings (tap F2 or Del at power
  on), find Secure Boot, set it to Disabled, save and restart.
  HydatekOS isn't signed yet.
- Make sure UEFI boot is on (not "Legacy" or "CSM" only).
- Check the drive is FAT32 and the EFI folder is at the top of the drive.

FORMATTING THE DRIVE (only if it isn't FAT32; this ERASES the drive)
- 32 GB or less, Windows: right-click the drive in File Explorer >
  Format > File system: FAT32 > Start.
- Larger than 32 GB, Windows: Windows only offers exFAT/NTFS. Use Rufus
  (https://rufus.ie): Boot selection "Non bootable", File system
  "FAT32", then Start.
- Mac: Disk Utility > the drive > Erase > Format "MS-DOS (FAT)",
  Scheme "Master Boot Record".

YOUR FILES
HydatekOS keeps your files and settings in the HYDATEK folder on the
drive, so the drive works as a portable HydatekOS you can carry between
PCs. To install it on a PC's own empty disk: Settings > Install.
