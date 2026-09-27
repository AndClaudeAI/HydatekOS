#!/usr/bin/env python3
"""Write Android's binary XML (AXML) for AndroidManifest.xml.

The Android SDK's aapt2 normally does this; HydatekOS builds its companion app
without the SDK, so this module encodes the manifest directly. Only what a
manifest needs is supported: one namespace, elements and typed attributes.
"""
import struct

ANDROID = "http://schemas.android.com/apk/res/android"

# android:* attribute resource ids (from android.R.attr)
ATTR = {
    "theme": 0x01010000, "label": 0x01010001, "icon": 0x01010002, "name": 0x01010003,
    "permission": 0x01010006, "exported": 0x01010010, "launchMode": 0x0101001D,
    "excludeFromRecents": 0x01010017,
    "scheme": 0x01010027, "host": 0x01010028, "minSdkVersion": 0x0101020C,
    "versionCode": 0x0101021B, "versionName": 0x0101021C, "maxSdkVersion": 0x01010271,
    "targetSdkVersion": 0x01010270, "allowBackup": 0x01010280, "usesCleartextTraffic": 0x010104EC,
    "foregroundServiceType": 0x01010599,
}

T_REF, T_STRING, T_INT_DEC, T_INT_HEX, T_BOOL = 0x01, 0x03, 0x10, 0x11, 0x12


class Ref(int):
    """A reference to a resource id, e.g. @android:drawable/..."""


class Hex(int):
    """An integer flags value."""


def E(tag, attrs=None, *children):
    return (tag, attrs or {}, list(children))


def encode(root):
    # --- collect strings: android attribute names first (they need the resource map)
    android_attrs, strings = [], []

    def walk(el):
        tag, attrs, kids = el
        for k in attrs:
            if k.startswith("android:"):
                n = k[8:]
                if n not in android_attrs:
                    android_attrs.append(n)
        for c in kids:
            walk(c)

    walk(root)
    android_attrs.sort(key=lambda n: ATTR[n])
    strings.extend(android_attrs)

    def s(v):
        if v not in strings:
            strings.append(v)
        return strings.index(v)

    s("android")
    s(ANDROID)

    def pre(el):
        tag, attrs, kids = el
        s(tag)
        for k, v in attrs.items():
            if not k.startswith("android:"):
                s(k)
            if isinstance(v, str):
                s(v)
        for c in kids:
            pre(c)

    pre(root)

    body = bytearray()

    def chunk(ty, header, payload):
        return struct.pack("<HHI", ty, len(header) + 8, len(header) + 8 + len(payload)) + header + payload

    def node_header():
        return struct.pack("<II", 1, 0xFFFFFFFF)  # line number, comment

    body += chunk(0x0100, node_header(), struct.pack("<II", s("android"), s(ANDROID)))

    def emit(el):
        nonlocal body
        tag, attrs, kids = el
        items = []
        for k, v in attrs.items():
            if k.startswith("android:"):
                ns, name = s(ANDROID), s(k[8:])
                order = ATTR[k[8:]]
            else:
                ns, name = 0xFFFFFFFF, s(k)
                order = 0x7FFFFFFF
            if isinstance(v, bool):
                raw, ty, data = 0xFFFFFFFF, T_BOOL, 0xFFFFFFFF if v else 0
            elif isinstance(v, Ref):
                raw, ty, data = 0xFFFFFFFF, T_REF, int(v)
            elif isinstance(v, Hex):
                raw, ty, data = 0xFFFFFFFF, T_INT_HEX, int(v)
            elif isinstance(v, int):
                raw, ty, data = 0xFFFFFFFF, T_INT_DEC, v
            else:
                raw, ty, data = s(v), T_STRING, s(v)
            items.append((order, struct.pack("<IIIHBBI", ns, name, raw, 8, 0, ty, data)))
        items.sort(key=lambda i: i[0])
        ext = struct.pack("<IIHHHHHH", 0xFFFFFFFF, s(tag), 0x14, 0x14, len(items), 0, 0, 0)
        body += chunk(0x0102, node_header(), ext + b"".join(i[1] for i in items))
        for c in kids:
            emit(c)
        body += chunk(0x0103, node_header(), struct.pack("<II", 0xFFFFFFFF, s(tag)))

    emit(root)
    body += chunk(0x0101, node_header(), struct.pack("<II", s("android"), s(ANDROID)))

    # --- string pool (UTF-16)
    offsets, data = [], bytearray()
    for st in strings:
        offsets.append(len(data))
        u = st.encode("utf-16-le")
        data += struct.pack("<H", len(u) // 2) + u + b"\x00\x00"
    while len(data) % 4:
        data += b"\x00"
    pool_hdr = struct.pack("<IIIII", len(strings), 0, 0, 28 + 4 * len(strings), 0)
    pool = chunk(0x0001, pool_hdr, b"".join(struct.pack("<I", o) for o in offsets) + bytes(data))
    resmap = chunk(0x0180, b"", b"".join(struct.pack("<I", ATTR[n]) for n in android_attrs))
    content = pool + resmap + bytes(body)
    return struct.pack("<HHI", 0x0003, 8, 8 + len(content)) + content
