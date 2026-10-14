package com.notemaker.core;

import java.io.*;
import java.net.*;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.util.*;
import static com.notemaker.core.Json.*;

/** HTTP peer, optionally authenticated with a private proxy descriptor. */
public final class Endpoint {
    public final Map<String,Object> descriptor;
    public Endpoint(Map<String,Object> descriptor) { this.descriptor=descriptor; }
    public static Endpoint resolve(String target) throws IOException {
        if(target.startsWith("http://")||target.startsWith("https://"))return new Endpoint(object("url",target.replaceAll("/+$","")));
        return new Endpoint(map(parse(new String(Files.readAllBytes(Paths.get(target)),StandardCharsets.UTF_8))));
    }
    public HttpURLConnection open(String path) throws IOException {
        HttpURLConnection c=(HttpURLConnection)new URL(string(descriptor,"url")+path).openConnection();
        c.setConnectTimeout(3000);c.setReadTimeout(path.equals("/command")?40000:5000);
        if(descriptor.containsKey("token"))c.setRequestProperty("Authorization","Bearer "+string(descriptor,"token"));
        return c;
    }
    public Map<String,Object> request(String path,Object body) throws IOException {
        HttpURLConnection c=open(path);
        try {
            if(body!=null){byte[] bytes=write(body).getBytes(StandardCharsets.UTF_8);c.setRequestMethod("POST");c.setDoOutput(true);c.setRequestProperty("Content-Type","application/json");c.setFixedLengthStreamingMode(bytes.length);try(OutputStream out=c.getOutputStream()){out.write(bytes);}}
            int status=c.getResponseCode();InputStream source=status==200?c.getInputStream():c.getErrorStream();
            if(source==null)throw new IOException("Peer HTTP "+status);
            try(InputStream in=source;ByteArrayOutputStream bytes=new ByteArrayOutputStream()){
                byte[] buffer=new byte[8192];int n;while((n=in.read(buffer))!=-1)bytes.write(buffer,0,n);
                Map<String,Object> result=map(parse(bytes.toString("UTF-8")));
                if(status!=200)throw new IOException(String.valueOf(result.get("error")));return result;
            }
        } finally {c.disconnect();}
    }
}
