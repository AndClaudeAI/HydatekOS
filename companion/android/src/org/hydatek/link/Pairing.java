package org.hydatek.link;

import java.net.URI;
import java.net.URLDecoder;

/**
 * Pairing details from the link in a HydatekOS pairing QR code:
 * {@code http://<pc>:7743/#k=<32-byte key, base64url>&d=<pairing id>}.
 */
public final class Pairing {
    public final String host;
    public final int port;
    public final byte[] key;
    public final String pairId;
    public final String url;

    private Pairing(String host, int port, byte[] key, String pairId, String url) {
        this.host = host;
        this.port = port;
        this.key = key;
        this.pairId = pairId;
        this.url = url;
    }

    /** Parses a pairing link (or a hydatek://pair?u=<link> wrapper); null if invalid. */
    public static Pairing parse(String text) {
        if (text == null) return null;
        try {
            text = text.trim();
            if (text.startsWith("hydatek://")) {
                int i = text.indexOf("u=");
                if (i < 0) return null;
                text = URLDecoder.decode(text.substring(i + 2), "UTF-8");
            }
            URI u = new URI(text);
            String frag = u.getRawFragment();
            if (u.getHost() == null || frag == null) return null;
            String k = null, d = null;
            for (String part : frag.split("&")) {
                if (part.startsWith("k=")) k = part.substring(2);
                else if (part.startsWith("d=")) d = part.substring(2);
            }
            if (k == null || d == null) return null;
            byte[] key = Hlp.unb64url(k);
            if (key.length != 32) return null;
            return new Pairing(u.getHost(), u.getPort() > 0 ? u.getPort() : 7743, key, d, text);
        } catch (Exception e) {
            return null;
        }
    }
}
