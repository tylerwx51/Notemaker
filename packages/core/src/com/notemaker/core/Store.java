package com.notemaker.core;

import java.io.*;
import java.net.*;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.util.*;
import static com.notemaker.core.Json.*;

/** Per-document cached replica with durable journal transactions and shared process locks. */
public final class Store {
    private static final Map<String,Coordination> COORDINATIONS=new java.util.concurrent.ConcurrentHashMap<>();
    private static final class Coordination {Map<String,Object> leases=new LinkedHashMap<>(),sessions=new LinkedHashMap<>();}
    public final File file;
    public final String document;
    private final boolean writable;
    // Selection leases are process-local coordination state, never part of the durable action journal.
    private final Coordination coordination;
    public Store(File directory,String document) throws IOException {this(directory,document,false);}
    public static Store writer(File directory,String document) throws IOException {return new Store(directory,document,true);}
    private Store(File directory,String document,boolean writable) throws IOException {
        this.writable=writable;
        Model.require(!document.isEmpty()&&document.length()<=200,"Document name needs 1–200 characters");
        if(writable)Files.createDirectories(directory.toPath());this.document=document;
        file=new File(directory,"document-"+UUID.nameUUIDFromBytes(document.getBytes(StandardCharsets.UTF_8))+".json");
        coordination=COORDINATIONS.computeIfAbsent(file.getCanonicalPath(),ignored->new Coordination());
    }
    public interface Edit { void apply(Map<String,Object> state) throws Exception; }
    private Map<String,Object> locked(Edit edit) throws Exception {Model.require(edit==null||writable,"Writes require the selected proxy");return Journal.access(file,document,edit,writable);}
    /** In-process snapshots retain the immutable materialization; JSON clients use effectiveModel. */
    public static final class Snapshot extends LinkedHashMap<String,Object>{private final Model model;public Snapshot(Model model){this.model=new Model(model);}}
    public static Model model(Map<String,Object> state){return state instanceof Snapshot?new Model(((Snapshot)state).model):state.containsKey("effectiveModel")?Model.fromJson(map(state.get("effectiveModel"))):state instanceof Journal.State?new Model(((Journal.State)state).model):new Model(state);}
    public Model model() throws Exception {return model(read());}
    private Map<String,Object> overlay(Map<String,Object> state){state.remove("leases");state.remove("activeSessions");state.put("leases",copy(coordination.leases));state.put("activeSessions",copy(coordination.sessions));return state;}
    public Map<String,Object> read() throws Exception {synchronized(coordination){return overlay(locked(null));}}
    /** Save the materialized replica as a durable base without changing its revision. */
    public Map<String,Object> checkpoint() throws Exception {synchronized(coordination){
        Model.require(writable,"Checkpoint requires the selected proxy");
        return Journal.checkpoint(file,document);
    }}
    public Map<String,Object> edit(Edit edit) throws Exception {synchronized(coordination){
        final Map<String,Object> leases=new LinkedHashMap<>(),sessions=new LinkedHashMap<>();
        Map<String,Object> state=locked(s->{overlay(s);edit.apply(s);leases.putAll(map(copy(s.get("leases"))));sessions.putAll(map(copy(s.get("activeSessions"))));s.remove("leases");s.remove("activeSessions");});
        coordination.leases=leases;coordination.sessions=sessions;return overlay(state);
    }}
    /** Atomic update of ephemeral coordination metadata without creating a journal revision. */
    public Map<String,Object> editTransient(Edit edit) throws Exception {synchronized(coordination){
        Model.require(writable,"Writes require the selected proxy");Map<String,Object> state=overlay(locked(null));edit.apply(state);
        coordination.leases=new LinkedHashMap<>(map(copy(state.get("leases"))));coordination.sessions=new LinkedHashMap<>(map(copy(state.get("activeSessions"))));return overlay(state);
    }}
    public Map<String,Object> append(Map<String,Object> operation,String actor) throws Exception {
        Model.require(!actor.isEmpty()&&actor.length()<=200,"Invalid actor");
        Model.validate(operation);
        Model.require(write(operation).getBytes(StandardCharsets.UTF_8).length<=3000000,"Action exceeds 3 MB");
        return edit(state->{Model.require(!Store.model(state).deleted,"Document is deleted");Collaboration.requireUnlocked(state,actor,Arrays.asList(operation),System.currentTimeMillis());Map<String,Object> op=map(copy(operation));op.put("id",UUID.randomUUID().toString());op.put("actor",actor);Model.withBases(op,Store.model(state));Object session=map(state.getOrDefault("activeSessions",object())).get(actor);if(session!=null)op.put("session",session);list(state.get("pending")).add(op);});
    }
    /** Permanent, idempotent lifecycle action; histories remain in the journal. */
    public Map<String,Object> deleteDocument(String actor) throws Exception {
        Model.require(!actor.isEmpty()&&actor.length()<=200,"Invalid actor");
        Map<String,Object> result=object("document",document,"deleted",true);
        edit(state->{
            if(Store.model(state).deleted)return;
            String id=Actions.id();
            list(state.get("pending")).add(object("kind","document-delete","id",id,"actor",actor));
            state.put("leases",object());state.put("activeSessions",object());result.put("operation",id);
        });return result;
    }
    public Map<String,Object> commit(String actor,Actions.Build build) throws Exception {
        return commit(actor,build,Collections.emptyList());
    }
    public Map<String,Object> commit(String actor,Actions.Build build,List<Object> dependencies) throws Exception {
        return commit(actor,build,dependencies,null,Collections.emptyList());
    }
    public Map<String,Object> commitSelected(String actor,String session,List<Object> entities,Actions.Build build,List<Object> dependencies) throws Exception {
        return commit(actor,build,dependencies,session,entities);
    }
    private Map<String,Object> commit(String actor,Actions.Build build,List<Object> dependencies,String requiredSession,List<Object> selected) throws Exception {
        Model.require(!actor.isEmpty()&&actor.length()<=200,"Invalid actor");
        final Map<String,Object> result=object();
        edit(state->{
            Model model=Store.model(state);Model.require(!model.deleted,"Document is deleted");if(requiredSession!=null)Collaboration.requireSelection(state,model,actor,requiredSession,selected);Model baseline=new Model(model);Timing.mark("geometry_begin");List<Object> requested=build.create(model),ops=new ArrayList<>();Timing.mark("geometry_done");
            for(Object value:requested){Map<String,Object> op=map(value);ops.add(op);String kind=string(op,"kind");
                String key=kind.equals("object")?"object":kind.equals("layer")?"layer":kind.equals("stroke")?"stroke":null;
                if(key!=null&&!(key.equals("object")?model.objects:key.equals("layer")?model.layers:model.strokes).containsKey(op.get(key)))ops.add(object("kind",key.equals("stroke")?"active":key+"-active",key,op.get(key),"active",true));
            }
            Model.require(!ops.isEmpty()&&ops.size()<=200,"Action needs 1–200 operations");
            List<Object> inverse=new ArrayList<>();
            for(Object v:ops){Map<String,Object> op=map(v);Model.require(!Arrays.asList("batch","document","document-delete").contains(op.get("kind")),"Use document create; nested batches are not allowed");Model.validate(op);inverse.add(0,Actions.inverse(model,op));model.apply(op);}
            Timing.mark("inverse_done");
            Collaboration.requireUnlocked(state,actor,ops,System.currentTimeMillis());
            Collaboration.requireUnlocked(state,actor,dependencies,System.currentTimeMillis());
            Actions.validateReferences(model,ops);
            Timing.mark("locks_done");
            Map<String,Object> batch=object("kind","batch","id",UUID.randomUUID().toString(),"actor",actor,"actions",ops);Model.withBases(batch,baseline);Object session=map(state.getOrDefault("activeSessions",object())).get(actor);if(session!=null)batch.put("session",session);
            Model.require(write(batch).getBytes(StandardCharsets.UTF_8).length<=3000000,"Action exceeds 3 MB");
            Model.require(write(inverse).getBytes(StandardCharsets.UTF_8).length<=3000000,"Undo snapshot exceeds 3 MB; split this action");
            Timing.mark("limits_done");
            list(state.get("pending")).add(batch);
            Map<String,Object> history=history(state,actor);list(history.get("undo")).add(object("forward",copy(ops),"inverse",inverse));history.put("redo",new ArrayList<>());
            result.put("operation",batch.get("id"));result.put("actions",ops);
        });return result;
    }
    private static Map<String,Object> history(Map<String,Object>state,String actor){Map<String,Object> actors=map(state.get("actors"));actors.putIfAbsent(actor,object("undo",new ArrayList<>(),"redo",new ArrayList<>()));return map(actors.get(actor));}
    public Map<String,Object> history(String actor,boolean redo) throws Exception {
        final Map<String,Object> result=object();
        edit(state->{Model.require(!Store.model(state).deleted,"Document is deleted");Map<String,Object> h=history(state,actor);List<Object> from=list(h.get(redo?"redo":"undo")),to=list(h.get(redo?"undo":"redo"));Model.require(!from.isEmpty(),"No "+(redo?"redo":"undo")+" for actor "+actor);Map<String,Object> entry=map(from.get(from.size()-1));Collaboration.requireUnlocked(state,actor,list(entry.get(redo?"forward":"inverse")),System.currentTimeMillis());from.remove(from.size()-1);to.add(entry);
            String id=UUID.randomUUID().toString();Map<String,Object> batch=object("kind","batch","id",id,"actor",actor,"actions",copy(entry.get(redo?"forward":"inverse")));Model.withBases(batch,Store.model(state));Object session=map(state.getOrDefault("activeSessions",object())).get(actor);if(session!=null)batch.put("session",session);list(state.get("pending")).add(batch);result.put("operation",id);
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
            for(Object op:fresh)Collaboration.requireIncomingUnlocked(state,map(op),System.currentTimeMillis());
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
        Map<String,Object> localLocks=read();Object localLeases=localLocks.getOrDefault("leases",object());if(!map(localLeases).isEmpty())upstream.request("/api/locks",object("document",document,"leases",localLeases,"remove",Collections.emptyList()));
        Map<String,Object> locks=upstream.request("/api/locks?document="+URLEncoder.encode(document,"UTF-8"),null);
        if(locks.containsKey("leases"))editTransient(state->{Map<String,Object> leases=map(copy(locks.get("leases")));Collaboration.expire(state,System.currentTimeMillis());state.put("leases",leases);});
        return object("uploaded",batch.size(),"downloaded",downloaded,"pending",list(read().get("pending")).size(),"cursor",read().get("cursor"),"previews",locks.getOrDefault("previews",object()));
    }
    public List<Object> conflicts() throws Exception {return model().visibleConflicts();}
    public Object conflict(String id,String choice,String actor) throws Exception {
        Model.require(Arrays.asList("local","remote","both").contains(choice),"Conflict choice must be local, remote, or both");
        return commit(actor,m->{Map<String,Object> c=m.conflicts.get(id);Model.require(c!=null&&!Boolean.FALSE.equals(m.conflictActive.get(id)),"Unknown active conflict");
            String side=choice.equals("remote")?"remote":"local";Map<String,Object> chosen=map(copy(c.get(side)));String kind=string(chosen,"kind");List<Object> actions=new ArrayList<>();actions.add(chosen);actions.add(object("kind",kind.equals("stroke")?"active":"object-active",kind,chosen.get(kind),"active",c.get(side+"Active")));
            if(choice.equals("both")){Map<String,Object> duplicate=map(copy(c.get("remote")));duplicate.put(kind,Actions.id());if(kind.equals("object")){duplicate.put("x",number(duplicate,"x")+24);duplicate.put("y",number(duplicate,"y")+24);}else{List<Object> points=new ArrayList<>();for(Object value:list(duplicate.get("points"))){List<Object> p=list(value);points.add(Arrays.asList(((Number)p.get(0)).doubleValue()+24,((Number)p.get(1)).doubleValue()+24,p.get(2)));}duplicate.put("points",points);}actions.add(duplicate);actions.add(object("kind",kind.equals("stroke")?"active":"object-active",kind,duplicate.get(kind),"active",c.get("remoteActive")));}
            actions.add(object("kind","conflict-status","conflict",id,"active",false));return actions;});
    }
}
