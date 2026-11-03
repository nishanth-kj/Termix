package com.termix.app;

import android.webkit.JavascriptInterface;

public class Ipc {
    public final RustWebView webView;
    public final RustWebViewClient webViewClient;

    public Ipc(RustWebView webView, RustWebViewClient webViewClient) {
        this.webView = webView;
        this.webViewClient = webViewClient;
    }

    @JavascriptInterface
    public void postMessage(String message) {
        if (message != null) {
            Rust.ipc(webView.id, webViewClient.currentUrl, message);
        }
    }
}
