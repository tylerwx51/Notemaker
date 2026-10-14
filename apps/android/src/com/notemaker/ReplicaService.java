package com.notemaker;

import android.app.Service;
import android.content.Intent;
import android.os.IBinder;
import com.notemaker.cli.LocalProxy;
import java.util.concurrent.*;

/** Local replica owner. Android may stop background execution; the journal survives. */
public class ReplicaService extends Service {
    static final ExecutorService writes=Executors.newSingleThreadExecutor(r->new Thread(r,"replica-writer"));
    static volatile LocalProxy proxy;
    static synchronized LocalProxy ensure(android.content.Context context) throws java.io.IOException {if(proxy==null)proxy=new LocalProxy(context.getFilesDir(),8766);return proxy;}
    @Override public int onStartCommand(Intent intent,int flags,int id){
        final String document=intent==null?"notes":intent.getStringExtra("document");
        final String host=intent==null?"":intent.getStringExtra("host");
        writes.execute(()->{try{ensure(this).configure(document,host);}catch(Exception e){android.util.Log.e("NoteMaker","Cannot start local proxy",e);}});
        return START_NOT_STICKY;
    }
    @Override public IBinder onBind(Intent intent){return null;}
    @Override public void onDestroy(){writes.execute(()->{synchronized(ReplicaService.class){LocalProxy old=proxy;proxy=null;if(old!=null)try{old.close();}catch(Exception e){android.util.Log.e("NoteMaker","Proxy shutdown",e);}}});super.onDestroy();}
}
