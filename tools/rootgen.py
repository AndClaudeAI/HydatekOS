#!/usr/bin/env python3
"""Build kernel/src/tls/roots.bin, HydatekOS's built-in list of trusted
certificate authorities, from Mozilla's CA list as shipped in the
ca-certificates package (/usr/share/ca-certificates/mozilla).

Each authority is stored as its subject name and public key (both DER),
each prefixed by a 2-byte big-endian length: all a trust anchor needs."""
import base64, glob, os, re, sys

SRC = sys.argv[1] if len(sys.argv) > 1 else "/usr/share/ca-certificates/mozilla"
OUT = os.path.join(os.path.dirname(__file__), "..", "kernel", "src", "tls", "roots.bin")

def tlv(d, p):
    tag = d[p]; l = d[p + 1]; q = p + 2
    if l & 0x80:
        n = l & 0x7f; l = int.from_bytes(d[q:q + n], "big"); q += n
    return tag, q, q + l  # tag, body start, end

def children(d, start, end):
    out = []; p = start
    while p < end:
        t, b, e = tlv(d, p); out.append((t, p, b, e)); p = e
    return out

def anchor(der):
    _, b, e = tlv(der, 0)
    tbs = children(der, b, e)[0]
    f = children(der, tbs[2], tbs[3])
    if f[0][0] == 0xa0:
        f = f[1:]
    # serial, sigalg, issuer, validity, subject, spki
    subject = der[f[4][1]:f[4][3]]
    spki = der[f[5][1]:f[5][3]]
    return subject, spki

out = bytearray(); n = 0
for path in sorted(glob.glob(os.path.join(SRC, "*.crt"))):
    pem = open(path).read()
    for m in re.finditer(r"-----BEGIN CERTIFICATE-----(.*?)-----END CERTIFICATE-----", pem, re.S):
        subject, spki = anchor(base64.b64decode(m.group(1)))
        for part in (subject, spki):
            out += len(part).to_bytes(2, "big") + part
        n += 1
open(OUT, "wb").write(out)
print(f"{n} authorities, {len(out)} bytes -> {os.path.normpath(OUT)}")
