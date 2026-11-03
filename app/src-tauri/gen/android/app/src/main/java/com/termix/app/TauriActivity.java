package com.termix.app;

import android.content.Intent;
import android.content.res.Configuration;
import android.os.Bundle;
import app.tauri.plugin.PluginManager;

public abstract class TauriActivity extends WryActivity {
    @Override
    public boolean getHandleBackNavigation() {
        return false;
    }

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        PluginManager.INSTANCE.onCreate(this);
    }

    public PluginManager getPluginManager() {
        return PluginManager.INSTANCE;
    }

    @Override
    public void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        PluginManager.INSTANCE.onNewIntent(intent);
    }

    @Override
    protected void onRestart() {
        super.onRestart();
        PluginManager.INSTANCE.onRestart(this);
    }

    @Override
    protected void onResume() {
        super.onResume();
        PluginManager.INSTANCE.onResume(this);
    }

    @Override
    protected void onPause() {
        super.onPause();
        PluginManager.INSTANCE.onPause(this);
    }

    @Override
    protected void onStop() {
        super.onStop();
        PluginManager.INSTANCE.onStop(this);
    }

    @Override
    protected void onDestroy() {
        super.onDestroy();
        PluginManager.INSTANCE.onDestroy(this);
    }

    @Override
    public void onConfigurationChanged(Configuration newConfig) {
        super.onConfigurationChanged(newConfig);
        PluginManager.INSTANCE.onConfigurationChanged(newConfig);
    }
}
