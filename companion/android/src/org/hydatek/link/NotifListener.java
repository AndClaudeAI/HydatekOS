package org.hydatek.link;

import android.app.Notification;
import android.content.pm.ApplicationInfo;
import android.content.pm.PackageManager;
import android.os.Bundle;
import android.service.notification.NotificationListenerService;
import android.service.notification.StatusBarNotification;

import java.util.ArrayList;
import java.util.List;

/** Mirrors the phone's notifications to the PC (needs notification access). */
public class NotifListener extends NotificationListenerService {
    static volatile NotifListener instance;

    @Override
    public void onListenerConnected() {
        instance = this;
        LinkService.refreshDevice();
    }

    @Override
    public void onListenerDisconnected() {
        instance = null;
    }

    static boolean interesting(StatusBarNotification sbn, String self) {
        Notification n = sbn.getNotification();
        if (sbn.getPackageName().equals(self)) return false;
        if ((n.flags & (Notification.FLAG_ONGOING_EVENT | Notification.FLAG_GROUP_SUMMARY)) != 0) return false;
        Bundle ex = n.extras;
        return ex != null && (ex.getCharSequence(Notification.EXTRA_TITLE) != null || ex.getCharSequence(Notification.EXTRA_TEXT) != null);
    }

    Hlp.Msg toMsg(StatusBarNotification sbn, boolean live) {
        Bundle ex = sbn.getNotification().extras;
        CharSequence title = ex.getCharSequence(Notification.EXTRA_TITLE);
        CharSequence text = ex.getCharSequence(Notification.EXTRA_TEXT);
        String app = sbn.getPackageName();
        try {
            PackageManager pm = getPackageManager();
            ApplicationInfo ai = pm.getApplicationInfo(app, 0);
            app = pm.getApplicationLabel(ai).toString();
        } catch (PackageManager.NameNotFoundException ignored) {
            // fall back to the package name
        }
        return new Hlp.Msg("notif")
                .put("id", sbn.getKey())
                .put("app", app)
                .put("title", title == null ? "" : title.toString())
                .put("body", text == null ? "" : text.toString())
                .put("time", PhoneData.when(sbn.getPostTime()))
                .put("live", live ? "1" : "0");
    }

    @Override
    public void onNotificationPosted(StatusBarNotification sbn) {
        if (interesting(sbn, getPackageName())) LinkService.post(toMsg(sbn, true));
    }

    @Override
    public void onNotificationRemoved(StatusBarNotification sbn) {
        LinkService.post(new Hlp.Msg("notif_rm").put("id", sbn.getKey()));
    }

    /** Current notifications, oldest first. */
    List<Hlp.Msg> current() {
        List<Hlp.Msg> out = new ArrayList<Hlp.Msg>();
        try {
            StatusBarNotification[] all = getActiveNotifications();
            if (all == null) return out;
            for (StatusBarNotification sbn : all) {
                if (interesting(sbn, getPackageName())) out.add(toMsg(sbn, false));
            }
        } catch (RuntimeException ignored) {
            // listener not bound yet
        }
        return out;
    }

    void dismiss(String key) {
        try {
            cancelNotification(key);
        } catch (RuntimeException ignored) {
            // already gone
        }
    }
}
