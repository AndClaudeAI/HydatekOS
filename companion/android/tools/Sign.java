import com.android.apksig.ApkSigner;
import com.android.apksig.ApkVerifier;

import java.io.File;
import java.io.FileInputStream;
import java.security.KeyStore;
import java.security.PrivateKey;
import java.security.cert.X509Certificate;
import java.util.Collections;

/**
 * Signs an APK with APK Signature Scheme v2 (apksig), then verifies it.
 * v1 (JAR) signing is skipped: every supported device (Android 8+) verifies v2.
 */
public class Sign {
    public static void main(String[] a) throws Exception {
        File in = new File(a[0]), out = new File(a[1]), ks = new File(a[2]);
        char[] pass = a[3].toCharArray();
        KeyStore store = KeyStore.getInstance("PKCS12");
        try (FileInputStream f = new FileInputStream(ks)) {
            store.load(f, pass);
        }
        String alias = store.aliases().nextElement();
        PrivateKey key = (PrivateKey) store.getKey(alias, pass);
        X509Certificate cert = (X509Certificate) store.getCertificate(alias);
        ApkSigner.SignerConfig signer = new ApkSigner.SignerConfig.Builder("HYDATEK", key, Collections.singletonList(cert)).build();
        new ApkSigner.Builder(Collections.singletonList(signer))
                .setInputApk(in).setOutputApk(out).setMinSdkVersion(26)
                .setV1SigningEnabled(false).setV2SigningEnabled(true)
                .build().sign();
        ApkVerifier.Result r = new ApkVerifier.Builder(out).build().verify();
        if (!r.isVerified()) {
            for (Object e : r.getErrors()) System.err.println("error: " + e);
            throw new IllegalStateException("signature does not verify");
        }
        System.out.println("signed and verified: APK Signature Scheme v2 = " + r.isVerifiedUsingV2Scheme());
    }
}
