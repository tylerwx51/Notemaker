package com.notemaker.core;

import java.util.*;
import static com.notemaker.core.Json.*;

/** Layer-owned scope: candidate lookup and ID resolution happen before modification. */
public final class LayerEditor {
    private final LinkedHashMap<String,DrawItem> items=new LinkedHashMap<>();
    public final String layer;
    public final int page;
    public static int page(Map<String,Object> request){Object raw=request.getOrDefault("page",0);Model.require(raw instanceof Number,"Invalid page");double n=((Number)raw).doubleValue();Model.require(Double.isFinite(n)&&n==Math.rint(n)&&n>=0&&n<=10000,"Invalid page");return (int)n;}
    public LayerEditor(Model model,String layer,int page){
        this(model,layer,page,false,true);
    }
    public LayerEditor(Model model,String layer,int page,boolean includeInactive){
        this(model,layer,page,includeInactive,true);
    }
    static LayerEditor forErase(Model model,String layer,int page){return new LayerEditor(model,layer,page,false,false);}
    private LayerEditor(Model model,String layer,int page,boolean includeInactive,boolean indexed){
        this.layer=layer;this.page=page;Model.require(model.layers.containsKey(layer)&&!Boolean.FALSE.equals(model.layerActive.get(layer)),"Layer is not editable");Model.require(page>=0&&page<=10000,"Invalid page");
        Collection<Map<String,Object>> source;
        if(indexed)source=model.layerItems(layer,page);
        else{
            List<Map<String,Object>> scoped=new ArrayList<>();
            for(Map<String,Object> o:model.strokes.values())if(layer.equals(o.get("layer"))&&((Number)o.get("page")).intValue()==page)scoped.add(o);
            for(Map<String,Object> o:model.objects.values())if(layer.equals(o.get("layer"))&&((Number)o.get("page")).intValue()==page)scoped.add(o);
            source=scoped;
        }
        for(Map<String,Object> o:source){DrawItem item=new DrawItem(model,o);if(includeInactive||item.active())items.put(item.kind+":"+item.id,item);}
    }
    public Collection<DrawItem> items(){return Collections.unmodifiableCollection(items.values());}
    public DrawItem resolve(String kind,String id){DrawItem item=items.get(kind+":"+id);Model.require(item!=null,"Item is outside the editable layer/page: "+id);return item;}
    public List<Object> resolve(List<Object> refs){for(Object raw:refs){Map<String,Object> r=map(raw);resolve(string(r,"kind"),string(r,"id"));}return refs;}
    /** Check explicit references without constructing every item in the layer. */
    public static void requireReferences(Model model,String layer,int page,List<Object> refs){
        Model.require(model.layers.containsKey(layer)&&!Boolean.FALSE.equals(model.layerActive.get(layer)),"Layer is not editable");
        Model.require(page>=0&&page<=10000,"Invalid page");
        for(Object raw:refs){Map<String,Object> ref=map(raw);String kind=string(ref,"kind"),id=string(ref,"id");
            Map<String,Object> item="stroke".equals(kind)?model.strokes.get(id):"object".equals(kind)?model.objects.get(id):null;
            boolean active=!Boolean.FALSE.equals(("stroke".equals(kind)?model.active:model.objectActive).get(id));
            Model.require(item!=null&&active&&layer.equals(item.get("layer"))&&((Number)item.get("page")).intValue()==page,
                    "Item is outside the editable layer/page: "+id);
        }
    }
    public List<Object> select(double[][] region,boolean topmost,Coverage.Factory factory){List<DrawItem> ordered=new ArrayList<>(items.values());Collections.reverse(ordered);List<Object> out=new ArrayList<>();for(DrawItem item:ordered)if(region==null?item.hasCoverage(factory):item.intersects(region,factory)){out.add(item.ref());if(topmost)break;}return out;}
    /** Test topmost items first without preparing DrawItems that cannot be reached. */
    public static List<Object> select(Model model,String layer,int page,double[][] region,boolean topmost,Coverage.Factory factory){
        Model.require(model.layers.containsKey(layer)&&!Boolean.FALSE.equals(model.layerActive.get(layer)),"Layer is not editable");
        Model.require(page>=0&&page<=10000,"Invalid page");
        List<Map<String,Object>> ordered=model.layerItems(layer,page);List<Object> out=new ArrayList<>();
        for(int i=ordered.size()-1;i>=0;i--){Map<String,Object> source=ordered.get(i);String kind=string(source,"kind"),id=string(source,kind);
            if(Boolean.FALSE.equals(("stroke".equals(kind)?model.active:model.objectActive).get(id)))continue;
            DrawItem item=new DrawItem(model,source);
            if(region==null?item.hasCoverage(factory):item.intersects(region,factory)){out.add(item.ref());if(topmost)break;}
        }
        return out;
    }
}
