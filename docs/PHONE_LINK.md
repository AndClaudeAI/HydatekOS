# Phone Link and the Hydatek Link Protocol (HLP/1)

Phone Link connects a HydatekOS PC to a phone on the same network, like
Windows' "Link to Windows". Everything on the PC side is HydatekOS code: the
network stack, the HTTP/WebSocket server, the cryptography and the UI.

![Pairing](screenshots/phone-link-pairing.png)

## Two ways to connect a phone

| | Browser companion | HydatekOS Link for Android |
|---|---|---|
| Install | Nothing: scan the QR code | Download from the PC (`http://<pc>:7743/app.apk`) |
| Phones | Any (Android, iPhone, tablets) | Android 8.0+ |
| Send photos and files to the PC | ✅ | ✅ (Phone Link › Photos, Files › My phone) |
| Send files and text from the PC | ✅ | ✅ (saved to Downloads, text copied) |
| Texts: read, reply | — | ✅ |
| Notifications, dismiss from the PC | — | ✅ (needs notification access) |
| Call log, place and end calls | — | ✅ (audio stays on the phone) |
| Battery | Chrome on Android | ✅ |
| Unlock the PC with the phone's fingerprint | — | ✅ (Android 10+, fingerprint set up) |
| Screen mirroring | — | Planned; the demo phone shows it today |

Browsers can't read SMS, notifications or calls, which is why the Android app
exists. There's also a **demo phone** (simulated inside HydatekOS) for trying
Phone Link without a device.

## Pairing

1. The PC and the phone must be on the same network. Milestone 1's PC side
   needs **wired Ethernet** through the firmware's network driver; Wi-Fi
   drivers are on the roadmap.
2. Phone Link shows a QR code for
   `http://<pc-ip>:7743/#k=<pairing key>&d=<pairing id>`.
3. Scanning it opens the browser companion. The page keeps the key in the
   browser's storage and removes it from the address bar. The URL *fragment*
   (`#…`) is never sent to the server, so the key doesn't cross the network.
4. On Android, **Pair the app** on that page hands the same link to the
   Android app (`hydatek://pair?u=…`). You can also paste the link into the app.

The pairing key is 32 random bytes from HydatekOS's RNG (RDRAND, the firmware
RNG and TSC jitter, mixed with SHA-256 and expanded with ChaCha20). **Unpair**
in Phone Link generates a new key, which locks out every previously paired
phone.

## Protocol

**Transport.** WebSocket at `ws://<pc>:7743/hlp` (RFC 6455). The same port serves
`GET /` (the companion page), `GET /hlp.js` and `GET /app.apk`. The PC also
advertises `hydatek-xxxx.local` and the DNS-SD service `_hydatek-link._tcp` over
mDNS.

**Handshake** (text frames):

```
phone → PC   hello\nnonce=<16 random bytes, base64url>\npair=<pairing id>
PC → phone   welcome\nnonce=<16 random bytes, base64url>\nname=<PC name>
```

Both sides then derive per-direction keys from the pairing key `K`:

```
salt = client_nonce || server_nonce
c2s  = HKDF-SHA256(salt, K, "hlp1 c2s")      phone → PC
s2c  = HKDF-SHA256(salt, K, "hlp1 s2c")      PC → phone
```

**Frames.** Every later message is a binary frame:

```
u64 counter (big-endian, starts at 0, +1 per message and direction)
ChaCha20-Poly1305(key, nonce = 00000000 || counter, aad = "hlp1", payload)
```

The phone's first encrypted message proves it holds `K`. Until then the PC
sends nothing but the handshake. Any frame that fails authentication, or
arrives with an unexpected counter (a replay), ends the session. Fresh nonces
on both sides give every connection new keys.

**Payload.** UTF-8 header lines (the operation, then `key=value` lines, with `\`
escaped as `\\` and newlines as `\n`), optionally followed by a blank line and
binary data.

### Operations

| From | Op | Fields / data |
|---|---|---|
| phone | `device` | `name`, `kind` (`android`/`web`), `caps` (`sms,notif,calls,photos,files,clip,bio`), `battery`, `charging`, `model` |
| phone | `battery` | `level`, `charging` |
| phone | `thread` | `id`, `name`, `number` |
| phone | `msg` | `thread`, `name`, `number`, `me` (0/1), `time`, `text`, `live` (1 = new, show a notification) |
| phone | `notif` / `notif_rm` | `id`, `app`, `title`, `body`, `time`, `live` / `id` |
| phone | `call` | `name`, `number`, `when`, `missed` |
| phone | `call_state` | `state` (`ringing`/`active`/`idle`), `name`, `number` |
| phone | `photo` | `id`, `name`, `w`, `h`, `size` + 64×64 RGB thumbnail |
| both | `file` | `name`, `size`, `mime` + file bytes (≤ 48 MB) |
| both | `clip` | `text` |
| PC | `welcome` | `name` (first encrypted message) |
| PC | `sms` | `thread`, `number`, `text` |
| PC | `dial` / `hangup` | `number` / — |
| PC | `get_photo` | `id` (phone answers with `file`) |
| PC | `notif_dismiss` | `id` |
| PC | `unlock_req` / `unlock_cancel` | `id` (random, per request), `name` (PC name) / `id` |
| phone | `unlock` | `id`, `ok` (1 = the owner's fingerprint matched) |
| both | `ping` / `pong` | keep-alive (the PC pings every 15 s and drops peers silent for 60 s) |

## Security notes

- Traffic is encrypted and authenticated with keys only the paired devices hold.
  The key travels only in the QR code (or a link you paste).
- The browser companion page itself is served over plain HTTP, because
  HydatekOS has no TLS certificates on a home network. An attacker who can
  *actively* tamper with your LAN could serve a modified page. **Pair only on
  networks you trust.** The Android app doesn't have this exposure; its code is
  installed once.
- **Fingerprint unlock.** The PC sends `unlock_req` with a fresh random id; the
  phone shows Android's fingerprint prompt (strong biometrics only) and answers
  `unlock`. The PC accepts only an answer carrying the id of its pending request,
  within 60 s, over the authenticated session, and only if fingerprint unlock is
  on in Settings. It's opt-in and needs a PIN or password as a fallback. Anyone who
  holds the paired, unlocked-by-fingerprint phone can unlock the PC; unpair a lost
  phone.
- HLP/1 uses a pre-shared key, not a Diffie–Hellman exchange, so it has no
  forward secrecy: someone who later learns `K` and recorded the traffic could
  decrypt it. Unpair to rotate `K`. Adding X25519 is planned for HLP/2.

## Source map

| Part | Where |
|---|---|
| Network stack (ARP, IPv4, DHCP, TCP, mDNS) | `kernel/src/net/` |
| Crypto, RNG, QR encoder | `kernel/src/crypto.rs`, `rng.rs`, `qr.rs` |
| HLP format and session | `kernel/src/hlp.rs` |
| HTTP/WebSocket server | `kernel/src/linksrv.rs` |
| Phone Link model and UI | `kernel/src/link.rs`, `kernel/src/apps/phonelink.rs` |
| Browser companion | `companion/web/` |
| Android app | `companion/android/` ([README](../companion/android/README.md)) |

## Tests

- `tools/test.sh` checks the kernel (Rust, run on the host), the browser
  companion (Node) and the Android protocol code (JVM). All three are checked
  against RFC 8439/5869/4231 test vectors and the same interop vectors
  (`companion/test-vectors.json`). QR codes are verified by decoding them with
  OpenCV.
- End to end, against HydatekOS running in QEMU:
  - `companion/web/test/fake-phone.js`: every operation in both directions, and
    6 MB transfers each way with matching SHA-256.
  - `companion/web/test/bad-phone.js`: a wrong key or wrong pairing id is
    rejected, and the real session is unaffected.
  - `companion/web/test/browser-e2e.js`: the companion page in headless
    Chromium, emulating a phone.
  - `HlpTest` with a pairing URL: the Android app's Java client.
