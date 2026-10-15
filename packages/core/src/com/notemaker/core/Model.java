package com.notemaker.core;

import java.util.*;
import static com.notemaker.core.Json.*;

/** Deterministic replay. Server order, followed by unacknowledged local operations. */
public final class Model {
    public String type="fixed";
    private boolean documentSeen;
    public final Map<String,String> versions=new LinkedHashMap<>();
    public final Map<String,String> versionActors=new LinkedHashMap<>();
    public final Map<String,Map<String,Object>> conflicts=new LinkedHashMap<>();
    public final Map<String,Boolean> conflictActive=new LinkedHashMap<>();
    public final Map<String,Map<String,Object>> layers=new LinkedHashMap<>(), strokes=new LinkedHashMap<>(), objects=new LinkedHashMap<>();
    public final Map<String,Boolean> active=new LinkedHashMap<>(), objectActive=new LinkedHashMap<>(), layerActive=new LinkedHashMap<>();
    private Map<String,LinkedHashMap<String,Map<String,Object>>> itemIndex;
    private final Map<String,Integer> itemOrder=new HashMap<>();
    private String itemScope(Map<String,Object> o){return o.get("layer")+":"+((Number)o.get("page")).intValue();}
    private void indexItem(Map<String,Object> o){String kind=string(o,"kind"),key=kind+":"+string(o,kind);if(!itemOrder.containsKey(key))itemOrder.put(key,itemOrder.size());itemIndex.computeIfAbsent(itemScope(o),k->new LinkedHashMap<>()).put(key,o);}
    public Collection<Map<String,Object>> layerItems(String layer,int page){
        if(itemIndex==null){itemIndex=new HashMap<>();for(Map<String,Object> o:strokes.values())indexItem(o);for(Map<String,Object> o:objects.values())indexItem(o);}
        Collection<Map<String,Object>> source=itemIndex.getOrDefault(layer+":"+page,new LinkedHashMap<>()).values();
        List<Map<String,Object>> ordered=new ArrayList<>(source);ordered.sort(Comparator.<Map<String,Object>>comparingInt(o->"stroke".equals(o.get("kind"))?0:1).thenComparingInt(o->itemOrder.get(o.get("kind")+":"+o.get(string(o,"kind")))));return Collections.unmodifiableList(ordered);
    }
    private void putItem(Map<String,Object> o,Map<String,Map<String,Object>> table,String kind){Map<String,Object> old=table.put(string(o,kind),o);if(itemIndex!=null){if(old!=null&&!itemScope(old).equals(itemScope(o)))itemIndex.get(itemScope(old)).remove(kind+":"+string(o,kind));indexItem(o);}}
    public Model(){layers.put("base",object("kind","layer","layer","base","name","Layer 1","force",false));}
    /** Entity snapshots are immutable after publication; copy only the indexes. */
    public Model(Model other){type=other.type;documentSeen=other.documentSeen;layers.putAll(other.layers);strokes.putAll(other.strokes);objects.putAll(other.objects);active.putAll(other.active);objectActive.putAll(other.objectActive);layerActive.putAll(other.layerActive);versions.putAll(other.versions);versionActors.putAll(other.versionActors);conflicts.putAll(other.conflicts);conflictActive.putAll(other.conflictActive);}
    public static Model fromJson(Map<String,Object> value){
        Model model=new Model();model.type=string(value,"type");model.documentSeen=true;
        for(String key:Arrays.asList("layers","strokes","objects"))for(Object op:list(value.get(key)))model.apply(map(op));
        for(Map.Entry<String,Object> e:map(value.get("strokeActive")).entrySet())model.active.put(e.getKey(),(Boolean)e.getValue());
        for(Map.Entry<String,Object> e:map(value.get("objectActive")).entrySet())model.objectActive.put(e.getKey(),(Boolean)e.getValue());
        for(Map.Entry<String,Object> e:map(value.get("layerActive")).entrySet())model.layerActive.put(e.getKey(),(Boolean)e.getValue());for(Map.Entry<String,Object> e:map(value.getOrDefault("versions",object())).entrySet())model.versions.put(e.getKey(),(String)e.getValue());
        for(Map.Entry<String,Object> e:map(value.getOrDefault("versionActors",object())).entrySet())model.versionActors.put(e.getKey(),(String)e.getValue());
        for(Object c:list(value.getOrDefault("conflicts",Collections.emptyList())))model.conflicts.put(string(map(c),"id"),map(c));return model;
    }
    public Model(Map<String,Object> state){this();for(Object v:list(state.get("events")))apply(map(v));for(Object v:list(state.get("pending")))apply(map(v));}
    public void apply(Map<String,Object> op){
        String operation=(String)op.get("id");List<Object> actions="batch".equals(op.get("kind"))?list(op.get("actions")):Arrays.asList(op);
        Map<String,Object> bases=map(op.getOrDefault("bases",object())),visibility=map(op.getOrDefault("visibility",object()));Map<String,Map<String,Object>> before=new LinkedHashMap<>();Map<String,Boolean> beforeActive=new HashMap<>();Map<String,String> oldVersions=new HashMap<>();
        for(Object value:actions){String key=entityKey(map(value));if(key!=null&&!before.containsKey(key)){before.put(key,snapshot(key));beforeActive.put(key,isActive(key));oldVersions.put(key,versions.getOrDefault(key,""));}}
        for(Object value:actions)applyRaw(map(value));
        if(operation!=null)for(String key:before.keySet()){
            Map<String,Object> previous=before.get(key),next=snapshot(key);String prior=oldVersions.get(key);boolean nextActive=(Boolean)visibility.getOrDefault(key,isActive(key));
            boolean trackedBranch=false;
            if(next!=null&&bases.containsKey(key))for(Map.Entry<String,Map<String,Object>> entry:new ArrayList<>(conflicts.entrySet())){
                Map<String,Object> conflict=entry.getValue();if(!key.equals(conflict.get("key"))||Boolean.FALSE.equals(conflictActive.get(entry.getKey())))continue;
                String side=Objects.equals(bases.get(key),conflict.get("localVersion"))?"local":Objects.equals(bases.get(key),conflict.get("remoteVersion"))?"remote":null;
                if(side!=null){Map<String,Object> updated=new LinkedHashMap<>(conflict);updated.put(side,copy(next));updated.put(side+"Active",nextActive);updated.put(side+"Version",operation);updated.put(side+"Actor",op.getOrDefault("actor",""));conflicts.put(entry.getKey(),updated);trackedBranch=true;}
            }
            if(!trackedBranch&&bases.containsKey(key)&&!Objects.equals(bases.get(key),prior)&&previous!=null&&next!=null&&(!previous.equals(next)||beforeActive.get(key)!=isActive(key))){
                String id=UUID.nameUUIDFromBytes((key+":"+prior+":"+operation).getBytes(java.nio.charset.StandardCharsets.UTF_8)).toString();String kind=key.substring(0,key.indexOf(':'));
                conflicts.putIfAbsent(id,object("id",id,"key",key,"kind",kind,"entity",key.substring(key.indexOf(':')+1),"page",next.getOrDefault("page",0),"local",copy(previous),"remote",copy(next),"localActive",beforeActive.get(key),"remoteActive",nextActive,"localVersion",prior,"remoteVersion",operation,"localActor",versionActors.getOrDefault(key,""),"remoteActor",op.getOrDefault("actor","")));
            }versions.put(key,operation);versionActors.put(key,(String)op.getOrDefault("actor",""));
        }
    }
    private static String entityKey(Map<String,Object> op){String kind=string(op,"kind");String entity=kind.equals("object")||kind.equals("object-active")?"object":kind.equals("stroke")||kind.equals("active")?"stroke":null;return entity==null?null:entity+":"+string(op,entity);}
    private Map<String,Object> snapshot(String key){return (key.startsWith("object:")?objects:strokes).get(key.substring(key.indexOf(':')+1));}
    private boolean isActive(String key){return !Boolean.FALSE.equals((key.startsWith("object:")?objectActive:active).get(key.substring(key.indexOf(':')+1)));}
    public static void withBases(Map<String,Object> batch,Model model){Map<String,Object> bases=object(),visibility=object();List<Object> actions="batch".equals(batch.get("kind"))?list(batch.get("actions")):Arrays.asList(batch);for(Object value:actions){Map<String,Object> op=map(value);String key=entityKey(op);if(key!=null){bases.put(key,model.versions.getOrDefault(key,""));visibility.putIfAbsent(key,model.isActive(key));if(Arrays.asList("active","object-active").contains(op.get("kind")))visibility.put(key,op.get("active"));}}batch.put("bases",bases);batch.put("visibility",visibility);}
    public List<Object> visibleConflicts(){List<Object> out=new ArrayList<>();for(Map.Entry<String,Map<String,Object>> e:conflicts.entrySet())if(!Boolean.FALSE.equals(conflictActive.get(e.getKey())))out.add(e.getValue());return out;}
    private void applyRaw(Map<String,Object> op){
        switch(string(op,"kind")){
            case "batch":for(Object a:list(op.get("actions")))applyRaw(map(a));break;
            case "conflict-status":conflictActive.put(string(op,"conflict"),(Boolean)op.get("active"));break;
            case "document":if(!documentSeen){type=string(op,"type");documentSeen=true;}break;
            case "layer":layers.put(string(op,"layer"),op);break;
            case "stroke":putItem(op,strokes,"stroke");break;
            case "object":putItem(op,objects,"object");break;
            case "active":active.put(string(op,"stroke"),(Boolean)op.get("active"));break;
            case "object-active":objectActive.put(string(op,"object"),(Boolean)op.get("active"));break;
            case "layer-active":layerActive.put(string(op,"layer"),(Boolean)op.get("active"));break;
            default:throw new IllegalArgumentException("Unknown operation kind: "+op.get("kind"));
        }
    }
    public Map<String,Object> json(){return object("type",type,"layers",new ArrayList<>(layers.values()),"strokes",new ArrayList<>(strokes.values()),"objects",new ArrayList<>(objects.values()),"strokeActive",active,"objectActive",objectActive,"layerActive",layerActive,"versions",versions,"versionActors",versionActors,"conflicts",visibleConflicts());}
    public static void validate(Map<String,Object> o){
        String kind=string(o,"kind");
        for(String key:Arrays.asList("actor","session"))if(o.containsKey(key))id(o,key);
        if(o.containsKey("bases")){Map<String,Object> bases=map(o.get("bases"));require(bases.size()<=200,"Too many base versions");for(Map.Entry<String,Object> e:bases.entrySet())require(e.getKey().length()<=220&&e.getValue() instanceof String&&((String)e.getValue()).length()<=200,"Invalid base version");}
        if(o.containsKey("visibility")){Map<String,Object> visibility=map(o.get("visibility"));require(visibility.size()<=200,"Too many visibility entries");for(Map.Entry<String,Object> e:visibility.entrySet())require(e.getKey().length()<=220&&e.getValue() instanceof Boolean,"Invalid branch visibility");}
        switch(kind){
            case "batch":List<Object> actions=list(o.get("actions"));require(!actions.isEmpty()&&actions.size()<=200,"Batch needs 1–200 actions");for(Object a:actions){Map<String,Object> op=map(a);require(!"batch".equals(op.get("kind")),"Nested batch");validate(op);}break;
            case "conflict-status":id(o,"conflict");bool(o,"active");break;
            case "document":require(Arrays.asList("fixed","pages","infinite").contains(string(o,"type")),"Unknown document type");break;
            case "layer":id(o,"layer");id(o,"name");bool(o,"force");if(o.containsKey("order"))range(o,"order",-1000000,1000000);break;
            case "active":id(o,"stroke");bool(o,"active");break;
            case "object-active":id(o,"object");bool(o,"active");break;
            case "layer-active":id(o,"layer");require(!"base".equals(o.get("layer")),"Cannot hide base layer globally");bool(o,"active");break;
            case "stroke":
                id(o,"stroke");drawing(o);if(o.containsKey("brush"))require(Arrays.asList("pen","pencil","marker","brush","oily","fountain","charcoal","neoBrush","latinCalligraphy","asiaCalligraphy","square").contains(string(o,"brush")),"Unknown brush");List<Object> points=list(o.get("points"));require(points.size()>0&&points.size()<=10000,"Invalid points");
                for(Object p:points){List<Object> xyz=list(p);require(xyz.size()==3,"Point needs x, y, pressure");for(Object n:xyz)require(n instanceof Number&&Double.isFinite(((Number)n).doubleValue())&&Math.abs(((Number)n).doubleValue())<=1000000,"Invalid point");double pressure=((Number)xyz.get(2)).doubleValue();require(pressure>=0&&pressure<=1,"Invalid pressure");}break;
            case "object":
                id(o,"object");drawing(o);String shape=string(o,"shape");require(ShapeCatalog.known(shape),"Unknown shape");
                if(shape.equals("svg"))SvgAsset.validate(string(o,"svg"));
                if(shape.equals("image"))ImageAsset.validate(string(o,"image"));
                for(String key:Arrays.asList("bold","italic","underline","flipX","flipY"))if(o.containsKey(key))bool(o,key);
                if(o.containsKey("rotation"))range(o,"rotation",-360,360);
                if(o.containsKey("fontFamily"))require(Arrays.asList("sans-serif","serif","monospace").contains(string(o,"fontFamily")),"Unknown font family");
                if(o.containsKey("alignment"))require(Arrays.asList("left","center","right").contains(string(o,"alignment")),"Unknown alignment");
                for(String key:Arrays.asList("x","y","w","h"))range(o,key,-1000000,1000000);
                if(!Arrays.asList("arrow","arrowLine","line").contains(shape)){require(number(o,"w")>0&&number(o,"h")>0,"Positive object dimensions required");}
                if(o.containsKey("text")){require(string(o,"text").length()<=10000,"Text too long");range(o,"fontSize",1,500);}
                if(o.containsKey("rotation"))range(o,"rotation",-360,360);for(String key:Arrays.asList("flipX","flipY"))if(o.containsKey(key))bool(o,key);
                if(o.containsKey("fill"))color(o,"fill");
                if(o.containsKey("from"))id(o,"from");if(o.containsKey("to"))id(o,"to");break;
            default:throw new IllegalArgumentException("Unknown operation kind: "+kind);
        }
    }
    static void drawing(Map<String,Object>o){id(o,"layer");range(o,"page",0,10000);require(number(o,"page")==Math.rint(number(o,"page")),"Page must be an integer");range(o,"width",1,100);color(o,"color");if(o.containsKey("erasures")){List<Object> masks=list(o.get("erasures"));require(masks.size()<=2000,"Too many erasure regions");int count=0;for(Object mask:masks){count+=Geometry.polygon(mask).length;require(count<=64000,"Erasure mask too complex");}}}
    static void color(Map<String,Object>o,String k){range(o,k,Integer.MIN_VALUE,Integer.MAX_VALUE);require(number(o,k)==Math.rint(number(o,k)),"Color must be signed ARGB integer");}
    static void range(Map<String,Object>o,String k,double lo,double hi){double v=number(o,k);require(v>=lo&&v<=hi,"Out of range: "+k);}
    static void id(Map<String,Object>o,String k){String s=string(o,k);require(!s.isEmpty()&&s.length()<=200,"Invalid "+k);}
    static void bool(Map<String,Object>o,String k){require(o.get(k) instanceof Boolean,"Expected boolean: "+k);}
    public static void require(boolean condition,String message){if(!condition)throw new IllegalArgumentException(message);}
}
