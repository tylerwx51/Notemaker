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
        this(model,layer,page,false);
    }
    public LayerEditor(Model model,String layer,int page,boolean includeInactive){
        this.layer=layer;this.page=page;Model.require(model.layers.containsKey(layer)&&!Boolean.FALSE.equals(model.layerActive.get(layer)),"Layer is not editable");Model.require(page>=0&&page<=10000,"Invalid page");
        for(Map<String,Object> o:model.layerItems(layer,page)){DrawItem item=new DrawItem(model,o);if(includeInactive||item.active())items.put(item.kind+":"+item.id,item);}
    }
    public Collection<DrawItem> items(){return Collections.unmodifiableCollection(items.values());}
    public DrawItem resolve(String kind,String id){DrawItem item=items.get(kind+":"+id);Model.require(item!=null,"Item is outside the editable layer/page: "+id);return item;}
    public List<Object> resolve(List<Object> refs){for(Object raw:refs){Map<String,Object> r=map(raw);resolve(string(r,"kind"),string(r,"id"));}return refs;}
    public List<Object> select(double[][] region,boolean topmost,Coverage.Factory factory){List<DrawItem> ordered=new ArrayList<>(items.values());Collections.reverse(ordered);List<Object> out=new ArrayList<>();for(DrawItem item:ordered)if(region==null?item.hasCoverage(factory):item.intersects(region,factory)){out.add(item.ref());if(topmost)break;}return out;}
}
