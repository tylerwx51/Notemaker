package com.notemaker;

import android.view.View;
import android.view.ViewGroup;
import android.widget.BaseAdapter;

/** BaseAdapter forwarding getCount/getItem/getItemId/getView to Rust. */
public final class NativeAdapter extends BaseAdapter {
    static { Native.load(); }
    private final long handle;
    public NativeAdapter(long handle) { this.handle = handle; }
    @Override protected void finalize() throws Throwable { try { Native.release(handle); } finally { super.finalize(); } }
    @Override public int getCount() { Object r = Native.invoke(handle, "getCount()", new Object[0]); return r == null ? 0 : (Integer) r; }
    @Override public Object getItem(int position) { return Native.invoke(handle, "getItem(int)", new Object[]{position}); }
    @Override public long getItemId(int position) { Object r = Native.invoke(handle, "getItemId(int)", new Object[]{position}); return r == null ? position : ((Number) r).longValue(); }
    @Override public View getView(int position, View recycled, ViewGroup parent) { return (View) Native.invoke(handle, "getView(int,View,ViewGroup)", new Object[]{position, recycled, parent}); }
}
