package com.notemaker.core;

import java.util.*;
import static com.notemaker.core.Json.*;

/** Checkpoint-only prefix references for successive snapshots of one stroke. */
final class CheckpointPointPrefixes {
    private static final String FORMAT="stroke-prefix-v1";
    private static final String FORMAT_KEY="checkpointPointFormat";
    private static final String COUNTS_KEY="checkpointPointPrefixes";

    private CheckpointPointPrefixes() {}

    static void encode(Map<String,Object> state) {
        Encoder encoder=new Encoder();
        List<Object> events=encoder.operations(list(state.get("events")));
        List<Object> pending=encoder.operations(list(state.get("pending")));
        if(!encoder.compressed)return;
        state.put("events",events);
        state.put("pending",pending);
        state.put(FORMAT_KEY,FORMAT);
        state.put(COUNTS_KEY,encoder.counts);
    }

    static void decode(Map<String,Object> state) {
        Object format=state.remove(FORMAT_KEY);
        if(format==null){Model.require(!state.containsKey(COUNTS_KEY),"Checkpoint point prefix counts without format");return;}
        Model.require(FORMAT.equals(format),"Unknown checkpoint point format");
        Decoder decoder=new Decoder(list(state.remove(COUNTS_KEY)));
        state.put("events",decoder.operations(list(state.get("events"))));
        state.put("pending",decoder.operations(list(state.get("pending"))));
        Model.require(decoder.index==decoder.counts.size(),"Checkpoint point prefix count mismatch");
    }

    private static final class Encoder {
        final Map<String,List<Object>> prior=new HashMap<>();
        final List<Object> counts=new ArrayList<>();
        boolean compressed;

        List<Object> operations(List<Object> source){
            List<Object> out=new ArrayList<>(source.size());
            for(Object raw:source)out.add(operation(map(raw)));
            return out;
        }
        Map<String,Object> operation(Map<String,Object> op){
            if("batch".equals(op.get("kind"))){
                Map<String,Object> out=new LinkedHashMap<>(op);
                out.put("actions",operations(list(op.get("actions"))));
                return out;
            }
            if(!"stroke".equals(op.get("kind")))return op;
            String id=string(op,"stroke");List<Object> points=list(op.get("points"));
            List<Object> previous=prior.get(id);int prefix=0;
            if(previous!=null)while(prefix<points.size()&&prefix<previous.size()&&Objects.equals(points.get(prefix),previous.get(prefix)))prefix++;
            prior.put(id,points);
            if(prefix<8){counts.add(0);return op;}
            counts.add(prefix);compressed=true;
            Map<String,Object> out=new LinkedHashMap<>(op);
            out.put("points",new ArrayList<>(points.subList(prefix,points.size())));
            return out;
        }
    }

    private static final class Decoder {
        final Map<String,List<Object>> prior=new HashMap<>();
        final List<Object> counts;
        int index;
        Decoder(List<Object> counts){this.counts=counts;}

        List<Object> operations(List<Object> source){
            List<Object> out=new ArrayList<>(source.size());
            for(Object raw:source)out.add(operation(map(raw)));
            return out;
        }
        Map<String,Object> operation(Map<String,Object> op){
            if("batch".equals(op.get("kind"))){
                Map<String,Object> out=new LinkedHashMap<>(op);
                out.put("actions",operations(list(op.get("actions"))));
                return out;
            }
            if(!"stroke".equals(op.get("kind")))return op;
            Model.require(index<counts.size(),"Checkpoint point prefix count missing");
            Object raw=counts.get(index++);
            Model.require(raw instanceof Number,"Invalid checkpoint point prefix");
            int prefix=((Number)raw).intValue();
            Model.require(prefix>=0&&((Number)raw).doubleValue()==prefix,"Invalid checkpoint point prefix");
            String id=string(op,"stroke");List<Object> tail=list(op.get("points"));
            List<Object> previous=prior.get(id);
            Model.require(prefix==0||previous!=null&&prefix<=previous.size(),"Invalid checkpoint point prefix reference");
            if(prefix==0){prior.put(id,tail);return op;}
            Model.require(prefix+tail.size()<=10000,"Checkpoint stroke has too many points");
            List<Object> points=new ArrayList<>(prefix+tail.size());
            points.addAll(previous.subList(0,prefix));points.addAll(tail);
            Map<String,Object> out=new LinkedHashMap<>(op);out.put("points",points);prior.put(id,points);return out;
        }
    }
}
