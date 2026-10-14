package com.notemaker.cli;

import com.notemaker.core.*;
import java.io.*;
import java.net.*;
import java.nio.channels.*;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.util.*;
import java.util.concurrent.*;
import static com.notemaker.core.Json.*;
import static com.notemaker.core.Model.require;

/** Sole local writer, durable replica peer, and event-driven upstream synchronizer. */
public final class LocalProxy implements Closeable {
    private final File directory;
    private final ServerSocket listener=new ServerSocket();
    private final RandomAccessFile owner;
    private final FileLock ownership;
    private final ExecutorService clients=Executors.newCachedThreadPool();
    private final Set<Socket> sockets=ConcurrentHashMap.newKeySet();
    private final Map<String,Store> documents=new ConcurrentHashMap<>();
    private final Object changes=new Object();
    private final Map<String,Object> settings;
    private final String id,token;
    private volatile Endpoint upstream;
    private volatile HttpURLConnection stream;
    private volatile boolean running=true;
    private boolean configuring;
    private boolean initialized;
    private long generation=0,uploadDue=Long.MAX_VALUE,retryAt=0,lastUpload=Long.MIN_VALUE/2;
    private boolean fetch=true;
    private final List<CompletableFuture<Map<String,Object>>> requests=new ArrayList<>();
    public final Map<String,Boolean> connected=new ConcurrentHashMap<>();
    public final Map<String,String> errors=new ConcurrentHashMap<>();
    private final Thread acceptor,synchronizer,subscriber;

    public LocalProxy(File directory,int port) throws IOException {
        this.directory=directory;Files.createDirectories(directory.toPath());
        owner=new RandomAccessFile(new File(directory,"proxy.lock"),"rw");
        FileLock held;try{held=owner.getChannel().tryLock();}catch(OverlappingFileLockException e){held=null;}
        if(held==null){owner.close();throw new IOException("A proxy already owns this replica");}ownership=held;
        try {
            File config=new File(directory,"proxy-config.json");
            settings=new ConcurrentHashMap<>(config.exists()?map(parse(new String(Files.readAllBytes(config.toPath()),StandardCharsets.UTF_8))):object("id",UUID.randomUUID().toString(),"token",UUID.randomUUID().toString()+UUID.randomUUID()));
            id=string(settings,"id");token=string(settings,"token");
            if(settings.containsKey("upstream"))upstream=new Endpoint(map(settings.get("upstream")));
            for(Object document:list(settings.getOrDefault("documents",Collections.emptyList())))store((String)document);
            File[] files=directory.listFiles((d,n)->n.startsWith("document-")&&n.endsWith(".json"));
            if(files!=null)for(File file:files){Map<String,Object> state=map(parse(new String(Files.readAllBytes(file.toPath()),StandardCharsets.UTF_8)));if(state.containsKey("document"))store(string(state,"document"));}
            for(Store local:documents.values()){Object root=local.read().get("serverId");if(root!=null){require(!settings.containsKey("rootId")||root.equals(settings.get("rootId")),"Replica contains different server databases");settings.put("rootId",root);}}
            settings.put("documents",new ArrayList<>(documents.keySet()));saveSettings();initialized=true;
            listener.setReuseAddress(true);listener.bind(new InetSocketAddress("127.0.0.1",port));
            durable(new File(directory,"proxy.json"),object("url","http://127.0.0.1:"+port(),"token",token,"id",id));
        } catch(Exception e){ownership.release();owner.close();listener.close();throw new IOException(e);}
        // Long-lived streams must not exhaust a fixed command-worker pool.
        acceptor=new Thread(()->{while(running)try{Socket socket=listener.accept();sockets.add(socket);clients.execute(()->handle(socket));}catch(IOException e){if(running)System.err.println("Proxy accept: "+e);}},"local-proxy");
        synchronizer=new Thread(this::syncLoop,"proxy-sync");subscriber=new Thread(this::subscribe,"proxy-notifications");
        synchronized(changes){uploadDue=hasPending()?now()+500:Long.MAX_VALUE;}
        acceptor.start();synchronizer.start();subscriber.start();
    }
    private static long now(){return System.nanoTime()/1000000;}
    private void durable(File destination,Object value) throws IOException {
        Path temporary=Files.createTempFile(directory.toPath(),"proxy-",".tmp");
        try(FileOutputStream out=new FileOutputStream(temporary.toFile())){out.write(write(value).getBytes(StandardCharsets.UTF_8));out.getFD().sync();}
        Files.move(temporary,destination.toPath(),StandardCopyOption.ATOMIC_MOVE,StandardCopyOption.REPLACE_EXISTING);
        try(FileChannel channel=FileChannel.open(directory.toPath(),StandardOpenOption.READ)){channel.force(true);}
    }
    private void saveSettings() throws IOException {durable(new File(directory,"proxy-config.json"),settings);}
    private Store store(String document) throws IOException {
        Store found=documents.get(document);if(found!=null)return found;
        Store created=Store.writer(directory,document);Store previous=documents.putIfAbsent(document,created);
        if(previous==null)synchronized(this){if(initialized){settings.put("documents",new ArrayList<>(documents.keySet()));saveSettings();wakeFetch();}}
        return previous==null?created:previous;
    }
    private void changed(boolean local){synchronized(changes){generation++;if(local)uploadDue=Math.min(uploadDue,now()+500);changes.notifyAll();}}
    public long version(){synchronized(changes){return generation;}}
    public long awaitChange(long previous) throws InterruptedException {synchronized(changes){while(running&&generation==previous)changes.wait();return generation;}}
    private boolean hasPending(){try{for(Store local:documents.values())if(!list(local.read().get("pending")).isEmpty())return true;}catch(Exception e){return true;}return false;}
    private void wakeFetch(){synchronized(changes){fetch=true;changes.notifyAll();}}
    public synchronized void configure(String document,String target) throws Exception {
        store(document);Endpoint candidate=target==null||target.isEmpty()?null:Endpoint.resolve(target);
        if(Objects.equals(upstream==null?null:upstream.descriptor,candidate==null?null:candidate.descriptor)){wakeFetch();return;}
        // Publish tentatively before validation so simultaneous cross-links see each other.
        Endpoint old=upstream;synchronized(changes){configuring=true;upstream=candidate;}
        try {
            if(candidate!=null){
                try{
                    Map<String,Object> parent=candidate.request("/api/info?trail="+id+"&validate=1",null);
                    require(((Number)parent.getOrDefault("protocol",0)).intValue()>=3,"Upstream requires replica protocol 3");
                    require(candidate.descriptor.containsKey("token")||"server".equals(parent.get("kind")),"Use a private descriptor for a proxy upstream");
                    Object root=parent.get("rootId");require(root==null||!settings.containsKey("rootId")||root.equals(settings.get("rootId")),"Server database differs from this replica");
                }catch(ConnectException|SocketTimeoutException|UnknownHostException e){if(candidate.descriptor.containsKey("token"))throw e;/* A known-address main server may be offline. */}
            }
            if(candidate==null)settings.remove("upstream");else settings.put("upstream",candidate.descriptor);
            saveSettings();
        } catch(Exception e){upstream=old;throw e;}
        finally{synchronized(changes){configuring=false;changes.notifyAll();}}
        HttpURLConnection active=stream;if(active!=null)active.disconnect();
        synchronized(changes){fetch=true;uploadDue=hasPending()?now()+500:Long.MAX_VALUE;retryAt=0;changes.notifyAll();}
        changed(false);
    }
    private Map<String,Object> info(String trail,boolean validate) throws Exception {
        List<String> lineage=new ArrayList<>();if(!trail.isEmpty())lineage.addAll(Arrays.asList(trail.split(",")));
        require(!lineage.contains(id),"Proxy cycle rejected");require(lineage.size()<64,"Proxy chain exceeds 64 hops");lineage.add(id);
        Object root=settings.get("rootId");Endpoint peer=upstream;
        if(validate&&peer!=null&&peer.descriptor.containsKey("token")){
            Map<String,Object> parent=peer.request("/api/info?trail="+URLEncoder.encode(String.join(",",lineage),"UTF-8")+"&validate=1",null);
            root=parent.get("rootId");lineage=new ArrayList<>();for(Object value:list(parent.get("lineage")))lineage.add((String)value);
        }
        return object("kind","proxy","protocol",3,"id",id,"rootId",root,"lineage",lineage,"documents",new ArrayList<>(documents.keySet()));
    }
    private void syncLoop(){long backoff=500;
        while(running)try{
            Endpoint peer;boolean upload;List<CompletableFuture<Map<String,Object>>> waiters;
            synchronized(changes){
                while(running&&(configuring||upstream==null||now()<retryAt||(!fetch&&now()<uploadDue))){long deadline=retryAt>now()?retryAt:uploadDue;changes.wait(configuring||upstream==null||deadline==Long.MAX_VALUE?0:Math.max(1,deadline-now()));}
                if(!running)return;peer=upstream;upload=now()>=uploadDue;fetch=false;if(upload)uploadDue=Long.MAX_VALUE;
                waiters=upload?new ArrayList<>(requests):new ArrayList<>();if(upload)requests.clear();
            }
            try {
                Map<String,Object> remote=peer.request("/api/info?trail="+id,null);require(((Number)remote.getOrDefault("protocol",0)).intValue()>=3,"Upstream requires replica protocol 3");
                require(!id.equals(remote.get("id")),"Proxy cycle rejected");
                Object root=remote.get("rootId");synchronized(this){if(root!=null){require(!settings.containsKey("rootId")||root.equals(settings.get("rootId")),"Server database differs from this replica");if(!root.equals(settings.get("rootId"))){settings.put("rootId",root);saveSettings();}}}
                Map<String,Object> result=object();boolean more=false;
                for(Map.Entry<String,Store> entry:documents.entrySet()){
                    Store local=entry.getValue();long before=((Number)local.read().get("revision")).longValue();
                    Map<String,Object> synced=local.sync(peer,remote,upload,()->{long delay=lastUpload+500-now();if(delay>0)Thread.sleep(delay);lastUpload=now();});result.put(entry.getKey(),synced);
                    boolean connectionChanged=!Boolean.TRUE.equals(connected.put(entry.getKey(),true));errors.remove(entry.getKey());
                    if(((Number)local.read().get("revision")).longValue()!=before||connectionChanged)changed(false);
                    more|=local.needsUpload(string(remote,"id"));
                }
                synchronized(changes){if(more)uploadDue=Math.min(uploadDue,now()+500);retryAt=0;}
                backoff=500;for(CompletableFuture<Map<String,Object>> waiter:waiters)waiter.complete(result);
            } catch(Exception e){
                for(String doc:documents.keySet()){connected.put(doc,false);errors.put(doc,e.toString());}changed(false);
                for(CompletableFuture<Map<String,Object>> waiter:waiters)waiter.completeExceptionally(e);
                synchronized(changes){retryAt=now()+backoff;fetch=true;uploadDue=Math.min(uploadDue,retryAt);}backoff=Math.min(30000,backoff*2);
            }
        }catch(InterruptedException e){return;}
    }
    private void subscribe(){long backoff=500;
        while(running)try{
            Endpoint peer=upstream;if(peer==null){synchronized(changes){if(upstream==null&&running)changes.wait();}continue;}
            HttpURLConnection c=peer.open("/api/stream");stream=c;c.setReadTimeout(45000);
            try {
                if(c.getResponseCode()!=200)throw new IOException("Notification stream HTTP "+c.getResponseCode());
                try(BufferedReader reader=new BufferedReader(new InputStreamReader(c.getInputStream(),StandardCharsets.UTF_8))){String line;while(running&&peer==upstream&&(line=reader.readLine())!=null)if(line.startsWith("data:")){backoff=500;wakeFetch();}}
            } finally {c.disconnect();stream=null;}
            if(peer==upstream)throw new IOException("Notification stream ended");
        }catch(InterruptedException e){return;}catch(Exception e){try{Thread.sleep(backoff);}catch(InterruptedException stop){return;}backoff=Math.min(30000,backoff*2);}
    }
    private Map<String,Object> syncCommand(String document,String target) throws Exception {
        if(target!=null)configure(document,target);require(upstream!=null,"No upstream configured");
        CompletableFuture<Map<String,Object>> result=new CompletableFuture<>();synchronized(changes){requests.add(result);uploadDue=Math.min(uploadDue,now()+500);changes.notifyAll();}
        Map<String,Object> all=result.get(35,TimeUnit.SECONDS);return map(all.get(document));
    }
    public int port(){return listener.getLocalPort();}
    public void await() throws InterruptedException {acceptor.join();}
    @Override public void close() throws IOException {
        running=false;listener.close();HttpURLConnection active=stream;if(active!=null)active.disconnect();
        synchronizer.interrupt();subscriber.interrupt();synchronized(changes){changes.notifyAll();for(CompletableFuture<Map<String,Object>> request:requests)request.completeExceptionally(new IOException("Proxy closed"));}
        for(Socket socket:sockets)try{socket.close();}catch(IOException ignored){}clients.shutdownNow();
        // Never release ownership while an old writer can still commit.
        boolean interrupted=false;
        while(synchronizer.isAlive()||subscriber.isAlive()||!clients.isTerminated())try{synchronizer.join();subscriber.join();clients.awaitTermination(1,TimeUnit.SECONDS);}catch(InterruptedException e){interrupted=true;}
        ownership.release();owner.close();
        if(interrupted)Thread.currentThread().interrupt();
    }
    private static String line(InputStream in) throws IOException {
        ByteArrayOutputStream out=new ByteArrayOutputStream();int c;while((c=in.read())!=-1&&c!='\n'){if(out.size()>8192)throw new IOException("Header too long");if(c!='\r')out.write(c);}return out.toString("US-ASCII");
    }
    private static Map<String,String> query(URI uri) throws Exception {Map<String,String> out=new HashMap<>();if(uri.getRawQuery()!=null)for(String part:uri.getRawQuery().split("&")){String[] pair=part.split("=",2);out.put(URLDecoder.decode(pair[0],"UTF-8"),pair.length==2?URLDecoder.decode(pair[1],"UTF-8"):"");}return out;}
    private void handle(Socket socket){try(Socket connection=socket){
        connection.setSoTimeout(15000);InputStream in=new BufferedInputStream(connection.getInputStream());String request=line(in),authorization="";int length=0,total=0;
        while(true){String header=line(in);if(header.isEmpty())break;total+=header.length();require(total<=16384,"Headers too large");int colon=header.indexOf(':');require(colon>0,"Invalid header");String key=header.substring(0,colon),value=header.substring(colon+1).trim();if(key.equalsIgnoreCase("Content-Length"))length=Integer.parseInt(value);if(key.equalsIgnoreCase("Authorization"))authorization=value;}
        int status=200;Object response;
        try {
            require(authorization.equals("Bearer "+token),"Unauthorized proxy request");String[] parts=request.split(" ");URI uri=new URI(parts[1]);Map<String,String> q=query(uri);String path=uri.getPath();
            if(parts[0].equals("GET")&&path.equals("/api/stream")){events(connection);return;}
            Map<String,Object> body=object();if(parts[0].equals("POST")){require(length>0&&length<=4000000,"Request exceeds 4 MB");byte[] bytes=new byte[length];new DataInputStream(in).readFully(bytes);body=map(parse(new String(bytes,StandardCharsets.UTF_8)));}
            if(parts[0].equals("GET")&&path.equals("/api/info"))response=info(q.getOrDefault("trail",""),q.containsKey("validate"));
            else if(parts[0].equals("GET")&&path.equals("/api/replica")){
                String doc=q.getOrDefault("document","notes");boolean fresh=!documents.containsKey(doc);response=store(doc).replica(Integer.parseInt(q.getOrDefault("after","0")),Integer.parseInt(q.getOrDefault("pendingAfter","0")));if(fresh)wakeFetch();
            }else if(parts[0].equals("POST")&&path.equals("/api/events")){
                List<Object> operations=list(body.get("operations"));require(operations.size()<=100,"Upload exceeds 100 actions");Store local=store(string(body,"document"));long before=((Number)local.read().get("revision")).longValue();Object root=body.get("rootId");
                synchronized(this){
                    require(root==null||!settings.containsKey("rootId")||root.equals(settings.get("rootId")),"Server database differs from this replica");
                    response=local.accept(operations,root);
                    if(root!=null&&!root.equals(settings.get("rootId"))){settings.put("rootId",root);saveSettings();}
                }
                if(((Number)local.read().get("revision")).longValue()!=before)changed(true);
            }else if(parts[0].equals("POST")&&path.equals("/command"))response=object("result",execute(body));
            else throw new IllegalArgumentException("Unsupported endpoint");
        }catch(Exception e){status=400;response=object("error",e.toString());}
        byte[] data=write(response).getBytes(StandardCharsets.UTF_8);OutputStream out=connection.getOutputStream();out.write(("HTTP/1.1 "+status+" "+(status==200?"OK":"Bad Request")+"\r\nContent-Type: application/json\r\nContent-Length: "+data.length+"\r\nConnection: close\r\n\r\n").getBytes(StandardCharsets.US_ASCII));out.write(data);out.flush();
    }catch(Exception e){if(running)System.err.println("Proxy request: "+e);}finally{sockets.remove(socket);}}
    private void events(Socket socket) throws Exception {
        OutputStream out=socket.getOutputStream();out.write("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n".getBytes(StandardCharsets.US_ASCII));long seen=-1;
        while(running){long current;synchronized(changes){if(generation==seen)changes.wait(15000);current=generation;}out.write((current==seen?": heartbeat\n\n":"data: "+current+"\n\n").getBytes(StandardCharsets.UTF_8));out.flush();seen=current;}
    }
    public Object execute(Map<String,Object> body) throws Exception {
        require(running,"Proxy is closed");
        List<String> args=new ArrayList<>();for(Object value:list(body.get("args"))){require(value instanceof String&&!((String)value).startsWith("--"),"Proxy options belong in document/actor fields");args.add((String)value);}require(!args.isEmpty(),"Missing command");
        String doc=(String)body.getOrDefault("document","notes"),actor=(String)body.getOrDefault("actor","cli");Store local=store(doc);String cmd=args.get(0);
        if(cmd.equals("configure")){require(args.size()==2,"configure UPSTREAM_URL_OR_DESCRIPTOR (empty to disconnect)");configure(doc,args.get(1));return object("configured",true);}
        if(cmd.equals("sync")){require(args.size()<=2,"sync [UPSTREAM_URL_OR_DESCRIPTOR]");return syncCommand(doc,args.size()==2?args.get(1):null);}
        require(Arrays.asList("document","inspect","snapshot","append","settings","status","log","diagram","svg","apply","layer","object","stroke","undo","redo").contains(cmd),"Unsupported proxy command");
        List<String> localArgs=new ArrayList<>(Arrays.asList("--state",directory.getAbsolutePath(),"--document",doc,"--actor",actor));localArgs.addAll(args);
        Main.proxyInput.set(body.get("input"));Main.inProxy.set(true);long before=((Number)local.read().get("revision")).longValue();
        try{Object result=Main.run(localArgs.toArray(new String[0]));if(cmd.equals("status")){map(result).put("connected",Boolean.TRUE.equals(connected.get(doc)));map(result).put("upstreamError",errors.get(doc));map(result).put("proxyId",id);}return result;}finally{
            Main.proxyInput.remove();Main.inProxy.remove();
            boolean action=!Arrays.asList("settings","inspect","snapshot","status","log").contains(cmd)&&!(cmd.equals("document")&&args.size()>1&&args.get(1).equals("list"));
            if(((Number)local.read().get("revision")).longValue()!=before)changed(action);
        }
    }
    public static Object request(Map<String,Object> descriptor,String document,String actor,List<String> args,Object input) throws Exception {
        return new Endpoint(descriptor).request("/command",object("document",document,"actor",actor,"args",args,"input",input)).get("result");
    }
}
