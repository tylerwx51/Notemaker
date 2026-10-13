package com.notemaker.core;

import java.io.*;
import java.net.*;
import java.nio.channels.*;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.util.*;
import java.util.concurrent.ConcurrentHashMap;
import static com.notemaker.core.Json.*;

/** Per-document cached replica with durable journal transactions and shared process locks. */
public final class Store {
    private static final Map<String,Object> MONITORS=new ConcurrentHashMap<>();
    public final File file;
    public final String document;
    public Store(File directory,String document) throws IOException {
        Model.require(!document.isEmpty()&&document.length()<=200,"Document name needs 1–200 characters");
        Files.createDirectories(directory.toPath());this.document=document;
        file=new File(directory,"document-"+UUID.nameUUIDFromBytes(document.getBytes(StandardCharsets.UTF_8))+".json");
    }
    public interface Edit { void apply(Map<String,Object> state) throws Exception; }
    private Map<String,Object> locked(Edit edit) throws Exception {return Journal.access(file,document,edit);}
    public static Model model(Map<String,Object> state){return state instanceof Journal.State?new Model(((Journal.State)state).model):new Model(state);}
    public Model model() throws Exception {return model(read());}
    public Map<String,Object> read() throws Exception {return locked(null);}
    public Map<String,Object> edit(Edit edit) throws Exception {return locked(edit);}
    public Map<String,Object> append(Map<String,Object> operation,String actor) throws Exception {
        Model.validate(operation);
        return edit(state->{Map<String,Object> op=map(copy(operation));op.put("id",UUID.randomUUID().toString());op.put("actor",actor);list(state.get("pending")).add(op);});
    }
    public Map<String,Object> commit(String actor,Actions.Build build) throws Exception {
        Model.require(!actor.isEmpty()&&actor.length()<=200,"Invalid actor");
        final Map<String,Object> result=object();
        edit(state->{
            Model model=Store.model(state);List<Object> requested=build.create(model),ops=new ArrayList<>();
            for(Object value:requested){Map<String,Object> op=map(value);ops.add(op);String kind=string(op,"kind");
                String key=kind.equals("object")?"object":kind.equals("layer")?"layer":kind.equals("stroke")?"stroke":null;
                if(key!=null&&!(key.equals("object")?model.objects:key.equals("layer")?model.layers:model.strokes).containsKey(op.get(key)))ops.add(object("kind",key.equals("stroke")?"active":key+"-active",key,op.get(key),"active",true));
            }
            Model.require(!ops.isEmpty()&&ops.size()<=200,"Action needs 1–200 operations");
            List<Object> inverse=new ArrayList<>();
            for(Object v:ops){Map<String,Object> op=map(v);Model.require(!Arrays.asList("batch","document").contains(op.get("kind")),"Use document create; nested batches are not allowed");Model.validate(op);inverse.add(0,Actions.inverse(model,op));model.apply(op);}
            Actions.validateReferences(model,ops);
            Map<String,Object> batch=object("kind","batch","id",UUID.randomUUID().toString(),"actor",actor,"actions",ops);
            Model.require(write(batch).getBytes(StandardCharsets.UTF_8).length<3000000,"Action exceeds 3 MB");
            Model.require(write(inverse).getBytes(StandardCharsets.UTF_8).length<2999000,"Undo snapshot exceeds 3 MB; split this action");
            list(state.get("pending")).add(batch);
            Map<String,Object> history=history(state,actor);list(history.get("undo")).add(object("forward",copy(ops),"inverse",inverse));history.put("redo",new ArrayList<>());
            result.put("operation",batch.get("id"));result.put("actions",ops);
        });return result;
    }
    private static Map<String,Object> history(Map<String,Object>state,String actor){Map<String,Object> actors=map(state.get("actors"));actors.putIfAbsent(actor,object("undo",new ArrayList<>(),"redo",new ArrayList<>()));return map(actors.get(actor));}
    public Map<String,Object> history(String actor,boolean redo) throws Exception {
        final Map<String,Object> result=object();
        edit(state->{Map<String,Object> h=history(state,actor);List<Object> from=list(h.get(redo?"redo":"undo")),to=list(h.get(redo?"undo":"redo"));Model.require(!from.isEmpty(),"No "+(redo?"redo":"undo")+" for actor "+actor);Map<String,Object> entry=map(from.remove(from.size()-1));to.add(entry);
            String id=UUID.randomUUID().toString();list(state.get("pending")).add(object("kind","batch","id",id,"actor",actor,"actions",copy(entry.get(redo?"forward":"inverse"))));result.put("operation",id);
        });return result;
    }
    public Map<String,Object> sync(String url) throws Exception {
        Model.require(url.startsWith("http://")||url.startsWith("https://"),"Server must be HTTP(S)");
        // Serialize sync workers, but never hold the state lock over a network request.
        String path=file.getCanonicalPath()+".sync.lock";
        synchronized(MONITORS.computeIfAbsent(path,k->new Object())) {
            try(RandomAccessFile syncFile=new RandomAccessFile(path,"rw");FileLock syncLock=syncFile.getChannel().tryLock()) {
                if(syncLock==null)return object("busy",true);
                Map<String,Object> initial=read();long after=((Number)initial.get("cursor")).longValue();
                String serverId=string(http("GET",url+"/api/info",null),"id");
                if(initial.containsKey("serverId"))Model.require(initial.get("serverId").equals(serverId),"Server database differs from this replica; use a separate state directory");
                List<Object> batch=new ArrayList<>();int bytes=0;
                for(Object v:list(initial.get("pending"))){int size=write(v).getBytes(StandardCharsets.UTF_8).length;if(batch.size()>=100||(!batch.isEmpty()&&bytes+size>3000000))break;batch.add(v);bytes+=size;}
                Set<String> ack=new HashSet<>();
                if(!batch.isEmpty())for(Object id:list(http("POST",url+"/api/events",object("document",document,"operations",batch)).get("accepted")))ack.add((String)id);
                List<Object> received=new ArrayList<>();long last=after,savedCursor=after;int downloaded=0,receivedBytes=0;
                while(true){
                    List<Object> rows=list(http("GET",url+"/api/events?document="+URLEncoder.encode(document,"UTF-8")+"&after="+last,null).get("events"));
                    for(Object row:rows){
                        Map<String,Object> r=map(row),op=map(r.get("op"));long seq=((Number)r.get("seq")).longValue();Model.require(seq>last,"Invalid server cursor");Model.validate(op);
                        int size=write(op).getBytes(StandardCharsets.UTF_8).length;
                        if(!received.isEmpty()&&receivedBytes+size>3000000){receive(received,savedCursor,last,serverId);savedCursor=last;received=new ArrayList<>();receivedBytes=0;}
                        received.add(op);receivedBytes+=size;last=seq;downloaded++;
                    }
                    if(rows.size()<1000)break;
                }
                if(!received.isEmpty())receive(received,savedCursor,last,serverId);
                final long cursor=last;
                // Remove acknowledgements only after complete catch-up. Interrupted downloads
                // keep their durable cursor and outbox, so a retry is idempotent.
                if(!ack.isEmpty()||!initial.containsKey("serverId"))edit(state->{
                    Model.require(((Number)state.get("cursor")).longValue()==cursor,"Sync cursor changed");
                    state.put("serverId",serverId);list(state.get("pending")).removeIf(v->ack.contains(map(v).get("id")));
                });
                return object("uploaded",ack.size(),"downloaded",downloaded,"cursor",last,"pending",list(read().get("pending")).size());
            }
        }
    }
    private void receive(List<Object> operations,long after,long cursor,String serverId) throws Exception {
        edit(state->{Model.require(((Number)state.get("cursor")).longValue()==after,"Sync cursor changed");list(state.get("events")).addAll(operations);state.put("cursor",cursor);state.put("serverId",serverId);});
    }
    private static Map<String,Object> http(String method,String url,Object body) throws Exception {
        HttpURLConnection c=(HttpURLConnection)new URL(url).openConnection();c.setConnectTimeout(2000);c.setReadTimeout(5000);c.setRequestMethod(method);
        try{if(body!=null){c.setDoOutput(true);c.setRequestProperty("Content-Type","application/json");try(OutputStream out=c.getOutputStream()){out.write(write(body).getBytes(StandardCharsets.UTF_8));}}
            int status=c.getResponseCode();if(status!=200)throw new IOException("Server HTTP "+status);
            try(InputStream in=c.getInputStream();ByteArrayOutputStream out=new ByteArrayOutputStream()){byte[] b=new byte[8192];int n;while((n=in.read(b))!=-1)out.write(b,0,n);return map(parse(new String(out.toByteArray(),StandardCharsets.UTF_8)));}
        }finally{c.disconnect();}
    }
}
