package com.notemaker.core;

import java.io.*;
import java.net.*;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.util.*;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.atomic.AtomicBoolean;
import static com.notemaker.core.Json.*;

/** HTTP peer, optionally authenticated with a private proxy descriptor. */
public final class Endpoint {
    public final Map<String,Object> descriptor;
    public static final class HttpError extends IOException {
        public final int status;
        public HttpError(int status,String message){super(message);this.status=status;}
    }
    private static final class RequestState { final AtomicBoolean cancelled=new AtomicBoolean(); volatile HttpURLConnection connection; }
    private final Map<Thread,RequestState> activeRequests=new ConcurrentHashMap<>();
    public Endpoint(Map<String,Object> descriptor) { this.descriptor=descriptor; }
    /** Interrupt this peer's request from a particular worker without disturbing other callers. */
    public boolean cancel(Thread thread){RequestState state=activeRequests.get(thread);if(state==null)return false;state.cancelled.set(true);HttpURLConnection connection=state.connection;if(connection!=null){Thread closer=new Thread(connection::disconnect,"peer-request-cancel");closer.setDaemon(true);closer.start();}return true;}
    public static Endpoint resolve(String target) throws IOException {
        if(target.startsWith("http://")||target.startsWith("https://"))return new Endpoint(object("url",target.replaceAll("/+$","")));
        return new Endpoint(map(parse(new String(Files.readAllBytes(Paths.get(target)),StandardCharsets.UTF_8))));
    }
    public HttpURLConnection open(String path) throws IOException {
        HttpURLConnection c=(HttpURLConnection)new URL(string(descriptor,"url")+path).openConnection();
        c.setConnectTimeout(3000);
        c.setReadTimeout(path.equals("/command")||path.equals("/api/ui/command")?40000:path.startsWith("/api/locks")||path.startsWith("/api/replica")||path.startsWith("/api/info")?2000:path.startsWith("/api/stream")?45000:5000);
        if(descriptor.containsKey("token"))c.setRequestProperty("Authorization","Bearer "+string(descriptor,"token"));
        return c;
    }
    public Map<String,Object> request(String path,Object body) throws IOException {
        Thread thread=Thread.currentThread();RequestState state=new RequestState();activeRequests.put(thread,state);
        HttpURLConnection c=null;
        try {
            c=open(path);state.connection=c;
            if(state.cancelled.get())throw new IOException("Peer request cancelled");
            if(body!=null&&path.startsWith("/api/events"))c.setReadTimeout(40000);
            if(body!=null){byte[] bytes=write(body).getBytes(StandardCharsets.UTF_8);c.setRequestMethod("POST");c.setDoOutput(true);c.setRequestProperty("Content-Type","application/json");c.setFixedLengthStreamingMode(bytes.length);try(OutputStream out=c.getOutputStream()){out.write(bytes);}}
            int status=c.getResponseCode();InputStream source=status==200?c.getInputStream():c.getErrorStream();
            if(source==null)throw new HttpError(status,"Peer HTTP "+status);
            try(InputStream in=source;ByteArrayOutputStream bytes=new ByteArrayOutputStream()){
                byte[] buffer=new byte[8192];int n;while((n=in.read(buffer))!=-1)bytes.write(buffer,0,n);
                Map<String,Object> result=map(parse(bytes.toString("UTF-8")));
                if(status!=200)throw new HttpError(status,String.valueOf(result.get("error")));return result;
            }
        } finally {activeRequests.remove(thread,state);if(c!=null)c.disconnect();}
    }
}
