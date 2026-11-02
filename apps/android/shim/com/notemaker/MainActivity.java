package com.notemaker;

import android.app.Activity;
import android.content.Intent;
import android.os.Bundle;

/** Manifest activity; all behavior lives in Rust (main_activity.rs). */
public class MainActivity extends Activity {
    static { Native.load(); }
    /** Rust instance handle, owned by Rust; read by Rust through JNI. */
    public long handle;
    private Object event(String name, Object... args) { return Native.component(this, name, args); }
    @Override public void onCreate(Bundle b) { super.onCreate(b); event("onCreate", b); }
    @Override public void onDestroy() { event("onDestroy"); super.onDestroy(); }
    @Override protected void onStart() { super.onStart(); event("onStart"); }
    @Override protected void onStop() { event("onStop"); super.onStop(); }
    @Override protected void onActivityResult(int requestCode, int resultCode, Intent data) { super.onActivityResult(requestCode, resultCode, data); event("onActivityResult", requestCode, resultCode, data); }
}
