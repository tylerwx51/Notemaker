package com.notemaker.core;

import java.util.*;
import static com.notemaker.core.Json.*;

/** Deterministic replay. Server order, followed by unacknowledged local operations. */
public final class Model {
    public String type="fixed";
    private boolean documentSeen;
    public final Map<String,Map<String,Object>> layers=new LinkedHashMap<>(), strokes=new LinkedHashMap<>(), objects=new LinkedHashMap<>();
    public final Map<String,Boolean> active=new LinkedHashMap<>(), objectActive=new LinkedHashMap<>(), layerActive=new LinkedHashMap<>();
    public Model(){layers.put("base",object("kind","layer","layer","base","name","Layer 1","force",false));}
    /** Entity snapshots are immutable after publication; copy only the indexes. */
    public Model(Model other){type=other.type;documentSeen=other.documentSeen;layers.putAll(other.layers);strokes.putAll(other.strokes);objects.putAll(other.objects);active.putAll(other.active);objectActive.putAll(other.objectActive);layerActive.putAll(other.layerActive);}
    public static Model fromJson(Map<String,Object> value){
        Model model=new Model();model.type=string(value,"type");model.documentSeen=true;
        for(String key:Arrays.asList("layers","strokes","objects"))for(Object op:list(value.get(key)))model.apply(map(op));
        for(Map.Entry<String,Object> e:map(value.get("strokeActive")).entrySet())model.active.put(e.getKey(),(Boolean)e.getValue());
        for(Map.Entry<String,Object> e:map(value.get("objectActive")).entrySet())model.objectActive.put(e.getKey(),(Boolean)e.getValue());
        for(Map.Entry<String,Object> e:map(value.get("layerActive")).entrySet())model.layerActive.put(e.getKey(),(Boolean)e.getValue());return model;
    }
    public Model(Map<String,Object> state){this();for(Object v:list(state.get("events")))apply(map(v));for(Object v:list(state.get("pending")))apply(map(v));}
    public void apply(Map<String,Object> op){
        switch(string(op,"kind")){
            case "batch":for(Object a:list(op.get("actions")))apply(map(a));break;
            case "document":if(!documentSeen){type=string(op,"type");documentSeen=true;}break;
            case "layer":layers.put(string(op,"layer"),op);break;
            case "stroke":strokes.put(string(op,"stroke"),op);break;
            case "object":objects.put(string(op,"object"),op);break;
            case "active":active.put(string(op,"stroke"),(Boolean)op.get("active"));break;
            case "object-active":objectActive.put(string(op,"object"),(Boolean)op.get("active"));break;
            case "layer-active":layerActive.put(string(op,"layer"),(Boolean)op.get("active"));break;
            default:throw new IllegalArgumentException("Unknown operation kind: "+op.get("kind"));
        }
    }
    public Map<String,Object> json(){return object("type",type,"layers",new ArrayList<>(layers.values()),"strokes",new ArrayList<>(strokes.values()),"objects",new ArrayList<>(objects.values()),"strokeActive",active,"objectActive",objectActive,"layerActive",layerActive);}
    public static void validate(Map<String,Object> o){
        String kind=string(o,"kind");
        switch(kind){
            case "batch":List<Object> actions=list(o.get("actions"));require(!actions.isEmpty()&&actions.size()<=200,"Batch needs 1–200 actions");for(Object a:actions){Map<String,Object> op=map(a);require(!"batch".equals(op.get("kind")),"Nested batch");validate(op);}break;
            case "document":require(Arrays.asList("fixed","pages","infinite").contains(string(o,"type")),"Unknown document type");break;
            case "layer":id(o,"layer");id(o,"name");bool(o,"force");break;
            case "active":id(o,"stroke");bool(o,"active");break;
            case "object-active":id(o,"object");bool(o,"active");break;
            case "layer-active":id(o,"layer");require(!"base".equals(o.get("layer")),"Cannot hide base layer globally");bool(o,"active");break;
            case "stroke":
                id(o,"stroke");drawing(o);List<Object> points=list(o.get("points"));require(points.size()>0&&points.size()<=10000,"Invalid points");
                for(Object p:points){List<Object> xyz=list(p);require(xyz.size()==3,"Point needs x, y, pressure");for(Object n:xyz)require(n instanceof Number&&Double.isFinite(((Number)n).doubleValue())&&Math.abs(((Number)n).doubleValue())<=1000000,"Invalid point");double pressure=((Number)xyz.get(2)).doubleValue();require(pressure>=0&&pressure<=1,"Invalid pressure");}break;
            case "object":
                id(o,"object");drawing(o);String shape=string(o,"shape");require(Arrays.asList("rect","ellipse","text","arrow").contains(shape),"Unknown shape");
                for(String key:Arrays.asList("x","y","w","h"))range(o,key,-1000000,1000000);
                if(!shape.equals("arrow")){require(number(o,"w")>0&&number(o,"h")>0,"Positive object dimensions required");}
                if(o.containsKey("text")){require(string(o,"text").length()<=10000,"Text too long");range(o,"fontSize",1,500);}
                if(o.containsKey("fill"))color(o,"fill");
                if(o.containsKey("from"))id(o,"from");if(o.containsKey("to"))id(o,"to");break;
            default:throw new IllegalArgumentException("Unknown operation kind: "+kind);
        }
    }
    static void drawing(Map<String,Object>o){id(o,"layer");range(o,"page",0,10000);require(number(o,"page")==Math.rint(number(o,"page")),"Page must be an integer");range(o,"width",1,100);color(o,"color");}
    static void color(Map<String,Object>o,String k){range(o,k,Integer.MIN_VALUE,Integer.MAX_VALUE);require(number(o,k)==Math.rint(number(o,k)),"Color must be signed ARGB integer");}
    static void range(Map<String,Object>o,String k,double lo,double hi){double v=number(o,k);require(v>=lo&&v<=hi,"Out of range: "+k);}
    static void id(Map<String,Object>o,String k){String s=string(o,k);require(!s.isEmpty()&&s.length()<=200,"Invalid "+k);}
    static void bool(Map<String,Object>o,String k){require(o.get(k) instanceof Boolean,"Expected boolean: "+k);}
    public static void require(boolean condition,String message){if(!condition)throw new IllegalArgumentException(message);}
}
