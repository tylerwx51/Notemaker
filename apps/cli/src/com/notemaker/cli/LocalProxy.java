package com.notemaker.cli;

import com.notemaker.core.*;
import java.io.*;
import java.net.*;
import java.nio.channels.*;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.util.*;
import java.util.concurrent.*;
import java.util.concurrent.atomic.AtomicBoolean;
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
    private final Map<String,Object> ephemeralPreviews=new ConcurrentHashMap<>();
    private final Object changes=new Object();
    private final Map<String,Object> settings;
    private final String id,token;
    private final AtomicBoolean closed=new AtomicBoolean();
    private final AtomicBoolean preemptFetch=new AtomicBoolean();
    private volatile Endpoint upstream;
    private volatile HttpURLConnection stream;
    private volatile boolean running=true;
    private boolean configuring;
    private boolean initialized;
    private long generation=0,uploadDue=Long.MAX_VALUE,retryAt=0,lastUpload=Long.MIN_VALUE/2;
    private boolean fetch=true;
    private volatile String syncDocument="";
    private volatile boolean syncUploading;
    private volatile String activeDocument="";
    private final List<CompletableFuture<Map<String,Object>>> requests=new ArrayList<>();
    public final Map<String,Boolean> connected=new ConcurrentHashMap<>();
    public final Map<String,String> errors=new ConcurrentHashMap<>();
    private final Thread acceptor,synchronizer,subscriber;

    public LocalProxy(File directory,int port) throws IOException {
        this.directory=directory;Files.createDirectories(directory.toPath());
        owner=new RandomAccessFile(new File(directory,"proxy.lock"),"rw");
        FileLock held;try{held=owner.getChannel().tryLock();}catch(OverlappingFileLockException e){held=null;}
        if(held==null){owner.close();throw new IOException("A proxy already owns this replica");}ownership=held;
        Timing.mark("proxy_lock_done");
        try {
            File config=new File(directory,"proxy-config.json");
            settings=new ConcurrentHashMap<>(config.exists()?map(parse(new String(Files.readAllBytes(config.toPath()),StandardCharsets.UTF_8))):object("id",UUID.randomUUID().toString(),"token",UUID.randomUUID().toString()+UUID.randomUUID()));
            id=string(settings,"id");token=string(settings,"token");
            Timing.mark("proxy_config_read");
            if(settings.containsKey("upstream"))upstream=new Endpoint(map(settings.get("upstream")));
            for(Object document:list(settings.getOrDefault("documents",Collections.emptyList())))store((String)document);
            Timing.mark("proxy_known_documents");
            Set<String> knownFiles=new HashSet<>();
            for(Store local:documents.values())knownFiles.add(local.file.getName());
            File[] files=directory.listFiles((d,n)->n.startsWith("document-")&&n.endsWith(".json"));
            if(files!=null)for(File file:files){
                // Known documents are validated by Store.read below. Parsing a
                // checkpointed base here would deserialize it twice on startup.
                if(knownFiles.contains(file.getName()))continue;
                Map<String,Object> state=map(parse(new String(Files.readAllBytes(file.toPath()),StandardCharsets.UTF_8)));
                if(state.containsKey("document"))store(string(state,"document"));
            }
            Timing.mark("proxy_document_scan");
            for(Store local:documents.values()){Object root=local.read().get("serverId");if(root!=null){require(!settings.containsKey("rootId")||root.equals(settings.get("rootId")),"Replica contains different server databases");settings.put("rootId",root);}}
            Timing.mark("proxy_root_validation");
            settings.put("documents",new ArrayList<>(documents.keySet()));saveSettings();initialized=true;
            Timing.mark("proxy_config_synced");
            listener.setReuseAddress(true);listener.bind(new InetSocketAddress("127.0.0.1",port));
            Timing.mark("proxy_bound");
            durable(new File(directory,"proxy.json"),object("url","http://127.0.0.1:"+port(),"token",token,"id",id));
            Timing.mark("proxy_descriptor_synced");
        } catch(Exception e){ownership.release();owner.close();listener.close();throw new IOException(e);}
        // Long-lived streams must not exhaust a fixed command-worker pool.
        acceptor=new Thread(()->{while(running)try{Socket socket=listener.accept();sockets.add(socket);clients.execute(()->handle(socket));}catch(IOException e){if(running)System.err.println("Proxy accept: "+e);}},"local-proxy");
        synchronizer=new Thread(this::syncLoop,"proxy-sync");subscriber=new Thread(this::subscribe,"proxy-notifications");
        synchronized(changes){uploadDue=hasPending()?now()+500:Long.MAX_VALUE;}
        Timing.mark("proxy_pending_checked");
        acceptor.start();synchronizer.start();subscriber.start();Runtime.getRuntime().addShutdownHook(new Thread(()->{try{close();}catch(IOException ignored){}} ,"notemaker-proxy-shutdown"));
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
    private void changed(boolean local){
        synchronized(changes){generation++;if(local)uploadDue=Math.min(uploadDue,now()+500);changes.notifyAll();}
        // A fetch-only pass over an idle document must not hold up a newly edited
        // active document until the peer's read timeout. Abort only the sync
        // worker's request; the next cycle will retry under the normal upload gate.
        if(local)preemptFetchSweep();
    }
    public long version(){synchronized(changes){return generation;}}
    public long awaitChange(long previous) throws InterruptedException {synchronized(changes){while(running&&generation==previous)changes.wait();return generation;}}
    private boolean hasPending(){try{for(Store local:documents.values())if(!list(local.read().get("pending")).isEmpty())return true;}catch(Exception e){return true;}return false;}
    private void wakeFetch(){synchronized(changes){fetch=true;changes.notifyAll();}preemptFetchSweep();}
    private void preemptFetchSweep(){
        if(syncUploading||!synchronizer.isAlive()||!preemptFetch.compareAndSet(false,true))return;
        Endpoint peer=upstream;if(peer==null||!peer.cancel(synchronizer))preemptFetch.set(false);
    }
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
                if(!running)return;peer=upstream;upload=now()>=uploadDue;syncUploading=upload;fetch=false;if(upload)uploadDue=Long.MAX_VALUE;
                waiters=upload?new ArrayList<>(requests):new ArrayList<>();if(upload)requests.clear();
            }
            try {
                Map<String,Object> remote=peer.request("/api/info?trail="+id,null);require(((Number)remote.getOrDefault("protocol",0)).intValue()>=3,"Upstream requires replica protocol 3");
                require(!id.equals(remote.get("id")),"Proxy cycle rejected");
                Object root=remote.get("rootId");synchronized(this){if(root!=null){require(!settings.containsKey("rootId")||root.equals(settings.get("rootId")),"Server database differs from this replica");if(!root.equals(settings.get("rootId"))){settings.put("rootId",root);saveSettings();}}}
                Map<String,Object> result=object();boolean more=false;
                List<Map.Entry<String,Store>> syncDocuments=new ArrayList<>(documents.entrySet());String priority=activeDocument;
                syncDocuments.sort((a,b)->{
                    boolean aa=a.getKey().equals(priority),bb=b.getKey().equals(priority);if(aa!=bb)return aa?-1:1;
                    try{boolean ap=a.getValue().needsUpload(string(remote,"id")),bp=b.getValue().needsUpload(string(remote,"id"));if(ap!=bp)return ap?-1:1;}catch(Exception ignored){}
                    return a.getKey().compareTo(b.getKey());
                });
                for(Map.Entry<String,Store> entry:syncDocuments){
                    syncDocument=entry.getKey();
                    Store local=entry.getValue();Map<String,Object> leasesBefore=map(copy(local.read().getOrDefault("leases",object())));long before=((Number)local.read().get("revision")).longValue();
                    Map<String,Object> synced=local.sync(peer,remote,upload,()->{long delay=lastUpload+500-now();if(delay>0)Thread.sleep(delay);lastUpload=now();});for(String key:new ArrayList<>(ephemeralPreviews.keySet()))if(key.startsWith(entry.getKey()+":"))ephemeralPreviews.remove(key);for(Object value:map(synced.getOrDefault("previews",object())).values()){Map<String,Object> preview=map(value);String key=entry.getKey()+":"+preview.get("session");if(((Number)preview.getOrDefault("expires",0)).longValue()>System.currentTimeMillis())ephemeralPreviews.put(key,preview);}result.put(entry.getKey(),synced);
                    boolean connectionChanged=!Boolean.TRUE.equals(connected.put(entry.getKey(),true));errors.remove(entry.getKey());
                    if(((Number)local.read().get("revision")).longValue()!=before||connectionChanged||!leasesBefore.equals(local.read().getOrDefault("leases",object())))changed(false);
                    more|=local.needsUpload(string(remote,"id"));
                }
                synchronized(changes){if(more)uploadDue=Math.min(uploadDue,now()+500);retryAt=0;}
                preemptFetch.set(false);
                backoff=500;for(CompletableFuture<Map<String,Object>> waiter:waiters)waiter.complete(result);
            } catch(Exception e){
                if(preemptFetch.getAndSet(false)){
                    synchronized(changes){retryAt=0;fetch=true;changes.notifyAll();}
                    backoff=500;
                    continue;
                }
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
        if(!closed.compareAndSet(false,true))return;
        for(Map.Entry<String,Store> entry:documents.entrySet())try{String doc=entry.getKey();releaseOwned(entry.getValue(),doc,null);for(String key:new ArrayList<>(ephemeralPreviews.keySet()))if(key.startsWith(doc+":")){Map<String,Object> preview=map(ephemeralPreviews.get(key));if(id.equals(preview.get("owner"))&&ephemeralPreviews.remove(key,preview))forwardPreviewClear(doc,String.valueOf(preview.get("session")),String.valueOf(preview.get("actor")));}}catch(Exception ignored){}
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
        connection.setSoTimeout(45000);InputStream in=new BufferedInputStream(connection.getInputStream());String request=line(in),authorization="";int length=0,total=0;
        while(true){String header=line(in);if(header.isEmpty())break;total+=header.length();require(total<=16384,"Headers too large");int colon=header.indexOf(':');require(colon>0,"Invalid header");String key=header.substring(0,colon),value=header.substring(colon+1).trim();if(key.equalsIgnoreCase("Content-Length"))length=Integer.parseInt(value);if(key.equalsIgnoreCase("Authorization"))authorization=value;}
        int status=200;Object response;
        try {
            require(authorization.equals("Bearer "+token),"Unauthorized proxy request");String[] parts=request.split(" ");URI uri=new URI(parts[1]);Map<String,String> q=query(uri);String path=uri.getPath();
            if(parts[0].equals("GET")&&path.equals("/api/stream")){events(connection);return;}
            byte[] requestBody=null;Map<String,Object> body=object();if(parts[0].equals("POST")){require(length>0&&length<=4000000,"Request exceeds 4 MB");requestBody=new byte[length];new DataInputStream(in).readFully(requestBody);body=map(parse(new String(requestBody,StandardCharsets.UTF_8)));}
            if(path.equals("/api/chats")||path.startsWith("/api/chats/")){
                forwardChat(connection,parts[0],uri,requestBody);return;
            }
            if(parts[0].equals("GET")&&path.equals("/api/info"))response=info(q.getOrDefault("trail",""),q.containsKey("validate"));
            else if(parts[0].equals("GET")&&path.equals("/api/replica")){
                String doc=q.getOrDefault("document","notes");activeDocument=doc;boolean fresh=!documents.containsKey(doc);response=store(doc).replica(Integer.parseInt(q.getOrDefault("after","0")),Integer.parseInt(q.getOrDefault("pendingAfter","0")));if(fresh)wakeFetch();
            }else if(parts[0].equals("GET")&&path.equals("/api/locks")){response=lockState(store(q.getOrDefault("document","notes")).read());}
            else if(parts[0].equals("POST")&&path.equals("/api/locks")){Endpoint peer=upstream;Map<String,Object> decision=null;if(peer!=null)try{decision=peer.request("/api/locks",body);}catch(Exception unavailable){/* Keep arbitration at the nearest reachable proxy. */}Map<String,Object> accepted=body;if(decision!=null&&decision.containsKey("selected")){accepted=map(copy(body));Map<String,Object> filtered=object();for(Object key:list(decision.get("selected")))if(map(body.getOrDefault("leases",object())).containsKey(key))filtered.put(String.valueOf(key),map(body.get("leases")).get(key));accepted.put("leases",filtered);accepted.put("remove",body.getOrDefault("remove",Collections.emptyList()));}String doc=string(body,"document");Store lockStore=store(doc);Map<String,Object> leasesBefore=map(copy(lockStore.read().getOrDefault("leases",object()))),previewsBefore=previewSnapshot(doc,lockStore.read());Object local=applyRemoteLeases(lockStore,accepted);for(Object session:list(body.getOrDefault("clearPreviews",Collections.emptyList())))ephemeralPreviews.remove(doc+":"+session);Map<String,Object> liveLeases=map(lockStore.read().getOrDefault("leases",object()));for(Object value:map(body.getOrDefault("previews",object())).values()){Map<String,Object> normalized=normalizePreview(map(value),liveLeases);if(normalized!=null)ephemeralPreviews.put(doc+":"+normalized.get("session"),normalized);}response=decision==null?local:decision;Map<String,Object> previewsAfter=previewSnapshot(doc,lockStore.read());if(!leasesBefore.equals(lockStore.read().getOrDefault("leases",object()))||!previewsBefore.equals(previewsAfter))changed(false);}
            else if(parts[0].equals("POST")&&path.equals("/api/events")){
                List<Object> operations=list(body.get("operations"));require(operations.size()<=100,"Upload exceeds 100 actions");String document=string(body,"document");activeDocument=document;Store local=store(document);long before=((Number)local.read().get("revision")).longValue();Object root=body.get("rootId");
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
    /** Chat requests belong to the primary server, but follow the same configured proxy chain as notes. */
    private void forwardChat(Socket socket,String method,URI uri,byte[] body) throws Exception {
        Endpoint peer=upstream;require(peer!=null,"No upstream proxy configured for chat. All app traffic must go through the proxy.");
        String path=uri.getRawPath()+(uri.getRawQuery()==null?"":"?"+uri.getRawQuery());
        HttpURLConnection c=peer.open(path);c.setRequestMethod(method);c.setReadTimeout(path.contains("/stream")?45000:10000);
        try {
            if(body!=null){c.setDoOutput(true);c.setRequestProperty("Content-Type","application/json");c.setFixedLengthStreamingMode(body.length);try(OutputStream out=c.getOutputStream()){out.write(body);}}
            int status=c.getResponseCode();InputStream source=status<400?c.getInputStream():c.getErrorStream();
            OutputStream out=socket.getOutputStream();String type=c.getContentType();
            out.write(("HTTP/1.1 "+status+" "+(status<400?"OK":"Upstream Error")+"\r\nContent-Type: "+(type==null?"application/json":type)+"\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n").getBytes(StandardCharsets.US_ASCII));out.flush();
            if(source!=null)try(InputStream in=source){byte[] buffer=new byte[8192];int count;while((count=in.read(buffer))!=-1){out.write(buffer,0,count);out.flush();}}
        } finally {c.disconnect();}
    }
    private void events(Socket socket) throws Exception {
        OutputStream out=socket.getOutputStream();out.write("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n".getBytes(StandardCharsets.US_ASCII));long seen=-1;
        while(running){long current;synchronized(changes){if(generation==seen)changes.wait(15000);current=generation;}out.write((current==seen?": heartbeat\n\n":"data: "+current+"\n\n").getBytes(StandardCharsets.UTF_8));out.flush();seen=current;}
    }
    public Object execute(Map<String,Object> body) throws Exception {
        require(running,"Proxy is closed");
        List<String> args=new ArrayList<>();for(Object value:list(body.get("args"))){require(value instanceof String&&!((String)value).startsWith("--"),"Proxy options belong in document/actor fields");args.add((String)value);}require(!args.isEmpty(),"Missing command");
        String doc=(String)body.getOrDefault("document","notes"),actor=(String)body.getOrDefault("actor","cli");activeDocument=doc;Store local=store(doc);String cmd=args.get(0);
        if(cmd.equals("snapshot")){
            require(args.size()==1,"snapshot takes no arguments");Map<String,Object> state=local.read();Collaboration.expire(state,System.currentTimeMillis());
            Map<String,Object> result=Main.snapshot(state);result.put("leases",copy(state.getOrDefault("leases",object())));result.put("previews",previewSnapshot(doc,state));return result;
        }
        if(cmd.equals("append")&&args.size()==3&&args.get(1).equals("snapshot")&&args.get(2).equals("-")){
            Map<String,Object> state=local.append(map(body.get("input")),actor);List<Object> pending=list(state.get("pending"));
            Collaboration.expire(state,System.currentTimeMillis());Map<String,Object> snapshot=Main.snapshot(state);
            snapshot.put("leases",copy(state.getOrDefault("leases",object())));snapshot.put("previews",previewSnapshot(doc,state));
            changed(true);return object("operation",map(pending.get(pending.size()-1)).get("id"),"snapshot",snapshot);
        }
        if((cmd.equals("undo")||cmd.equals("redo"))&&args.size()==2&&args.get(1).equals("snapshot")){
            Map<String,Object> result=local.history(actor,cmd.equals("redo"),true);
            Map<String,Object> state=map(result.remove("state"));Collaboration.expire(state,System.currentTimeMillis());
            Map<String,Object> snapshot=Main.snapshot(state);snapshot.put("leases",copy(state.getOrDefault("leases",object())));snapshot.put("previews",previewSnapshot(doc,state));
            result.put("snapshot",snapshot);changed(true);return result;
        }
        if(cmd.equals("object")&&args.size()==4&&args.get(1).equals("put")&&args.get(2).equals("snapshot")&&args.get(3).equals("-")){
            Map<String,Object> drawing=Actions.drawing(map(body.get("input")),"base");drawing.put("kind","object");drawing.remove("id");
            Map<String,Object> result=local.commitWithState(actor,model->{
                if(model.objects.containsKey(drawing.get("object")))new LayerEditor(model,string(drawing,"layer"),((Number)drawing.get("page")).intValue(),true).resolve("object",string(drawing,"object"));
                return Arrays.asList(drawing);
            });
            Map<String,Object> state=map(result.remove("state"));Collaboration.expire(state,System.currentTimeMillis());
            Map<String,Object> snapshot=Main.snapshot(state);snapshot.put("leases",copy(state.getOrDefault("leases",object())));snapshot.put("previews",previewSnapshot(doc,state));
            result.put("snapshot",snapshot);changed(true);return result;
        }
        if(cmd.equals("configure")){require(args.size()==2,"configure UPSTREAM_URL_OR_DESCRIPTOR (empty to disconnect)");configure(doc,args.get(1));return object("configured",true);}
        if(cmd.equals("sync")){require(args.size()<=2,"sync [UPSTREAM_URL_OR_DESCRIPTOR]");return syncCommand(doc,args.size()==2?args.get(1):null);}
        if(cmd.equals("selection")&&args.size()==3&&args.get(1).equals("select"))return acquireSelection(local,doc,actor,map(body.get("input")));
        if(cmd.equals("selection")&&args.size()==3&&args.get(1).equals("heartbeat"))return heartbeatSelection(local,doc,actor,args.get(2));
        if(cmd.equals("selection")&&args.size()==2&&args.get(1).equals("disconnect"))return releaseOwned(local,doc,actor);
        if(cmd.equals("transform")&&args.size()==3&&args.get(1).equals("cancel")){String session=args.get(2);clearPreview(doc,session,actor);return object("cancelled",true,"session",session);}
        if(cmd.equals("transform")&&args.size()==3&&args.get(1).equals("preview")&&args.get(2).equals("-")){
            Map<String,Object> request=map(body.get("input"));Map<String,Object> transform=map(request.get("transform"));return preview(doc,actor,string(request,"session"),list(request.get("entities")),transform);
        }
        require(Arrays.asList("document","inspect","snapshot","append","settings","status","log","checkpoint","diagram","svg","image","erase","fill","template","shape","apply","layer","object","stroke","undo","redo","selection","transform","conflicts","conflict").contains(cmd),"Unsupported proxy command");
        List<String> localArgs=new ArrayList<>(Arrays.asList("--state",directory.getAbsolutePath(),"--document",doc,"--actor",actor));localArgs.addAll(args);
        Main.proxyInput.set(body.get("input"));Main.inProxy.set(true);Map<String,Object> beforeState=local.read();long before=((Number)beforeState.get("revision")).longValue();Map<String,Object> previousLeases=map(copy(beforeState.getOrDefault("leases",object())));
        Object result=null;try{result=Main.run(localArgs.toArray(new String[0]));if(cmd.equals("transform")&&args.size()>1&&args.get(1).equals("commit")){Map<String,Object> request=map(body.get("input"));clearPreview(doc,string(request,"session"),actor);}if(cmd.equals("status")){map(result).put("connected",Boolean.TRUE.equals(connected.get(doc)));map(result).put("upstreamError",errors.get(doc));map(result).put("proxyId",id);}if(cmd.equals("snapshot")){Map<String,Object> state=local.read();Collaboration.expire(state,System.currentTimeMillis());map(result).put("leases",copy(state.getOrDefault("leases",object())));map(result).put("previews",previewSnapshot(doc,state));map(result).put("conflicts",local.conflicts());}return result;}finally{
            Main.proxyInput.remove();Main.inProxy.remove();
            if(cmd.equals("selection"))try{String session=args.size()>1?args.get(args.size()-1):"";Set<String> cleared=new HashSet<>();if(args.size()>1&&args.get(1).equals("disconnect")){session="";for(Object raw:previousLeases.values()){Map<String,Object> lease=map(raw);if(actor.equals(lease.get("actor")))cleared.add(String.valueOf(lease.get("session")));}for(Map.Entry<String,Object> entry:ephemeralPreviews.entrySet())if(entry.getKey().startsWith(doc+":")){Map<String,Object> preview=map(entry.getValue());if(actor.equals(preview.get("actor")))cleared.add(String.valueOf(preview.get("session")));}}else if(args.size()>1&&args.get(1).equals("deselect"))cleared.add(session);for(String oldSession:cleared)clearPreview(doc,oldSession,actor);Map<String,Object> current=map(local.read().getOrDefault("leases",object()));List<Object> removed=new ArrayList<>();for(Map.Entry<String,Object> entry:previousLeases.entrySet())if((session.isEmpty()||session.equals(map(entry.getValue()).get("session")))&&actor.equals(map(entry.getValue()).get("actor"))&&!current.containsKey(entry.getKey()))removed.add(entry.getKey());Map<String,Object> changedLeases=object();for(Map.Entry<String,Object> entry:current.entrySet())if(!Objects.equals(previousLeases.get(entry.getKey()),entry.getValue()))changedLeases.put(entry.getKey(),entry.getValue());forwardLocks(doc,changedLeases,removed,session,actor);if(!previousLeases.equals(current))changed(false);}catch(Exception ignored){}
            boolean action=uploadsChanges(cmd,args);
            Map<String,Object> afterState=local.read();
            if(((Number)afterState.get("revision")).longValue()!=before)changed(action);
        }
    }
    private static boolean uploadsChanges(String cmd,List<String> args){
        if(cmd.equals("selection"))return args.size()>1&&Arrays.asList("cut","delete","duplicate","paste").contains(args.get(1));
        if(cmd.equals("transform"))return args.size()>1&&args.get(1).equals("commit");
        if(cmd.equals("conflict"))return args.size()>1&&args.get(1).equals("resolve");
        return !Arrays.asList("settings","inspect","snapshot","status","log","shape","conflicts").contains(cmd)&&!(cmd.equals("document")&&args.size()>1&&args.get(1).equals("list"));
    }
    public static Object request(Map<String,Object> descriptor,String document,String actor,List<String> args,Object input) throws Exception {
        return new Endpoint(descriptor).request("/command",object("document",document,"actor",actor,"args",args,"input",input)).get("result");
    }

    private Map<String,Object> lockState(Map<String,Object> state){Collaboration.expire(state,System.currentTimeMillis());return object("leases",copy(state.getOrDefault("leases",object())),"previews",previewSnapshot(string(state,"document"),state));}
    private Map<String,Object> previewSnapshot(String document,Map<String,Object> state){long now=System.currentTimeMillis();Map<String,Object> out=object(),leases=map(state.getOrDefault("leases",object()));for(String key:new ArrayList<>(ephemeralPreviews.keySet())){Map<String,Object> preview=map(ephemeralPreviews.get(key));Map<String,Object> normalized=key.startsWith(document+":")?normalizePreview(preview,leases):null;if(normalized==null){if(key.startsWith(document+":"))ephemeralPreviews.remove(key,preview);}else{ephemeralPreviews.put(key,normalized);out.put(String.valueOf(normalized.get("session")),normalized);}}return out;}
    private static Map<String,Object> normalizePreview(Map<String,Object> preview,Map<String,Object> leases){long now=System.currentTimeMillis(),expires=((Number)preview.getOrDefault("expires",0)).longValue(),leaseLimit=Long.MAX_VALUE;String actor=String.valueOf(preview.get("actor")),session=String.valueOf(preview.get("session"));List<Object> entities=list(preview.getOrDefault("entities",Collections.emptyList()));if(entities.isEmpty())return null;for(Object value:entities){Map<String,Object> entity=map(value);Object raw=leases.get(Collaboration.key(String.valueOf(entity.get("kind")),String.valueOf(entity.get("id")),entity.getOrDefault("page",0)));Map<String,Object> lease=raw==null?object():map(raw);if(!actor.equals(lease.get("actor"))||!session.equals(lease.get("session")))return null;long liveUntil=((Number)lease.getOrDefault("expires",0)).longValue();if(liveUntil<=now)return null;leaseLimit=Math.min(leaseLimit,liveUntil);}long capped=Math.min(expires,Math.min(now+Collaboration.LEASE_MILLIS,leaseLimit));if(capped<=now)return null;Map<String,Object> normalized=map(copy(preview));normalized.put("expires",capped);return normalized;}
    private static boolean previewLive(Map<String,Object> preview,Map<String,Object> leases,long now){Map<String,Object> normalized=normalizePreview(preview,leases);return normalized!=null&&((Number)normalized.get("expires")).longValue()>now;}
    private boolean clearPreview(String document,String session,String actor){String key=document+":"+session;Object raw=ephemeralPreviews.get(key);if(raw==null)return false;Map<String,Object> preview=map(raw);if(!actor.equals(preview.get("actor")))return false;if(ephemeralPreviews.remove(key,raw)){forwardPreviewClear(document,session,actor);changed(false);return true;}return false;}
    private static Object applyRemoteLeases(Store store,Map<String,Object> body) throws Exception {
        Map<String,Object> incoming=map(body.getOrDefault("leases",object()));List<Object> accepted=new ArrayList<>(),skipped=new ArrayList<>();store.editTransient(s->{Map<String,Object> current=map(s.getOrDefault("leases",object()));Collaboration.expire(s,System.currentTimeMillis());
            for(Object key:list(body.getOrDefault("remove",Collections.emptyList()))){Object raw=current.get(key);Map<String,Object> lease=raw==null?object():map(raw);if(lease.isEmpty()||("".equals(body.get("session"))||Objects.equals(body.get("session"),lease.get("session")))&&Objects.equals(body.get("actor"),lease.get("actor")))current.remove(key);}
            long now=System.currentTimeMillis();for(Map.Entry<String,Object> e:incoming.entrySet()){Map<String,Object> lease=map(copy(e.getValue()));long expiry=Math.min(((Number)lease.getOrDefault("expires",0)).longValue(),now+Collaboration.LEASE_MILLIS);if(expiry<=now)continue;lease.put("expires",expiry);Object oldRaw=current.get(e.getKey());Map<String,Object> old=oldRaw==null?object():map(oldRaw);if(!old.isEmpty()&&((Number)old.getOrDefault("expires",0)).longValue()>now&&(!Objects.equals(old.get("actor"),lease.get("actor"))||!Objects.equals(old.get("session"),lease.get("session")))){skipped.add(object("key",e.getKey(),"actor",old.get("actor"),"session",old.get("session")));continue;}current.put(e.getKey(),lease);accepted.add(e.getKey());}s.put("leases",current);
        });return object("accepted",accepted,"selected",accepted,"skipped",skipped);
    }
    private static final class SelectionScope {final String layer;final int page;SelectionScope(String layer,int page){this.layer=layer;this.page=page;}}
    private static void requireSessionScope(Map<String,Object> state,String session,SelectionScope scope){
        if(session==null)return;
        for(Object raw:map(state.getOrDefault("leases",object())).values()){
            Map<String,Object> lease=map(raw);
            if(session.equals(lease.get("session")))Model.require(scope.layer.equals(lease.getOrDefault("layer","base"))&&
                    scope.page==((Number)lease.getOrDefault("page",0)).intValue(),"Selection session belongs to another layer/page");
        }
    }
    private static SelectionScope selectionScope(Store store,Map<String,Object> request) throws Exception {
        String layer=(String)request.getOrDefault("layer","base");int page=LayerEditor.page(request);
        List<Object> refs=list(request.get("entities"));Model.require(!refs.isEmpty()&&refs.size()<=200,"Selection needs 1–200 entities");
        LayerEditor.requireReferences(store.model(),layer,page,refs);
        SelectionScope scope=new SelectionScope(layer,page);
        if(request.containsKey("session"))requireSessionScope(store.read(),(String)request.get("session"),scope);
        return scope;
    }
    public static Object select(Store store,String actor,Map<String,Object> request) throws Exception {
        return selectValidated(store,actor,request,selectionScope(store,request));
    }
    private static Object selectValidated(Store store,String actor,Map<String,Object> request,SelectionScope scope) throws Exception {
        List<Object> selected=new ArrayList<>(),skipped=new ArrayList<>();String session=String.valueOf(request.getOrDefault("session",UUID.randomUUID().toString()));long now=System.currentTimeMillis();
        Object page=request.getOrDefault("page",0);Model.require(page instanceof Number&&((Number)page).intValue()>=0&&((Number)page).intValue()<=10000,"Invalid page");List<Object> entities=list(request.get("entities"));Model.require(!entities.isEmpty()&&entities.size()<=200,"Selection needs 1–200 entities");
        store.editTransient(s->{LayerEditor.requireReferences(Store.model(s),scope.layer,scope.page,entities);Collaboration.expire(s,now);requireSessionScope(s,session,scope);Map<String,Object> leases=map(s.getOrDefault("leases",object()));Map<String,Object> sessions=map(s.getOrDefault("activeSessions",object()));for(Object value:entities){Map<String,Object> e=map(value);String kind=string(e,"kind"),id=string(e,"id");String key=Collaboration.key(kind,id,page);Object existing=leases.get(key);Map<String,Object> prior=existing==null?object():map(existing);if(!prior.isEmpty()&&(!actor.equals(prior.get("actor"))||!session.equals(prior.get("session")))){skipped.add(object("kind",kind,"id",id,"page",page,"actor",prior.get("actor")));continue;}Map<String,Object> lease=object("layer",scope.layer,"session",session,"actor",actor,"page",page,"expires",now+Collaboration.LEASE_MILLIS,"created",now);if(request.containsKey("owner"))lease.put("owner",request.get("owner"));leases.put(key,lease);selected.add(object("kind",kind,"id",id,"page",page));}if(!selected.isEmpty())sessions.put(actor,session);s.put("activeSessions",sessions);s.put("leases",leases);});
        return object("session",session,"selected",selected,"skipped",skipped,"expires",now+Collaboration.LEASE_MILLIS);
    }
    private Object acquireSelection(Store store,String document,String actor,Map<String,Object> request) throws Exception {
        SelectionScope scope=selectionScope(store,request);
        String session=String.valueOf(request.getOrDefault("session",UUID.randomUUID().toString()));Object page=request.getOrDefault("page",0);List<Object> entities=list(request.get("entities"));Map<String,Object> candidates=object();long expires=System.currentTimeMillis()+Collaboration.LEASE_MILLIS;
        for(Object value:entities){Map<String,Object> e=map(value);String kind=string(e,"kind"),id=string(e,"id");String key=Collaboration.key(kind,id,page);candidates.put(key,object("layer",scope.layer,"session",session,"actor",actor,"owner",this.id,"page",page,"expires",expires,"created",System.currentTimeMillis()));}
        Map<String,Object> decision=null;Endpoint peer=upstream;if(peer!=null)try{decision=peer.request("/api/locks",object("document",document,"session",session,"actor",actor,"leases",candidates,"remove",Collections.emptyList()));}catch(Exception unavailable){/* Offline selection remains local until the next sync attempt. */}
        List<Object> accepted=new ArrayList<>(),skipped=new ArrayList<>();Set<String> keys=new HashSet<>();if(decision==null)keys.addAll(candidates.keySet());else for(Object key:list(decision.getOrDefault("selected",Collections.emptyList())))keys.add(String.valueOf(key));
        List<Object> filtered=new ArrayList<>();for(Object value:entities){Map<String,Object> e=map(value);String key=Collaboration.key(string(e,"kind"),string(e,"id"),page);if(keys.contains(key))filtered.add(value);else{Map<String,Object> remote=null;if(decision!=null)for(Object item:list(decision.getOrDefault("skipped",Collections.emptyList())))if(key.equals(map(item).get("key")))remote=map(item);skipped.add(object("kind",e.get("kind"),"id",e.get("id"),"page",page,"actor",remote==null?"another session":remote.get("actor")));}}
        if(decision!=null){Map<String,Object> authoritative=object();try{authoritative=map(peer.request("/api/locks?document="+URLEncoder.encode(document,"UTF-8"),null).getOrDefault("leases",object()));}catch(Exception ignored){}Map<String,Object> selectedRoot=object();for(String key:candidates.keySet())if(authoritative.containsKey(key))selectedRoot.put(key,authoritative.get(key));store.editTransient(s->{Map<String,Object> local=map(s.getOrDefault("leases",object()));for(String key:candidates.keySet())local.remove(key);s.put("leases",local);});if(!selectedRoot.isEmpty())applyRemoteLeases(store,object("document",document,"actor",actor,"session",session,"leases",selectedRoot,"remove",Collections.emptyList()));}
        if(!filtered.isEmpty()){Object result=selectValidated(store,actor,object("layer",scope.layer,"session",session,"page",page,"entities",filtered,"owner",id),scope);Map<String,Object> local=map(result);accepted.addAll(list(local.get("selected")));skipped.addAll(list(local.get("skipped")));}if(!accepted.isEmpty())changed(false);return object("session",session,"selected",accepted,"skipped",skipped,"expires",expires);
    }
    public static void assertSession(Store store,String actor,String session,List<Object> entities) throws Exception {
        Map<String,Object> state=store.read();Collaboration.requireSelection(state,Store.model(state),actor,session,entities);
    }
    public static List<Object> selected(Store store,String actor,String session) throws Exception {
        Map<String,Object> state=store.read();Collaboration.expire(state,System.currentTimeMillis());List<Object> entities=new ArrayList<>();for(Map.Entry<String,Object> entry:map(state.getOrDefault("leases",object())).entrySet()){Map<String,Object> lease=map(entry.getValue());if(session.equals(lease.get("session"))){Model.require(actor.equals(lease.get("actor")),"Session belongs to another actor");String[] bits=entry.getKey().split(":",3);entities.add(object("page",Integer.parseInt(bits[0]),"kind",bits[1],"id",bits[2]));}}Model.require(!entities.isEmpty(),"Selection session has no active entities");return entities;
    }
    public static Object release(Store store,String actor,String session,boolean heartbeat) throws Exception {
        long now=System.currentTimeMillis();final int[] count={0};store.editTransient(s->{Collaboration.expire(s,now);Map<String,Object> leases=map(s.getOrDefault("leases",object()));for(String key:new ArrayList<>(leases.keySet())){Map<String,Object> lease=map(leases.get(key));if(session.equals(lease.get("session"))){Model.require(actor.equals(lease.get("actor")),"Session belongs to another actor");if(heartbeat){lease.put("expires",now+Collaboration.LEASE_MILLIS);leases.put(key,lease);}else leases.remove(key);count[0]++;}}if(!heartbeat&&session.equals(map(s.getOrDefault("activeSessions",object())).get(actor)))map(s.get("activeSessions")).remove(actor);s.put("leases",leases);});
        return object(heartbeat?"renewed":"released",count[0],"session",session,"expires",heartbeat?now+Collaboration.LEASE_MILLIS:0);
    }
    public static Object releaseActor(Store store,String actor) throws Exception {
        List<Object> released=new ArrayList<>();store.editTransient(s->{Map<String,Object> leases=map(s.getOrDefault("leases",object()));for(String key:new ArrayList<>(leases.keySet()))if(actor.equals(map(leases.get(key)).get("actor"))){released.add(key);leases.remove(key);}map(s.getOrDefault("activeSessions",object())).remove(actor);s.put("leases",leases);});return object("released",released.size());
    }
    private Object releaseOwned(Store store,String document,String actor) throws Exception {
        List<Object> released=new ArrayList<>();List<String> sessions=new ArrayList<>();Map<String,List<Object>> byActor=new HashMap<>();store.editTransient(s->{Map<String,Object> leases=map(s.getOrDefault("leases",object()));for(String key:new ArrayList<>(leases.keySet())){Map<String,Object> lease=map(leases.get(key));if(id.equals(lease.get("owner"))&&(actor==null||actor.equals(lease.get("actor")))){released.add(key);byActor.computeIfAbsent(String.valueOf(lease.get("actor")),ignored->new ArrayList<>()).add(key);String session=String.valueOf(lease.get("session"));if(!sessions.contains(session))sessions.add(session);leases.remove(key);}}Map<String,Object> active=map(s.getOrDefault("activeSessions",object()));for(Map.Entry<String,Object> entry:new ArrayList<>(active.entrySet()))if((actor==null||actor.equals(entry.getKey()))&&sessions.contains(String.valueOf(entry.getValue())))active.remove(entry.getKey());s.put("leases",leases);s.put("activeSessions",active);});for(String session:sessions)clearPreview(document,session,actor);for(Map.Entry<String,List<Object>> group:byActor.entrySet())forwardLocks(document,Collections.emptyMap(),group.getValue(),"",group.getKey());return object("released",released.size());
    }
    private Object heartbeatSelection(Store store,String document,String actor,String session) throws Exception {
        long now=System.currentTimeMillis(),expires=now+Collaboration.LEASE_MILLIS;Map<String,Object> local=map(store.read().getOrDefault("leases",object())),candidates=object();
        for(Map.Entry<String,Object> entry:local.entrySet()){Map<String,Object> lease=map(entry.getValue());if(session.equals(lease.get("session"))&&actor.equals(lease.get("actor"))){Map<String,Object> renewed=map(copy(lease));renewed.put("expires",expires);candidates.put(entry.getKey(),renewed);}}
        Model.require(!candidates.isEmpty(),"Selection session has no active entities");Endpoint peer=upstream;Map<String,Object> decision=null;if(peer!=null)try{decision=peer.request("/api/locks",object("document",document,"session",session,"actor",actor,"leases",candidates,"remove",Collections.emptyList()));}catch(Exception offline){}
        Set<String> accepted=new HashSet<>();if(decision==null)accepted.addAll(candidates.keySet());else for(Object key:list(decision.getOrDefault("selected",Collections.emptyList())))accepted.add(String.valueOf(key));List<Object> skipped=new ArrayList<>();for(String key:candidates.keySet())if(!accepted.contains(key))skipped.add(object("key",key));
        store.editTransient(state->{Map<String,Object> leases=map(state.getOrDefault("leases",object()));for(String key:candidates.keySet())if(accepted.contains(key))leases.put(key,candidates.get(key));else leases.remove(key);if(accepted.isEmpty()&&session.equals(map(state.getOrDefault("activeSessions",object())).get(actor)))map(state.get("activeSessions")).remove(actor);state.put("leases",leases);});
        if(!accepted.isEmpty()||!skipped.isEmpty())changed(false);
        if(peer!=null){List<Object> removed=new ArrayList<>();for(Object value:skipped)removed.add(String.valueOf(map(value).get("key")));forwardLocks(document,mapSubset(candidates,accepted),removed,session,actor);}
        return object("renewed",accepted.size(),"session",session,"expires",accepted.isEmpty()?0:expires,"skipped",skipped);
    }
    private static Map<String,Object> mapSubset(Map<String,Object> input,Set<String> keys){Map<String,Object> out=object();for(String key:keys)if(input.containsKey(key))out.put(key,input.get(key));return out;}
    public Object preview(String document,String actor,String session,List<Object> entities,Map<String,Object> transform) throws Exception {
        Store store=store(document);Map<String,Object> state=store.read();long now=System.currentTimeMillis();Collaboration.expire(state,now);List<Object> blocked=Collaboration.conflicts(state,actor,entities);Model.require(blocked.isEmpty(),"Preview includes unlocked entities: "+write(blocked));
        for(Object value:entities){Map<String,Object> e=map(value);Object raw=map(state.getOrDefault("leases",object())).get(Collaboration.key(string(e,"kind"),string(e,"id"),e.getOrDefault("page",0)));Map<String,Object> lease=raw==null?object():map(raw);Model.require(session.equals(lease.get("session"))&&actor.equals(lease.get("actor")),"Preview requires this session's selection lease");}
        long leaseLimit=Long.MAX_VALUE;for(Object value:entities){Map<String,Object> e=map(value),lease=map(map(state.get("leases")).get(Collaboration.key(string(e,"kind"),string(e,"id"),e.getOrDefault("page",0))));leaseLimit=Math.min(leaseLimit,((Number)lease.get("expires")).longValue());}long expiry=Math.min(now+Collaboration.LEASE_MILLIS,leaseLimit);Map<String,Object> preview=object("session",session,"actor",actor,"owner",id,"entities",copy(entities),"transform",transform,"expires",expiry);ephemeralPreviews.put(document+":"+session,preview);forwardLocks(document,Collections.emptyMap(),Collections.emptyList(),session,actor);changed(false);return object("previewing",true,"session",session,"expires",preview.get("expires"));
    }
    private void forwardLocks(String document,Map<String,Object> leases,List<Object> remove,String session,String actor){Endpoint peer=upstream;if(peer==null)return;try{Map<String,Object> previews=object();for(Map.Entry<String,Object> entry:ephemeralPreviews.entrySet())if(entry.getKey().startsWith(document+":"))previews.put(String.valueOf(map(entry.getValue()).get("session")),entry.getValue());peer.request("/api/locks",object("document",document,"session",session,"actor",actor,"leases",leases,"remove",remove,"previews",previews));}catch(Exception ignored){}}
    private void forwardPreviewClear(String document,String session,String actor){Endpoint peer=upstream;if(peer==null)return;try{peer.request("/api/locks",object("document",document,"session",session,"actor",actor,"leases",object(),"remove",Collections.emptyList(),"clearPreviews",Arrays.asList(session)));}catch(Exception ignored){}}
}
