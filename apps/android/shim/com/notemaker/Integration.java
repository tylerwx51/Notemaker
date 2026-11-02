package com.notemaker;

import android.app.Instrumentation;
import android.os.Bundle;

/** Manifest instrumentation; all behavior lives in Rust (integration.rs). */
public class Integration extends Instrumentation {
    static { Native.load(); }
    public Bundle arguments;
    @Override public void onCreate(Bundle arguments) { super.onCreate(arguments); this.arguments = arguments; Native.component(this, "onCreate", new Object[]{arguments}); }
    @Override public void onStart() { Native.component(this, "onStart", new Object[0]); }
}
