package com.notemaker;

import android.content.Context;
import android.graphics.Canvas;
import android.view.MotionEvent;

/** LinearLayout whose overridden callbacks forward to Rust. {@code mask} selects forwarded events. */
public class NativeLinearLayout extends android.widget.LinearLayout {
    static { Native.load(); }
    public static final int DRAW = 1, DISPATCH_DRAW = 2, TOUCH = 4, DETACHED = 8, SIZE = 16, ATTACHED = 32;
    private final long handle;
    private final int mask;
    public NativeLinearLayout(Context context, long handle, int mask) { super(context); this.handle = handle; this.mask = mask; }
    @Override protected void finalize() throws Throwable { try { Native.release(handle); } finally { super.finalize(); } }
    private Object call(String method, Object... args) { return Native.invoke(handle, method, args); }
    @Override protected void onDraw(Canvas canvas) { if ((mask & DRAW) != 0) call("onDraw(Canvas)", this, canvas); else super.onDraw(canvas); }
    @Override protected void dispatchDraw(Canvas canvas) { if ((mask & DISPATCH_DRAW) != 0) call("dispatchDraw(Canvas)", this, canvas); else super.dispatchDraw(canvas); }
    @Override public boolean onTouchEvent(MotionEvent event) {
        if ((mask & TOUCH) == 0) return super.onTouchEvent(event);
        Object r = call("onTouchEvent(MotionEvent)", this, event);
        return r instanceof Boolean ? (Boolean) r : super.onTouchEvent(event);
    }
    @Override protected void onDetachedFromWindow() { if ((mask & DETACHED) != 0) call("onDetachedFromWindow()", this); super.onDetachedFromWindow(); }
    @Override protected void onAttachedToWindow() { super.onAttachedToWindow(); if ((mask & ATTACHED) != 0) call("onAttachedToWindow()", this); }
    @Override protected void onSizeChanged(int w, int h, int ow, int oh) { super.onSizeChanged(w, h, ow, oh); if ((mask & SIZE) != 0) call("onSizeChanged(int,int,int,int)", this, w, h, ow, oh); }
    public void superOnDraw(Canvas canvas) { super.onDraw(canvas); }
    public void superDispatchDraw(Canvas canvas) { super.dispatchDraw(canvas); }
    public boolean superOnTouchEvent(MotionEvent event) { return super.onTouchEvent(event); }
}
