package com.termix.app;

import android.annotation.SuppressLint;
import android.content.Context;
import android.webkit.CookieManager;
import android.webkit.WebView;
import androidx.webkit.WebViewCompat;
import androidx.webkit.WebViewFeature;
import java.util.Collections;
import java.util.Map;

@SuppressLint("SetJavaScriptEnabled")
public class RustWebView extends WebView {
    public final String[] initScripts;
    public final String id;
    public final boolean isDocumentStartScriptEnabled;

    @SuppressLint("RestrictedApi")
    public RustWebView(Context context, String[] initScripts, String id) {
        super(context);
        this.initScripts = initScripts;
        this.id = id;
        getSettings().setJavaScriptEnabled(true);
        getSettings().setDomStorageEnabled(true);
        getSettings().setGeolocationEnabled(true);
        getSettings().setDatabaseEnabled(true);
        getSettings().setMediaPlaybackRequiresUserGesture(false);
        getSettings().setJavaScriptCanOpenWindowsAutomatically(true);

        if (WebViewFeature.isFeatureSupported(WebViewFeature.DOCUMENT_START_SCRIPT)) {
            isDocumentStartScriptEnabled = true;
            for (String script : initScripts) {
                WebViewCompat.addDocumentStartJavaScript(this, script, Collections.singleton("*"));
            }
        } else {
            isDocumentStartScriptEnabled = false;
        }
    }

    public void loadUrlMainThread(String url) {
        post(() -> loadUrl(url));
    }

    public void loadUrlMainThread(String url, Map<String, String> additionalHttpHeaders) {
        post(() -> loadUrl(url, additionalHttpHeaders));
    }

    @Override
    public void loadUrl(String url) {
        if (!Rust.shouldOverride(id, url)) {
            super.loadUrl(url);
        }
    }

    @Override
    public void loadUrl(String url, Map<String, String> additionalHttpHeaders) {
        if (!Rust.shouldOverride(id, url)) {
            super.loadUrl(url, additionalHttpHeaders);
        }
    }

    public void loadHTMLMainThread(String html) {
        post(() -> loadDataWithBaseURL(null, html, "text/html", "UTF-8", null));
    }

    public void evalScript(int evalId, String script) {
        post(() -> evaluateJavascript(script, result -> Rust.onEval(id, evalId, result)));
    }

    public void clearAllBrowsingData() {
        try {
            getContext().deleteDatabase("webviewCache.db");
            getContext().deleteDatabase("webview.db");
            clearCache(true);
            clearHistory();
            clearFormData();
        } catch (Exception ex) {
            Logger.error("Unable to create temporary media capture file: " + ex.getMessage());
        }
    }

    public String getCookies(String url) {
        return CookieManager.getInstance().getCookie(url);
    }
}
