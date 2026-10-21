package com.notemaker.core;

import java.io.*;
import java.nio.channels.*;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.util.*;
import java.util.concurrent.ConcurrentHashMap;
import java.util.zip.CRC32;
import static com.notemaker.core.Json.*;

/** Locked append-only transactions plus a process-local materialized replica. */
final class Journal {
    private static final Map<String,Journal> caches=new ConcurrentHashMap<>();
    static final class State extends LinkedHashMap<String,Object> { Model model; }
    private Map<String,Object> state;
    private Model committed, effective;
    private Set<Object> receivedIds;
    private long offset, stamp=-1, size=-1;
    /** A rejected edit has not touched the journal or the cached materialization. */
    private static final class Rejected extends Exception {
        final Exception original;
        Rejected(Exception original){this.original=original;}
    }

    static Map<String,Object> access(File file,String document,Store.Edit edit,boolean writable) throws Exception {
        String path=file.getCanonicalPath();Journal cache=caches.computeIfAbsent(path,k->new Journal());
        synchronized(cache){if(!file.getParentFile().exists())return cache.accessLocked(file,document,edit,false);try(RandomAccessFile lock=new RandomAccessFile(path+".lock","rw");FileLock held=lock.getChannel().lock()){
            try{return cache.accessLocked(file,document,edit,writable);}catch(Rejected e){throw e.original;}catch(Exception e){cache.state=null;throw e;}
        }}
    }
    static Map<String,Object> checkpoint(File file,String document) throws Exception {
        String path=file.getCanonicalPath();Journal cache=caches.computeIfAbsent(path,k->new Journal());
        synchronized(cache){try(RandomAccessFile lock=new RandomAccessFile(path+".lock","rw");FileLock held=lock.getChannel().lock()){
            try {
                cache.accessLocked(file,document,null,true);
                File wal=new File(file+".journal");long before=wal.length();
                if(before==0)return object("revision",cache.state.get("revision"),"journalBytesBefore",0,"journalBytesAfter",0);
                Path temporary=Files.createTempFile(file.getParentFile().toPath(),"document-checkpoint-",".tmp");
                try {
                    Map<String,Object> persisted=new LinkedHashMap<>(cache.state);
                    List<Object> relay=list(cache.state.get("relay")),events=list(cache.state.get("events")),pending=list(cache.state.get("pending"));
                    // An empty stored relay is reconstructed from events and
                    // pending on read. If the same operations have a different
                    // relay order, keep only their IDs in that order.
                    if(relay.size()==events.size()+pending.size()){
                        boolean same=true;
                        for(int i=0;i<relay.size();i++)if(!Objects.equals(relay.get(i),i<events.size()?events.get(i):pending.get(i-events.size()))){same=false;break;}
                        if(same)persisted.put("relay",Collections.emptyList());
                        else{List<Object> order=relayIds(relay,events,pending);if(order!=null){persisted.put("relay",order);persisted.put("relayFormat","ids");}}
                    }
                    CheckpointPointPrefixes.encode(persisted);
                    try(FileOutputStream out=new FileOutputStream(temporary.toFile())){
                        out.write(write(persisted).getBytes(StandardCharsets.UTF_8));out.getFD().sync();
                    }
                    Files.move(temporary,file.toPath(),StandardCopyOption.ATOMIC_MOVE,StandardCopyOption.REPLACE_EXISTING);
                    try(FileChannel directory=FileChannel.open(file.getParentFile().toPath(),StandardOpenOption.READ)){directory.force(true);}
                    // A crash before truncation leaves the old WAL in place. Its
                    // revisions are already in the new base and replay skips them.
                    try(RandomAccessFile log=new RandomAccessFile(wal,"rw")){log.setLength(0);log.getFD().sync();}
                    cache.offset=0;cache.stamp=file.lastModified();cache.size=file.length();
                    return object("revision",cache.state.get("revision"),"journalBytesBefore",before,"journalBytesAfter",0);
                } finally {Files.deleteIfExists(temporary);}
            }catch(Exception e){cache.state=null;throw e;}
        }}
    }
    private Map<String,Object> accessLocked(File file,String document,Store.Edit edit,boolean writable) throws Exception {
        File wal=new File(file+".journal");
        if(state==null||stamp!=file.lastModified()||size!=file.length()||wal.length()<offset){
            if(file.exists()){
                byte[] bytes=Files.readAllBytes(file.toPath());Timing.mark("journal_base_read");
                String source=new String(bytes,StandardCharsets.UTF_8);Timing.mark("journal_base_decoded");
                state=map(parse(source));
            }else state=object("events",new ArrayList<>(),"pending",new ArrayList<>(),"cursor",0);
            Timing.mark("journal_base_parsed");
            CheckpointPointPrefixes.decode(state);
            Timing.mark("journal_points_expanded");
            state.putIfAbsent("relay",new ArrayList<>());
            Object relayFormat=state.remove("relayFormat");
            if("ids".equals(relayFormat))state.put("relay",expandRelayIds(list(state.get("relay")),list(state.get("events")),list(state.get("pending"))));
            else{Model.require(relayFormat==null,"Unknown relay format");if(list(state.get("relay")).isEmpty()){list(state.get("relay")).addAll(list(state.get("events")));list(state.get("relay")).addAll(list(state.get("pending")));}}
            receivedIds=new HashSet<>();for(Object op:list(state.get("relay")))receivedIds.add(map(op).get("id"));
            state.putIfAbsent("actors",object());state.putIfAbsent("revision",0);state.putIfAbsent("document",document);
            Timing.mark("journal_relay_ready");
            committed=new Model();for(Object op:list(state.get("events")))committed.apply(map(op));
            Timing.mark("journal_committed_ready");
            effective=new Model(committed);for(Object op:list(state.get("pending")))effective.apply(map(op));
            Timing.mark("journal_effective_ready");
            offset=0;stamp=file.lastModified();size=file.length();
        }
        if(!file.exists()&&writable){
            File initial=new File(file+".initial");
            try(FileOutputStream out=new FileOutputStream(initial)){out.write(write(state).getBytes(StandardCharsets.UTF_8));out.getFD().sync();}
            Files.move(initial.toPath(),file.toPath(),StandardCopyOption.ATOMIC_MOVE,StandardCopyOption.REPLACE_EXISTING);
            stamp=file.lastModified();size=file.length();
        }
        if(!wal.exists()&&!writable)return snapshot();
        boolean newJournal=!wal.exists();
        try(RandomAccessFile log=new RandomAccessFile(wal,writable?"rw":"r")){
            if(newJournal)try(FileChannel directory=FileChannel.open(file.getParentFile().toPath(),StandardOpenOption.READ)){directory.force(true);}
            log.seek(offset);boolean replayed=false;
            while(log.getFilePointer()<log.length()){
                long start=log.getFilePointer();
                if(log.length()-start<4){if(writable){log.setLength(start);log.getFD().sync();}break;}
                int length=log.readInt();
                if(length<0||length>40000000)throw new IOException("Invalid journal record at "+start);
                if(log.length()-log.getFilePointer()<length+4L){if(writable){log.setLength(start);log.getFD().sync();}break;}
                byte[] bytes=new byte[length];log.readFully(bytes);CRC32 crc=new CRC32();crc.update(bytes);
                if((int)crc.getValue()!=log.readInt())throw new IOException("Journal checksum mismatch at "+start);
                Map<String,Object> delta=map(parse(new String(bytes,StandardCharsets.UTF_8)));
                if(((Number)delta.get("revision")).longValue()>((Number)state.get("revision")).longValue()){apply(delta,true);replayed=true;}
                offset=log.getFilePointer();
            }
            if(replayed){effective=new Model(committed);for(Object op:list(state.get("pending")))effective.apply(map(op));}
            Timing.mark("journal_wal_ready");
            if(edit!=null){
                Timing.mark("journal_begin");
                State next=snapshot();Map<String,Object> delta;byte[] bytes;
                try{Timing.mark("edit_begin");edit.apply(next);Timing.mark("edit_done");delta=difference(next);Timing.mark("delta_done");bytes=write(delta).getBytes(StandardCharsets.UTF_8);Timing.mark("serialize_done");Model.require(bytes.length<=40000000,"Local transaction exceeds 40 MB");}
                catch(Exception e){throw new Rejected(e);}
                CRC32 crc=new CRC32();crc.update(bytes);
                log.seek(offset);log.writeInt(bytes.length);log.write(bytes);log.writeInt((int)crc.getValue());log.getFD().sync();
                Timing.mark("sync_done");
                apply(delta);offset=log.getFilePointer();
                Timing.mark("apply_done");
            }
        }
        return snapshot();
    }
    private State snapshot(){
        State out=new State();out.putAll(state);
        out.put("relay",new ArrayList<>(list(state.get("relay"))));
        out.put("events",new ArrayList<>(list(state.get("events"))));out.put("pending",new ArrayList<>(list(state.get("pending"))));
        if(state.containsKey("leases"))out.put("leases",copy(state.get("leases")));
        if(state.containsKey("activeSessions"))out.put("activeSessions",copy(state.get("activeSessions")));
        Map<String,Object> actors=object();
        // Copy mutable history stacks, sharing immutable action/geometry snapshots.
        for(Map.Entry<String,Object> entry:map(state.get("actors")).entrySet()){
            Map<String,Object> history=new LinkedHashMap<>(map(entry.getValue()));
            history.put("undo",new ArrayList<>(list(history.get("undo"))));history.put("redo",new ArrayList<>(list(history.get("redo"))));actors.put(entry.getKey(),history);
        }
        out.put("actors",actors);out.model=new Model(effective);return out;
    }
    private Map<String,Object> difference(Map<String,Object> next){
        List<Object> oldEvents=list(state.get("events")),newEvents=list(next.get("events"));
        Model.require(newEvents.size()>=oldEvents.size()&&newEvents.subList(0,oldEvents.size()).equals(oldEvents),"Committed events are append-only");
        Map<String,Object> before=new LinkedHashMap<>(),after=new LinkedHashMap<>();
        for(Object op:list(state.get("pending")))before.put(string(map(op),"id"),op);
        for(Object op:list(next.get("pending")))after.put(string(map(op),"id"),op);
        List<Object> added=new ArrayList<>(),removed=new ArrayList<>();
        for(String id:before.keySet())if(!after.containsKey(id))removed.add(id);
        for(Map.Entry<String,Object> e:after.entrySet()){if(!before.containsKey(e.getKey()))added.add(e.getValue());else Model.require(before.get(e.getKey()).equals(e.getValue()),"Pending operations are immutable");}
        Map<String,Object> history=historyDifference(map(state.get("actors")),map(next.get("actors")));
        Map<String,Object> listChanges=object();
        for(String key:Arrays.asList("undo","redo"))if(next.containsKey(key)&&state.containsKey(key)&&!Objects.equals(state.get(key),next.get(key)))listChanges.put(key,historySplice(list(state.get(key)),list(next.get(key))));
        Map<String,Object> meta=object();for(Map.Entry<String,Object> e:next.entrySet())if(!Arrays.asList("events","pending","relay","revision").contains(e.getKey())&&!Objects.equals(state.get(e.getKey()),e.getValue())){
            if(!e.getKey().equals("actors")||history==null)meta.put(e.getKey(),e.getValue());
        }
        for(String key:listChanges.keySet())meta.remove(key);
        Map<String,Object> delta=object("revision",((Number)state.get("revision")).longValue()+1,"events",new ArrayList<>(newEvents.subList(oldEvents.size(),newEvents.size())),"added",added,"removed",removed,"meta",meta);
        if(history!=null&&!history.isEmpty())delta.put("history",history);
        if(!listChanges.isEmpty())delta.put("lists",listChanges);
        return delta;
    }
    private static List<Object> relayIds(List<Object> relay,List<Object> events,List<Object> pending){
        Map<Object,Object> indexed=new HashMap<>();
        for(List<Object> source:Arrays.asList(events,pending))for(Object value:source){Map<String,Object> op=map(value);Object id=op.get("id");if(!(id instanceof String)||indexed.putIfAbsent(id,op)!=null)return null;}
        if(relay.size()!=indexed.size())return null;
        Set<Object> seen=new HashSet<>();List<Object> order=new ArrayList<>();
        for(Object value:relay){Map<String,Object> op=map(value);Object id=op.get("id");if(!seen.add(id)||!Objects.equals(indexed.get(id),op))return null;order.add(id);}
        return order;
    }
    private static List<Object> expandRelayIds(List<Object> ids,List<Object> events,List<Object> pending){
        Map<Object,Object> indexed=new HashMap<>();
        for(List<Object> source:Arrays.asList(events,pending))for(Object value:source){Map<String,Object> op=map(value);Object id=op.get("id");Model.require(id instanceof String&&indexed.putIfAbsent(id,op)==null,"Duplicate or invalid checkpoint operation ID");}
        List<Object> relay=new ArrayList<>();for(Object id:ids){Model.require(id instanceof String&&indexed.containsKey(id),"Invalid checkpoint relay ID");relay.add(indexed.remove(id));}
        Model.require(indexed.isEmpty(),"Checkpoint relay order omits operations");return relay;
    }
    /** Append only the changed suffix of each actor's history; fall back for unknown metadata. */
    private static Map<String,Object> historyDifference(Map<String,Object> before,Map<String,Object> after){
        Map<String,Object> result=object();
        for(String actor:before.keySet())if(!after.containsKey(actor))return null;
        for(Map.Entry<String,Object> entry:after.entrySet()){
            String actor=entry.getKey();Map<String,Object> prior=before.containsKey(actor)?map(before.get(actor)):object();
            Map<String,Object> current=map(entry.getValue());
            Map<String,Object> oldExtra=new LinkedHashMap<>(prior),newExtra=new LinkedHashMap<>(current);
            oldExtra.remove("undo");oldExtra.remove("redo");newExtra.remove("undo");newExtra.remove("redo");
            if(!oldExtra.equals(newExtra))return null;
            List<Object> oldUndo=list(prior.getOrDefault("undo",Collections.emptyList())),newUndo=list(current.getOrDefault("undo",Collections.emptyList()));
            List<Object> oldRedo=list(prior.getOrDefault("redo",Collections.emptyList())),newRedo=list(current.getOrDefault("redo",Collections.emptyList()));
            Map<String,Object> undo=historySplice(oldUndo,newUndo),redo=historySplice(oldRedo,newRedo);
            if(!before.containsKey(actor)&&undo.isEmpty()&&redo.isEmpty())return null;
            if(!undo.isEmpty()||!redo.isEmpty())result.put(actor,object("undo",undo,"redo",redo));
        }
        return result;
    }
    private static Map<String,Object> historySplice(List<Object> before,List<Object> after){
        int shared=0;while(shared<before.size()&&shared<after.size()&&Objects.equals(before.get(shared),after.get(shared)))shared++;
        return shared==before.size()&&shared==after.size()?object():object("pop",before.size()-shared,"add",new ArrayList<>(after.subList(shared,after.size())));
    }
    private void apply(Map<String,Object> delta){apply(delta,false);}
    private void apply(Map<String,Object> delta,boolean replay){
        long revision=((Number)delta.get("revision")).longValue();
        Model.require(revision==((Number)state.get("revision")).longValue()+1,"Journal revision gap");
        List<Object> events=list(delta.get("events")),added=list(delta.get("added")),removed=list(delta.get("removed"));
        for(List<Object> incoming:Arrays.asList(events,added))for(Object op:incoming)if(receivedIds.add(map(op).get("id")))list(state.get("relay")).add(op);
        list(state.get("events")).addAll(events);
        Set<Object> ack=new HashSet<>(removed);list(state.get("pending")).removeIf(o->ack.contains(map(o).get("id")));list(state.get("pending")).addAll(added);
        for(Object op:events)committed.apply(map(op));
        if(!replay){if(!events.isEmpty()||!removed.isEmpty()){
            effective=new Model(committed);for(Object op:list(state.get("pending")))effective.apply(map(op));
        }else for(Object op:added)effective.apply(map(op));}
        state.putAll(map(delta.get("meta")));
        for(Map.Entry<String,Object> entry:map(delta.getOrDefault("lists",object())).entrySet()){
            Model.require(entry.getKey().equals("undo")||entry.getKey().equals("redo"),"Invalid settings list splice");
            applySplice(list(state.get(entry.getKey())),map(entry.getValue()));
        }
        for(Map.Entry<String,Object> entry:map(delta.getOrDefault("history",object())).entrySet()){
            Map<String,Object> actors=map(state.get("actors"));
            Map<String,Object> actor=map(actors.computeIfAbsent(entry.getKey(),k->object("undo",new ArrayList<>(),"redo",new ArrayList<>())));
            Map<String,Object> changes=map(entry.getValue());
            for(String stack:Arrays.asList("undo","redo")){
                Map<String,Object> change=map(changes.get(stack));if(change.isEmpty())continue;
                applySplice(list(actor.get(stack)),change);
            }
        }
        state.put("revision",revision);
    }
    private static void applySplice(List<Object> values,Map<String,Object> change){
        int pop=((Number)change.get("pop")).intValue();
        Model.require(pop>=0&&pop<=values.size(),"Invalid history splice");
        values.subList(values.size()-pop,values.size()).clear();values.addAll(list(change.get("add")));
    }
}
