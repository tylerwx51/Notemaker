package com.notemaker;

import android.app.Dialog;
import android.content.DialogInterface;
import android.graphics.Canvas;
import android.os.Handler;
import android.os.Message;
import android.text.Editable;
import android.text.TextWatcher;
import android.view.*;
import android.widget.*;

/** One forwarding implementation of every listener interface the app uses. */
public final class NativeCallback implements Runnable, View.OnClickListener, View.OnLongClickListener,
        View.OnTouchListener, View.OnLayoutChangeListener, View.OnApplyWindowInsetsListener, TextWatcher,
        SeekBar.OnSeekBarChangeListener, CompoundButton.OnCheckedChangeListener, RadioGroup.OnCheckedChangeListener,
        DialogInterface.OnClickListener, DialogInterface.OnMultiChoiceClickListener, DialogInterface.OnDismissListener,
        DialogInterface.OnShowListener, DialogInterface.OnCancelListener, AdapterView.OnItemClickListener,
        AdapterView.OnItemLongClickListener, AdapterView.OnItemSelectedListener, ViewTreeObserver.OnDrawListener,
        ViewTreeObserver.OnGlobalLayoutListener, ViewTreeObserver.OnPreDrawListener, Window.OnFrameMetricsAvailableListener,
        PopupWindow.OnDismissListener, Handler.Callback, Choreographer.FrameCallback, java.util.concurrent.Callable<Object> {
    static { Native.load(); }
    private final long handle;
    public NativeCallback(long handle) { this.handle = handle; }
    @Override protected void finalize() throws Throwable { try { Native.release(handle); } finally { super.finalize(); } }
    private Object call(String method, Object... args) { return Native.invoke(handle, method, args); }
    private boolean bool(Object value) { return value instanceof Boolean && (Boolean) value; }

    @Override public void run() { call("run()"); }
    @Override public Object call() { return call("call()"); }
    @Override public void onClick(View v) { call("onClick(View)", v); }
    @Override public boolean onLongClick(View v) { return bool(call("onLongClick(View)", v)); }
    @Override public boolean onTouch(View v, MotionEvent e) { return bool(call("onTouch(View,MotionEvent)", v, e)); }
    @Override public void onLayoutChange(View v, int l, int t, int r, int b, int ol, int ot, int or, int ob) {
        call("onLayoutChange(View,int,int,int,int,int,int,int,int)", v, l, t, r, b, ol, ot, or, ob);
    }
    @Override public WindowInsets onApplyWindowInsets(View v, WindowInsets insets) {
        Object result = call("onApplyWindowInsets(View,WindowInsets)", v, insets);
        return result == null ? insets : (WindowInsets) result;
    }
    @Override public void beforeTextChanged(CharSequence s, int start, int count, int after) { call("beforeTextChanged(CharSequence,int,int,int)", s, start, count, after); }
    @Override public void onTextChanged(CharSequence s, int start, int before, int count) { call("onTextChanged(CharSequence,int,int,int)", s, start, before, count); }
    @Override public void afterTextChanged(Editable s) { call("afterTextChanged(Editable)", s); }
    @Override public void onProgressChanged(SeekBar bar, int progress, boolean user) { call("onProgressChanged(SeekBar,int,boolean)", bar, progress, user); }
    @Override public void onStartTrackingTouch(SeekBar bar) { call("onStartTrackingTouch(SeekBar)", bar); }
    @Override public void onStopTrackingTouch(SeekBar bar) { call("onStopTrackingTouch(SeekBar)", bar); }
    @Override public void onCheckedChanged(CompoundButton button, boolean checked) { call("onCheckedChanged(CompoundButton,boolean)", button, checked); }
    @Override public void onCheckedChanged(RadioGroup group, int id) { call("onCheckedChanged(RadioGroup,int)", group, id); }
    @Override public void onClick(DialogInterface dialog, int which) { call("onClick(DialogInterface,int)", dialog, which); }
    @Override public void onClick(DialogInterface dialog, int which, boolean checked) { call("onClick(DialogInterface,int,boolean)", dialog, which, checked); }
    @Override public void onDismiss(DialogInterface dialog) { call("onDismiss(DialogInterface)", dialog); }
    @Override public void onShow(DialogInterface dialog) { call("onShow(DialogInterface)", dialog); }
    @Override public void onCancel(DialogInterface dialog) { call("onCancel(DialogInterface)", dialog); }
    @Override public void onItemClick(AdapterView<?> parent, View view, int position, long id) { call("onItemClick(AdapterView,View,int,long)", parent, view, position, id); }
    @Override public boolean onItemLongClick(AdapterView<?> parent, View view, int position, long id) { return bool(call("onItemLongClick(AdapterView,View,int,long)", parent, view, position, id)); }
    @Override public void onItemSelected(AdapterView<?> parent, View view, int position, long id) { call("onItemSelected(AdapterView,View,int,long)", parent, view, position, id); }
    @Override public void onNothingSelected(AdapterView<?> parent) { call("onNothingSelected(AdapterView)", parent); }
    @Override public void onDraw() { call("onDraw()"); }
    @Override public void onGlobalLayout() { call("onGlobalLayout()"); }
    @Override public boolean onPreDraw() { Object r = call("onPreDraw()"); return r == null || bool(r); }
    @Override public void onFrameMetricsAvailable(Window window, FrameMetrics metrics, int dropped) { call("onFrameMetricsAvailable(Window,FrameMetrics,int)", window, metrics, dropped); }
    @Override public void onDismiss() { call("onDismiss()"); }
    @Override public boolean handleMessage(Message message) { return bool(call("handleMessage(Message)", message)); }
    @Override public void doFrame(long frameTimeNanos) { call("doFrame(long)", frameTimeNanos); }
}
