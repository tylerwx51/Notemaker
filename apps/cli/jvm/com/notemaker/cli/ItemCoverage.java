package com.notemaker.cli;

import com.notemaker.core.*;
import java.awt.*;
import java.awt.geom.*;
import java.awt.font.FontRenderContext;
import java.util.*;

/** Vector paint support on the JVM; raster assets use lazy, bounded alpha tiles. */
public final class ItemCoverage implements Coverage, Render.Surface {
    private final java.util.List<Area> parts=new ArrayList<>(),cuts=new ArrayList<>();
    private final java.util.List<Rectangle2D> partBounds=new ArrayList<>(),cutBounds=new ArrayList<>();
    private Rectangle2D box;
    private InkGeometry ink;
    private boolean raster;
    private final java.util.List<double[][]> masks=new ArrayList<>();
    private static final Map<Map<String,Object>,Coverage> CACHE=new LinkedHashMap<Map<String,Object>,Coverage>(16,.75f,true){protected boolean removeEldestEntry(Map.Entry<Map<String,Object>,Coverage> e){return size()>10000;}};
    public static synchronized Coverage create(Map<String,Object> item){
        Map<String,Object> key=new LinkedHashMap<>(item);for(String k:Arrays.asList("id","actor","seq","at","action","session","bases","visibility","object","stroke","layer","page"))key.remove(k);
        Coverage cached=CACHE.get(key);if(cached!=null)return cached;
        ItemCoverage c=new ItemCoverage();if("stroke".equals(item.get("kind")))c.ink=new InkGeometry();Render.drawItem(new Model(),item,c);
        if(c.ink!=null){c.ink.finish();double[] b=c.ink.bounds();c.box=new Rectangle2D.Double(b[0],b[1],b[2]-b[0],b[3]-b[1]);}
        for(double[][] p:c.masks){Area cut=new Area(path(p,true));c.cuts.add(cut);c.cutBounds.add(cut.getBounds2D());}
        Coverage result=c.raster?new Coverage.Tiles(c.bounds(),(x,y,size)->Png.coverageTile(new Model(),item,x,y,size)):c;CACHE.put(key,result);return result;
    }
    public static Path2D path(double[][] p,boolean close){Path2D path=new Path2D.Double(Path2D.WIND_EVEN_ODD);if(p.length>0){path.moveTo(p[0][0],p[0][1]);for(int i=1;i<p.length;i++)path.lineTo(p[i][0],p[i][1]);if(close)path.closePath();}return path;}
    private void add(Shape shape){Area part=new Area(shape);Rectangle2D b=part.getBounds2D();parts.add(part);partBounds.add(b);if(!part.isEmpty()){if(box==null)box=(Rectangle2D)b.clone();else box.add(b);}}
    private void paint(Shape s,int color,double width,Integer fill){if(fill!=null&&(fill>>>24)!=0)add(s);if((color>>>24)!=0&&width>0)add(new BasicStroke((float)width,BasicStroke.CAP_ROUND,BasicStroke.JOIN_ROUND).createStrokedShape(s));}
    public double[] bounds(){Rectangle2D b=box==null?new Rectangle2D.Double():box;return new double[]{b.getMinX(),b.getMinY(),b.getMaxX(),b.getMaxY()};}
    public boolean intersects(double[][] polygon){
        Area query=new Area(path(polygon,true));Rectangle2D q=query.getBounds2D();
        for(int i=0;i<cuts.size();i++)if(q.intersects(cutBounds.get(i))){query.subtract(cuts.get(i));if(query.isEmpty())return false;}
        q=query.getBounds2D();
        if(ink!=null)for(int i=0;i<ink.size();i++)if(ink.overlaps(i,q.getMinX(),q.getMinY(),q.getMaxX(),q.getMaxY())){
            double x=ink.value(i,1),y=ink.value(i,2),w=ink.value(i,3),h=ink.value(i,4);Shape shape;
            if(ink.value(i,0)==0)shape=new BasicStroke((float)ink.value(i,5),BasicStroke.CAP_ROUND,BasicStroke.JOIN_ROUND).createStrokedShape(new Line2D.Double(x,y,w,h));
            else shape=ink.value(i,0)==1?new Ellipse2D.Double(x,y,w,h):new Rectangle2D.Double(x,y,w,h);
            Area hit=new Area(query);hit.intersect(new Area(shape));if(!hit.isEmpty())return true;
        }
        for(int i=0;i<parts.size();i++)if(q.intersects(partBounds.get(i))){Area hit=new Area(query);hit.intersect(parts.get(i));if(!hit.isEmpty())return true;}return false;
    }
    public void beginErase(java.util.List<double[][]> polygons){masks.addAll(polygons);}
    public void endErase(){}
    public void line(double x,double y,double ex,double ey,int color,double width){if(ink!=null){if((color>>>24)!=0&&width>0)ink.line(x,y,ex,ey,width);return;}paint(new Line2D.Double(x,y,ex,ey),color,width,null);}
    public void rect(double x,double y,double w,double h,int color,double width,Integer fill,boolean ellipse){if(ink!=null&&width==0){if(fill!=null&&(fill>>>24)!=0&&w>0&&h>0)ink.dot(x,y,w,h,ellipse);return;}Shape shape=ellipse?new Ellipse2D.Double(x,y,w,h):new Rectangle2D.Double(x,y,w,h);if(fill!=null&&(fill>>>24)!=0)add(shape);if((color>>>24)!=0&&width>0)add(new BasicStroke((float)width).createStrokedShape(shape));}
    public void polygon(double[][] p,int color,double width,Integer fill,boolean closed){paint(path(p,closed),color,width,fill);}
    public void text(double x,double y,String text,int color,double size){styledText(x,y,text,color,size,"sans-serif",false,false,false,"left");}
    public void styledText(double x,double y,String text,int color,double size,String family,boolean bold,boolean italic,boolean underline,String alignment){if((color>>>24)==0||text.isEmpty())return;Font f=new Font("serif".equals(family)?Font.SERIF:"monospace".equals(family)?Font.MONOSPACED:Font.SANS_SERIF,(bold?Font.BOLD:0)|(italic?Font.ITALIC:0),1).deriveFont((float)size);FontRenderContext context=new FontRenderContext(null,true,false);double width=f.getStringBounds(text,context).getWidth();double left=x-("center".equals(alignment)?width/2:"right".equals(alignment)?width:0);add(f.createGlyphVector(context,text).getOutline((float)left,(float)y));if(underline)line(left,y+2,left+width,y+2,color,1);}
    public void image(double x,double y,double w,double h,String data){raster=true;add(new Rectangle2D.Double(x,y,w,h));}
    public void svg(double x,double y,double w,double h,String source){raster=true;add(new Rectangle2D.Double(x,y,w,h));}
}
