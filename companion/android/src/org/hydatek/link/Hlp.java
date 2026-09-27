package org.hydatek.link;

import java.io.ByteArrayOutputStream;
import java.math.BigInteger;
import java.nio.charset.StandardCharsets;
import java.security.MessageDigest;
import java.util.ArrayList;
import java.util.List;

import javax.crypto.Mac;
import javax.crypto.spec.SecretKeySpec;

/**
 * Hydatek Link Protocol (HLP/1): message format, HKDF key derivation and the
 * ChaCha20-Poly1305 session. Plain Java so it runs (and is tested) on any JVM.
 * The wire format is specified in docs/PHONE_LINK.md.
 */
public final class Hlp {
    private Hlp() {}

    static final byte[] AAD = "hlp1".getBytes(StandardCharsets.US_ASCII);

    // ------------------------------------------------------------ messages

    /** One protocol message: an operation, key/value fields and an optional blob. */
    public static final class Msg {
        public final String op;
        public final List<String[]> fields = new ArrayList<String[]>();
        public byte[] blob = new byte[0];

        public Msg(String op) {
            this.op = op;
        }

        public Msg put(String k, String v) {
            fields.add(new String[] {k, v == null ? "" : v});
            return this;
        }

        public Msg put(String k, long v) {
            return put(k, Long.toString(v));
        }

        public Msg blob(byte[] b) {
            blob = b == null ? new byte[0] : b;
            return this;
        }

        public String get(String k) {
            for (String[] f : fields) {
                if (f[0].equals(k)) return f[1];
            }
            return "";
        }

        public byte[] encode() {
            StringBuilder sb = new StringBuilder(op);
            for (String[] f : fields) {
                sb.append('\n').append(f[0]).append('=').append(escape(f[1]));
            }
            byte[] head = sb.toString().getBytes(StandardCharsets.UTF_8);
            if (blob.length == 0) return head;
            ByteArrayOutputStream out = new ByteArrayOutputStream(head.length + 2 + blob.length);
            out.write(head, 0, head.length);
            out.write('\n');
            out.write('\n');
            out.write(blob, 0, blob.length);
            return out.toByteArray();
        }

        public static Msg decode(byte[] p) {
            int split = -1;
            for (int i = 0; i + 1 < p.length; i++) {
                if (p[i] == '\n' && p[i + 1] == '\n') {
                    split = i;
                    break;
                }
            }
            String head = new String(p, 0, split < 0 ? p.length : split, StandardCharsets.UTF_8);
            String[] lines = head.split("\n", -1);
            Msg m = new Msg(lines[0].trim());
            for (int i = 1; i < lines.length; i++) {
                int eq = lines[i].indexOf('=');
                if (eq > 0) m.put(lines[i].substring(0, eq), unescape(lines[i].substring(eq + 1)));
            }
            if (split >= 0) {
                byte[] b = new byte[p.length - split - 2];
                System.arraycopy(p, split + 2, b, 0, b.length);
                m.blob = b;
            }
            return m;
        }
    }

    static String escape(String v) {
        StringBuilder sb = new StringBuilder(v.length());
        for (int i = 0; i < v.length(); i++) {
            char c = v.charAt(i);
            if (c == '\\') sb.append("\\\\");
            else if (c == '\n') sb.append("\\n");
            else if (c != '\r') sb.append(c);
        }
        return sb.toString();
    }

    static String unescape(String v) {
        StringBuilder sb = new StringBuilder(v.length());
        for (int i = 0; i < v.length(); i++) {
            char c = v.charAt(i);
            if (c == '\\' && i + 1 < v.length()) {
                char n = v.charAt(++i);
                sb.append(n == 'n' ? '\n' : n);
            } else if (c != '\\') {
                sb.append(c);
            }
        }
        return sb.toString();
    }

    // ------------------------------------------------------------ HKDF

    static byte[] hmac(byte[] key, byte[]... parts) {
        try {
            Mac mac = Mac.getInstance("HmacSHA256");
            // an empty HMAC key is equivalent to a single zero byte (both pad to zeros)
            mac.init(new SecretKeySpec(key.length == 0 ? new byte[1] : key, "HmacSHA256"));
            for (byte[] p : parts) mac.update(p);
            return mac.doFinal();
        } catch (Exception e) {
            throw new IllegalStateException(e);
        }
    }

    static byte[] hkdf(byte[] salt, byte[] ikm, byte[] info, int len) {
        byte[] prk = hmac(salt, ikm);
        byte[] out = new byte[len];
        byte[] t = new byte[0];
        int pos = 0;
        for (int c = 1; pos < len; c++) {
            t = hmac(prk, t, info, new byte[] {(byte) c});
            int n = Math.min(32, len - pos);
            System.arraycopy(t, 0, out, pos, n);
            pos += n;
        }
        return out;
    }

    public static byte[] sha256(byte[] data) {
        try {
            return MessageDigest.getInstance("SHA-256").digest(data);
        } catch (Exception e) {
            throw new IllegalStateException(e);
        }
    }

    // ------------------------------------------------------------ ChaCha20

    private static int rotl(int v, int c) {
        return (v << c) | (v >>> (32 - c));
    }

    private static int le32(byte[] b, int o) {
        return (b[o] & 0xff) | (b[o + 1] & 0xff) << 8 | (b[o + 2] & 0xff) << 16 | (b[o + 3] & 0xff) << 24;
    }

    static void chachaBlock(byte[] key, int counter, byte[] nonce, byte[] out) {
        int[] s = new int[16];
        s[0] = 0x61707865;
        s[1] = 0x3320646e;
        s[2] = 0x79622d32;
        s[3] = 0x6b206574;
        for (int i = 0; i < 8; i++) s[4 + i] = le32(key, 4 * i);
        s[12] = counter;
        for (int i = 0; i < 3; i++) s[13 + i] = le32(nonce, 4 * i);
        int[] x = s.clone();
        for (int i = 0; i < 10; i++) {
            qr(x, 0, 4, 8, 12);
            qr(x, 1, 5, 9, 13);
            qr(x, 2, 6, 10, 14);
            qr(x, 3, 7, 11, 15);
            qr(x, 0, 5, 10, 15);
            qr(x, 1, 6, 11, 12);
            qr(x, 2, 7, 8, 13);
            qr(x, 3, 4, 9, 14);
        }
        for (int i = 0; i < 16; i++) {
            int v = x[i] + s[i];
            out[4 * i] = (byte) v;
            out[4 * i + 1] = (byte) (v >>> 8);
            out[4 * i + 2] = (byte) (v >>> 16);
            out[4 * i + 3] = (byte) (v >>> 24);
        }
    }

    private static void qr(int[] x, int a, int b, int c, int d) {
        x[a] += x[b];
        x[d] = rotl(x[d] ^ x[a], 16);
        x[c] += x[d];
        x[b] = rotl(x[b] ^ x[c], 12);
        x[a] += x[b];
        x[d] = rotl(x[d] ^ x[a], 8);
        x[c] += x[d];
        x[b] = rotl(x[b] ^ x[c], 7);
    }

    static byte[] chachaXor(byte[] key, int counter, byte[] nonce, byte[] data, int off, int len) {
        byte[] out = new byte[len];
        byte[] ks = new byte[64];
        for (int o = 0, c = counter; o < len; o += 64, c++) {
            chachaBlock(key, c, nonce, ks);
            int n = Math.min(64, len - o);
            for (int i = 0; i < n; i++) out[o + i] = (byte) (data[off + o + i] ^ ks[i]);
        }
        return out;
    }

    // ------------------------------------------------------------ Poly1305

    private static final BigInteger P1305 = BigInteger.ONE.shiftLeft(130).subtract(BigInteger.valueOf(5));
    private static final BigInteger CLAMP = new BigInteger("0ffffffc0ffffffc0ffffffc0fffffff", 16);
    private static final BigInteger MASK128 = BigInteger.ONE.shiftLeft(128).subtract(BigInteger.ONE);

    private static BigInteger le(byte[] b, int off, int len, boolean pad) {
        byte[] be = new byte[len + 2];
        for (int i = 0; i < len; i++) be[be.length - 1 - i] = b[off + i];
        if (pad) be[be.length - 1 - len] = 1;
        return new BigInteger(1, be);
    }

    static byte[] poly1305(byte[] key, byte[] msg) {
        BigInteger r = le(key, 0, 16, false).and(CLAMP);
        BigInteger s = le(key, 16, 16, false);
        BigInteger acc = BigInteger.ZERO;
        for (int o = 0; o < msg.length; o += 16) {
            int n = Math.min(16, msg.length - o);
            acc = acc.add(le(msg, o, n, true)).multiply(r).mod(P1305);
        }
        acc = acc.add(s).and(MASK128);
        byte[] out = new byte[16];
        byte[] be = acc.toByteArray();
        for (int i = 0; i < 16 && i < be.length; i++) out[i] = be[be.length - 1 - i];
        return out;
    }

    private static byte[] tag(byte[] key, byte[] nonce, byte[] aad, byte[] ct) {
        byte[] block = new byte[64];
        chachaBlock(key, 0, nonce, block);
        byte[] otk = new byte[32];
        System.arraycopy(block, 0, otk, 0, 32);
        int aadPad = (16 - aad.length % 16) % 16;
        int ctPad = (16 - ct.length % 16) % 16;
        byte[] mac = new byte[aad.length + aadPad + ct.length + ctPad + 16];
        System.arraycopy(aad, 0, mac, 0, aad.length);
        System.arraycopy(ct, 0, mac, aad.length + aadPad, ct.length);
        long al = aad.length, cl = ct.length;
        for (int i = 0; i < 8; i++) {
            mac[mac.length - 16 + i] = (byte) (al >>> (8 * i));
            mac[mac.length - 8 + i] = (byte) (cl >>> (8 * i));
        }
        return poly1305(otk, mac);
    }

    public static byte[] seal(byte[] key, byte[] nonce, byte[] aad, byte[] plain) {
        byte[] ct = chachaXor(key, 1, nonce, plain, 0, plain.length);
        byte[] t = tag(key, nonce, aad, ct);
        byte[] out = new byte[ct.length + 16];
        System.arraycopy(ct, 0, out, 0, ct.length);
        System.arraycopy(t, 0, out, ct.length, 16);
        return out;
    }

    /** Returns null if the frame does not authenticate. */
    public static byte[] open(byte[] key, byte[] nonce, byte[] aad, byte[] sealed, int off, int len) {
        if (len < 16) return null;
        byte[] ct = new byte[len - 16];
        System.arraycopy(sealed, off, ct, 0, ct.length);
        byte[] want = tag(key, nonce, aad, ct);
        int diff = 0;
        for (int i = 0; i < 16; i++) diff |= want[i] ^ sealed[off + ct.length + i];
        if (diff != 0) return null;
        return chachaXor(key, 1, nonce, ct, 0, ct.length);
    }

    // ------------------------------------------------------------ session

    /** Directional keys and counters for one connection. */
    public static final class Session {
        private final byte[] tx, rx;
        private long txCtr, rxCtr;

        public Session(byte[] key, byte[] clientNonce, byte[] serverNonce, boolean client) {
            byte[] salt = new byte[clientNonce.length + serverNonce.length];
            System.arraycopy(clientNonce, 0, salt, 0, clientNonce.length);
            System.arraycopy(serverNonce, 0, salt, clientNonce.length, serverNonce.length);
            byte[] c2s = hkdf(salt, key, "hlp1 c2s".getBytes(StandardCharsets.US_ASCII), 32);
            byte[] s2c = hkdf(salt, key, "hlp1 s2c".getBytes(StandardCharsets.US_ASCII), 32);
            tx = client ? c2s : s2c;
            rx = client ? s2c : c2s;
        }

        private static byte[] nonce(long ctr) {
            byte[] n = new byte[12];
            for (int i = 0; i < 8; i++) n[4 + i] = (byte) (ctr >>> (56 - 8 * i));
            return n;
        }

        public synchronized byte[] seal(byte[] plain) {
            long ctr = txCtr++;
            byte[] body = Hlp.seal(tx, nonce(ctr), AAD, plain);
            byte[] out = new byte[8 + body.length];
            for (int i = 0; i < 8; i++) out[i] = (byte) (ctr >>> (56 - 8 * i));
            System.arraycopy(body, 0, out, 8, body.length);
            return out;
        }

        /** Returns null for a forged, replayed or out-of-order frame. */
        public byte[] open(byte[] frame) {
            if (frame.length < 24) return null;
            long ctr = 0;
            for (int i = 0; i < 8; i++) ctr = ctr << 8 | (frame[i] & 0xff);
            if (ctr != rxCtr) return null;
            byte[] p = Hlp.open(rx, nonce(ctr), AAD, frame, 8, frame.length - 8);
            if (p != null) rxCtr++;
            return p;
        }
    }

    // ------------------------------------------------------------ helpers

    public static String b64url(byte[] b) {
        String s = java.util.Base64.getEncoder().encodeToString(b);
        return s.replace('+', '-').replace('/', '_').replace("=", "");
    }

    public static byte[] unb64url(String s) {
        return java.util.Base64.getDecoder().decode(padded(s.replace('-', '+').replace('_', '/')));
    }

    private static String padded(String s) {
        StringBuilder sb = new StringBuilder(s);
        while (sb.length() % 4 != 0) sb.append('=');
        return sb.toString();
    }

    public static String hex(byte[] b) {
        StringBuilder sb = new StringBuilder();
        for (byte x : b) sb.append(String.format("%02x", x & 0xff));
        return sb.toString();
    }

    public static byte[] unhex(String s) {
        byte[] out = new byte[s.length() / 2];
        for (int i = 0; i < out.length; i++) out[i] = (byte) Integer.parseInt(s.substring(2 * i, 2 * i + 2), 16);
        return out;
    }
}
