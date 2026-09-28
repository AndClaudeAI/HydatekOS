#!/usr/bin/env python3
"""Regenerates the test certificates: a private test authority that only
HydatekOS's tests trust. Needs the Python `cryptography` package."""
import datetime, ipaddress
from cryptography import x509
from cryptography.x509.oid import NameOID
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import ec, rsa, padding

now = datetime.datetime(2025, 1, 1, tzinfo=datetime.timezone.utc)
LONG = datetime.timedelta(days=3650 * 3)


def name(cn, org=None):
    attrs = ([x509.NameAttribute(NameOID.ORGANIZATION_NAME, org)] if org else []) + [x509.NameAttribute(NameOID.COMMON_NAME, cn)]
    return x509.Name(attrs)


def save(stem, key, cert):
    open(stem + ".key", "wb").write(key.private_bytes(serialization.Encoding.PEM, serialization.PrivateFormat.PKCS8, serialization.NoEncryption()))
    open(stem + ".pem", "wb").write(cert.public_bytes(serialization.Encoding.PEM))
    open(stem + ".der", "wb").write(cert.public_bytes(serialization.Encoding.DER))


def make(subject, key, issuer, issuer_key, ca, sans=(), start=now, end=now + LONG, alg=hashes.SHA256(), pss=False):
    b = (x509.CertificateBuilder().subject_name(subject).issuer_name(issuer).public_key(key.public_key())
         .serial_number(x509.random_serial_number()).not_valid_before(start).not_valid_after(end)
         .add_extension(x509.BasicConstraints(ca=ca, path_length=None), critical=True))
    if sans:
        b = b.add_extension(x509.SubjectAlternativeName(sans), critical=False)
    if pss:
        return b.sign(issuer_key, alg, rsa_padding=padding.PSS(mgf=padding.MGF1(alg), salt_length=alg.digest_size))
    return b.sign(issuer_key, alg)


dns = lambda n: x509.DNSName(n)
ip = lambda a: x509.IPAddress(ipaddress.ip_address(a))
LOCAL = [dns("localhost"), dns("*.test.hydatek"), ip("127.0.0.1"), ip("10.0.2.2")]

root_key = rsa.generate_private_key(65537, 3072)
root_name = name("HydatekOS Test Root", "HydatekOS Test")
save("root", root_key, make(root_name, root_key, root_name, root_key, True))

inter_key = ec.generate_private_key(ec.SECP384R1())
inter_name = name("HydatekOS Test Intermediate", "HydatekOS Test")
save("inter", inter_key, make(inter_name, inter_key, root_name, root_key, True, alg=hashes.SHA384(), pss=True))

k = ec.generate_private_key(ec.SECP256R1())
save("ec256", k, make(name("localhost"), k, inter_name, inter_key, False, LOCAL, alg=hashes.SHA384()))
k = rsa.generate_private_key(65537, 2048)
save("rsa2048", k, make(name("localhost"), k, root_name, root_key, False, LOCAL))
k = ec.generate_private_key(ec.SECP384R1())
save("ec384", k, make(name("localhost"), k, root_name, root_key, False, LOCAL, alg=hashes.SHA512()))
k = ec.generate_private_key(ec.SECP256R1())
old = datetime.datetime(2020, 1, 1, tzinfo=datetime.timezone.utc)
save("expired", k, make(name("localhost"), k, inter_name, inter_key, False, LOCAL, start=old, end=old + datetime.timedelta(days=365)))
k = ec.generate_private_key(ec.SECP256R1())
save("stranger", k, make(name("localhost"), k, name("localhost"), k, False, LOCAL))
for leaf in ("ec256", "expired"):
    open(leaf + "-chain.pem", "wb").write(open(leaf + ".pem", "rb").read() + open("inter.pem", "rb").read())
