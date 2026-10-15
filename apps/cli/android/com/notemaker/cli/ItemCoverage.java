package com.notemaker.cli;

import com.notemaker.core.*;
import android.graphics.*;
import java.util.*;

/** Native vector paint support shared by the Android app and Android CLI. */
public final class ItemCoverage implements Coverage, Render.Surface {
    private final java.util.List<Path> parts=new ArrayList<>(),cuts=new ArrayList<>();
    private final java.util.List<RectF> partBounds=new ArrayList<>(),cutBounds=new ArrayList<>();
    private final RectF box=new RectF();
    private boolean raster;
    private final java.util.List<double[][]> masks=new ArrayList<>();
    private static final Map<Map<String,Object>,Coverage> CACHE=new LinkedHashMap<Map<String,Object>,Coverage>(16,.75f,true){protected boolean removeEldestEntry(Map.Entry<Map<String,Object>,Coverage> e){return size()>10000;}};
    public static synchronized Coverage create(Map<String,Object> item){
        Png.ensureFonts();Map<String,Object> key=new LinkedHashMap<>(item);for(String k:Arrays.asList("id","actor","seq","at","action","session","bases","visibility","object","stroke","layer","page"))key.remove(k);
        Coverage cached=CACHE.get(key);if(cached!=null)return cached;
        ItemCoverage c=new ItemCoverage();Render.drawItem(new Model(),item,c);for(double[][] p:c.masks){Path cut=path(p,true);RectF b=new RectF();cut.computeBounds(b,true);c.cuts.add(cut);c.cutBounds.add(b);}
        Coverage result=c.raster?new Coverage.Tiles(c.bounds(),(x,y,size)->Png.coverageTile(new Model(),item,x,y,size)):c;CACHE.put(key,result);return result;
    }
    public static Path path(double[][] p,boolean close){Path path=new Path();path.setFillType(Path.FillType.EVEN_ODD);if(p.length>0){path.moveTo((float)p[0][0],(float)p[0][1]);for(int i=1;i<p.length;i++)path.lineTo((float)p[i][0],(float)p[i][1]);if(close)path.close();}return path;}
    private void add(Path p){RectF b=new RectF();p.computeBounds(b,true);parts.add(p);partBounds.add(b);box.union(b);}
    private void paint(Path p,int color,double width,Integer fill){if(fill!=null&&(fill>>>24)!=0)add(p);if((color>>>24)!=0&&width>0){Paint brush=new Paint();brush.setStyle(Paint.Style.STROKE);brush.setStrokeWidth((float)width);brush.setStrokeCap(Paint.Cap.ROUND);brush.setStrokeJoin(Paint.Join.ROUND);Path stroke=new Path();brush.getFillPath(p,stroke);add(stroke);}}
    public double[] bounds(){return new double[]{box.left,box.top,box.right,box.bottom};}
    public boolean intersects(double[][] polygon){
        Path query=path(polygon,true);RectF q=new RectF();query.computeBounds(q,true);
        for(int i=0;i<cuts.size();i++)if(RectF.intersects(q,cutBounds.get(i))){query.op(cuts.get(i),Path.Op.DIFFERENCE);if(query.isEmpty())return false;}
        query.computeBounds(q,true);Path hit=new Path();
        for(int i=0;i<parts.size();i++)if(RectF.intersects(q,partBounds.get(i))){hit.op(query,parts.get(i),Path.Op.INTERSECT);if(!hit.isEmpty())return true;}
        return false;
    }
    public void beginErase(java.util.List<double[][]> polygons){masks.addAll(polygons);}
    public void endErase(){}
    public void line(double x,double y,double ex,double ey,int color,double width){Path p=new Path();p.moveTo((float)x,(float)y);p.lineTo((float)ex,(float)ey);paint(p,color,width,null);}
    public void rect(double x,double y,double w,double h,int color,double width,Integer fill,boolean ellipse){Path p=new Path();RectF r=new RectF((float)x,(float)y,(float)(x+w),(float)(y+h));if(ellipse)p.addOval(r,Path.Direction.CW);else p.addRect(r,Path.Direction.CW);if(fill!=null&&(fill>>>24)!=0)add(p);if((color>>>24)!=0&&width>0){Paint brush=new Paint();brush.setStyle(Paint.Style.STROKE);brush.setStrokeWidth((float)width);brush.setStrokeJoin(Paint.Join.MITER);Path stroke=new Path();brush.getFillPath(p,stroke);add(stroke);}}
    public void polygon(double[][] p,int color,double width,Integer fill,boolean closed){paint(path(p,closed),color,width,fill);}
    public void text(double x,double y,String text,int color,double size){styledText(x,y,text,color,size,"sans-serif",false,false,false,"left");}
    public void styledText(double x,double y,String text,int color,double size,String family,boolean bold,boolean italic,boolean underline,String alignment){if((color>>>24)==0||text.isEmpty())return;Paint paint=new Paint(Paint.ANTI_ALIAS_FLAG);paint.setTextSize((float)size);paint.setTypeface(Typeface.create(family,(bold?Typeface.BOLD:0)|(italic?Typeface.ITALIC:0)));float width=paint.measureText(text),left=(float)x-("center".equals(alignment)?width/2:"right".equals(alignment)?width:0);Path p=new Path();paint.getTextPath(text,0,text.length(),left,(float)y,p);add(p);if(underline)line(left,y+2,left+width,y+2,color,1);}
    public void image(double x,double y,double w,double h,String data){raster=true;rect(x,y,w,h,0,0,-1,false);}
    public void svg(double x,double y,double w,double h,String source){raster=true;rect(x,y,w,h,0,0,-1,false);}
}
