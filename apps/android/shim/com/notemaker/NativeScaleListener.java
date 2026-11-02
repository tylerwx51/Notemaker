package com.notemaker;

import android.view.ScaleGestureDetector;

/** SimpleOnScaleGestureListener forwarding to a Rust callback. */
public final class NativeScaleListener extends ScaleGestureDetector.SimpleOnScaleGestureListener {
    static { Native.load(); }
    private final long handle;
    public NativeScaleListener(long handle) { this.handle = handle; }
    @Override protected void finalize() throws Throwable { try { Native.release(handle); } finally { super.finalize(); } }
    @Override public boolean onScale(ScaleGestureDetector d) { Object r = Native.invoke(handle, "onScale(ScaleGestureDetector)", new Object[]{d}); return r instanceof Boolean ? (Boolean) r : super.onScale(d); }
    @Override public boolean onScaleBegin(ScaleGestureDetector d) { Object r = Native.invoke(handle, "onScaleBegin(ScaleGestureDetector)", new Object[]{d}); return r instanceof Boolean ? (Boolean) r : super.onScaleBegin(d); }
    @Override public void onScaleEnd(ScaleGestureDetector d) { Native.invoke(handle, "onScaleEnd(ScaleGestureDetector)", new Object[]{d}); }
}
