package com.notemaker.cli;

import com.notemaker.core.*;
import java.io.*;
import java.net.*;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.util.*;
import java.util.concurrent.*;
import static com.notemaker.core.Json.*;

/** Loopback JSON command service. App and agents share the same cached Store instances. */
public final class LocalProxy implements Closeable {
    private final File directory;
    private final ServerSocket listener;
    private final String token=UUID.randomUUID().toString()+UUID.randomUUID();
    private final ExecutorService clients=Executors.newFixedThreadPool(4);
    private final Map<String,String> upstream=new ConcurrentHashMap<>();
    public final Map<String,Boolean> connected=new ConcurrentHashMap<>();
    private volatile boolean running=true;
    private final Thread acceptor,synchronizer;
    public LocalProxy(File directory,int port) throws IOException {
        this.directory=directory;Files.createDirectories(directory.toPath());
        listener=new ServerSocket();listener.setReuseAddress(true);listener.bind(new InetSocketAddress("127.0.0.1",port));
        File descriptor=new File(directory,"proxy.json");
        Path temporary=Files.createTempFile(directory.toPath(),"proxy-",".tmp");
        Files.write(temporary,write(object("url","http://127.0.0.1:"+listener.getLocalPort(),"token",token)).getBytes(StandardCharsets.UTF_8));
        Files.move(temporary,descriptor.toPath(),StandardCopyOption.ATOMIC_MOVE,StandardCopyOption.REPLACE_EXISTING);
        acceptor=new Thread(()->{while(running)try{Socket socket=listener.accept();clients.execute(()->handle(socket));}catch(IOException e){if(running)System.err.println("Proxy accept: "+e);}},"local-proxy");
        synchronizer=new Thread(()->{while(running){for(Map.Entry<String,String> entry:upstream.entrySet()){
            boolean success=false;try{new Store(directory,entry.getKey()).sync(entry.getValue());success=true;}catch(Exception e){/* Offline edits remain available. */}
            if(Objects.equals(upstream.get(entry.getKey()),entry.getValue()))connected.put(entry.getKey(),success);
        }try{Thread.sleep(500);}catch(InterruptedException e){return;}}},"proxy-sync");
        acceptor.start();synchronizer.start();
    }
    public void configure(String document,String server){if(server!=null&&!server.isEmpty()){if(!server.equals(upstream.put(document,server)))connected.put(document,false);}else{upstream.remove(document);connected.put(document,false);}}
    public int port(){return listener.getLocalPort();}
    public void await() throws InterruptedException {acceptor.join();}
    @Override public void close() throws IOException {running=false;listener.close();synchronizer.interrupt();clients.shutdown();}
    private static String line(InputStream in) throws IOException {
        ByteArrayOutputStream out=new ByteArrayOutputStream();int c;
        while((c=in.read())!=-1&&c!='\n'){if(out.size()>8192)throw new IOException("Header too long");if(c!='\r')out.write(c);}return out.toString("US-ASCII");
    }
    private void handle(Socket socket){try(Socket connection=socket){
        connection.setSoTimeout(15000);InputStream in=new BufferedInputStream(connection.getInputStream());
        String request=line(in),authorization="";int length=-1,total=0;
        while(true){String header=line(in);if(header.isEmpty())break;total+=header.length();if(total>16384)throw new IOException("Headers too large");int colon=header.indexOf(':');if(colon<0)throw new IOException("Invalid header");String key=header.substring(0,colon).trim(),value=header.substring(colon+1).trim();if(key.equalsIgnoreCase("Content-Length"))length=Integer.parseInt(value);if(key.equalsIgnoreCase("Authorization"))authorization=value;}
        int status=200;Object response;
        try{
            Model.require(authorization.equals("Bearer "+token),"Unauthorized proxy request");
            Model.require(request.equals("POST /command HTTP/1.1")||request.equals("POST /command HTTP/1.0"),"Use POST /command");
            Model.require(length>=0&&length<=4000000,"Request exceeds 4 MB");byte[] bytes=new byte[length];new DataInputStream(in).readFully(bytes);
            Map<String,Object> body=map(parse(new String(bytes,StandardCharsets.UTF_8)));
            List<Object> raw=list(body.get("args"));Model.require(!raw.isEmpty(),"Missing command");List<String> args=new ArrayList<>();
            for(Object value:raw){Model.require(value instanceof String,"Arguments must be strings");String arg=(String)value;Model.require(!arg.startsWith("--"),"Proxy options belong in document/actor fields");args.add(arg);}
            Model.require(Arrays.asList("document","inspect","status","log","diagram","svg","apply","layer","object","stroke","undo","redo","sync").contains(args.get(0)),"Unsupported proxy command");
            String doc=(String)body.getOrDefault("document","notes"),actor=(String)body.getOrDefault("actor","cli");
            List<String> local=new ArrayList<>(Arrays.asList("--state",directory.getAbsolutePath(),"--document",doc,"--actor",actor));local.addAll(args);
            Main.proxyInput.set(body.get("input"));Main.inProxy.set(true);
            try{response=object("result",Main.run(local.toArray(new String[0])));}finally{Main.proxyInput.remove();Main.inProxy.remove();}
        }catch(Exception e){status=400;response=object("error",e.toString());}
        byte[] data=write(response).getBytes(StandardCharsets.UTF_8);OutputStream out=connection.getOutputStream();
        out.write(("HTTP/1.1 "+status+" "+(status==200?"OK":"Bad Request")+"\r\nContent-Type: application/json\r\nContent-Length: "+data.length+"\r\nConnection: close\r\n\r\n").getBytes(StandardCharsets.US_ASCII));out.write(data);out.flush();
    }catch(Exception e){System.err.println("Proxy request: "+e);}}

    public static Object request(Map<String,Object> descriptor,String document,String actor,List<String> args,Object input) throws Exception {
        HttpURLConnection c=(HttpURLConnection)new URL(string(descriptor,"url")+"/command").openConnection();c.setConnectTimeout(3000);c.setReadTimeout(30000);c.setRequestMethod("POST");c.setDoOutput(true);c.setRequestProperty("Content-Type","application/json");c.setRequestProperty("Authorization","Bearer "+string(descriptor,"token"));
        byte[] bytes=write(object("document",document,"actor",actor,"args",args,"input",input)).getBytes(StandardCharsets.UTF_8);c.setFixedLengthStreamingMode(bytes.length);
        try{try(OutputStream out=c.getOutputStream()){out.write(bytes);}int status=c.getResponseCode();
            try(InputStream in=status==200?c.getInputStream():c.getErrorStream();ByteArrayOutputStream out=new ByteArrayOutputStream()){byte[] buffer=new byte[8192];int n;while((n=in.read(buffer))!=-1)out.write(buffer,0,n);Map<String,Object> reply=map(parse(out.toString("UTF-8")));if(status!=200)throw new IOException(String.valueOf(reply.get("error")));return reply.get("result");}
        }finally{c.disconnect();}
    }
}
