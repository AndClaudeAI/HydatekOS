//! TLS: secure connections for the browser. Everything here is HydatekOS's
//! own code: hashes, ciphers, key exchange, signatures, certificates and the
//! TLS 1.3 / 1.2 handshake.

pub mod aes;
pub mod bignum;
pub mod ec;
pub mod rsa;
pub mod sha2;
pub mod x25519;
pub mod der;
pub mod x509;
pub mod client;
