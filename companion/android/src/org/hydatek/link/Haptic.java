package org.hydatek.link;

/**
 * HydatekOS's haptic patterns ("ms:amp:gap,..." from kernel/src/haptics.rs)
 * as Android's vibration waveform: alternating off/on timings and their
 * amplitudes (VibrationEffect.createWaveform). Plain Java, so the JVM test
 * checks it.
 */
public final class Haptic {
    public final long[] timings;
    public final int[] amplitudes;

    private Haptic(long[] t, int[] a) {
        timings = t;
        amplitudes = a;
    }

    /** Null for anything that isn't a short, sane pattern. */
    public static Haptic parse(String s) {
        if (s == null || s.isEmpty()) return null;
        String[] parts = s.split(",");
        if (parts.length > 16) return null;
        // a leading 0 ms pause, then on, off for each pulse
        long[] t = new long[1 + parts.length * 2];
        int[] a = new int[t.length];
        int i = 1;
        for (String p : parts) {
            String[] f = p.split(":");
            if (f.length != 3) return null;
            int ms, amp, gap;
            try {
                ms = Integer.parseInt(f[0].trim());
                amp = Integer.parseInt(f[1].trim());
                gap = Integer.parseInt(f[2].trim());
            } catch (NumberFormatException e) {
                return null;
            }
            if (ms <= 0 || ms > 1000 || gap < 0 || gap > 1000 || amp < 1 || amp > 255) return null;
            t[i] = ms;
            a[i] = amp;
            t[i + 1] = gap;
            a[i + 1] = 0;
            i += 2;
        }
        return new Haptic(t, a);
    }
}
