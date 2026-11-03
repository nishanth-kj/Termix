package com.termix.app;

import android.content.Intent;
import android.webkit.WebResourceRequest;
import android.webkit.WebResourceResponse;

public final class Rust {
    static {
        System.loadLibrary("termix_app");
    }

    private Rust() {}

    public static native void onFirstActivityCreate();

    public static native void onCreate(WryActivity activity);

    public static native void onStart(WryActivity activity);

    public static native void onResume(WryActivity activity);

    public static native void onPause(WryActivity activity);

    public static native void onStop(WryActivity activity);

    public static native void onDestroy(WryActivity activity);

    public static native void onWindowFocusChanged(WryActivity activity, boolean focus);

    public static native void onLowMemory();

    public static native void onNewIntent(Intent intent);

    public static native void onFirstActivityCreateWry();

    public static native void onWebviewDestroy(WryActivity activity, String webviewId);

    public static native void ipc(String webviewId, String url, String message);

    public static native String assetLoaderDomain(String webviewId);

    public static native WebResourceResponse handleRequest(
            String webviewId, WebResourceRequest request, boolean isDocumentStartScriptEnabled);

    public static native boolean shouldOverride(String webviewId, String url);

    public static native void onPageLoading(String webviewId, String url);

    public static native void onPageLoaded(String webviewId, String url);

    public static native void onEval(String webviewId, int id, String result);

    public static native void handleReceivedTitle(String webviewId, String title);
}
