package com.notemaker.core;

import java.util.*;
import static com.notemaker.core.Json.*;

/** Adapter retaining existing journal identities. No active-layer state lives here. */
public final class DrawItem {
    public final String kind,id;
    public final Map<String,Object> content;
    private final boolean active;
    private final Map<String,Object> geometry;
    private Coverage cachedCoverage;
    private Coverage.Factory coverageFactory;
    private static final class Identity {
        final Map<String,Object> value;Identity(Map<String,Object> value){this.value=value;}
        public int hashCode(){return System.identityHashCode(value);}
        public boolean equals(Object other){return other instanceof Identity&&((Identity)other).value==value;}
    }
    private static final class Prepared {volatile double[] bounds;volatile Map<String,Object> local;}
    private static final Map<Identity,Prepared> PREPARED=new LinkedHashMap<Identity,Prepared>(16,.75f,true){protected boolean removeEldestEntry(Map.Entry<Identity,Prepared> entry){return size()>10000;}};
    private Prepared prepared;
    private Prepared prepared(){if(prepared==null)synchronized(PREPARED){Identity key=new Identity(geometry);prepared=PREPARED.get(key);if(prepared==null){prepared=new Prepared();PREPARED.put(key,prepared);}}return prepared;}
    public DrawItem(Model model,Map<String,Object> content){this.content=content;kind=string(content,"kind");id=string(content,kind);active=!Boolean.FALSE.equals((kind.equals("stroke")?model.active:model.objectActive).get(id));geometry=resolveGeometry(model);}
    public Map<String,Object> ref(){return object("kind",kind,"id",id,"page",content.get("page"));}
    public boolean active(){return active;}
    public Map<String,Object> geometry(){return geometry;}
    private Map<String,Object> resolveGeometry(Model model){
        if(kind.equals("object")&&(content.containsKey("from")||content.containsKey("to"))){Map<String,Object> o=new LinkedHashMap<>(content);double[] e=Render.endpoints(model,content);o.put("x",e[0]);o.put("y",e[1]);o.put("w",e[2]-e[0]);o.put("h",e[3]-e[1]);o.remove("from");o.remove("to");o.put("rotation",0);o.put("flipX",false);o.put("flipY",false);return o;}return content;
    }
    public double[] local(double x,double y){
        Map<String,Object> o=geometry();if(kind.equals("stroke")){List<Object> p=list(list(o.get("points")).get(0));return new double[]{x-((Number)p.get(0)).doubleValue(),y-((Number)p.get(1)).doubleValue()};}
        double w=number(o,"w"),h=number(o,"h"),a=Math.toRadians(((Number)o.getOrDefault("rotation",0)).doubleValue()),dx=x-number(o,"x")-w/2,dy=y-number(o,"y")-h/2;
        return new double[]{(dx*Math.cos(a)+dy*Math.sin(a))*(Boolean.TRUE.equals(o.get("flipX"))?-1:1)+w/2,(-dx*Math.sin(a)+dy*Math.cos(a))*(Boolean.TRUE.equals(o.get("flipY"))?-1:1)+h/2};
    }
    public Map<String,Object> localContent(){Prepared prepared=prepared();if(prepared.local!=null)return prepared.local;Map<String,Object> o=new LinkedHashMap<>(geometry());if(kind.equals("object")){o.put("x",0);o.put("y",0);o.put("rotation",0);o.put("flipX",false);o.put("flipY",false);}else{List<Object> points=new ArrayList<>();for(Object v:list(content.get("points"))){List<Object> p=list(v);double[] q=local(((Number)p.get(0)).doubleValue(),((Number)p.get(1)).doubleValue());points.add(Arrays.asList(q[0],q[1],p.get(2)));}o.put("points",points);}prepared.local=o;return o;}
    private Coverage coverage(Coverage.Factory factory){if(cachedCoverage==null||coverageFactory!=factory){cachedCoverage=factory.create(localContent());coverageFactory=factory;}return cachedCoverage;}
    /** Conservative unmasked local bounds; glyph overhangs require native coverage. */
    private double[] coarseBounds(){
        Prepared prepared=prepared();if(prepared.bounds!=null)return prepared.bounds;
        Map<String,Object> o=geometry();double pad=number(o,"width")*2;
        if(kind.equals("stroke")){
            double l=Double.POSITIVE_INFINITY,t=l,r=Double.NEGATIVE_INFINITY,b=r;
            for(Object raw:list(o.get("points"))){List<Object> p=list(raw);double x=((Number)p.get(0)).doubleValue(),y=((Number)p.get(1)).doubleValue();l=Math.min(l,x);t=Math.min(t,y);r=Math.max(r,x);b=Math.max(b,y);}
            double[] first=local(l,t),last=local(r,b);return prepared.bounds=new double[]{first[0]-pad,first[1]-pad,last[0]+pad,last[1]+pad};
        }
        if(!((String)o.getOrDefault("text","")).isEmpty())return null;
        if(Arrays.asList("arrow","arrowLine").contains(o.get("shape")))pad+=Math.max(12,number(o,"width")*4);
        pad+=1;double w=number(o,"w"),h=number(o,"h");return prepared.bounds=new double[]{Math.min(0,w)-pad,Math.min(0,h)-pad,Math.max(0,w)+pad,Math.max(0,h)+pad};
    }
    private static boolean overlaps(double[] b,double[] q){return b[2]>=q[0]&&q[2]>=b[0]&&b[3]>=q[1]&&q[3]>=b[1];}
    public boolean intersects(double[][] region,Coverage.Factory factory){
        double[] coarse=coarseBounds();
        if(kind.equals("stroke")){double[] q=Geometry.bounds(region),origin=local(0,0);q[0]+=origin[0];q[2]+=origin[0];q[1]+=origin[1];q[3]+=origin[1];if(!overlaps(coarse,q))return false;}
        double[][] p=new double[region.length][];for(int i=0;i<p.length;i++)p[i]=local(region[i][0],region[i][1]);double[] q=Geometry.bounds(p);if(coarse!=null&&!overlaps(coarse,q))return false;Coverage area=coverage(factory);return overlaps(area.bounds(),q)&&area.intersects(p);
    }
    public boolean hasCoverage(Coverage.Factory factory){Coverage coverage=coverage(factory);double[] b=coverage.bounds();return b[2]>b[0]&&b[3]>b[1]&&coverage.intersects(Geometry.rect(b[0],b[1],b[2]-b[0],b[3]-b[1]));}
    public Map<String,Object> erase(List<double[][]> regions){Map<String,Object> next=map(copy(content));for(String key:Arrays.asList("id","actor","seq","at","action","session","bases","visibility"))next.remove(key);List<Object> masks=new ArrayList<>(list(next.getOrDefault("erasures",Collections.emptyList())));Map<String,Object> o=geometry();for(double[][] region:regions){double[][] p=new double[region.length][];for(int i=0;i<p.length;i++){p[i]=local(region[i][0],region[i][1]);if(kind.equals("object")){p[i][0]/=unit(number(o,"w"));p[i][1]/=unit(number(o,"h"));}}masks.add(Geometry.json(p));}next.put("erasures",masks);Model.validate(next);return next;}
    private static double unit(double n){return n==0?1:n;}
    /** Decode persistent local masks into the unrotated paint coordinates. */
    public static List<double[][]> erasures(Map<String,Object> o){List<Object> rawMasks=list(o.getOrDefault("erasures",Collections.emptyList()));if(rawMasks.isEmpty())return Collections.emptyList();List<double[][]> out=new ArrayList<>();boolean stroke="stroke".equals(o.get("kind"))||(!o.containsKey("kind")&&o.containsKey("points"));double x,y,w,h;if(stroke){List<Object> p=list(list(o.get("points")).get(0));x=((Number)p.get(0)).doubleValue();y=((Number)p.get(1)).doubleValue();w=h=1;}else{x=number(o,"x");y=number(o,"y");w=unit(number(o,"w"));h=unit(number(o,"h"));}for(Object raw:list(o.getOrDefault("erasures",Collections.emptyList()))){double[][] p=Geometry.polygon(raw);for(double[] v:p){v[0]=x+v[0]*w;v[1]=y+v[1]*h;}out.add(p);}return out;}
}
