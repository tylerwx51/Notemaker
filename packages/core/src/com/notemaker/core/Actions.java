package com.notemaker.core;

import java.util.*;
import static com.notemaker.core.Json.*;
import static com.notemaker.core.Model.require;

/** Shared action builders; CLI and application edits use the same operation model. */
public final class Actions {
    private Actions() {}
    public interface Build { List<Object> create(Model model); }
    public static String id(){return UUID.randomUUID().toString();}
    public static Map<String,Object> drawing(Map<String,Object> input,String layer){
        Map<String,Object> o=map(copy(input));o.put("kind","object");o.putIfAbsent("object",id());o.putIfAbsent("layer",layer);o.putIfAbsent("page",0);o.putIfAbsent("color",-16777216);o.putIfAbsent("width",3);o.putIfAbsent("fontSize",24);o.putIfAbsent("text","");o.putIfAbsent("x",0);o.putIfAbsent("y",0);o.putIfAbsent("w",160);o.putIfAbsent("h",80);Model.validate(o);return o;
    }
    public static List<Object> diagram(Model model,Map<String,Object> input){
        String layer=(String)input.getOrDefault("layer",id());require(!model.layers.containsKey(layer),"Diagram layer already exists");
        List<Object> result=new ArrayList<>();result.add(object("kind","layer","layer",layer,"name",input.getOrDefault("name","Diagram"),"force",false));
        Set<String> ids=new HashSet<>(model.objects.keySet());List<Object> objects=list(input.get("objects"));require(!objects.isEmpty(),"Diagram needs objects");
        for(Object v:objects){Map<String,Object> o=drawing(map(v),layer);o.put("layer",layer);require(ids.add(string(o,"object")),"Duplicate object ID");result.add(o);}
        for(Object v:result){Map<String,Object> o=map(v);for(String k:Arrays.asList("from","to"))if(o.containsKey(k)){
            Map<String,Object> target=null;for(Object candidate:result){Map<String,Object> c=map(candidate);if(o.get(k).equals(c.get("object")))target=c;}
            require(target!=null&&!"arrow".equals(target.get("shape"))&&!Objects.equals(target.get("object"),o.get("object")),"Connector endpoint must be a node in this diagram");
            require(((Number)o.get("page")).intValue()==((Number)target.get("page")).intValue(),"Connector endpoints must be on the same page");
        }}return result;
    }
    public static void validateReferences(Model model,List<Object> operations){
        for(Object v:operations){Map<String,Object> op=map(v);String kind=string(op,"kind");
            if(kind.equals("object")||kind.equals("stroke")){
                require(model.layers.containsKey(op.get("layer")),"Unknown layer");
                require(model.type.equals("pages")||number(op,"page")==0,"Only multipage documents support page > 0");
            }
        }
        for(Map<String,Object> o:model.objects.values())for(String key:Arrays.asList("from","to"))if(o.containsKey(key)){
            Map<String,Object> target=model.objects.get(o.get(key));
            require("arrow".equals(o.get("shape"))&&target!=null&&!"arrow".equals(target.get("shape")),"Connector endpoint must be a non-arrow node");
            require(number(o,"page")==number(target,"page"),"Connector endpoints must be on the same page");
        }
    }
    public static Object inverse(Model model,Map<String,Object> op){
        String k=string(op,"kind");String key=k.startsWith("object")?"object":k.startsWith("layer")?"layer":"stroke";
        String id=op.containsKey(key)?string(op,key):"";
        if(k.equals("document"))throw new IllegalArgumentException("Document creation is not undoable");
        if(k.equals("object")||k.equals("stroke")||k.equals("layer")){
            Map<String,Object> old=(k.equals("object")?model.objects:k.equals("layer")?model.layers:model.strokes).get(id);
            if(old!=null)return copy(old);
            return object("kind",k.equals("stroke")?"active":k+"-active",key,id,"active",false);
        }
        Map<String,Boolean> active=k.equals("active")?model.active:k.equals("object-active")?model.objectActive:model.layerActive;
        return object("kind",k,key,id,"active",!Boolean.FALSE.equals(active.get(id)));
    }
}
