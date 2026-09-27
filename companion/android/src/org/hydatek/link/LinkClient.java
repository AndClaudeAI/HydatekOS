package org.hydatek.link;

import java.io.BufferedInputStream;
import java.io.ByteArrayOutputStream;
import java.io.EOFException;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.net.InetSocketAddress;
import java.net.Socket;
import java.nio.charset.StandardCharsets;
import java.security.SecureRandom;

/**
 * One connection to a HydatekOS PC: a minimal WebSocket client (RFC 6455) plus
 * the HLP handshake. Pure Java; the Android service and the JVM tests share it.
 * {@link #run()} blocks until the connection ends.
 */
public final class LinkClient implements Runnable {

    /** Events delivered on the connection's thread. */
    public interface Listener {
        void onConnected(LinkClient client, String desktopName);

        void onMessage(LinkClient client, Hlp.Msg msg);

        /** @param fatal true when retrying won't help (e.g. unknown pairing) */
        void onClosed(LinkClient client, String reason, boolean fatal);
    }

    private static final int MAX_MESSAGE = 48 << 20;

    private final Pairing pairing;
    private final Hlp.Msg device;
    private final Listener listener;
    private final SecureRandom random = new SecureRandom();
    private Socket socket;
    private OutputStream out;
    private Hlp.Session session;
    private volatile boolean closed;

    public LinkClient(Pairing pairing, Hlp.Msg device, Listener listener) {
        this.pairing = pairing;
        this.device = device;
        this.listener = listener;
    }

    public boolean isReady() {
        return session != null && !closed;
    }

    /** Encrypt and send; returns false if not connected. Thread-safe. */
    public boolean send(Hlp.Msg m) {
        Hlp.Session s = session;
        if (s == null || closed) return false;
        try {
            synchronized (this) {
                writeFrame(2, s.seal(m.encode()));
            }
            return true;
        } catch (IOException e) {
            close();
            return false;
        }
    }

    public void close() {
        closed = true;
        try {
            if (socket != null) socket.close();
        } catch (IOException ignored) {
            // already closed
        }
    }

    @Override
    public void run() {
        String reason = "Disconnected";
        boolean fatal = false;
        try {
            socket = new Socket();
            socket.connect(new InetSocketAddress(pairing.host, pairing.port), 8000);
            socket.setTcpNoDelay(true);
            socket.setSoTimeout(90_000);
            InputStream in = new BufferedInputStream(socket.getInputStream(), 65536);
            out = socket.getOutputStream();
            handshake(in);

            byte[] cn = new byte[16];
            random.nextBytes(cn);
            Hlp.Msg hello = new Hlp.Msg("hello").put("nonce", Hlp.b64url(cn)).put("pair", pairing.pairId);
            synchronized (this) {
                writeFrame(1, hello.encode());
            }
            while (!closed) {
                int[] op = new int[1];
                byte[] data = readMessage(in, op);
                if (op[0] == 1) {
                    Hlp.Msg m = Hlp.Msg.decode(data);
                    if (m.op.equals("welcome") && session == null) {
                        session = new Hlp.Session(pairing.key, cn, Hlp.unb64url(m.get("nonce")), true);
                        send(device); // the first encrypted message proves we hold the key
                        listener.onConnected(this, m.get("name"));
                    } else if (m.op.equals("error")) {
                        reason = m.get("reason");
                        fatal = true;
                        break;
                    }
                } else if (op[0] == 2 && session != null) {
                    byte[] p = session.open(data);
                    if (p == null) {
                        reason = "The PC's reply did not authenticate";
                        break;
                    }
                    Hlp.Msg m = Hlp.Msg.decode(p);
                    if (m.op.equals("ping")) {
                        send(new Hlp.Msg("pong"));
                    } else {
                        listener.onMessage(this, m);
                    }
                }
            }
        } catch (EOFException e) {
            reason = "The PC closed the connection";
        } catch (IOException e) {
            reason = e.getMessage() == null ? e.toString() : e.getMessage();
        } finally {
            close();
            session = null;
            listener.onClosed(this, reason, fatal);
        }
    }

    private void handshake(InputStream in) throws IOException {
        byte[] k = new byte[16];
        random.nextBytes(k);
        String key = java.util.Base64.getEncoder().encodeToString(k);
        String req = "GET /hlp HTTP/1.1\r\nHost: " + pairing.host + ":" + pairing.port
                + "\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: " + key
                + "\r\nSec-WebSocket-Version: 13\r\n\r\n";
        out.write(req.getBytes(StandardCharsets.US_ASCII));
        out.flush();
        ByteArrayOutputStream head = new ByteArrayOutputStream();
        int state = 0;
        while (state < 4) {
            int b = in.read();
            if (b < 0) throw new EOFException();
            head.write(b);
            if (head.size() > 8192) throw new IOException("Bad handshake");
            state = (b == (state % 2 == 0 ? '\r' : '\n')) ? state + 1 : (b == '\r' ? 1 : 0);
        }
        String resp = head.toString("US-ASCII");
        if (!resp.startsWith("HTTP/1.1 101")) throw new IOException("This PC doesn't offer Phone Link");
    }

    private void writeFrame(int op, byte[] payload) throws IOException {
        byte[] mask = new byte[4];
        random.nextBytes(mask);
        int n = payload.length;
        ByteArrayOutputStream h = new ByteArrayOutputStream(14);
        h.write(0x80 | op);
        if (n < 126) {
            h.write(0x80 | n);
        } else if (n < 65536) {
            h.write(0x80 | 126);
            h.write(n >>> 8);
            h.write(n);
        } else {
            h.write(0x80 | 127);
            for (int i = 7; i >= 0; i--) h.write(i >= 4 ? 0 : (n >>> (8 * i)) & 0xff);
        }
        h.write(mask, 0, 4);
        out.write(h.toByteArray());
        byte[] buf = new byte[Math.min(n, 64 * 1024)];
        for (int o = 0; o < n; o += buf.length) {
            int len = Math.min(buf.length, n - o);
            for (int i = 0; i < len; i++) buf[i] = (byte) (payload[o + i] ^ mask[(o + i) & 3]);
            out.write(buf, 0, len);
        }
        out.flush();
    }

    private static void readFully(InputStream in, byte[] b, int off, int len) throws IOException {
        while (len > 0) {
            int n = in.read(b, off, len);
            if (n < 0) throw new EOFException();
            off += n;
            len -= n;
        }
    }

    /** Reads one complete (possibly fragmented) message; control frames are handled here. */
    private byte[] readMessage(InputStream in, int[] opOut) throws IOException {
        ByteArrayOutputStream msg = new ByteArrayOutputStream();
        int msgOp = -1;
        while (true) {
            byte[] h = new byte[2];
            readFully(in, h, 0, 2);
            boolean fin = (h[0] & 0x80) != 0;
            int op = h[0] & 0x0f;
            long len = h[1] & 0x7f;
            if (len == 126) {
                byte[] e = new byte[2];
                readFully(in, e, 0, 2);
                len = (e[0] & 0xff) << 8 | (e[1] & 0xff);
            } else if (len == 127) {
                byte[] e = new byte[8];
                readFully(in, e, 0, 8);
                len = 0;
                for (byte x : e) len = len << 8 | (x & 0xff);
            }
            if (len > MAX_MESSAGE) throw new IOException("Message too large");
            byte[] p = new byte[(int) len];
            readFully(in, p, 0, p.length);
            if (op == 8) throw new EOFException();
            if (op == 9) {
                synchronized (this) {
                    writeFrame(10, p);
                }
                continue;
            }
            if (op == 10) continue;
            if (op != 0) msgOp = op;
            msg.write(p, 0, p.length);
            if (msg.size() > MAX_MESSAGE) throw new IOException("Message too large");
            if (fin) {
                opOut[0] = msgOp;
                return msg.toByteArray();
            }
        }
    }
}
