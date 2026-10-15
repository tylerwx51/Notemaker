package com.notemaker.core;

import java.util.*;
import static com.notemaker.core.Json.*;
import static com.notemaker.core.Model.require;

/** Reversible drawing tools expressed only as existing atomic operation batches. */
public final class DrawingActions {
    private DrawingActions() {}
    private static Map<String,Object> cloneOp(Map<String,Object> op){Map<String,Object> r=map(copy(op));for(String k:Arrays.asList("id","actor","seq","at","action","session","bases","visibility"))r.remove(k);return r;}
    private static double opt(Map<String,Object> v,String k,double fallback){return v.containsKey(k)?number(v,k):fallback;}
    private static List<Object> checked(List<Object> result){require(!result.isEmpty(),"No matching editable content");require(result.size()<=200,"Operation exceeds 200 actions; select less content");for(Object op:result)Model.validate(map(op));return result;}
    public static List<Object> copySelection(Model model,List<Object> entities){
        require(!entities.isEmpty()&&entities.size()<=100,"Clipboard needs 1–100 entities");Set<String> nodes=new HashSet<>();for(Object value:entities){Map<String,Object> e=map(value);if("object".equals(e.get("kind")))nodes.add(string(e,"id"));}
        List<Object> out=new ArrayList<>();for(Object value:entities){Map<String,Object> e=map(value);String kind=string(e,"kind"),id=string(e,"id");require(Arrays.asList("object","stroke").contains(kind),"Clipboard supports objects and strokes");Map<String,Object> original=(kind.equals("object")?model.objects:model.strokes).get(id);require(original!=null,"Unknown clipboard entity");Map<String,Object> next=cloneOp(original);
            if(kind.equals("object")&&(original.containsKey("from")||original.containsKey("to"))&&(!nodes.contains(original.get("from"))||!nodes.contains(original.get("to")))){double[] ends=Render.endpoints(model,original);next.put("x",ends[0]);next.put("y",ends[1]);next.put("w",ends[2]-ends[0]);next.put("h",ends[3]-ends[1]);next.remove("from");next.remove("to");next.put("rotation",0);next.put("flipX",false);next.put("flipY",false);}out.add(next);
        }return out;
    }
    public static List<Object> paste(Model model,Map<String,Object> input){
        List<Object> entities=list(input.get("entities"));require(!entities.isEmpty()&&entities.size()<=100,"Clipboard needs 1–100 entities");double dx=opt(input,"dx",24),dy=opt(input,"dy",24);require(Double.isFinite(dx)&&Double.isFinite(dy)&&Math.abs(dx)<=1000000&&Math.abs(dy)<=1000000,"Invalid paste offset");String layer=(String)input.getOrDefault("layer","base");require(model.layers.containsKey(layer),"Unknown paste layer");
        Map<String,String> ids=new HashMap<>();for(Object value:entities){Map<String,Object> source=map(value);String kind=string(source,"kind");require(Arrays.asList("object","stroke").contains(kind),"Clipboard supports objects and strokes");Model.validate(source);String key=kind+":"+string(source,kind);require(!ids.containsKey(key),"Duplicate clipboard entity");ids.put(key,Actions.id());}
        List<Object> out=new ArrayList<>();for(Object value:entities){Map<String,Object> source=map(value),next=cloneOp(source);String kind=string(source,"kind");next.put(kind,ids.get(kind+":"+string(source,kind)));next.put("layer",layer);if(input.containsKey("page"))next.put("page",input.get("page"));
            if(kind.equals("object")){next.put("x",number(source,"x")+dx);next.put("y",number(source,"y")+dy);for(String key:Arrays.asList("from","to"))if(source.containsKey(key)){require(ids.containsKey("object:"+source.get(key)),"Clipboard connector refers to a missing node");next.put(key,ids.get("object:"+source.get(key)));}}
            else{List<Object> points=new ArrayList<>();for(Object point:list(source.get("points"))){List<Object> p=list(point);points.add(Arrays.asList(((Number)p.get(0)).doubleValue()+dx,((Number)p.get(1)).doubleValue()+dy,p.get(2)));}next.put("points",points);}out.add(next);
        }return checked(out);
    }
    public static List<Object> duplicateLayer(Model m,String id){
        require(m.layers.containsKey(id),"Unknown layer");String target=Actions.id();List<Object> result=new ArrayList<>();Map<String,Object> layer=cloneOp(m.layers.get(id));layer.put("layer",target);layer.put("name",layer.get("name")+" copy");result.add(layer);
        Map<String,String> ids=new HashMap<>();for(Map<String,Object> o:m.objects.values())if(id.equals(o.get("layer"))&&!Boolean.FALSE.equals(m.objectActive.get(o.get("object"))))ids.put(string(o,"object"),Actions.id());
        for(Map<String,Object> o:m.objects.values())if(ids.containsKey(o.get("object"))){Map<String,Object> n=cloneOp(o);n.put("layer",target);n.put("object",ids.get(o.get("object")));for(String k:Arrays.asList("from","to"))if(ids.containsKey(o.get(k)))n.put(k,ids.get(o.get(k)));result.add(n);}
        for(Map<String,Object> s:m.strokes.values())if(id.equals(s.get("layer"))&&!Boolean.FALSE.equals(m.active.get(s.get("stroke")))){Map<String,Object> n=cloneOp(s);n.put("layer",target);n.put("stroke",Actions.id());result.add(n);}return checked(result);
    }
    public static List<Object> mergeLayer(Model m,String source,String target){
        require(!source.equals(target)&&!source.equals("base"),"Select a non-base source and a different target");require(m.layers.containsKey(source)&&m.layers.containsKey(target),"Unknown layer");require(!Boolean.FALSE.equals(m.layerActive.get(target)),"Target layer is hidden");List<Object> result=new ArrayList<>();
        for(Map<String,Map<String,Object>> group:Arrays.asList(m.strokes,m.objects))for(Map<String,Object> o:group.values())if(source.equals(o.get("layer"))){Map<String,Object> n=cloneOp(o);n.put("layer",target);result.add(n);}
        result.add(object("kind","layer-active","layer",source,"active",false));return checked(result);
    }
    public static List<Object> reorderLayer(Model m,String id,int index){
        require(m.layers.containsKey(id)&&index>=0&&index<m.layers.size(),"Invalid layer position");List<Map<String,Object>> layers=new ArrayList<>(m.layers.values());Map<String,String> indices=new HashMap<>();int i=0;for(Map<String,Object> l:layers)indices.put(string(l,"layer"),String.valueOf(i++));
        layers.sort(Comparator.comparingDouble(l->opt(l,"order",Integer.parseInt(indices.get(string(l,"layer"))))));Map<String,Object> moving=m.layers.get(id);layers.remove(moving);layers.add(index,moving);List<Object> result=new ArrayList<>();i=0;for(Map<String,Object> l:layers){Map<String,Object> n=cloneOp(l);n.put("order",i++);result.add(n);}return checked(result);
    }
    public static List<Object> image(Model m,byte[] bytes,Map<String,Object> placement){Map<String,Object> o=map(copy(placement));o.put("shape","image");o.put("image",ImageAsset.fromBytes(bytes));return checked(Arrays.asList(Actions.drawing(o,(String)o.getOrDefault("layer","base"))));}
    public static List<Object> template(Model m,Map<String,Object> input){
        String type=string(input,"template");require(Arrays.asList("blank","ruled","grid","dots","planner").contains(type),"Unknown template");double w=opt(input,"w",1000),h=opt(input,"h",1400),spacing=opt(input,"spacing",80);require(w>0&&h>0&&w<=1000000&&h<=1000000&&spacing>=8&&spacing<=1000000,"Invalid template dimensions or spacing");require((w/spacing+1)*(h/spacing+1)<=10000,"Template has too many marks");
        int color=((Number)input.getOrDefault("color",-3355444)).intValue();String hex=String.format(Locale.ROOT,"#%06x",color&0xffffff);double scale=Math.max(1,Math.max(w,h)/1000);StringBuilder svg=new StringBuilder("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 "+w/scale+" "+h/scale+"\" width=\""+w/scale+"\" height=\""+h/scale+"\"><g stroke=\""+hex+"\" stroke-opacity=\""+(color>>>24)/255.0+"\" stroke-width=\""+1/scale+"\">");
        if(type.equals("ruled")||type.equals("grid"))for(double y=spacing;y<h;y+=spacing)svg.append("<path d=\"M0 ").append(y/scale).append(" H").append(w/scale).append("\"/>");
        if(type.equals("grid"))for(double x=spacing;x<w;x+=spacing)svg.append("<path d=\"M").append(x/scale).append(" 0 V").append(h/scale).append("\"/>");
        if(type.equals("dots"))for(double y=spacing;y<h;y+=spacing)for(double x=spacing;x<w;x+=spacing)svg.append("<circle cx=\"").append(x/scale).append("\" cy=\"").append(y/scale).append("\" r=\"").append(2/scale).append("\" fill=\"").append(hex).append("\"/>");
        if(type.equals("planner")){for(int row=0;row<=7;row++)svg.append("<path d=\"M0 ").append(row*h/7/scale).append(" H").append(w/scale).append("\"/>");svg.append("<path d=\"M").append(w*.2/scale).append(" 0 V").append(h/scale).append("\"/>");}
        svg.append("</g></svg>");Map<String,Object> o=object("shape","svg","svg",svg.toString(),"w",w,"h",h);for(String key:Arrays.asList("object","layer","page","x","y"))if(input.containsKey(key))o.put(key,input.get(key));
        List<Object> actions=new ArrayList<>();
        if(!Boolean.FALSE.equals(input.get("background"))){
            String layer="template-"+LayerEditor.page(input);double order=-1;for(Map<String,Object> existing:m.layers.values())order=Math.min(order,opt(existing,"order",0)-1);
            if(!m.layers.containsKey(layer))actions.add(object("kind","layer","layer",layer,"name","Page template","force",false,"order",order));
            else if(Boolean.FALSE.equals(m.layerActive.get(layer)))actions.add(object("kind","layer-active","layer",layer,"active",true));
            o.put("layer",layer);o.putIfAbsent("object",layer+"-background");
        }
        Map<String,Object> drawing=Actions.drawing(o,(String)o.getOrDefault("layer","base"));actions.add(drawing);
        if(Boolean.FALSE.equals(m.objectActive.get(drawing.get("object"))))actions.add(object("kind","object-active","object",drawing.get("object"),"active",true));
        return checked(actions);
    }
    public static List<Object> fill(Model m,Map<String,Object> input){
        if(input.containsKey("x")&&input.containsKey("y"))return RegionFill.create(m,input);
        Model.color(input,"color");int color=((Number)input.get("color")).intValue();
        if(input.containsKey("object")){Map<String,Object> old=new LayerEditor(m,(String)input.getOrDefault("layer","base"),LayerEditor.page(input)).resolve("object",string(input,"object")).content;require(!Arrays.asList("svg","image","text","line","arrow","wave","curve","polyline").contains(old.get("shape")),"Object has no enclosed native fill");Map<String,Object> o=cloneOp(old);o.put("fill",color);return checked(Arrays.asList(o));}
        Map<String,Object> s=new LayerEditor(m,(String)input.getOrDefault("layer","base"),LayerEditor.page(input)).resolve("stroke",string(input,"stroke")).content;List<Object> p=list(s.get("points"));require(p.size()>=3,"Fill needs a closed outline");double[] first=point(p.get(0)),last=point(p.get(p.size()-1));require(Math.hypot(first[0]-last[0],first[1]-last[1])<=Math.max(20,number(s,"width")*3),"Stroke outline is open");
        double[] bounds=bounds(p);double pad=number(s,"width"),w=bounds[2]-bounds[0]+2*pad,h=bounds[3]-bounds[1]+2*pad,scale=Math.max(1,Math.max(w,h)/1000);StringBuilder polygon=new StringBuilder();for(Object v:p){double[] a=point(v);polygon.append((a[0]-bounds[0]+pad)/scale).append(',').append((a[1]-bounds[1]+pad)/scale).append(' ');}
        String svg="<svg xmlns=\"http://www.w3.org/2000/svg\" width=\""+w/scale+"\" height=\""+h/scale+"\" viewBox=\"0 0 "+w/scale+" "+h/scale+"\"><polygon points=\""+polygon+"\" stroke=\""+String.format(Locale.ROOT,"#%06x",((Number)s.get("color")).intValue()&0xffffff)+"\" stroke-width=\""+number(s,"width")/scale+"\" fill=\""+String.format(Locale.ROOT,"#%06x",color&0xffffff)+"\" fill-opacity=\""+(color>>>24)/255.0+"\"/></svg>";
        Map<String,Object> filled=Actions.drawing(object("shape","svg","svg",svg,"x",bounds[0]-pad,"y",bounds[1]-pad,"w",w,"h",h,"page",s.get("page")),string(s,"layer"));if(s.containsKey("erasures"))filled=new DrawItem(m,filled).erase(DrawItem.erasures(s));return checked(Arrays.asList(filled));
    }
    private static double[] point(Object p){List<Object> a=list(p);require(a.size()>=2&&a.size()<=3,"Point needs x/y");double[] r={((Number)a.get(0)).doubleValue(),((Number)a.get(1)).doubleValue(),a.size()==3?((Number)a.get(2)).doubleValue():1};for(double v:r)require(Double.isFinite(v)&&Math.abs(v)<=1000000,"Invalid eraser point");return r;}
    private static double[] bounds(List<Object> pts){double[] b={Double.POSITIVE_INFINITY,Double.POSITIVE_INFINITY,Double.NEGATIVE_INFINITY,Double.NEGATIVE_INFINITY};for(Object o:pts){double[] p=point(o);b[0]=Math.min(b[0],p[0]);b[1]=Math.min(b[1],p[1]);b[2]=Math.max(b[2],p[0]);b[3]=Math.max(b[3],p[1]);}return b;}
    public static List<Object> erase(Model m,Map<String,Object> input,Coverage.Factory factory){
        String mode=(String)input.getOrDefault("mode","stroke");require(Arrays.asList("stroke","item","partial","area","page").contains(mode),"Unknown eraser mode");
        LayerEditor editor=new LayerEditor(m,(String)input.getOrDefault("layer","base"),LayerEditor.page(input));
        double radius=opt(input,"radius",12);require(Double.isFinite(radius)&&radius>=1&&radius<=10000,"Invalid eraser radius");
        List<double[][]> regions=mode.equals("page")?Collections.emptyList():eraseRegions(input);
        List<Object> out=new ArrayList<>();for(DrawItem item:editor.items()){
            boolean hit=mode.equals("page");List<double[][]> touching=new ArrayList<>();for(double[][] region:regions)if(item.intersects(region,factory)){hit=true;if(mode.equals("partial"))touching.add(region);else break;}
            if(hit)out.add(mode.equals("partial")?item.erase(touching):object("kind",item.kind.equals("stroke")?"active":"object-active",item.kind,item.id,"active",false));
        }return checked(out);
    }
    /** Shared exact swept regions for transient feedback and durable erasing. */
    public static List<double[][]> eraseRegions(Map<String,Object> input){
        double radius=opt(input,"radius",12);require(Double.isFinite(radius)&&radius>=1&&radius<=10000,"Invalid eraser radius");List<double[][]> regions=new ArrayList<>();
            List<Object> path=list(input.get("points"));require(!path.isEmpty()&&path.size()<=2000,"Eraser needs 1–2000 points");List<double[]> distinct=new ArrayList<>();
            for(Object raw:path){double[] p=point(raw);int size=distinct.size();if(size>0){double[] last=distinct.get(size-1);if(p[0]==last[0]&&p[1]==last[1])continue;
                if(size>1){double[] a=distinct.get(size-2);double ax=last[0]-a[0],ay=last[1]-a[1],bx=p[0]-last[0],by=p[1]-last[1];if(ax*by==ay*bx&&ax*bx+ay*by>=0){distinct.set(size-1,p);continue;}}}distinct.add(p);}
            double[] prior=distinct.get(0);if(distinct.size()==1)regions.add(Geometry.capsule(prior,prior,radius));for(int i=1;i<distinct.size();i++){double[] next=distinct.get(i);regions.add(Geometry.capsule(prior,next,radius));prior=next;}
        return regions;
    }
}
