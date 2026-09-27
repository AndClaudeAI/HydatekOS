# HydatekOS Link for Android

The phone side of [Phone Link](../../docs/PHONE_LINK.md). It shares texts,
notifications, calls and photos with a HydatekOS PC on the same network, sends
texts and places calls when the PC asks, and exchanges files and clipboard text.

## Install on a phone

1. Pair the phone's browser first: open Phone Link on the PC and scan the code.
2. On the page that opens, tap **Download the app**. The PC serves it from
   `http://<pc>:7743/app.apk` if the disk image was built with it (see below).
3. Install it. Android will ask you to allow installs from your browser.
4. Back on the page, tap **Pair the app**. The app opens already paired.
5. In the app, tap **Allow texts, calls and photos** and **Allow notification
   access**. On Android 13+, sideloaded apps first need *App info › ⋮ › Allow
   restricted settings* before SMS and notification access can be granted.

The app keeps a small "HydatekOS Link" notification while it's connected (a
foreground service), and reconnects on its own when the PC comes back.

## Build

The Android SDK isn't needed:

```sh
companion/android/build.sh      # -> companion/android/build/hydatek-link.apk
tools/mkimage.sh                # puts the APK on the HydatekOS disk image
```

`build.sh` downloads its build inputs from Maven Central once:
- Robolectric's `android-all` Android 14 framework jar, to compile against
- `dalvik-dx`, to make `classes.dex`
- `apksig`, for APK Signature Scheme v2

It then:
- compiles with `javac --release 8`;
- writes the binary `AndroidManifest.xml` with `tools/axml.py` (no aapt2);
- signs with a local key (`build/debug.p12`, created on first run). Set
  `HYDATEK_KEYSTORE` / `HYDATEK_KEYSTORE_PASS` to sign with your own.

The app has no resource table: its icon and theme are Android framework
resources, and its screens are built in code.

## Code

| File | Role |
|---|---|
| `Hlp.java` | Message format, HKDF, ChaCha20-Poly1305 session (plain Java) |
| `LinkClient.java` | WebSocket client + HLP handshake over a plain socket (plain Java) |
| `Pairing.java` | Parses the pairing link |
| `LinkService.java` | Foreground service: connection, sync, commands from the PC |
| `PhoneData.java` | Texts, call log, contacts, photo thumbnails |
| `NotifListener.java` | Notification mirroring |
| `MainActivity.java` | Pairing and permissions |
| `UnlockActivity.java` | Fingerprint prompt when the PC asks to unlock |

## Testing status

- **Tested on a JVM:** the protocol code (`Hlp`, `LinkClient`, `Pairing`), by
  `test/HlpTest.java`. It checks the RFC vectors and the shared interop vectors,
  and runs a live encrypted session with HydatekOS in QEMU, including a 3 MB
  file.
- **Tested against HydatekOS in QEMU with a scripted phone:** fingerprint unlock
  (`fake-phone.js` with `UNLOCK=approve|deny` and `FORGE=1`): an approval unlocks,
  a denial or an approval for a different request doesn't.
- **Checked statically:** the Android parts compile against the Android 14
  framework. The APK is signature-verified by apksig, and androguard parses its
  manifest and dex.
- **Not yet run on a phone or emulator.** The build environment has no Android
  emulator. The SMS, call, notification, photo and fingerprint (BiometricPrompt)
  code follows the platform APIs, but still needs a device test.
