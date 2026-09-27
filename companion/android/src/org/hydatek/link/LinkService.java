package org.hydatek.link;

import android.Manifest;
import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.app.Service;
import android.content.BroadcastReceiver;
import android.content.ClipData;
import android.content.ClipboardManager;
import android.content.ContentValues;
import android.content.Context;
import android.content.Intent;
import android.content.IntentFilter;
import android.content.SharedPreferences;
import android.content.pm.ServiceInfo;
import android.database.ContentObserver;
import android.database.Cursor;
import android.net.Uri;
import android.os.BatteryManager;
import android.os.Build;
import android.os.Bundle;
import android.os.Environment;
import android.os.Handler;
import android.os.IBinder;
import android.os.Looper;
import android.provider.MediaStore;
import android.provider.Telephony;
import android.telecom.TelecomManager;
import android.telephony.SmsManager;
import android.telephony.TelephonyManager;

import java.io.File;
import java.io.FileOutputStream;
import java.io.OutputStream;
import java.util.ArrayList;
import java.util.List;

/**
 * Keeps the encrypted connection to the paired HydatekOS PC alive (as a
 * foreground service), syncs texts, calls, photos and notifications, and
 * carries out the PC's commands: send a text, place or end a call, share a
 * photo, dismiss a notification, receive text and files.
 */
public class LinkService extends Service implements LinkClient.Listener {
    static final String PREFS = "link";
    static final String CHANNEL = "link";
    static final String CHANNEL_EVENTS = "events";
    static final int NOTE_ID = 1;

    static volatile LinkService instance;
    static volatile String status = "Not paired";

    private final Handler main = new Handler(Looper.getMainLooper());
    private volatile boolean running;
    private volatile LinkClient client;
    private Thread worker;
    private long lastSmsId = -1;
    private int lastBattery = -1;
    private int eventId = 100;

    // ------------------------------------------------------------ static API

    static Pairing savedPairing(Context c) {
        return Pairing.parse(c.getSharedPreferences(PREFS, MODE_PRIVATE).getString("pair_url", null));
    }

    static void savePairing(Context c, Pairing p) {
        SharedPreferences.Editor e = c.getSharedPreferences(PREFS, MODE_PRIVATE).edit();
        if (p == null) e.remove("pair_url");
        else e.putString("pair_url", p.url);
        e.apply();
    }

    static void start(Context c) {
        Intent i = new Intent(c, LinkService.class);
        if (Build.VERSION.SDK_INT >= 26) c.startForegroundService(i);
        else c.startService(i);
    }

    static void stop(Context c) {
        c.stopService(new Intent(c, LinkService.class));
    }

    /** Send if connected (called from the notification listener). */
    static void post(Hlp.Msg m) {
        LinkService s = instance;
        if (s != null && s.client != null) s.client.send(m);
    }

    /** Re-announce capabilities, e.g. after notification access was granted. */
    static void refreshDevice() {
        LinkService s = instance;
        if (s != null && s.client != null && s.client.isReady()) {
            s.client.send(s.deviceMsg());
            NotifListener nl = NotifListener.instance;
            if (nl != null) for (Hlp.Msg m : nl.current()) s.client.send(m);
        }
    }

    // ------------------------------------------------------------ lifecycle

    @Override
    public IBinder onBind(Intent intent) {
        return null;
    }

    @Override
    public void onCreate() {
        super.onCreate();
        instance = this;
        NotificationManager nm = getSystemService(NotificationManager.class);
        nm.createNotificationChannel(new NotificationChannel(CHANNEL, "Connection", NotificationManager.IMPORTANCE_LOW));
        nm.createNotificationChannel(new NotificationChannel(CHANNEL_EVENTS, "From your PC", NotificationManager.IMPORTANCE_DEFAULT));
    }

    @Override
    public int onStartCommand(Intent intent, int flags, int startId) {
        Notification n = statusNotification("Connecting…");
        if (Build.VERSION.SDK_INT >= 29) {
            startForeground(NOTE_ID, n, ServiceInfo.FOREGROUND_SERVICE_TYPE_CONNECTED_DEVICE);
        } else {
            startForeground(NOTE_ID, n);
        }
        if (!running) {
            running = true;
            registerWatchers();
            worker = new Thread(new Runnable() {
                public void run() {
                    loop();
                }
            }, "hydatek-link");
            worker.start();
        }
        return START_STICKY;
    }

    @Override
    public void onDestroy() {
        running = false;
        LinkClient c = client;
        if (c != null) c.close();
        if (worker != null) worker.interrupt();
        unregisterWatchers();
        instance = null;
        status = "Stopped";
        super.onDestroy();
    }

    /** Connect, and reconnect with backoff until stopped. */
    private void loop() {
        long backoff = 2000;
        while (running) {
            Pairing p = savedPairing(this);
            if (p == null) {
                setStatus("Not paired");
                stopSelf();
                return;
            }
            setStatus("Connecting to " + p.host + "…");
            LinkClient c = new LinkClient(p, deviceMsg(), this);
            client = c;
            long started = System.currentTimeMillis();
            c.run();
            client = null;
            if (!running) return;
            if (System.currentTimeMillis() - started > 30_000) backoff = 2000;
            try {
                Thread.sleep(backoff);
            } catch (InterruptedException e) {
                return;
            }
            backoff = Math.min(backoff * 2, 30_000);
        }
    }

    Hlp.Msg deviceMsg() {
        String name = getSharedPreferences(PREFS, MODE_PRIVATE).getString("name", Build.MODEL);
        Hlp.Msg m = new Hlp.Msg("device").put("name", name).put("kind", "android").put("model", Build.MANUFACTURER + " " + Build.MODEL)
                .put("caps", PhoneData.caps(this));
        int[] b = battery();
        if (b[0] >= 0) m.put("battery", b[0]).put("charging", b[1]);
        return m;
    }

    private int[] battery() {
        Intent i = registerReceiver(null, new IntentFilter(Intent.ACTION_BATTERY_CHANGED));
        if (i == null) return new int[] {-1, 0};
        int level = i.getIntExtra(BatteryManager.EXTRA_LEVEL, -1);
        int scale = i.getIntExtra(BatteryManager.EXTRA_SCALE, 100);
        int plugged = i.getIntExtra(BatteryManager.EXTRA_PLUGGED, 0);
        return new int[] {level < 0 ? -1 : level * 100 / Math.max(scale, 1), plugged != 0 ? 1 : 0};
    }

    // ------------------------------------------------------------ LinkClient.Listener

    @Override
    public void onConnected(final LinkClient c, String desktopName) {
        setStatus("Connected to " + (desktopName.isEmpty() ? "your PC" : desktopName));
        // initial sync off the socket thread
        new Thread(new Runnable() {
            public void run() {
                List<Hlp.Msg> batch = new ArrayList<Hlp.Msg>();
                batch.addAll(PhoneData.texts(LinkService.this));
                batch.addAll(PhoneData.calls(LinkService.this, 30));
                NotifListener nl = NotifListener.instance;
                if (nl != null) batch.addAll(nl.current());
                for (Hlp.Msg m : batch) if (!c.send(m)) return;
                for (Hlp.Msg m : PhoneData.photos(LinkService.this, 24)) if (!c.send(m)) return;
                lastSmsId = latestSmsId();
            }
        }, "hydatek-sync").start();
    }

    @Override
    public void onMessage(LinkClient c, Hlp.Msg m) {
        try {
            handle(c, m);
        } catch (RuntimeException e) {
            event("Phone Link", "Couldn't do that: " + e.getMessage());
        }
    }

    @Override
    public void onClosed(LinkClient c, String reason, boolean fatal) {
        if (fatal) {
            savePairing(this, null);
            setStatus(reason);
            event("Phone Link", reason);
            running = false;
            stopSelf();
        } else if (running) {
            setStatus("Offline — " + reason + ". Retrying…");
        }
    }

    // ------------------------------------------------------------ commands from the PC

    private void handle(LinkClient c, Hlp.Msg m) {
        String op = m.op;
        if (op.equals("sms")) {
            if (!PhoneData.granted(this, Manifest.permission.SEND_SMS)) {
                event("Phone Link", "Allow HydatekOS Link to send texts");
                return;
            }
            String number = m.get("number");
            SmsManager sms = SmsManager.getDefault();
            ArrayList<String> parts = sms.divideMessage(m.get("text"));
            if (parts.size() > 1) sms.sendMultipartTextMessage(number, null, parts, null, null);
            else sms.sendTextMessage(number, null, m.get("text"), null, null);
        } else if (op.equals("dial")) {
            if (!PhoneData.granted(this, Manifest.permission.CALL_PHONE)) {
                event("Phone Link", "Allow HydatekOS Link to make calls");
                return;
            }
            TelecomManager tm = getSystemService(TelecomManager.class);
            tm.placeCall(Uri.fromParts("tel", m.get("number"), null), new Bundle());
        } else if (op.equals("hangup")) {
            if (Build.VERSION.SDK_INT >= 28 && PhoneData.granted(this, "android.permission.ANSWER_PHONE_CALLS")) {
                getSystemService(TelecomManager.class).endCall();
            }
        } else if (op.equals("get_photo")) {
            try {
                Hlp.Msg f = PhoneData.photoFile(this, Long.parseLong(m.get("id")));
                if (f != null) c.send(f);
            } catch (NumberFormatException ignored) {
                // not one of ours
            }
        } else if (op.equals("notif_dismiss")) {
            NotifListener nl = NotifListener.instance;
            if (nl != null) nl.dismiss(m.get("id"));
        } else if (op.equals("clip")) {
            final String text = m.get("text");
            main.post(new Runnable() {
                public void run() {
                    ClipboardManager cm = getSystemService(ClipboardManager.class);
                    cm.setPrimaryClip(ClipData.newPlainText("From your PC", text));
                }
            });
            event("Text from your PC (copied)", text);
        } else if (op.equals("file")) {
            String saved = saveDownload(m.get("name"), m.blob);
            event("Received from your PC", saved == null ? "Couldn't save " + m.get("name") : saved + " is in Downloads");
        }
    }

    private String saveDownload(String name, byte[] data) {
        String safe = name.replaceAll("[\\\\/:*?\"<>|]", "_");
        if (safe.isEmpty()) safe = "file";
        try {
            if (Build.VERSION.SDK_INT >= 29) {
                ContentValues v = new ContentValues();
                v.put(MediaStore.MediaColumns.DISPLAY_NAME, safe);
                v.put(MediaStore.MediaColumns.RELATIVE_PATH, Environment.DIRECTORY_DOWNLOADS);
                Uri uri = getContentResolver().insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI, v);
                if (uri == null) return null;
                OutputStream out = getContentResolver().openOutputStream(uri);
                try {
                    out.write(data);
                } finally {
                    out.close();
                }
            } else {
                File dir = Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DOWNLOADS);
                dir.mkdirs();
                FileOutputStream out = new FileOutputStream(new File(dir, safe));
                try {
                    out.write(data);
                } finally {
                    out.close();
                }
            }
            return safe;
        } catch (Exception e) {
            return null;
        }
    }

    // ------------------------------------------------------------ live updates

    private ContentObserver smsObserver;
    private BroadcastReceiver receiver;

    private long latestSmsId() {
        if (!PhoneData.granted(this, Manifest.permission.READ_SMS)) return -1;
        Cursor cur = null;
        try {
            cur = getContentResolver().query(Telephony.Sms.CONTENT_URI, new String[] {Telephony.Sms._ID}, null, null, Telephony.Sms._ID + " DESC");
            return cur != null && cur.moveToFirst() ? cur.getLong(0) : -1;
        } catch (RuntimeException e) {
            return -1;
        } finally {
            if (cur != null) cur.close();
        }
    }

    /** Push texts newer than the last one we saw. */
    private void pushNewTexts() {
        LinkClient c = client;
        if (c == null || !c.isReady() || lastSmsId < 0 || !PhoneData.granted(this, Manifest.permission.READ_SMS)) return;
        Cursor cur = null;
        try {
            cur = getContentResolver().query(Telephony.Sms.CONTENT_URI,
                    new String[] {Telephony.Sms._ID, Telephony.Sms.THREAD_ID, Telephony.Sms.ADDRESS, Telephony.Sms.TYPE, Telephony.Sms.DATE, Telephony.Sms.BODY},
                    Telephony.Sms._ID + " > ?", new String[] {Long.toString(lastSmsId)}, Telephony.Sms._ID + " ASC");
            while (cur != null && cur.moveToNext()) {
                lastSmsId = Math.max(lastSmsId, cur.getLong(0));
                int type = cur.getInt(3);
                if (type != Telephony.Sms.MESSAGE_TYPE_INBOX && type != Telephony.Sms.MESSAGE_TYPE_SENT) continue;
                c.send(PhoneData.smsMessage(this, cur.getLong(1), cur.getString(2), type, cur.getLong(4), cur.getString(5), true));
            }
        } catch (RuntimeException ignored) {
            // provider busy; next change will catch up
        } finally {
            if (cur != null) cur.close();
        }
    }

    private void registerWatchers() {
        smsObserver = new ContentObserver(main) {
            @Override
            public void onChange(boolean selfChange) {
                new Thread(new Runnable() {
                    public void run() {
                        pushNewTexts();
                    }
                }).start();
            }
        };
        try {
            getContentResolver().registerContentObserver(Telephony.Sms.CONTENT_URI, true, smsObserver);
        } catch (SecurityException ignored) {
            // no SMS permission
        }
        receiver = new BroadcastReceiver() {
            @Override
            public void onReceive(Context context, Intent intent) {
                String a = intent.getAction();
                if (Intent.ACTION_BATTERY_CHANGED.equals(a)) {
                    int[] b = battery();
                    if (b[0] != lastBattery) {
                        lastBattery = b[0];
                        post(new Hlp.Msg("battery").put("level", b[0]).put("charging", b[1]));
                    }
                } else if (TelephonyManager.ACTION_PHONE_STATE_CHANGED.equals(a)) {
                    String st = intent.getStringExtra(TelephonyManager.EXTRA_STATE);
                    String number = intent.getStringExtra(TelephonyManager.EXTRA_INCOMING_NUMBER);
                    String state = TelephonyManager.EXTRA_STATE_RINGING.equals(st) ? "ringing" : TelephonyManager.EXTRA_STATE_OFFHOOK.equals(st) ? "active" : "idle";
                    Hlp.Msg m = new Hlp.Msg("call_state").put("state", state);
                    if (number != null) m.put("number", number).put("name", PhoneData.contactName(LinkService.this, number));
                    post(m);
                    if (state.equals("idle")) {
                        new Thread(new Runnable() {
                            public void run() {
                                try {
                                    Thread.sleep(1500); // let the call log catch up
                                } catch (InterruptedException ignored) {
                                    return;
                                }
                                for (Hlp.Msg c : PhoneData.calls(LinkService.this, 1)) post(c);
                            }
                        }).start();
                    }
                }
            }
        };
        IntentFilter f = new IntentFilter();
        f.addAction(Intent.ACTION_BATTERY_CHANGED);
        f.addAction(TelephonyManager.ACTION_PHONE_STATE_CHANGED);
        if (Build.VERSION.SDK_INT >= 33) registerReceiver(receiver, f, Context.RECEIVER_EXPORTED);
        else registerReceiver(receiver, f);
    }

    private void unregisterWatchers() {
        if (smsObserver != null) getContentResolver().unregisterContentObserver(smsObserver);
        if (receiver != null) unregisterReceiver(receiver);
    }

    // ------------------------------------------------------------ notifications

    private Notification statusNotification(String text) {
        Intent open = new Intent(this, MainActivity.class);
        PendingIntent pi = PendingIntent.getActivity(this, 0, open, PendingIntent.FLAG_IMMUTABLE);
        return new Notification.Builder(this, CHANNEL)
                .setSmallIcon(android.R.drawable.stat_notify_sync)
                .setContentTitle("HydatekOS Link")
                .setContentText(text)
                .setContentIntent(pi)
                .setOngoing(true)
                .build();
    }

    private void setStatus(String s) {
        status = s;
        NotificationManager nm = getSystemService(NotificationManager.class);
        nm.notify(NOTE_ID, statusNotification(s));
        MainActivity.statusChanged();
    }

    private void event(String title, String text) {
        NotificationManager nm = getSystemService(NotificationManager.class);
        nm.notify(eventId++, new Notification.Builder(this, CHANNEL_EVENTS)
                .setSmallIcon(android.R.drawable.stat_sys_download_done)
                .setContentTitle(title)
                .setContentText(text)
                .setAutoCancel(true)
                .build());
    }
}
