#!/usr/bin/env python3
"""AndroidManifest.xml for HydatekOS Link, written as binary XML. Usage: manifest.py OUT"""
import sys
from axml import E, Ref, Hex, encode

VERSION_CODE, VERSION_NAME = 1, "0.1"

# Framework resources (android.R): drawable/sym_action_chat, style/Theme.DeviceDefault.Light
ICON = Ref(0x0108008E)
THEME = Ref(0x0103012B)
FGS_CONNECTED_DEVICE = Hex(0x10)
SINGLE_TASK = 2

PERMISSIONS = [
    "INTERNET", "ACCESS_NETWORK_STATE", "CHANGE_NETWORK_STATE", "FOREGROUND_SERVICE",
    "FOREGROUND_SERVICE_CONNECTED_DEVICE", "POST_NOTIFICATIONS", "READ_SMS", "SEND_SMS", "RECEIVE_SMS",
    "READ_CONTACTS", "READ_CALL_LOG", "READ_PHONE_STATE", "CALL_PHONE", "ANSWER_PHONE_CALLS",
    "READ_MEDIA_IMAGES",
]


def perm(name, max_sdk=None):
    a = {"android:name": "android.permission." + name}
    if max_sdk:
        a["android:maxSdkVersion"] = max_sdk
    return E("uses-permission", a)


manifest = E(
    "manifest",
    {"package": "org.hydatek.link", "android:versionCode": VERSION_CODE, "android:versionName": VERSION_NAME},
    E("uses-sdk", {"android:minSdkVersion": 26, "android:targetSdkVersion": 34}),
    *[perm(p) for p in PERMISSIONS],
    perm("READ_EXTERNAL_STORAGE", 32),
    perm("WRITE_EXTERNAL_STORAGE", 28),
    E(
        "application",
        {"android:label": "HydatekOS Link", "android:icon": ICON, "android:theme": THEME,
         "android:allowBackup": False, "android:usesCleartextTraffic": True},
        E(
            "activity",
            {"android:name": "org.hydatek.link.MainActivity", "android:exported": True, "android:launchMode": SINGLE_TASK},
            E("intent-filter", {},
              E("action", {"android:name": "android.intent.action.MAIN"}),
              E("category", {"android:name": "android.intent.category.LAUNCHER"})),
            E("intent-filter", {},
              E("action", {"android:name": "android.intent.action.VIEW"}),
              E("category", {"android:name": "android.intent.category.DEFAULT"}),
              E("category", {"android:name": "android.intent.category.BROWSABLE"}),
              E("data", {"android:scheme": "hydatek", "android:host": "pair"})),
        ),
        E("service", {"android:name": "org.hydatek.link.LinkService", "android:exported": False,
                      "android:foregroundServiceType": FGS_CONNECTED_DEVICE}),
        E(
            "service",
            {"android:name": "org.hydatek.link.NotifListener", "android:exported": True,
             "android:label": "HydatekOS Link",
             "android:permission": "android.permission.BIND_NOTIFICATION_LISTENER_SERVICE"},
            E("intent-filter", {}, E("action", {"android:name": "android.service.notification.NotificationListenerService"})),
        ),
    ),
)

with open(sys.argv[1], "wb") as f:
    f.write(encode(manifest))
