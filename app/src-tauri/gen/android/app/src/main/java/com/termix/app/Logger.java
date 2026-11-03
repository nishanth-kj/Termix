package com.termix.app;

import android.text.TextUtils;
import android.util.Log;

public final class Logger {
    private static final String LOG_TAG_CORE = "Tauri";

    private Logger() {}

    public static String tags(String... subtags) {
        if (subtags.length > 0) {
            return LOG_TAG_CORE + "/" + TextUtils.join("/", subtags);
        }
        return LOG_TAG_CORE;
    }

    public static void verbose(String message) {
        verbose(LOG_TAG_CORE, message);
    }

    private static void verbose(String tag, String message) {
        if (shouldLog()) {
            Log.v(tag, message);
        }
    }

    public static void debug(String message) {
        debug(LOG_TAG_CORE, message);
    }

    public static void debug(String tag, String message) {
        if (shouldLog()) {
            Log.d(tag, message);
        }
    }

    public static void info(String message) {
        info(LOG_TAG_CORE, message);
    }

    public static void info(String tag, String message) {
        if (shouldLog()) {
            Log.i(tag, message);
        }
    }

    public static void warn(String message) {
        warn(LOG_TAG_CORE, message);
    }

    public static void warn(String tag, String message) {
        if (shouldLog()) {
            Log.w(tag, message);
        }
    }

    public static void error(String message) {
        error(LOG_TAG_CORE, message, null);
    }

    public static void error(String message, Throwable error) {
        error(LOG_TAG_CORE, message, error);
    }

    public static void error(String tag, String message, Throwable error) {
        if (shouldLog()) {
            Log.e(tag, message, error);
        }
    }

    private static boolean shouldLog() {
        return BuildConfig.DEBUG;
    }
}
