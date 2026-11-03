package com.termix.app;

import android.os.Bundle;
import androidx.activity.EdgeToEdge;

public class MainActivity extends TauriActivity {
    @Override
    protected void onCreate(Bundle savedInstanceState) {
        EdgeToEdge.enable(this);
        super.onCreate(savedInstanceState);
    }
}
