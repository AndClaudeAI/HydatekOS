package org.hydatek.link;

import android.Manifest;
import android.content.ContentResolver;
import android.content.ContentUris;
import android.content.Context;
import android.content.pm.PackageManager;
import android.database.Cursor;
import android.graphics.Bitmap;
import android.graphics.BitmapFactory;
import android.net.Uri;
import android.os.Build;
import android.provider.CallLog;
import android.provider.ContactsContract;
import android.provider.MediaStore;
import android.provider.Telephony;
import android.util.Size;

import java.io.ByteArrayOutputStream;
import java.io.InputStream;
import java.text.SimpleDateFormat;
import java.util.ArrayList;
import java.util.Calendar;
import java.util.HashMap;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;

/** Reads texts, calls and photos from the phone and turns them into HLP messages. */
final class PhoneData {
    private PhoneData() {}

    static final int MAX_THREADS = 25;
    static final int MSGS_PER_THREAD = 15;

    static boolean granted(Context c, String perm) {
        return c.checkSelfPermission(perm) == PackageManager.PERMISSION_GRANTED;
    }

    static boolean canReadPhotos(Context c) {
        return Build.VERSION.SDK_INT >= 33 ? granted(c, "android.permission.READ_MEDIA_IMAGES") : granted(c, Manifest.permission.READ_EXTERNAL_STORAGE);
    }

    /** Capabilities we can offer right now, given the granted permissions. */
    static String caps(Context c) {
        List<String> caps = new ArrayList<String>();
        if (granted(c, Manifest.permission.READ_SMS)) caps.add("sms");
        if (NotifListener.instance != null) caps.add("notif");
        if (granted(c, Manifest.permission.READ_CALL_LOG)) caps.add("calls");
        if (canReadPhotos(c)) caps.add("photos");
        caps.add("files");
        caps.add("clip");
        StringBuilder sb = new StringBuilder();
        for (String s : caps) sb.append(sb.length() == 0 ? "" : ",").append(s);
        return sb.toString();
    }

    static String when(long millis) {
        Calendar now = Calendar.getInstance();
        Calendar t = Calendar.getInstance();
        t.setTimeInMillis(millis);
        boolean today = now.get(Calendar.YEAR) == t.get(Calendar.YEAR) && now.get(Calendar.DAY_OF_YEAR) == t.get(Calendar.DAY_OF_YEAR);
        String pattern = today ? "HH:mm" : "d MMM, HH:mm";
        return new SimpleDateFormat(pattern, Locale.getDefault()).format(t.getTime());
    }

    private static final Map<String, String> nameCache = new HashMap<String, String>();

    static String contactName(Context c, String number) {
        if (number == null || number.isEmpty()) return "";
        if (!granted(c, Manifest.permission.READ_CONTACTS)) return number;
        synchronized (nameCache) {
            String cached = nameCache.get(number);
            if (cached != null) return cached;
        }
        String name = number;
        Uri uri = Uri.withAppendedPath(ContactsContract.PhoneLookup.CONTENT_FILTER_URI, Uri.encode(number));
        Cursor cur = null;
        try {
            cur = c.getContentResolver().query(uri, new String[] {ContactsContract.PhoneLookup.DISPLAY_NAME}, null, null, null);
            if (cur != null && cur.moveToFirst()) name = cur.getString(0);
        } catch (RuntimeException ignored) {
            // lookup failures just show the number
        } finally {
            if (cur != null) cur.close();
        }
        synchronized (nameCache) {
            nameCache.put(number, name);
        }
        return name;
    }

    /** One SMS as an HLP message. */
    static Hlp.Msg smsMessage(Context c, long thread, String address, int type, long date, String body, boolean live) {
        return new Hlp.Msg("msg")
                .put("thread", thread)
                .put("name", contactName(c, address))
                .put("number", address == null ? "" : address)
                .put("me", type == Telephony.Sms.MESSAGE_TYPE_INBOX ? "0" : "1")
                .put("time", when(date))
                .put("text", body == null ? "" : body)
                .put("live", live ? "1" : "0");
    }

    /** Recent conversations: thread records followed by their messages, oldest first. */
    static List<Hlp.Msg> texts(Context c) {
        List<Hlp.Msg> out = new ArrayList<Hlp.Msg>();
        if (!granted(c, Manifest.permission.READ_SMS)) return out;
        Map<Long, List<Object[]>> threads = new LinkedHashMap<Long, List<Object[]>>();
        Cursor cur = null;
        try {
            cur = c.getContentResolver().query(Telephony.Sms.CONTENT_URI,
                    new String[] {Telephony.Sms.THREAD_ID, Telephony.Sms.ADDRESS, Telephony.Sms.TYPE, Telephony.Sms.DATE, Telephony.Sms.BODY},
                    null, null, Telephony.Sms.DATE + " DESC");
            int scanned = 0;
            while (cur != null && cur.moveToNext() && scanned++ < 2000) {
                long t = cur.getLong(0);
                List<Object[]> list = threads.get(t);
                if (list == null) {
                    if (threads.size() >= MAX_THREADS) continue;
                    list = new ArrayList<Object[]>();
                    threads.put(t, list);
                }
                if (list.size() < MSGS_PER_THREAD) {
                    list.add(new Object[] {cur.getString(1), cur.getInt(2), cur.getLong(3), cur.getString(4)});
                }
            }
        } catch (RuntimeException ignored) {
            // provider unavailable
        } finally {
            if (cur != null) cur.close();
        }
        // newest conversation last so it ends up on top on the PC
        List<Long> ids = new ArrayList<Long>(threads.keySet());
        for (int i = ids.size() - 1; i >= 0; i--) {
            long t = ids.get(i);
            List<Object[]> msgs = threads.get(t);
            String address = (String) msgs.get(0)[0];
            out.add(new Hlp.Msg("thread").put("id", t).put("name", contactName(c, address)).put("number", address == null ? "" : address));
            for (int j = msgs.size() - 1; j >= 0; j--) {
                Object[] m = msgs.get(j);
                out.add(smsMessage(c, t, (String) m[0], (Integer) m[1], (Long) m[2], (String) m[3], false));
            }
        }
        return out;
    }

    /** Recent calls, newest first. */
    static List<Hlp.Msg> calls(Context c, int limit) {
        List<Hlp.Msg> out = new ArrayList<Hlp.Msg>();
        if (!granted(c, Manifest.permission.READ_CALL_LOG)) return out;
        Cursor cur = null;
        try {
            cur = c.getContentResolver().query(CallLog.Calls.CONTENT_URI,
                    new String[] {CallLog.Calls.NUMBER, CallLog.Calls.CACHED_NAME, CallLog.Calls.TYPE, CallLog.Calls.DATE},
                    null, null, CallLog.Calls.DATE + " DESC");
            while (cur != null && cur.moveToNext() && out.size() < limit) {
                String number = cur.getString(0);
                String name = cur.getString(1);
                int type = cur.getInt(2);
                out.add(new Hlp.Msg("call")
                        .put("name", name == null ? "" : name)
                        .put("number", number == null ? "" : number)
                        .put("when", when(cur.getLong(3)))
                        .put("missed", type == CallLog.Calls.MISSED_TYPE ? "1" : "0"));
            }
        } catch (RuntimeException ignored) {
            // provider unavailable
        } finally {
            if (cur != null) cur.close();
        }
        return out;
    }

    /** 64x64 center-cropped RGB thumbnail (the format the PC expects). */
    static byte[] rgb64(Bitmap src) {
        int s = Math.min(src.getWidth(), src.getHeight());
        Bitmap sq = Bitmap.createBitmap(src, (src.getWidth() - s) / 2, (src.getHeight() - s) / 2, s, s);
        Bitmap small = Bitmap.createScaledBitmap(sq, 64, 64, true);
        int[] px = new int[64 * 64];
        small.getPixels(px, 0, 64, 0, 0, 64, 64);
        byte[] rgb = new byte[64 * 64 * 3];
        for (int i = 0; i < px.length; i++) {
            rgb[3 * i] = (byte) (px[i] >> 16);
            rgb[3 * i + 1] = (byte) (px[i] >> 8);
            rgb[3 * i + 2] = (byte) px[i];
        }
        return rgb;
    }

    static Bitmap thumbnail(Context c, long id) {
        Uri uri = ContentUris.withAppendedId(MediaStore.Images.Media.EXTERNAL_CONTENT_URI, id);
        try {
            if (Build.VERSION.SDK_INT >= 29) {
                return c.getContentResolver().loadThumbnail(uri, new Size(128, 128), null);
            }
            BitmapFactory.Options o = new BitmapFactory.Options();
            o.inSampleSize = 16;
            InputStream in = c.getContentResolver().openInputStream(uri);
            try {
                return BitmapFactory.decodeStream(in, null, o);
            } finally {
                if (in != null) in.close();
            }
        } catch (Exception e) {
            return null;
        }
    }

    /** Recent photos with thumbnails, oldest first (newest ends on top on the PC). */
    static List<Hlp.Msg> photos(Context c, int limit) {
        List<Hlp.Msg> out = new ArrayList<Hlp.Msg>();
        if (!canReadPhotos(c)) return out;
        Cursor cur = null;
        try {
            cur = c.getContentResolver().query(MediaStore.Images.Media.EXTERNAL_CONTENT_URI,
                    new String[] {MediaStore.Images.Media._ID, MediaStore.Images.Media.DISPLAY_NAME, MediaStore.Images.Media.SIZE},
                    null, null, MediaStore.Images.Media.DATE_ADDED + " DESC");
            while (cur != null && cur.moveToNext() && out.size() < limit) {
                long id = cur.getLong(0);
                Bitmap b = thumbnail(c, id);
                if (b == null) continue;
                out.add(0, new Hlp.Msg("photo").put("id", id).put("name", cur.getString(1)).put("size", cur.getLong(2)).blob(rgb64(b)));
            }
        } catch (RuntimeException ignored) {
            // provider unavailable
        } finally {
            if (cur != null) cur.close();
        }
        return out;
    }

    /** The full photo as a `file` message, or null. */
    static Hlp.Msg photoFile(Context c, long id) {
        Uri uri = ContentUris.withAppendedId(MediaStore.Images.Media.EXTERNAL_CONTENT_URI, id);
        ContentResolver cr = c.getContentResolver();
        String name = "photo-" + id + ".jpg";
        Cursor cur = null;
        try {
            cur = cr.query(uri, new String[] {MediaStore.Images.Media.DISPLAY_NAME}, null, null, null);
            if (cur != null && cur.moveToFirst() && cur.getString(0) != null) name = cur.getString(0);
        } catch (RuntimeException ignored) {
            // keep the generated name
        } finally {
            if (cur != null) cur.close();
        }
        try {
            InputStream in = cr.openInputStream(uri);
            if (in == null) return null;
            ByteArrayOutputStream buf = new ByteArrayOutputStream();
            byte[] chunk = new byte[65536];
            int n;
            try {
                while ((n = in.read(chunk)) > 0) {
                    buf.write(chunk, 0, n);
                    if (buf.size() > (40 << 20)) return null;
                }
            } finally {
                in.close();
            }
            return new Hlp.Msg("file").put("name", name).put("size", buf.size()).blob(buf.toByteArray());
        } catch (Exception e) {
            return null;
        }
    }
}
