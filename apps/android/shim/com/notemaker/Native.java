package com.notemaker;

/** Entry points into libnotemaker.so. The shim holds no application logic. */
final class Native {
    private Native() {}
    static { System.loadLibrary("notemaker"); }
    static void load() {}
    /** Invokes Rust callback {@code handle} for {@code method} (a key such as "onClick(View)"). */
    static native Object invoke(long handle, String method, Object[] args);
    static native void release(long handle);
    /** Lifecycle dispatch for manifest components; returns a boxed result or null. */
    static native Object component(Object component, String event, Object[] args);
}
