package com.notemaker;

import com.notemaker.core.*;
import com.notemaker.cli.LocalProxy;
import java.io.*;
import java.net.HttpURLConnection;
import java.nio.charset.StandardCharsets;
import java.util.*;
import static com.notemaker.core.Json.*;

/** The editor's selected proxy. Local reads retain the incremental Store cache. */
final class AppReplica {
    final MainActivity app;
    final Store local;
    final String document;
    final Map<String,Object> descriptor;
    final File descriptorFile;
    volatile HttpURLConnection stream;
    AppReplica(MainActivity app,String document,String url,String token) throws Exception {
        this.app=app;this.document=document;local=new Store(app.getFilesDir(),document);
        descriptor=url.isEmpty()?null:object("url",url,"token",token);
        descriptorFile=new File(app.getFilesDir(),descriptor==null?"proxy.json":"selected-proxy-"+UUID.nameUUIDFromBytes((url+"\n"+token).getBytes(StandardCharsets.UTF_8))+".json");
        if(descriptor!=null)try(FileOutputStream out=new FileOutputStream(descriptorFile)){out.write(write(descriptor).getBytes(StandardCharsets.UTF_8));}
    }
    boolean remote(){return descriptor!=null;}
    Map<String,Object> read() throws Exception {
        return map(command(Arrays.asList("snapshot"),null));
    }
    Object command(List<String> args,Object input) throws Exception {
        return command(document,args,input);
    }
    Object command(String name,List<String> args,Object input) throws Exception {
        if(remote())return LocalProxy.request(descriptor,name,"human",args,input);
        boolean trace=app.traceColdLoad&&args.size()==1&&args.get(0).equals("snapshot");
        long started=trace?System.nanoTime():0;
        long ensureStarted=started;
        java.util.function.Consumer<String> previous=com.notemaker.core.Timing.observer.get();
        if(trace)com.notemaker.core.Timing.observer.set(phase->android.util.Log.i("NoteMakerStartup",
                phase+"="+((System.nanoTime()-ensureStarted)/1e6)));
        LocalProxy proxy;
        try{proxy=ReplicaService.ensure(app);}finally{if(trace)com.notemaker.core.Timing.observer.set(previous);}
        if(trace)android.util.Log.i("NoteMakerStartup","proxy_ensure_duration="+((System.nanoTime()-started)/1e6));
        started=trace?System.nanoTime():0;
        Object result=proxy.execute(object("document",name,"actor","human","args",args,"input",input));
        if(trace)android.util.Log.i("NoteMakerStartup","snapshot_execute_duration="+((System.nanoTime()-started)/1e6));
        return result;
    }
    void listen(Runnable changed) throws Exception {
        HttpURLConnection connection=new Endpoint(descriptor).open("/api/stream");stream=connection;connection.setReadTimeout(45000);
        try {
            if(connection.getResponseCode()!=200)throw new IOException("Cannot subscribe to selected proxy");
            try(BufferedReader in=new BufferedReader(new InputStreamReader(connection.getInputStream(),StandardCharsets.UTF_8))){String line;
                while(app.running&&app.replica==this&&(line=in.readLine())!=null)if(line.startsWith("data:"))changed.run();
            }
            if(app.running&&app.replica==this)throw new IOException("Selected proxy notification stream ended");
        }finally{connection.disconnect();stream=null;}
    }
    void close(){HttpURLConnection connection=stream;if(connection!=null)new Thread(connection::disconnect,"close-proxy-stream").start();}
}
