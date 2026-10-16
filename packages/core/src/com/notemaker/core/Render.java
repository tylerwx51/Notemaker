package com.notemaker.core;

import java.util.*;
import static com.notemaker.core.Json.*;

/** Shared scene traversal for Android Canvas, desktop PNG and SVG. */
public final class Render {
    private Render() {}
    public interface Surface {
        default void beginErase(List<double[][]> polygons){if(!polygons.isEmpty())throw new UnsupportedOperationException("Erasure masks unsupported");}
        default void endErase(){}
        default void polygon(double[][] points,int color,double width,Integer fill,boolean closed){throw new UnsupportedOperationException("Polygon paint unsupported");}
        void line(double x,double y,double x2,double y2,int color,double width);
        void rect(double x,double y,double w,double h,int color,double width,Integer fill,boolean ellipse);
        void text(double x,double baseline,String text,int color,double size);
        default void styledText(double x,double baseline,String text,int color,double size,String family,boolean bold,boolean italic,boolean underline,String alignment){text(x,baseline,text,color,size);}
        void svg(double x,double y,double w,double h,String source);
        default void saveTransform(double cx,double cy,double rotation,boolean flipX,boolean flipY){}
        default void restoreTransform(){}
        default void image(double x,double y,double w,double h,String data){svg(x,y,w,h,"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"100\" height=\"100\" viewBox=\"0 0 100 100\"><image width=\"100\" height=\"100\" preserveAspectRatio=\"none\" href=\""+data+"\"/></svg>");}
    }
    public static void draw(Model m,int page,Set<String> hidden,Surface c){
        List<Map.Entry<String,Map<String,Object>>> ordered=new ArrayList<>(m.layers.entrySet());
        ordered.sort(Comparator.comparingDouble(e->e.getValue().get("order") instanceof Number?((Number)e.getValue().get("order")).doubleValue():0));
        for(Map.Entry<String,Map<String,Object>> layer:ordered){
            String id=layer.getKey();if(Boolean.FALSE.equals(m.layerActive.get(id))||(hidden.contains(id)&&!Boolean.TRUE.equals(layer.getValue().get("force"))))continue;
            for(Map<String,Object> content:m.layerItems(id,page)){DrawItem item=new DrawItem(m,content);if(item.active())drawItem(m,content,c);}
        }
    }
    public static void drawItem(Model m,Map<String,Object> o,Surface c){if("stroke".equals(o.get("kind")))drawStroke(o,c);else drawObject(m,o,c);}
    public static void drawStroke(Map<String,Object> s,Surface c){
        c.beginErase(DrawItem.erasures(s));try{
                List<Object> points=list(s.get("points"));double[] prev=null;String brush=(String)s.getOrDefault("brush","pen");int original=((Number)s.get("color")).intValue();
                int alpha="marker".equals(brush)?Math.min((original>>>24),128):Arrays.asList("pencil","charcoal").contains(brush)?Math.min((original>>>24),190):(original>>>24);int color=(original&0x00ffffff)|(alpha<<24);
                double factor="marker".equals(brush)||"neoBrush".equals(brush)?1.8:Arrays.asList("brush","charcoal").contains(brush)?1.35:Arrays.asList("pencil","fountain","oily").contains(brush)?.82:Arrays.asList("latinCalligraphy","asiaCalligraphy").contains(brush)?1.5:1.0;
                for(Object v:points){List<Object> p=list(v);double x=((Number)p.get(0)).doubleValue(),y=((Number)p.get(1)).doubleValue(),w=number(s,"width")*(.25+.75*((Number)p.get(2)).doubleValue())*factor;
                    if(prev!=null){double angle=Math.atan2(y-prev[1],x-prev[0]),segmentWidth=(w+prev[2])/2;
                        if("latinCalligraphy".equals(brush)||"asiaCalligraphy".equals(brush)){double nibAngle="latinCalligraphy".equals(brush)?Math.PI/4:Math.PI/3;segmentWidth*=.12+.88*Math.abs(Math.sin(angle-nibAngle));}
                        c.line(prev[0],prev[1],x,y,color,segmentWidth);
                    }
                    boolean square="square".equals(brush);c.rect(x-w/2,y-w/2,w,w,color,0,color,!square);prev=new double[]{x,y,w};}
        }finally{c.endErase();}
    }
    public static void drawObject(Model m,Map<String,Object> o,Surface c){
        if(("arrow".equals(o.get("shape"))||"arrowLine".equals(o.get("shape")))&&(o.containsKey("from")||o.containsKey("to"))){drawObjectMasked(m,new DrawItem(m,o).geometry(),c);return;}
        double rotation=((Number)o.getOrDefault("rotation",0)).doubleValue();boolean flipX=Boolean.TRUE.equals(o.get("flipX")),flipY=Boolean.TRUE.equals(o.get("flipY"));
        if(rotation!=0||flipX||flipY){c.saveTransform(number(o,"x")+number(o,"w")/2,number(o,"y")+number(o,"h")/2,rotation,flipX,flipY);try{drawObjectMasked(m,o,c);}finally{c.restoreTransform();}}
        else drawObjectMasked(m,o,c);
    }
    private static void drawObjectMasked(Model m,Map<String,Object> o,Surface c){c.beginErase(DrawItem.erasures(o));try{drawObjectRaw(m,o,c);}finally{c.endErase();}}
    private static void drawObjectRaw(Model m,Map<String,Object> o,Surface c){
        double x=number(o,"x"),y=number(o,"y"),w=number(o,"w"),h=number(o,"h"),width=number(o,"width");int color=((Number)o.get("color")).intValue();String shape=string(o,"shape");
        if(shape.equals("template"))return; // Infinite guides are generated against the live viewport by the tablet renderer.
        if(shape.equals("svg")){c.svg(x,y,w,h,string(o,"svg"));return;}
        if(shape.equals("image")){c.image(x,y,w,h,string(o,"image"));return;}
        if(shape.equals("arrow")||shape.equals("arrowLine")){
            double[] ends=endpoints(m,o);x=ends[0];y=ends[1];double ex=ends[2],ey=ends[3];
            c.line(x,y,ex,ey,color,width);double angle=Math.atan2(ey-y,ex-x),length=Math.max(12,width*4);
            c.line(ex,ey,ex-length*Math.cos(angle-.45),ey-length*Math.sin(angle-.45),color,width);c.line(ex,ey,ex-length*Math.cos(angle+.45),ey-length*Math.sin(angle+.45),color,width);
            x=(x+ex)/2;y=(y+ey)/2;
        }else if(shape.equals("line")){
            c.line(x,y,x+w,y+h,color,width);x+=w/2;y+=h/2;
        }else if(ShapeCatalog.polygon(shape)){
            double[][] vertices=ShapeCatalog.vertices(shape),points=new double[vertices.length][2];for(int i=0;i<points.length;i++){points[i][0]=x+vertices[i][0]*w;points[i][1]=y+vertices[i][1]*h;}c.polygon(points,color,width,o.containsKey("fill")?((Number)o.get("fill")).intValue():null,!shape.equals("polyline"));
        }else if(Arrays.asList("cube","cylinder","cone","sphere","pyramid","wave","waveLine","curve").contains(shape)){
            double pad=width/2+1;c.svg(x-pad,y-pad,w+2*pad,h+2*pad,detailSvg(shape,color,width,o.containsKey("fill")?((Number)o.get("fill")).intValue():null));
        }else if(!shape.equals("text"))c.rect(x,y,w,h,color,width,o.containsKey("fill")?((Number)o.get("fill")).intValue():null,shape.equals("ellipse")||shape.equals("circle"));
        String text=(String)o.getOrDefault("text","");double size=((Number)o.getOrDefault("fontSize",24)).doubleValue();
        if(shape.equals("ellipse")){x+=25;y+=h/2-size;}
        String family=(String)o.getOrDefault("fontFamily","sans-serif");boolean bold=Boolean.TRUE.equals(o.get("bold")),italic=Boolean.TRUE.equals(o.get("italic")),underline=Boolean.TRUE.equals(o.get("underline"));
        String alignment=(String)o.getOrDefault("alignment","left");
        double textX=x+("center".equals(alignment)?w/2:"right".equals(alignment)?w:0)+(shape.equals("text")?0:10);
        int i=0;for(String line:text.split("\n",-1))c.styledText(textX,y+size+(i++)*size*1.2,line,color,size,family,bold,italic,underline,alignment);
    }
    private static String detailSvg(String shape,int stroke,double width,Integer fill){
        String raw=ShapeCatalog.drawingSvg(shape),color=String.format(Locale.ROOT,"#%06x",stroke&0xffffff),f=fill==null?"none":String.format(Locale.ROOT,"#%06x",fill&0xffffff);
        return raw.replace("fill=\"none\"","fill=\""+f+"\" fill-opacity=\""+(fill==null?1.0:((fill>>>24)/255.0))+"\" stroke=\""+color+"\" stroke-opacity=\""+((stroke>>>24)/255.0)+"\" stroke-width=\""+width+"\" stroke-linejoin=\"round\" stroke-linecap=\"round\"");
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
        if(node==null||x==tx&&y==ty)return new double[]{x,y};
        double dx=tx-x,dy=ty-y,angle=Math.toRadians(((Number)node.getOrDefault("rotation",0)).doubleValue()),cos=Math.cos(angle),sin=Math.sin(angle);
        boolean flipX=Boolean.TRUE.equals(node.get("flipX")),flipY=Boolean.TRUE.equals(node.get("flipY"));
        double lx=(dx*cos+dy*sin)*(flipX?-1:1),ly=(-dx*sin+dy*cos)*(flipY?-1:1),rx=number(node,"w")/2,ry=number(node,"h")/2;
        double factor=Arrays.asList("ellipse","circle").contains(string(node,"shape"))?1/Math.sqrt(lx*lx/(rx*rx)+ly*ly/(ry*ry)):Math.min(lx==0?Double.POSITIVE_INFINITY:rx/Math.abs(lx),ly==0?Double.POSITIVE_INFINITY:ry/Math.abs(ly));
        double fx=lx*factor*(flipX?-1:1),fy=ly*factor*(flipY?-1:1);return new double[]{x+fx*cos-fy*sin,y+fx*sin+fy*cos};
    }
    public static final class Svg implements Surface {
        private final StringBuilder s=new StringBuilder();
        private int clipId;
        private final Deque<Integer> clipCounts=new ArrayDeque<>();
        private String path(double[][] p,boolean closed){StringBuilder d=new StringBuilder();for(int i=0;i<p.length;i++)d.append(i==0?"M":"L").append(p[i][0]).append(' ').append(p[i][1]);if(closed)d.append('Z');return d.toString();}
        public void beginErase(List<double[][]> polygons){clipCounts.push(polygons.size());for(double[][] p:polygons){String id="erase-"+(clipId++);s.append("<defs><clipPath id=\"").append(id).append("\" clipPathUnits=\"userSpaceOnUse\"><path clip-rule=\"evenodd\" d=\"M-1000000000 -1000000000H1000000000V1000000000H-1000000000Z ").append(path(p,true)).append("\"/></clipPath></defs><g clip-path=\"url(#").append(id).append(")\">");}}
        public void endErase(){for(int i=clipCounts.pop();i>0;i--)s.append("</g>");}
        public void polygon(double[][] points,int color,double width,Integer fill,boolean closed){s.append("<path d=\"").append(path(points,closed)).append("\" stroke=\"").append(color(color)).append("\" stroke-opacity=\"").append(alpha(color)).append("\" stroke-width=\"").append(width).append("\" stroke-linejoin=\"round\" stroke-linecap=\"round\" fill=\"").append(fill==null?"none":color(fill)).append("\" fill-opacity=\"").append(fill==null?1:alpha(fill)).append("\"/>");}
        public Svg(double x,double y,double w,double h){s.append("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"").append(w).append("\" height=\"").append(h).append("\" viewBox=\"").append(x).append(' ').append(y).append(' ').append(w).append(' ').append(h).append("\"><rect x=\"").append(x).append("\" y=\"").append(y).append("\" width=\"").append(w).append("\" height=\"").append(h).append("\" fill=\"white\"/>");}
        private String color(int c){return String.format(Locale.ROOT,"#%06x",c&0xffffff);}
        private double alpha(int c){return (c>>>24)/255.0;}
        public void svg(double x,double y,double w,double h,String source){
            String data=Base64.getEncoder().encodeToString(SvgAsset.viewport(source).getBytes(java.nio.charset.StandardCharsets.UTF_8));
            s.append("<image x=\"").append(x).append("\" y=\"").append(y).append("\" width=\"").append(w).append("\" height=\"").append(h).append("\" preserveAspectRatio=\"none\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" xlink:href=\"data:image/svg+xml;base64,").append(data).append("\"/>");
        }
        public void saveTransform(double cx,double cy,double rotation,boolean flipX,boolean flipY){s.append("<g transform=\"translate(").append(cx).append(' ').append(cy).append(") rotate(").append(rotation).append(") scale(").append(flipX?-1:1).append(' ').append(flipY?-1:1).append(") translate(").append(-cx).append(' ').append(-cy).append(")\">");}
        public void restoreTransform(){s.append("</g>");}
        public void image(double x,double y,double w,double h,String data){s.append("<image x=\"").append(x).append("\" y=\"").append(y).append("\" width=\"").append(w).append("\" height=\"").append(h).append("\" preserveAspectRatio=\"none\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" xlink:href=\"").append(data).append("\"/>");}
        public void line(double x,double y,double ex,double ey,int color,double width){s.append("<line x1=\"").append(x).append("\" y1=\"").append(y).append("\" x2=\"").append(ex).append("\" y2=\"").append(ey).append("\" stroke=\"").append(color(color)).append("\" stroke-opacity=\"").append(alpha(color)).append("\" stroke-width=\"").append(width).append("\" stroke-linecap=\"round\"/>");}
        public void rect(double x,double y,double w,double h,int color,double width,Integer fill,boolean ellipse){s.append(ellipse?"<ellipse cx=\"":"<rect x=\"").append(ellipse?x+w/2:x).append(ellipse?"\" cy=\"":"\" y=\"").append(ellipse?y+h/2:y).append(ellipse?"\" rx=\"":"\" width=\"").append(ellipse?w/2:w).append(ellipse?"\" ry=\"":"\" height=\"").append(ellipse?h/2:h).append("\" stroke=\"").append(color(color)).append("\" stroke-opacity=\"").append(alpha(color)).append("\" stroke-width=\"").append(width).append("\" fill=\"").append(fill==null?"none":color(fill)).append("\" fill-opacity=\"").append(fill==null?1:alpha(fill)).append("\"/>");}
        public void text(double x,double y,String text,int color,double size){styledText(x,y,text,color,size,"sans-serif",false,false,false,"left");}
        public void styledText(double x,double y,String text,int color,double size,String family,boolean bold,boolean italic,boolean underline,String alignment){s.append("<text x=\"").append(x).append("\" y=\"").append(y).append("\" text-anchor=\"").append("center".equals(alignment)?"middle":"right".equals(alignment)?"end":"start").append("\" fill=\"").append(color(color)).append("\" fill-opacity=\"").append(alpha(color)).append("\" font-family=\"").append(family.replace("&","&amp;").replace("\"","&quot;")).append("\" font-size=\"").append(size).append("\" font-weight=\"").append(bold?"bold":"normal").append("\" font-style=\"").append(italic?"italic":"normal").append("\" text-decoration=\"").append(underline?"underline":"none").append("\">").append(text.replace("&","&amp;").replace("<","&lt;").replace(">","&gt;")).append("</text>");}
        @Override public String toString(){return s.toString()+"</svg>\n";}
    }
}
