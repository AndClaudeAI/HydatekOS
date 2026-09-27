package org.hydatek.link;

import android.Manifest;
import android.app.Activity;
import android.content.Intent;
import android.graphics.Color;
import android.graphics.Typeface;
import android.graphics.drawable.GradientDrawable;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.provider.Settings;
import android.text.InputType;
import android.util.TypedValue;
import android.view.Gravity;
import android.view.View;
import android.widget.Button;
import android.widget.EditText;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.TextView;
import android.widget.Toast;

import java.util.ArrayList;
import java.util.List;

/**
 * Pairing and permissions. The pairing link arrives from the browser
 * companion ("Pair the app" opens hydatek://pair?u=...) or can be pasted.
 */
public class MainActivity extends Activity {
    private static volatile MainActivity visible;
    private static final int BG = 0xFFF0E9DE, SURFACE = 0xFFF9F6F0, TEXT = 0xFF1E1B2C, TEXT2 = 0xFF5E5866, ACCENT = 0xFFB5581B;

    private TextView status;
    private LinearLayout pairedBox, unpairedBox;
    private EditText linkField;

    static boolean isVisible() {
        return visible != null;
    }

    static void statusChanged() {
        final MainActivity a = visible;
        if (a != null) {
            a.runOnUiThread(new Runnable() {
                public void run() {
                    a.refresh();
                }
            });
        }
    }

    private int dp(int v) {
        return (int) TypedValue.applyDimension(TypedValue.COMPLEX_UNIT_DIP, v, getResources().getDisplayMetrics());
    }

    private TextView text(String s, int sp, int color, boolean bold) {
        TextView t = new TextView(this);
        t.setText(s);
        t.setTextSize(sp);
        t.setTextColor(color);
        if (bold) t.setTypeface(Typeface.DEFAULT_BOLD);
        t.setPadding(0, dp(4), 0, dp(4));
        return t;
    }

    private Button button(String label, boolean primary, View.OnClickListener l) {
        Button b = new Button(this);
        b.setText(label);
        b.setAllCaps(false);
        b.setTextColor(primary ? Color.WHITE : TEXT);
        GradientDrawable bg = new GradientDrawable();
        bg.setCornerRadius(dp(12));
        bg.setColor(primary ? ACCENT : 0xFFEAE3D7);
        b.setBackground(bg);
        LinearLayout.LayoutParams lp = new LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, dp(48));
        lp.topMargin = dp(8);
        b.setLayoutParams(lp);
        b.setOnClickListener(l);
        return b;
    }

    private LinearLayout card() {
        LinearLayout c = new LinearLayout(this);
        c.setOrientation(LinearLayout.VERTICAL);
        c.setPadding(dp(18), dp(16), dp(18), dp(18));
        GradientDrawable bg = new GradientDrawable();
        bg.setCornerRadius(dp(20));
        bg.setColor(SURFACE);
        c.setBackground(bg);
        LinearLayout.LayoutParams lp = new LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, LinearLayout.LayoutParams.WRAP_CONTENT);
        lp.topMargin = dp(12);
        c.setLayoutParams(lp);
        return c;
    }

    @Override
    protected void onCreate(Bundle saved) {
        super.onCreate(saved);
        LinearLayout root = new LinearLayout(this);
        root.setOrientation(LinearLayout.VERTICAL);
        root.setPadding(dp(18), dp(24), dp(18), dp(32));
        root.setBackgroundColor(BG);

        root.addView(text("HydatekOS Link", 26, TEXT, true));
        status = text("", 14, TEXT2, false);
        root.addView(status);

        unpairedBox = card();
        unpairedBox.addView(text("Pair with your PC", 18, TEXT, true));
        unpairedBox.addView(text("On your HydatekOS PC, open Phone Link and scan the code with your camera. "
                + "On the page that opens, tap “Pair the app”. Or paste the pairing link here:", 14, TEXT2, false));
        linkField = new EditText(this);
        linkField.setHint("http://…:7743/#k=…");
        linkField.setInputType(InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_VARIATION_URI);
        unpairedBox.addView(linkField);
        unpairedBox.addView(button("Pair", true, new View.OnClickListener() {
            public void onClick(View v) {
                pair(linkField.getText().toString());
            }
        }));
        root.addView(unpairedBox);

        pairedBox = card();
        pairedBox.addView(text("Let your PC see", 18, TEXT, true));
        pairedBox.addView(text("Texts, contacts, calls and photos need permission. Notifications need notification access.", 14, TEXT2, false));
        pairedBox.addView(button("Allow texts, calls and photos", true, new View.OnClickListener() {
            public void onClick(View v) {
                askPermissions();
            }
        }));
        pairedBox.addView(button("Allow notification access", false, new View.OnClickListener() {
            public void onClick(View v) {
                startActivity(new Intent(Settings.ACTION_NOTIFICATION_LISTENER_SETTINGS));
            }
        }));
        if (Build.VERSION.SDK_INT >= 33) {
            pairedBox.addView(text("If Android says a setting is restricted, open App info › ⋮ › Allow restricted settings, then try again.", 13, TEXT2, false));
        }
        pairedBox.addView(button("Unpair", false, new View.OnClickListener() {
            public void onClick(View v) {
                LinkService.savePairing(MainActivity.this, null);
                LinkService.stop(MainActivity.this);
                LinkService.status = "Not paired";
                refresh();
            }
        }));
        root.addView(pairedBox);

        ScrollView sv = new ScrollView(this);
        sv.setBackgroundColor(BG);
        sv.addView(root);
        setContentView(sv);
        handleIntent(getIntent());
    }

    @Override
    protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        handleIntent(intent);
    }

    private void handleIntent(Intent i) {
        Uri data = i == null ? null : i.getData();
        if (data != null && "hydatek".equals(data.getScheme())) pair(data.toString());
    }

    private void pair(String link) {
        Pairing p = Pairing.parse(link);
        if (p == null) {
            Toast.makeText(this, "That isn't a HydatekOS pairing link", Toast.LENGTH_LONG).show();
            return;
        }
        LinkService.savePairing(this, p);
        LinkService.stop(this);
        LinkService.start(this);
        askPermissions();
        refresh();
    }

    private void askPermissions() {
        List<String> want = new ArrayList<String>();
        String[] base = {Manifest.permission.READ_SMS, Manifest.permission.SEND_SMS, Manifest.permission.RECEIVE_SMS,
                Manifest.permission.READ_CONTACTS, Manifest.permission.READ_CALL_LOG, Manifest.permission.READ_PHONE_STATE,
                Manifest.permission.CALL_PHONE};
        for (String p : base) want.add(p);
        if (Build.VERSION.SDK_INT >= 28) want.add("android.permission.ANSWER_PHONE_CALLS");
        if (Build.VERSION.SDK_INT >= 33) {
            want.add("android.permission.READ_MEDIA_IMAGES");
            want.add("android.permission.POST_NOTIFICATIONS");
        } else {
            want.add(Manifest.permission.READ_EXTERNAL_STORAGE);
            if (Build.VERSION.SDK_INT < 29) want.add(Manifest.permission.WRITE_EXTERNAL_STORAGE);
        }
        List<String> missing = new ArrayList<String>();
        for (String p : want) if (checkSelfPermission(p) != android.content.pm.PackageManager.PERMISSION_GRANTED) missing.add(p);
        if (!missing.isEmpty()) requestPermissions(missing.toArray(new String[0]), 1);
    }

    @Override
    public void onRequestPermissionsResult(int code, String[] perms, int[] results) {
        LinkService.refreshDevice();
    }

    @Override
    protected void onResume() {
        super.onResume();
        visible = this;
        if (LinkService.savedPairing(this) != null && LinkService.instance == null) LinkService.start(this);
        LinkService.refreshDevice();
        refresh();
    }

    @Override
    protected void onPause() {
        visible = null;
        super.onPause();
    }

    void refresh() {
        Pairing p = LinkService.savedPairing(this);
        boolean paired = p != null;
        unpairedBox.setVisibility(paired ? View.GONE : View.VISIBLE);
        pairedBox.setVisibility(paired ? View.VISIBLE : View.GONE);
        String s = paired ? LinkService.status : "Not paired";
        if (paired && NotifListener.instance == null) s += "\nNotification access: off";
        status.setText(s);
        status.setGravity(Gravity.START);
    }
}
