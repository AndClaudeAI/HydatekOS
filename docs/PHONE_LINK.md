# Phone Link and the Hydatek Link Protocol (HLP) — draft 0.1

Phone Link connects a HydatekOS desktop to a phone, the way "Link to Windows" / "Phone
Link" does on Windows. The phone can be HydatekOS Mobile or, later, a companion app on
Android.

## What ships in milestone 1

| Feature | Desktop side | Transport |
|---|---|---|
| Pairing flow (steps, 6-digit code, scannable code) | ✅ | virtual device |
| Messages: read, reply, unread badges, toasts | ✅ (Phone Link + Messages app) | virtual device |
| Notifications feed + toasts, Focus to silence | ✅ | virtual device |
| Photos: browse and copy to Pictures (also Files › My phone) | ✅ | virtual device |
| Calls: history, start and hang up | ✅ (UI; audio needs a phone transport) | virtual device |
| Screen mirroring with remote touch and keyboard | ✅ | in-process |

The **virtual device** is a HydatekOS Mobile instance running inside the desktop
(`link.rs` for its data, `shell/mobile.rs` for its screen). It lets the whole experience
be built and tested before HydatekOS has network drivers. All desktop code talks to the
`Link` model, so switching to a real transport means feeding that model from HLP frames
instead of the simulator.

## Protocol design

**Discovery.** The phone advertises `_hydatek-link._tcp` over mDNS on Wi-Fi, or an HLP
service UUID over Bluetooth LE.

**Transport.** TCP port 7743 on the LAN (preferred), or Bluetooth RFCOMM as a fallback.
Calls use Bluetooth HFP audio.

**Pairing.**
1. The desktop shows a code with `{desktop_id, x25519_public_key, 6-digit code}`.
2. The phone scans it, connects, and sends its own public key.
3. Both sides show the 6-digit code and the user confirms it (SAS).
4. The session key is HKDF(X25519(shared), code). Both sides store the peer key, so later
   reconnects happen automatically.

**Framing.** After the handshake, every frame is encrypted with ChaCha20-Poly1305:

```
u32  length (of everything after this field)
u8   channel
u32  sequence number
...  payload (UTF-8 "key=value" lines; binary for channel 5)
```

| Channel | Name | Direction | Payload |
|---|---|---|---|
| 0 | control | both | `hello`, `battery=82`, `device=…`, `ping` |
| 1 | messages | both | `thread=…`, `from=…`, `time=…`, `text=…` / `send` |
| 2 | notifications | phone → desktop | `app=…`, `title=…`, `body=…`, `time=…`, `dismiss` |
| 3 | photos | both | list (id, name, size); `get id` → chunks |
| 4 | calls | both | log entries; `dial number`, `hangup`, call state |
| 5 | screen | both | phone → desktop: dirty-rect tiles (RGB565 + RLE); desktop → phone: pointer/key events in phone coordinates (390x844 space) |

## Implementation plan

1. **M3:** network drivers (virtio-net, Intel e1000/i219, and a Realtek RTL8111 target),
   IPv4/DHCP/DNS/TCP, and mDNS. HLP runs over TCP.
2. **M4:** a Bluetooth HCI over USB (xHCI) driver and RFCOMM. HFP audio for calls
   (with the HDA audio driver).
3. **Companion app:** an Android app that implements HLP, plus HydatekOS Mobile's own
   HLP service.
