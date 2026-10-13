package com.notemaker.core;

import java.util.*;
import static com.notemaker.core.Json.*;

/** Shared scene traversal for Android Canvas, desktop PNG and SVG. */
public final class Render {
    private Render() {}
    public interface Surface {
        void line(double x,double y,double x2,double y2,int color,double width);
        void rect(double x,double y,double w,double h,int color,double width,Integer fill,boolean ellipse);
        void text(double x,double baseline,String text,int color,double size);
        void svg(double x,double y,double w,double h,String source);
    }
    public static void draw(Model m,int page,Set<String> hidden,Surface c){
        for(Map.Entry<String,Map<String,Object>> layer:m.layers.entrySet()){
            String id=layer.getKey();if(Boolean.FALSE.equals(m.layerActive.get(id))||(hidden.contains(id)&&!Boolean.TRUE.equals(layer.getValue().get("force"))))continue;
            for(Map.Entry<String,Map<String,Object>> entry:m.strokes.entrySet()){
                Map<String,Object>s=entry.getValue();if(!id.equals(s.get("layer"))||number(s,"page")!=page||Boolean.FALSE.equals(m.active.get(entry.getKey())))continue;
                List<Object> points=list(s.get("points"));double[] prev=null;
                for(Object v:points){List<Object> p=list(v);double x=((Number)p.get(0)).doubleValue(),y=((Number)p.get(1)).doubleValue(),w=number(s,"width")*(.25+.75*((Number)p.get(2)).doubleValue());int color=((Number)s.get("color")).intValue();if(prev!=null)c.line(prev[0],prev[1],x,y,color,(w+prev[2])/2);c.rect(x-w/2,y-w/2,w,w,color,0,color,true);prev=new double[]{x,y,w};}
            }
            for(Map.Entry<String,Map<String,Object>> entry:m.objects.entrySet()){
                Map<String,Object> o=entry.getValue();if(!id.equals(o.get("layer"))||number(o,"page")!=page||Boolean.FALSE.equals(m.objectActive.get(entry.getKey())))continue;
                drawObject(m,o,c);
            }
        }
    }
    public static void drawObject(Model m,Map<String,Object> o,Surface c){
        double x=number(o,"x"),y=number(o,"y"),w=number(o,"w"),h=number(o,"h"),width=number(o,"width");int color=((Number)o.get("color")).intValue();String shape=string(o,"shape");
        if(shape.equals("svg")){c.svg(x,y,w,h,string(o,"svg"));return;}
        if(shape.equals("arrow")){
            double[] ends=endpoints(m,o);x=ends[0];y=ends[1];double ex=ends[2],ey=ends[3];
            c.line(x,y,ex,ey,color,width);double angle=Math.atan2(ey-y,ex-x),length=Math.max(12,width*4);
            c.line(ex,ey,ex-length*Math.cos(angle-.45),ey-length*Math.sin(angle-.45),color,width);c.line(ex,ey,ex-length*Math.cos(angle+.45),ey-length*Math.sin(angle+.45),color,width);
            x=(x+ex)/2;y=(y+ey)/2;
        }else if(!shape.equals("text"))c.rect(x,y,w,h,color,width,o.containsKey("fill")?((Number)o.get("fill")).intValue():null,shape.equals("ellipse"));
        String text=(String)o.getOrDefault("text","");double size=((Number)o.getOrDefault("fontSize",24)).doubleValue();
        if(shape.equals("ellipse")){x+=25;y+=h/2-size;}
        int i=0;for(String line:text.split("\n",-1))c.text(x+(shape.equals("text")?0:10),y+size+(i++)*size*1.2,line,color,size);
    }
    /** Connector attachments follow node geometry; erased nodes do not resurrect. */
    public static double[] endpoints(Model m,Map<String,Object> o){
        double x=number(o,"x"),y=number(o,"y"),ex=x+number(o,"w"),ey=y+number(o,"h");
        Map<String,Object>a=m.objects.get(o.get("from")),b=m.objects.get(o.get("to"));
        if(a!=null){x=number(a,"x")+number(a,"w")/2;y=number(a,"y")+number(a,"h")/2;}
        if(b!=null){ex=number(b,"x")+number(b,"w")/2;ey=number(b,"y")+number(b,"h")/2;}
        double[] start=boundary(a,x,y,ex,ey),end=boundary(b,ex,ey,x,y);return new double[]{start[0],start[1],end[0],end[1]};
    }
    private static double[] boundary(Map<String,Object> node,double x,double y,double tx,double ty){
        if(node==null||x==tx&&y==ty)return new double[]{x,y};double dx=tx-x,dy=ty-y,rx=number(node,"w")/2,ry=number(node,"h")/2;
        double factor=string(node,"shape").equals("ellipse")?1/Math.sqrt(dx*dx/(rx*rx)+dy*dy/(ry*ry)):Math.min(dx==0?Double.POSITIVE_INFINITY:rx/Math.abs(dx),dy==0?Double.POSITIVE_INFINITY:ry/Math.abs(dy));
        return new double[]{x+factor*dx,y+factor*dy};
    }
    public static final class Svg implements Surface {
        private final StringBuilder s=new StringBuilder();
        public Svg(double x,double y,double w,double h){s.append("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"").append(w).append("\" height=\"").append(h).append("\" viewBox=\"").append(x).append(' ').append(y).append(' ').append(w).append(' ').append(h).append("\"><rect x=\"").append(x).append("\" y=\"").append(y).append("\" width=\"").append(w).append("\" height=\"").append(h).append("\" fill=\"white\"/>");}
        private String color(int c){return String.format(Locale.ROOT,"#%06x",c&0xffffff);}
        private double alpha(int c){return (c>>>24)/255.0;}
        public void svg(double x,double y,double w,double h,String source){
            String data=Base64.getEncoder().encodeToString(SvgAsset.viewport(source).getBytes(java.nio.charset.StandardCharsets.UTF_8));
            s.append("<image x=\"").append(x).append("\" y=\"").append(y).append("\" width=\"").append(w).append("\" height=\"").append(h).append("\" preserveAspectRatio=\"none\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" xlink:href=\"data:image/svg+xml;base64,").append(data).append("\"/>");
        }
        public void line(double x,double y,double ex,double ey,int color,double width){s.append("<line x1=\"").append(x).append("\" y1=\"").append(y).append("\" x2=\"").append(ex).append("\" y2=\"").append(ey).append("\" stroke=\"").append(color(color)).append("\" stroke-opacity=\"").append(alpha(color)).append("\" stroke-width=\"").append(width).append("\" stroke-linecap=\"round\"/>");}
        public void rect(double x,double y,double w,double h,int color,double width,Integer fill,boolean ellipse){s.append(ellipse?"<ellipse cx=\"":"<rect x=\"").append(ellipse?x+w/2:x).append(ellipse?"\" cy=\"":"\" y=\"").append(ellipse?y+h/2:y).append(ellipse?"\" rx=\"":"\" width=\"").append(ellipse?w/2:w).append(ellipse?"\" ry=\"":"\" height=\"").append(ellipse?h/2:h).append("\" stroke=\"").append(color(color)).append("\" stroke-opacity=\"").append(alpha(color)).append("\" stroke-width=\"").append(width).append("\" fill=\"").append(fill==null?"none":color(fill)).append("\" fill-opacity=\"").append(fill==null?1:alpha(fill)).append("\"/>");}
        public void text(double x,double y,String text,int color,double size){s.append("<text x=\"").append(x).append("\" y=\"").append(y).append("\" fill=\"").append(color(color)).append("\" fill-opacity=\"").append(alpha(color)).append("\" font-family=\"sans-serif\" font-size=\"").append(size).append("\">").append(text.replace("&","&amp;").replace("<","&lt;").replace(">","&gt;")).append("</text>");}
        @Override public String toString(){return s.toString()+"</svg>\n";}
    }
}
