package org.hydatek.link;

import android.app.Activity;
import android.content.DialogInterface;
import android.content.Intent;
import android.hardware.biometrics.BiometricManager;
import android.hardware.biometrics.BiometricPrompt;
import android.os.Build;
import android.os.Bundle;
import android.os.CancellationSignal;

/**
 * Asks for the phone owner's fingerprint (Android's BiometricPrompt) when the
 * paired PC wants to unlock, and sends the answer back over the encrypted link.
 * The window is transparent: only the system's fingerprint sheet shows.
 */
public class UnlockActivity extends Activity {
    static final String EXTRA_ID = "id";
    static final String EXTRA_PC = "pc";

    private static volatile UnlockActivity current;

    private String id;
    private boolean answered;
    private CancellationSignal cancel;

    /** The phone has a fingerprint (or other strong biometric) set up. */
    static boolean supported(android.content.Context c) {
        if (Build.VERSION.SDK_INT < 29) return false;
        BiometricManager bm = c.getSystemService(BiometricManager.class);
        if (bm == null) return false;
        if (Build.VERSION.SDK_INT >= 30) {
            return bm.canAuthenticate(BiometricManager.Authenticators.BIOMETRIC_STRONG) == BiometricManager.BIOMETRIC_SUCCESS;
        }
        return bm.canAuthenticate() == BiometricManager.BIOMETRIC_SUCCESS;
    }

    /** The PC withdrew the request (it was unlocked another way, or gave up). */
    static void withdraw(final String id) {
        final UnlockActivity a = current;
        if (a == null || id == null || !id.equals(a.id)) return;
        a.runOnUiThread(new Runnable() {
            public void run() {
                a.answered = true;
                if (a.cancel != null) a.cancel.cancel();
                a.finish();
            }
        });
    }

    @Override
    protected void onCreate(Bundle state) {
        super.onCreate(state);
        if (Build.VERSION.SDK_INT >= 27) {
            setShowWhenLocked(true);
            setTurnScreenOn(true);
        }
        start(getIntent());
    }

    @Override
    protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        // a newer request replaces the one on screen
        if (!answered && id != null) LinkService.answerUnlock(id, false);
        if (cancel != null) cancel.cancel();
        start(intent);
    }

    private void start(Intent intent) {
        current = this;
        answered = false;
        id = intent.getStringExtra(EXTRA_ID);
        String pc = intent.getStringExtra(EXTRA_PC);
        if (id == null || Build.VERSION.SDK_INT < 28) {
            finish();
            return;
        }
        BiometricPrompt.Builder b = new BiometricPrompt.Builder(this)
                .setTitle("Unlock " + (pc == null || pc.isEmpty() ? "your PC" : pc))
                .setSubtitle("HydatekOS Link")
                .setDescription("Touch the fingerprint sensor to unlock your PC.")
                .setNegativeButton("Cancel", getMainExecutor(), new DialogInterface.OnClickListener() {
                    public void onClick(DialogInterface d, int which) {
                        answer(false);
                    }
                });
        if (Build.VERSION.SDK_INT >= 29) b.setConfirmationRequired(false);
        if (Build.VERSION.SDK_INT >= 30) b.setAllowedAuthenticators(BiometricManager.Authenticators.BIOMETRIC_STRONG);
        cancel = new CancellationSignal();
        b.build().authenticate(cancel, getMainExecutor(), new BiometricPrompt.AuthenticationCallback() {
            @Override
            public void onAuthenticationSucceeded(BiometricPrompt.AuthenticationResult r) {
                answer(true);
            }

            @Override
            public void onAuthenticationError(int code, CharSequence msg) {
                // cancelled, locked out, or no fingerprint enrolled
                answer(false);
            }
            // onAuthenticationFailed: a finger that didn't match; the prompt stays up
        });
    }

    private void answer(boolean ok) {
        if (answered) return;
        answered = true;
        LinkService.answerUnlock(id, ok);
        finish();
    }

    @Override
    protected void onDestroy() {
        if (!answered && id != null) LinkService.answerUnlock(id, false);
        answered = true;
        if (cancel != null) cancel.cancel();
        if (current == this) current = null;
        super.onDestroy();
    }
}
