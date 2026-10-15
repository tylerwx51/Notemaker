package com.notemaker.core;

import java.util.*;
import static com.notemaker.core.Json.*;

/** Shared selection lease and transform rules used by proxy commands. */
public final class Collaboration {
    public static final long LEASE_MILLIS=15000;
    private Collaboration() {}

    /** Resolve a session's entire batch inside its layer before generating changes. */
    public static void requireSelection(Map<String,Object> state,Model model,String actor,String session,List<Object> entities){
        expire(state,System.currentTimeMillis());Map<String,Object> leases=map(state.getOrDefault("leases",object()));LayerEditor editor=null;
        Model.require(!entities.isEmpty(),"Selection needs entities");
        for(Object value:entities){Map<String,Object> ref=map(value);Map<String,Object> lease=map(leases.getOrDefault(key(string(ref,"kind"),string(ref,"id"),ref.getOrDefault("page",0)),object()));
            Model.require(session.equals(lease.get("session"))&&actor.equals(lease.get("actor")),"Edit requires an active selection lease for every entity");
            String layer=(String)lease.getOrDefault("layer","base");int page=((Number)lease.getOrDefault("page",0)).intValue();
            if(editor==null)editor=new LayerEditor(model,layer,page);else Model.require(editor.layer.equals(layer)&&editor.page==page,"Selection spans layers/pages");
        }editor.resolve(entities);
    }

    public static String key(String kind,String id,Object page){return String.valueOf(page==null?0:page)+":"+kind+":"+id;}
    public static void expire(Map<String,Object> state,long now){
        Map<String,Object> leases=map(state.getOrDefault("leases",object()));
        for(String k:new ArrayList<>(leases.keySet()))if(((Number)map(leases.get(k)).getOrDefault("expires",0)).longValue()<=now)leases.remove(k);
        state.put("leases",leases);
    }
    public static List<Object> conflicts(Map<String,Object> state,String actor,List<Object> entities){
        List<Object> blocked=new ArrayList<>();Map<String,Object> leases=map(state.getOrDefault("leases",object()));
        Map<String,Object> sessions=map(state.getOrDefault("activeSessions",object()));String active=(String)sessions.get(actor);
        for(Object value:entities){Map<String,Object> entity=map(value);String k=key(string(entity,"kind"),string(entity,"id"),entity.get("page"));
            for(Map.Entry<String,Object> entry:leases.entrySet()){if(!entry.getKey().equals(k)&&!entry.getKey().endsWith(":"+entity.get("kind")+":"+entity.get("id")))continue;Map<String,Object> lease=map(entry.getValue());if(!lease.isEmpty()&&(!actor.equals(lease.get("actor"))||!Objects.equals(active,lease.get("session"))))blocked.add(object("kind",entity.get("kind"),"id",entity.get("id"),"page",entity.getOrDefault("page",0),"actor",lease.get("actor"),"session",lease.get("session")));}
        }return blocked;
    }
    public static List<Object> affected(List<Object> operations){
        Map<String,Object> unique=new LinkedHashMap<>();for(Object value:operations){Map<String,Object> op=map(value);String kind=String.valueOf(op.get("kind"));
            if(kind.equals("batch")){for(Object nested:list(op.get("actions")))add(unique,map(nested));}else add(unique,op);
        }return new ArrayList<>(unique.values());
    }
    private static void add(Map<String,Object> out,Map<String,Object> op){
        String kind=String.valueOf(op.get("kind"));String entity=kind.equals("object")||kind.equals("object-active")?"object":kind.equals("stroke")||kind.equals("active")?"stroke":null;
        if(entity==null&&Arrays.asList("object","stroke").contains(kind)&&op.containsKey("id"))entity=kind;
        if(entity!=null){String id=String.valueOf(op.getOrDefault(entity,op.get("id")));Map<String,Object> ref=object("kind",entity,"id",id);if(op.containsKey("page"))ref.put("page",op.get("page"));out.putIfAbsent(key(entity,id,op.get("page")),ref);}
    }
    public static void requireUnlocked(Map<String,Object> state,String actor,List<Object> operations,long now){
        expire(state,now);Model model=Store.model(state);List<Object> entities=affected(operations);List<Object> expanded=new ArrayList<>(entities);
        for(Object value:operations){Map<String,Object> op=map(value);if("batch".equals(op.get("kind"))){for(Object nested:list(op.get("actions")))expandLayer(model,map(nested),expanded);}else expandLayer(model,op,expanded);}
        Set<String> touched=new HashSet<>();for(Object value:expanded){Map<String,Object> entity=map(value);if("object".equals(entity.get("kind")))touched.add(String.valueOf(entity.get("id")));}
        for(Map<String,Object> object:model.objects.values())if(touched.contains(object.get("from"))||touched.contains(object.get("to")))expanded.add(object("kind","object","id",object.get("object"),"page",object.getOrDefault("page",0)));
        for(Object value:expanded){Map<String,Object> entity=map(value);if(!entity.containsKey("page")){String kind=string(entity,"kind"),id=string(entity,"id");Map<String,Map<String,Object>> table=kind.equals("object")?model.objects:model.strokes;Map<String,Object> old=table.get(id);if(old!=null)entity.put("page",old.getOrDefault("page",0));}}
        List<Object> blocked=conflicts(state,actor,expanded);
        Model.require(blocked.isEmpty(),"Locked entities: "+write(blocked));
    }
    /** Guard a child's raw ingress against leases known to this proxy without requiring a local UI session record. */
    public static void requireIncomingUnlocked(Map<String,Object> state,Map<String,Object> operation,long now){
        expire(state,now);List<Object> operations="batch".equals(operation.get("kind"))?list(operation.get("actions")):Arrays.asList(operation);List<Object> entities=affected(operations);Model model=Store.model(state);List<Object> expanded=new ArrayList<>(entities);
        for(Object value:operations)expandLayer(model,map(value),expanded);
        Set<String> touched=new HashSet<>();for(Object value:expanded){Map<String,Object> entity=map(value);if("object".equals(entity.get("kind")))touched.add(String.valueOf(entity.get("id")));}for(Map<String,Object> object:model.objects.values())if(touched.contains(object.get("from"))||touched.contains(object.get("to")))expanded.add(object("kind","object","id",object.get("object"),"page",object.getOrDefault("page",0)));
        for(Object value:expanded){Map<String,Object> entity=map(value);if(!entity.containsKey("page")){String kind=string(entity,"kind"),id=string(entity,"id");Map<String,Map<String,Object>> table=kind.equals("object")?model.objects:model.strokes;Map<String,Object> old=table.get(id);if(old!=null)entity.put("page",old.getOrDefault("page",0));}}
        String actor=String.valueOf(operation.getOrDefault("actor","")),session=String.valueOf(operation.getOrDefault("session",""));Map<String,Object> leases=map(state.getOrDefault("leases",object()));Set<String> refs=new HashSet<>();for(Object value:expanded){Map<String,Object> entity=map(value);String kind=String.valueOf(entity.get("kind")),id=String.valueOf(entity.get("id"));refs.add(key(kind,id,entity.get("page")));}
        List<Object> blocked=new ArrayList<>();for(String ref:refs){String suffix=ref.substring(ref.indexOf(':')+1);for(Map.Entry<String,Object> entry:leases.entrySet()){if(!entry.getKey().equals(ref)&&!entry.getKey().endsWith(":"+suffix))continue;Map<String,Object> lease=map(entry.getValue());if(!lease.isEmpty()&&(!actor.equals(lease.get("actor"))||!session.equals(lease.get("session"))))blocked.add(object("key",entry.getKey(),"actor",lease.get("actor"),"session",lease.get("session")));}}
        Model.require(blocked.isEmpty(),"Locked entities: "+write(blocked));
    }
    private static void expandLayer(Model model,Map<String,Object> op,List<Object> out){String kind=String.valueOf(op.get("kind")),layer=String.valueOf(op.get("layer"));boolean affects="layer-active".equals(kind);if("layer".equals(kind)&&op.containsKey("order")){Map<String,Object> old=model.layers.get(layer);affects=old==null||!Objects.equals(old.get("order"),op.get("order"));}if(!affects)return;for(Map<String,Object> value:model.objects.values())if(layer.equals(value.get("layer")))out.add(object("kind","object","id",value.get("object"),"page",value.getOrDefault("page",0)));for(Map<String,Object> value:model.strokes.values())if(layer.equals(value.get("layer")))out.add(object("kind","stroke","id",value.get("stroke"),"page",value.getOrDefault("page",0)));}
    /** Transform selected objects and strokes into a single list of replacement actions. */
    public static List<Object> transform(Model model,List<Object> entities,double scale,double dx,double dy){
        return transform(model,entities,object("scale",scale,"dx",dx,"dy",dy));
    }
    public static List<Object> transform(Model model,List<Object> entities,Map<String,Object> spec){
        for(String key:Arrays.asList("scale","dx","dy","rotation","pivotX","pivotY"))if(spec.containsKey(key))Model.require(spec.get(key) instanceof Number,"Invalid transform field: "+key);
        for(String key:Arrays.asList("flipX","flipY"))if(spec.containsKey(key))Model.require(spec.get(key) instanceof Boolean,"Invalid transform field: "+key);
        double scale=((Number)spec.getOrDefault("scale",1)).doubleValue(),dx=((Number)spec.getOrDefault("dx",0)).doubleValue(),dy=((Number)spec.getOrDefault("dy",0)).doubleValue(),rotation=((Number)spec.getOrDefault("rotation",0)).doubleValue();boolean flipX=Boolean.TRUE.equals(spec.get("flipX")),flipY=Boolean.TRUE.equals(spec.get("flipY"));
        Model.require(Double.isFinite(scale)&&scale>0&&scale<=100,"Scale must be > 0 and <= 100");
        Model.require(Double.isFinite(dx)&&Double.isFinite(dy)&&Math.abs(dx)<=1000000&&Math.abs(dy)<=1000000,"Invalid translation");Model.require(Double.isFinite(rotation)&&Math.abs(rotation)<=360,"Rotation must be within 360 degrees");
        Set<String> selectedObjects=new HashSet<>();for(Object value:entities){Map<String,Object> e=map(value);if("object".equals(e.get("kind")))selectedObjects.add(string(e,"id"));}
        double minX=Double.POSITIVE_INFINITY,minY=minX,maxX=Double.NEGATIVE_INFINITY,maxY=maxX;for(Object value:entities){Map<String,Object> e=map(value);String kind=string(e,"kind"),id=string(e,"id");Map<String,Object> old=kind.equals("object")?model.objects.get(id):model.strokes.get(id);Model.require(old!=null,"Unknown transform entity: "+id);if(kind.equals("object")){boolean attached=("arrow".equals(old.get("shape"))||"arrowLine".equals(old.get("shape")))&&(old.containsKey("from")||old.containsKey("to"));if(attached&&(selectedObjects.contains(old.get("from"))||selectedObjects.contains(old.get("to"))))continue;double x=num(old,"x"),y=num(old,"y"),w=num(old,"w"),h=num(old,"h");if(attached){double[] ends=Render.endpoints(model,old);x=ends[0];y=ends[1];w=ends[2]-ends[0];h=ends[3]-ends[1];}minX=Math.min(minX,Math.min(x,x+w));maxX=Math.max(maxX,Math.max(x,x+w));minY=Math.min(minY,Math.min(y,y+h));maxY=Math.max(maxY,Math.max(y,y+h));}else for(Object point:list(old.get("points"))){List<Object> p=list(point);minX=Math.min(minX,num(p.get(0)));maxX=Math.max(maxX,num(p.get(0)));minY=Math.min(minY,num(p.get(1)));maxY=Math.max(maxY,num(p.get(1)));}}
        Model.require(minX!=Double.POSITIVE_INFINITY,"Transform needs entities");double px=((Number)spec.getOrDefault("pivotX",(minX+maxX)/2)).doubleValue(),py=((Number)spec.getOrDefault("pivotY",(minY+maxY)/2)).doubleValue();Model.require(Double.isFinite(px)&&Double.isFinite(py)&&Math.abs(px)<=1000000&&Math.abs(py)<=1000000,"Invalid transform pivot");double r=Math.toRadians(rotation),cos=Math.cos(r),sin=Math.sin(r);
        java.util.function.Function<double[],double[]> point=p->{double x=(p[0]-px)*(flipX?-1:1)*scale,y=(p[1]-py)*(flipY?-1:1)*scale;return new double[]{px+x*cos-y*sin+dx,py+x*sin+y*cos+dy};};
        List<Object> out=new ArrayList<>();Set<String> seen=new HashSet<>();
        for(Object value:entities){Map<String,Object> e=map(value);String kind=string(e,"kind"),id=string(e,"id");Model.require(seen.add(key(kind,id,e.get("page"))),"Duplicate transform entity");
            if(kind.equals("object")){Map<String,Object> old=model.objects.get(id);Model.require(old!=null,"Unknown object: "+id);
                boolean attached=("arrow".equals(old.get("shape"))||"arrowLine".equals(old.get("shape")))&&(old.containsKey("from")||old.containsKey("to"));
                if(attached&&(selectedObjects.contains(old.get("from"))||selectedObjects.contains(old.get("to"))))continue;
                Map<String,Object> next=map(copy(old));
                if(attached){double[] ends=Render.endpoints(model,old);next.put("x",ends[0]);next.put("y",ends[1]);next.put("w",ends[2]-ends[0]);next.put("h",ends[3]-ends[1]);next.remove("from");next.remove("to");}
                double x=num(next,"x"),y=num(next,"y"),w=num(next,"w"),h=num(next,"h");double[] center=point.apply(new double[]{x+w/2,y+h/2});next.put("x",center[0]-w*scale/2);next.put("y",center[1]-h*scale/2);next.put("w",w*scale);next.put("h",h*scale);double oldRotation=((Number)old.getOrDefault("rotation",0)).doubleValue();next.put("rotation",(rotation+((flipX^flipY)?-oldRotation:oldRotation))%360);next.put("flipX",Boolean.TRUE.equals(old.get("flipX"))^flipX);next.put("flipY",Boolean.TRUE.equals(old.get("flipY"))^flipY);
                if(old.containsKey("fontSize"))next.put("fontSize",num(old,"fontSize")*scale);Model.validate(next);out.add(next);
            }else if(kind.equals("stroke")){Map<String,Object> old=model.strokes.get(id);Model.require(old!=null,"Unknown stroke: "+id);Map<String,Object> next=map(copy(old));List<Object> points=new ArrayList<>();
                for(Object p:list(old.get("points"))){List<Object> xyz=list(p);double[] target=point.apply(new double[]{num(xyz.get(0)),num(xyz.get(1))});points.add(Arrays.asList(target[0],target[1],xyz.get(2)));}next.put("points",points);next.put("width",num(old,"width")*scale);if(old.containsKey("erasures")){List<Object> masks=new ArrayList<>();for(Object raw:list(old.get("erasures"))){double[][] polygon=Geometry.polygon(raw);for(double[] v:polygon){double x=v[0]*(flipX?-1:1)*scale,y=v[1]*(flipY?-1:1)*scale;v[0]=x*cos-y*sin;v[1]=x*sin+y*cos;}masks.add(Geometry.json(polygon));}next.put("erasures",masks);}Model.validate(next);out.add(next);
            }else throw new IllegalArgumentException("Transform kind must be object or stroke");
        }Model.require(!out.isEmpty()&&out.size()<=200,"Transform needs 1–200 entities");return out;
    }
    private static double num(Map<String,Object> o,String k){return ((Number)o.get(k)).doubleValue();}
    private static double num(Object v){return ((Number)v).doubleValue();}
}
