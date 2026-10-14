package com.notemaker.core;

import java.io.*;
import java.net.*;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.util.*;
import static com.notemaker.core.Json.*;

/** Per-document cached replica with durable journal transactions and shared process locks. */
public final class Store {
    public final File file;
    public final String document;
    private final boolean writable;
    public Store(File directory,String document) throws IOException {this(directory,document,false);}
    public static Store writer(File directory,String document) throws IOException {return new Store(directory,document,true);}
    private Store(File directory,String document,boolean writable) throws IOException {
        this.writable=writable;
        Model.require(!document.isEmpty()&&document.length()<=200,"Document name needs 1–200 characters");
        if(writable)Files.createDirectories(directory.toPath());this.document=document;
        file=new File(directory,"document-"+UUID.nameUUIDFromBytes(document.getBytes(StandardCharsets.UTF_8))+".json");
    }
    public interface Edit { void apply(Map<String,Object> state) throws Exception; }
    private Map<String,Object> locked(Edit edit) throws Exception {Model.require(edit==null||writable,"Writes require the selected proxy");return Journal.access(file,document,edit,writable);}
    public static Model model(Map<String,Object> state){return state.containsKey("effectiveModel")?Model.fromJson(map(state.get("effectiveModel"))):state instanceof Journal.State?new Model(((Journal.State)state).model):new Model(state);}
    public Model model() throws Exception {return model(read());}
    public Map<String,Object> read() throws Exception {return locked(null);}
    public Map<String,Object> edit(Edit edit) throws Exception {return locked(edit);}
    public Map<String,Object> append(Map<String,Object> operation,String actor) throws Exception {
        Model.require(!actor.isEmpty()&&actor.length()<=200,"Invalid actor");
        Model.validate(operation);
        Model.require(write(operation).getBytes(StandardCharsets.UTF_8).length<2999000,"Action exceeds 3 MB");
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
    /** Immutable ingress log gives downstreams a resumable provisional order. */
    public Map<String,Object> accept(List<Object> operations,Object root) throws Exception {
        Map<String,Object> before=read();Map<String,Object> known=index(before);List<Object> fresh=new ArrayList<>();
        Model.require(root==null||root instanceof String,"Invalid root identity");
        Model.require(root==null||!before.containsKey("serverId")||root.equals(before.get("serverId")),"Server database differs from this replica");
        for(Object value:operations){Map<String,Object> op=map(value);Model.validate(op);String id=string(op,"id");
            Model.require(!id.isEmpty()&&id.length()<=200,"Invalid operation ID");
            Model.require(write(op).getBytes(StandardCharsets.UTF_8).length<=3000000,"Action exceeds 3 MB");
            if(known.containsKey(id))Model.require(equivalent(known.get(id),op),"Operation id reused with different contents");
            else {known.put(id,op);fresh.add(op);}
        }
        if(!fresh.isEmpty()||(root!=null&&!before.containsKey("serverId")))edit(state->{
            Model.require(root==null||!state.containsKey("serverId")||root.equals(state.get("serverId")),"Server database differs from this replica");
            if(root!=null)state.put("serverId",root);
            Map<String,Object> current=index(state);for(Object op:fresh){String id=string(map(op),"id");if(current.containsKey(id))Model.require(equivalent(current.get(id),op),"Operation id reused with different contents");else {list(state.get("pending")).add(op);current.put(id,op);}}});
        List<Object> accepted=new ArrayList<>();for(Object op:operations)accepted.add(map(op).get("id"));return object("accepted",accepted);
    }
    private static Map<String,Object> index(Map<String,Object> state){Map<String,Object> out=new HashMap<>();for(String key:Arrays.asList("events","pending"))for(Object op:list(state.get(key)))out.put(string(map(op),"id"),op);return out;}
    private static List<Object> relay(Map<String,Object> state){return list(state.getOrDefault("relay",Collections.emptyList()));}
    public static List<Object> page(List<Object> values,int start){
        Model.require(start>=0&&start<=values.size(),"Invalid replica cursor");List<Object> out=new ArrayList<>();int bytes=0;
        for(int i=start;i<values.size()&&out.size()<100;i++){Object value=values.get(i);int size=write(value).getBytes(StandardCharsets.UTF_8).length;
            if(!out.isEmpty()&&bytes+size>3000000)break;out.add(value);bytes+=size;}return out;
    }
    public Map<String,Object> replica(int after,int pendingAfter) throws Exception {
        Map<String,Object> state=read();List<Object> events=list(state.get("events")),ingress=relay(state);
        List<Object> committed=page(events,after),provisional=page(ingress,pendingAfter);
        return object("events",committed,"after",after+committed.size(),"more",after+committed.size()<events.size(),
            "provisional",provisional,"pendingAfter",pendingAfter+provisional.size(),"pendingMore",pendingAfter+provisional.size()<ingress.size(),"rootId",state.get("serverId"));
    }
    public boolean needsUpload(String peer) throws Exception {
        Map<String,Object> s=read();Set<Object> sent=new HashSet<>();if(peer.equals(s.get("peerId")))sent.addAll(list(s.getOrDefault("forwarded",Collections.emptyList())));
        for(Object op:list(s.get("pending")))if(!sent.contains(map(op).get("id")))return true;return false;
    }
    public interface UploadGate {void start() throws Exception;}
    /** Called only by the proxy single sync worker; intermediate receipt is not final commit. */
    public Map<String,Object> sync(Endpoint upstream,Map<String,Object> info,boolean upload,UploadGate gate) throws Exception {
        String peer=string(info,"id");Object root=info.get("rootId");Map<String,Object> initial=read();
        if(root!=null&&initial.containsKey("serverId"))Model.require(root.equals(initial.get("serverId")),"Server database differs from this replica; use a separate state directory");
        if(!peer.equals(initial.get("peerId")))edit(s->{s.put("peerId",peer);s.put("forwarded",new ArrayList<>());s.put("relayCursor",0);});
        initial=read();Set<Object> sent=new HashSet<>(list(initial.getOrDefault("forwarded",Collections.emptyList())));List<Object> batch=new ArrayList<>();int bytes=0;
        if(upload)for(Object op:list(initial.get("pending"))){if(sent.contains(map(op).get("id")))continue;int size=write(op).getBytes(StandardCharsets.UTF_8).length;if(batch.size()>=100||(!batch.isEmpty()&&bytes+size>3000000))break;batch.add(op);bytes+=size;}
        if(!batch.isEmpty()){
            gate.start();
            List<Object> ack=list(upstream.request("/api/events",object("document",document,"operations",batch,"rootId",root==null?initial.get("serverId"):root)).get("accepted"));
            Set<Object> submitted=new HashSet<>();for(Object op:batch)submitted.add(map(op).get("id"));Model.require(submitted.containsAll(ack),"Unexpected acknowledgement");
            edit(s->{List<Object> forwarded=new ArrayList<>(list(s.getOrDefault("forwarded",Collections.emptyList())));for(Object id:ack)if(!forwarded.contains(id))forwarded.add(id);s.put("forwarded",forwarded);});
        }
        int downloaded=0;
        while(true){
            Map<String,Object> s=read();int after=list(s.get("events")).size(),pendingAfter=((Number)s.getOrDefault("relayCursor",0)).intValue();
            Map<String,Object> response=upstream.request("/api/replica?document="+URLEncoder.encode(document,"UTF-8")+"&after="+after+"&pendingAfter="+pendingAfter,null);
            Object identity=response.get("rootId");if(identity!=null&&s.containsKey("serverId"))Model.require(identity.equals(s.get("serverId")),"Server database differs from this replica");
            List<Object> events=list(response.get("events")),provisional=list(response.get("provisional"));
            Model.require(((Number)response.get("after")).intValue()==after+events.size(),"Invalid commit cursor");
            for(Object op:events)Model.validate(map(op));for(Object op:provisional)Model.validate(map(op));
            int next=((Number)response.get("pendingAfter")).intValue();Model.require(next==pendingAfter+provisional.size(),"Invalid relay cursor");
            if(!events.isEmpty()||!provisional.isEmpty()||(identity!=null&&!identity.equals(s.get("serverId"))))edit(state->{
                Map<String,Object> known=index(state);Set<Object> committed=new HashSet<>();for(Object op:list(state.get("events")))committed.add(map(op).get("id"));
                for(Object op:events){String id=string(map(op),"id");Model.require(!committed.contains(id),"Duplicate committed operation");if(known.containsKey(id))Model.require(equivalent(known.get(id),op),"Operation id reused with different contents");list(state.get("events")).add(op);known.put(id,op);committed.add(id);}
                List<Object> forwarded=new ArrayList<>(list(state.getOrDefault("forwarded",Collections.emptyList())));
                for(Object op:provisional){String id=string(map(op),"id");if(known.containsKey(id))Model.require(equivalent(known.get(id),op),"Operation id reused with different contents");else {list(state.get("pending")).add(op);known.put(id,op);}if(!committed.contains(id)&&!forwarded.contains(id))forwarded.add(id);}
                list(state.get("pending")).removeIf(op->committed.contains(map(op).get("id")));forwarded.removeIf(committed::contains);
                state.put("forwarded",forwarded);state.put("relayCursor",next);state.put("cursor",after+events.size());if(identity!=null)state.put("serverId",identity);
            });
            downloaded+=events.size();if(!Boolean.TRUE.equals(response.get("more"))&&!Boolean.TRUE.equals(response.get("pendingMore")))break;
        }
        return object("uploaded",batch.size(),"downloaded",downloaded,"pending",list(read().get("pending")).size(),"cursor",read().get("cursor"));
    }
}
