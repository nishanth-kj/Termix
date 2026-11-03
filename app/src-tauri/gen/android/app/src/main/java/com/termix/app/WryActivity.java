package com.termix.app;

import android.annotation.SuppressLint;
import android.content.Intent;
import android.content.pm.PackageInfo;
import android.os.Build;
import android.os.Bundle;
import android.webkit.WebView;
import androidx.activity.OnBackPressedCallback;
import androidx.activity.result.ActivityResult;
import androidx.activity.result.ActivityResultLauncher;
import androidx.activity.result.contract.ActivityResultContracts;
import androidx.appcompat.app.AppCompatActivity;
import androidx.lifecycle.DefaultLifecycleObserver;
import androidx.lifecycle.LifecycleOwner;
import androidx.lifecycle.ProcessLifecycleOwner;
import java.util.Map;
import java.util.Random;

public abstract class WryActivity extends AppCompatActivity {
    private static final String ACTIVITY_ID_KEY = "__wryActivityId";

    public interface PermissionCallback {
        void onResult(Boolean granted);
    }

    public interface ActivityCallback {
        void onResult(ActivityResult result);
    }

    public static final class WryLifecycleObserver implements DefaultLifecycleObserver {
        static final WryLifecycleObserver INSTANCE = new WryLifecycleObserver();

        private WryLifecycleObserver() {}

        @Override
        public void onCreate(LifecycleOwner owner) {
            Rust.onFirstActivityCreate();
            Rust.onFirstActivityCreateWry();
        }
    }

    private RustWebView webView;
    private ActivityResultLauncher<String[]> permissionLauncher;
    private ActivityResultLauncher<Intent> activityLauncher;
    private PermissionCallback permissionListener;
    private ActivityCallback activityListener;
    private int id;
    private final Random activityIds = new Random();

    public boolean getHandleBackNavigation() {
        return true;
    }

    public void onWebViewCreate(WebView created) {}

    public void requestPermissions(String[] permissions, PermissionCallback listener) {
        permissionListener = listener;
        permissionLauncher.launch(permissions);
    }

    public void launchActivityForResult(Intent intent, ActivityCallback listener) {
        activityListener = listener;
        activityLauncher.launch(intent);
    }

    public void setWebView(RustWebView created) {
        webView = created;
        if (getHandleBackNavigation()) {
            OnBackPressedCallback callback = new OnBackPressedCallback(true) {
                @Override
                public void handleOnBackPressed() {
                    if (webView == null) {
                        return;
                    }
                    if (webView.canGoBack()) {
                        webView.goBack();
                        return;
                    }
                    setEnabled(false);
                    WryActivity.this.onBackPressed();
                    setEnabled(true);
                }
            };
            getOnBackPressedDispatcher().addCallback(this, callback);
        }
        onWebViewCreate(created);
    }

    public int getId() {
        return id;
    }

    @SuppressLint({"WebViewApiAvailability", "ObsoleteSdkInt"})
    public String getVersion() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            PackageInfo current = WebView.getCurrentWebViewPackage();
            if (current != null && current.versionName != null) {
                return current.versionName;
            }
            return "";
        }

        String webViewPackage = "com.google.android.webview";
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.N) {
            webViewPackage = "com.android.chrome";
        }
        String version = packageVersion(webViewPackage);
        if (version != null) {
            return version;
        }
        version = packageVersion("com.android.webview");
        return version == null ? "" : version;
    }

    @SuppressWarnings("deprecation")
    private String packageVersion(String packageName) {
        try {
            PackageInfo info = getPackageManager().getPackageInfo(packageName, 0);
            return String.valueOf(info.versionName);
        } catch (Exception ex) {
            Logger.warn("Unable to get package info for '" + packageName + "'" + ex);
            return null;
        }
    }

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        Integer savedId = activityId(savedInstanceState);
        Integer intentId = activityId(getIntent().getExtras());
        if (savedId != null) {
            id = savedId;
        } else if (intentId != null) {
            id = intentId;
        } else {
            id = hashCode();
        }

        permissionLauncher = registerForActivityResult(
                new ActivityResultContracts.RequestMultiplePermissions(),
                this::onPermissionsResult);
        activityLauncher = registerForActivityResult(
                new ActivityResultContracts.StartActivityForResult(),
                result -> {
                    if (activityListener != null) {
                        activityListener.onResult(result);
                    }
                });

        Rust.onCreate(this);
        ProcessLifecycleOwner.get().getLifecycle().addObserver(WryLifecycleObserver.INSTANCE);
    }

    private void onPermissionsResult(Map<String, Boolean> granted) {
        if (permissionListener == null) {
            return;
        }
        boolean allGranted = true;
        for (Boolean value : granted.values()) {
            if (!Boolean.TRUE.equals(value)) {
                allGranted = false;
                break;
            }
        }
        permissionListener.onResult(allGranted);
    }

    private static Integer activityId(Bundle bundle) {
        if (bundle != null && bundle.containsKey(ACTIVITY_ID_KEY)) {
            return bundle.getInt(ACTIVITY_ID_KEY);
        }
        return null;
    }

    @Override
    protected void onStart() {
        super.onStart();
        Rust.onStart(this);
    }

    @Override
    public void onWindowFocusChanged(boolean hasFocus) {
        super.onWindowFocusChanged(hasFocus);
        Rust.onWindowFocusChanged(this, hasFocus);
    }

    @Override
    protected void onSaveInstanceState(Bundle outState) {
        super.onSaveInstanceState(outState);
        outState.putInt(ACTIVITY_ID_KEY, id);
    }

    @Override
    protected void onPause() {
        super.onPause();
        Rust.onPause(this);
        if (webView != null) {
            webView.onPause();
        }
    }

    @Override
    protected void onResume() {
        super.onResume();
        Rust.onResume(this);
        if (webView != null) {
            webView.onResume();
        }
    }

    @Override
    protected void onStop() {
        super.onStop();
        Rust.onStop(this);
    }

    @Override
    protected void onDestroy() {
        super.onDestroy();
        Rust.onDestroy(this);
        Rust.onWebviewDestroy(this, webView == null ? "" : webView.id);
    }

    @Override
    public void onLowMemory() {
        super.onLowMemory();
        Rust.onLowMemory();
    }

    @Override
    protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        Rust.onNewIntent(intent);
    }

    public Class<?> getAppClass(String name) throws ClassNotFoundException {
        return Class.forName(name);
    }

    public int startActivity(Class<?> cls) {
        Intent intent = new Intent(this, cls);
        int createdId = activityIds.nextInt();
        intent.putExtra(ACTIVITY_ID_KEY, createdId);
        startActivity(intent);
        return createdId;
    }
}
