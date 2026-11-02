package com.notemaker;

import android.app.Service;
import android.content.Intent;
import android.os.IBinder;

/** Manifest service; all behavior lives in Rust (replica_service.rs). */
public class ReplicaService extends Service {
    static { Native.load(); }
    @Override public int onStartCommand(Intent intent, int flags, int id) {
        Object r = Native.component(this, "onStartCommand", new Object[]{intent, flags, id});
        return r == null ? START_NOT_STICKY : (Integer) r;
    }
    @Override public IBinder onBind(Intent intent) { return null; }
    @Override public void onDestroy() { Native.component(this, "onDestroy", new Object[0]); super.onDestroy(); }
}
