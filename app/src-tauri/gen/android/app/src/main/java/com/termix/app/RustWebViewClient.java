package com.termix.app;

import android.graphics.Bitmap;
import android.net.Uri;
import android.os.Handler;
import android.os.Looper;
import android.webkit.WebResourceError;
import android.webkit.WebResourceRequest;
import android.webkit.WebResourceResponse;
import android.webkit.WebView;
import android.webkit.WebViewClient;
import androidx.webkit.WebViewAssetLoader;
import java.io.InputStream;
import java.util.HashMap;
import java.util.Map;
import java.util.regex.Pattern;

public class RustWebViewClient extends WebViewClient {
    private static final Pattern SINGLE_RANGE_WITH_START = Pattern.compile("bytes=\\d+-\\d*");

    private final Map<String, Boolean> interceptedState = new HashMap<>();
    public String currentUrl = "about:blank";
    private Uri lastInterceptedUrl;
    private String pendingUrlRedirect;
    private final WebViewAssetLoader assetLoader;

    public RustWebViewClient(RustWebView webView, android.content.Context context) {
        String domain = Rust.assetLoaderDomain(webView.id);
        if (domain == null) {
            assetLoader = null;
        } else {
            assetLoader = new WebViewAssetLoader.Builder()
                    .setDomain(domain)
                    .addPathHandler("/", new WebViewAssetLoader.AssetsPathHandler(context))
                    .build();
        }
    }

    @Override
    public WebResourceResponse shouldInterceptRequest(WebView view, WebResourceRequest request) {
        if (pendingUrlRedirect != null) {
            String redirect = pendingUrlRedirect;
            new Handler(Looper.getMainLooper()).post(() -> view.loadUrl(redirect));
            pendingUrlRedirect = null;
            return null;
        }

        lastInterceptedUrl = request.getUrl();
        if (assetLoader != null) {
            return assetLoader.shouldInterceptRequest(request.getUrl());
        }

        RustWebView rustWebView = (RustWebView) view;
        WebResourceResponse response = Rust.handleRequest(
                rustWebView.id, request, rustWebView.isDocumentStartScriptEnabled);
        if (response != null) {
            Map<String, String> headers = response.getResponseHeaders();
            if (headers != null) {
                headers.put("Cache-Control", "no-store");
            } else {
                Map<String, String> created = new HashMap<>();
                created.put("Cache-Control", "no-store");
                response.setResponseHeaders(created);
            }
            wrapPartialContentBody(request, response);
        }
        interceptedState.put(request.getUrl().toString(), response != null);
        return response;
    }

    @Override
    public boolean shouldOverrideUrlLoading(WebView view, WebResourceRequest request) {
        return Rust.shouldOverride(((RustWebView) view).id, request.getUrl().toString());
    }

    @Override
    public void onPageStarted(WebView view, String url, Bitmap favicon) {
        currentUrl = url;
        RustWebView rustWebView = (RustWebView) view;
        if (Boolean.FALSE.equals(interceptedState.get(url)) && !rustWebView.isDocumentStartScriptEnabled) {
            for (String script : rustWebView.initScripts) {
                view.evaluateJavascript(script, null);
            }
        }
        Rust.onPageLoading(rustWebView.id, url);
    }

    @Override
    public void onPageFinished(WebView view, String url) {
        Rust.onPageLoaded(((RustWebView) view).id, url);
    }

    @Override
    public void onReceivedError(WebView view, WebResourceRequest request, WebResourceError error) {
        if (error.getErrorCode() == ERROR_CONNECT
                && request.isForMainFrame()
                && !request.getUrl().equals(lastInterceptedUrl)) {
            view.stopLoading();
            view.loadUrl(request.getUrl().toString());
            pendingUrlRedirect = request.getUrl().toString();
        } else {
            super.onReceivedError(view, request, error);
        }
    }

    private static void wrapPartialContentBody(WebResourceRequest request, WebResourceResponse response) {
        if (response.getStatusCode() != 206) {
            return;
        }
        String range = findHeader(request.getRequestHeaders(), "Range");
        if (range == null || !SINGLE_RANGE_WITH_START.matcher(range.trim()).matches()) {
            return;
        }
        Long bodyStart = parseContentRangeStart(findHeader(response.getResponseHeaders(), "Content-Range"));
        if (bodyStart == null) {
            return;
        }
        response.setData(new SkippedPrefixInputStream(bodyStart, response.getData()));
    }

    private static String findHeader(Map<String, String> headers, String name) {
        if (headers == null) {
            return null;
        }
        for (Map.Entry<String, String> entry : headers.entrySet()) {
            if (entry.getKey().equalsIgnoreCase(name)) {
                return entry.getValue();
            }
        }
        return null;
    }

    private static Long parseContentRangeStart(String contentRange) {
        if (contentRange == null || !contentRange.startsWith("bytes ")) {
            return null;
        }
        String rest = contentRange.substring("bytes ".length());
        int dash = rest.indexOf('-');
        if (dash <= 0) {
            return null;
        }
        try {
            return Long.parseLong(rest.substring(0, dash));
        } catch (NumberFormatException ex) {
            return null;
        }
    }

    static final class SkippedPrefixInputStream extends InputStream {
        private long prefixRemaining;
        private final InputStream body;

        SkippedPrefixInputStream(long prefixLength, InputStream body) {
            this.prefixRemaining = prefixLength;
            this.body = body;
        }

        @Override
        public int available() throws java.io.IOException {
            long total = prefixRemaining + body.available();
            if (total > Integer.MAX_VALUE) {
                return 0;
            }
            return (int) total;
        }

        @Override
        public long skip(long count) throws java.io.IOException {
            if (count <= 0) {
                return 0;
            }
            if (prefixRemaining == 0L) {
                return body.skip(count);
            }
            long skipped = Math.min(count, Math.min(prefixRemaining, Integer.MAX_VALUE));
            prefixRemaining -= skipped;
            return skipped;
        }

        @Override
        public int read() throws java.io.IOException {
            return body.read();
        }

        @Override
        public int read(byte[] buffer, int offset, int length) throws java.io.IOException {
            return body.read(buffer, offset, length);
        }

        @Override
        public void close() throws java.io.IOException {
            body.close();
        }
    }
}
