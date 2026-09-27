import org.hydatek.link.Hlp;
import org.hydatek.link.LinkClient;
import org.hydatek.link.Pairing;

import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Paths;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.TimeUnit;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

/**
 * JVM tests for the Android app's protocol code.
 *   java HlpTest <test-vectors.json> [pairing-url-to-a-running-HydatekOS]
 */
public class HlpTest {
    static void check(boolean ok, String what) {
        if (!ok) throw new AssertionError(what);
        System.out.println("ok  " + what);
    }

    static String field(String json, String name) {
        Matcher m = Pattern.compile("\"" + name + "\": \"([0-9a-f]*)\"").matcher(json);
        m.find();
        return m.group(1);
    }

    static String[] array(String json, String name) {
        Matcher m = Pattern.compile("\"" + name + "\": \\[([^\\]]*)\\]").matcher(json);
        m.find();
        return m.group(1).replace("\"", "").replace(" ", "").split(",");
    }

    public static void main(String[] args) throws Exception {
        byte[] key = new byte[32];
        for (int i = 0; i < 32; i++) key[i] = (byte) (0x80 + i);
        byte[] nonce = Hlp.unhex("070000004041424344454647");
        byte[] aad = Hlp.unhex("50515253c0c1c2c3c4c5c6c7");
        byte[] pt = "Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, sunscreen would be it.".getBytes(StandardCharsets.US_ASCII);
        byte[] s = Hlp.seal(key, nonce, aad, pt);
        check(Hlp.hex(s).startsWith("d31a8d34648e60db7b86afbc53ef7ec2"), "RFC 8439 AEAD ciphertext");
        check(Hlp.hex(s).endsWith("1ae10b594f09e26a7e902ecbd0600691"), "RFC 8439 AEAD tag");
        check(Hlp.open(key, nonce, aad, s, 0, s.length) != null, "AEAD open");
        s[3] ^= 1;
        check(Hlp.open(key, nonce, aad, s, 0, s.length) == null, "AEAD rejects tampering");

        String v = new String(Files.readAllBytes(Paths.get(args[0])), StandardCharsets.UTF_8);
        Hlp.Session c = new Hlp.Session(Hlp.unhex(field(v, "key")), Hlp.unhex(field(v, "client_nonce")), Hlp.unhex(field(v, "server_nonce")), true);
        String[] c2s = array(v, "c2s"), c2sPlain = array(v, "c2s_plain"), s2c = array(v, "s2c"), s2cPlain = array(v, "s2c_plain");
        for (int i = 0; i < c2s.length; i++) check(Hlp.hex(c.seal(Hlp.unhex(c2sPlain[i]))).equals(c2s[i]), "interop c2s frame " + i);
        for (int i = 0; i < s2c.length; i++) check(Hlp.hex(c.open(Hlp.unhex(s2c[i]))).equals(s2cPlain[i]), "interop s2c frame " + i);
        Hlp.Msg m = Hlp.Msg.decode(Hlp.unhex(c2sPlain[1]));
        check(m.op.equals("file") && m.blob.length == 256, "decode kernel-encoded message with blob");
        Hlp.Msg rt = Hlp.Msg.decode(new Hlp.Msg("msg").put("text", "a\nb\\c=d").blob(new byte[] {1, 10, 10, 2}).encode());
        check(rt.get("text").equals("a\nb\\c=d") && rt.blob.length == 4, "message round trip");

        Pairing p = Pairing.parse("http://192.168.1.5:7743/#k=AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8&d=ab12cd");
        check(p != null && p.host.equals("192.168.1.5") && p.port == 7743 && p.key[31] == 31 && p.pairId.equals("ab12cd"), "parse pairing link");
        check(Pairing.parse("hydatek://pair?u=" + java.net.URLEncoder.encode(p.url, "UTF-8")) != null, "parse hydatek:// wrapper");
        check(Pairing.parse("http://example.com/") == null, "reject link without key");

        if (args.length > 1) live(args[1]);
        System.out.println("HlpTest: all passed");
    }

    /** Full session against a running HydatekOS (host override via HLP_HOST). */
    static void live(String url) throws Exception {
        String host = System.getenv("HLP_HOST");
        if (host != null) url = url.replaceFirst("//[^/]+/", "//" + host + "/");
        Pairing p = Pairing.parse(url);
        CountDownLatch connected = new CountDownLatch(1), welcomed = new CountDownLatch(1), closed = new CountDownLatch(1);
        final String[] name = new String[1];
        Hlp.Msg device = new Hlp.Msg("device").put("name", "JVM test phone").put("kind", "android").put("caps", "sms,notif,calls,photos,files,clip").put("battery", "91");
        LinkClient client = new LinkClient(p, device, new LinkClient.Listener() {
            public void onConnected(LinkClient cl, String desktop) { name[0] = desktop; connected.countDown(); }
            public void onMessage(LinkClient cl, Hlp.Msg msg) { if (msg.op.equals("welcome")) welcomed.countDown(); }
            public void onClosed(LinkClient cl, String reason, boolean fatal) { System.out.println("closed: " + reason); closed.countDown(); }
        });
        new Thread(client).start();
        check(connected.await(10, TimeUnit.SECONDS), "live: handshake with " + name[0]);
        check(welcomed.await(10, TimeUnit.SECONDS), "live: PC accepted our key (encrypted welcome)");
        client.send(new Hlp.Msg("thread").put("id", "1").put("name", "JVM").put("number", "+1555"));
        client.send(new Hlp.Msg("msg").put("thread", "1").put("me", "0").put("time", "now").put("text", "Hello from the Android app's Java code").put("live", "1"));
        byte[] big = new byte[3 << 20];
        new java.util.Random(1).nextBytes(big);
        long t0 = System.currentTimeMillis();
        check(client.send(new Hlp.Msg("file").put("name", "jvm-test.bin").blob(big)), "live: send 3 MB file");
        System.out.println("    3 MB written in " + (System.currentTimeMillis() - t0) + " ms, sha256 " + Hlp.hex(Hlp.sha256(big)).substring(0, 16));
        Thread.sleep(3000);
        client.close();
        check(closed.await(5, TimeUnit.SECONDS), "live: clean close");
    }
}
