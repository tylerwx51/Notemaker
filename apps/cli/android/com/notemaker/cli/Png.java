package com.notemaker.cli;
import com.notemaker.core.*;
import android.graphics.*;
import java.io.*;
import java.util.Collections;

public final class Png implements Render.Surface {
    private static final java.util.Map<String,com.caverock.androidsvg.SVG> SVGS=new java.util.LinkedHashMap<String,com.caverock.androidsvg.SVG>(16,.75f,true){protected boolean removeEldestEntry(java.util.Map.Entry<String,com.caverock.androidsvg.SVG> e){return size()>16;}};
    private final Canvas canvas;
    private final java.util.Deque<Integer> clips=new java.util.ArrayDeque<>();
    public static int[] coverageTile(Model m,java.util.Map<String,Object> item,int x,int y,int size){Bitmap image=Bitmap.createBitmap(size,size,Bitmap.Config.ARGB_8888);try{Canvas c=new Canvas(image);c.translate(-x,-y);Render.drawItem(m,item,new Png(c));int[] pixels=new int[size*size];image.getPixels(pixels,0,size,0,0,size,size);return pixels;}finally{image.recycle();}}
    public void beginErase(java.util.List<double[][]> polygons){clips.push(canvas.save());for(double[][] p:polygons)canvas.clipOutPath(ItemCoverage.path(p,true));}
    public void endErase(){canvas.restoreToCount(clips.pop());}
    public void polygon(double[][] points,int color,double width,Integer fill,boolean closed){Path p=ItemCoverage.path(points,closed);if(fill!=null){paint.setColor(fill);paint.setStyle(Paint.Style.FILL);canvas.drawPath(p,paint);}if(width>0){paint.setColor(color);paint.setStyle(Paint.Style.STROKE);paint.setStrokeWidth((float)width);paint.setStrokeJoin(Paint.Join.ROUND);canvas.drawPath(p,paint);}}
    private final java.util.Deque<Integer> transforms=new java.util.ArrayDeque<>();
    private final Paint paint=new Paint(Paint.ANTI_ALIAS_FLAG);
    public Png(Canvas canvas){this.canvas=canvas;paint.setStrokeCap(Paint.Cap.ROUND);}
    public static void ensureFonts(){try{
        // app_process lacks ActivityThread's font map. App UIDs cannot read the
        // system fallback XML, so initialize a small map from a readable font.
        // This path is isolated here; ordinary Activity rendering is unaffected.
        if(Typeface.DEFAULT==null){
            String path=System.getenv("NOTEMAKER_FONT");if(path==null)path="/system/fonts/Roboto-Regular.ttf";
            android.graphics.fonts.Font font=new android.graphics.fonts.Font.Builder(new File(path)).build();
            android.graphics.fonts.FontFamily family=new android.graphics.fonts.FontFamily.Builder(font).build();
            java.util.Map<String,android.graphics.fonts.FontFamily[]> fallback=new java.util.HashMap<>();fallback.put("sans-serif",new android.graphics.fonts.FontFamily[]{family});
            java.util.Map<String,Typeface> map=new java.util.HashMap<>();
            Typeface.class.getDeclaredMethod("initSystemDefaultTypefaces",java.util.Map.class,java.util.List.class,java.util.Map.class).invoke(null,fallback,Collections.emptyList(),map);
            Typeface.class.getDeclaredMethod("setSystemFontMap",java.util.Map.class).invoke(null,map);
        }
        }catch(Exception e){throw new IllegalStateException("Could not initialize fonts",e);}}
    public static void export(Model m,int page,double x,double y,int w,int h,File out)throws Exception {
        ensureFonts();Bitmap image=Bitmap.createBitmap(w,h,Bitmap.Config.ARGB_8888);Canvas c=new Canvas(image);c.drawColor(Color.WHITE);c.translate((float)-x,(float)-y);Render.draw(m,page,Collections.emptySet(),new Png(c));try(FileOutputStream stream=new FileOutputStream(out)){if(!image.compress(Bitmap.CompressFormat.PNG,100,stream))throw new IOException("PNG encoding failed");}finally{image.recycle();}
    }
    public void line(double x,double y,double ex,double ey,int color,double width){paint.setColor(color);paint.setStyle(Paint.Style.STROKE);paint.setStrokeWidth((float)width);canvas.drawLine((float)x,(float)y,(float)ex,(float)ey,paint);}
    public void rect(double x,double y,double w,double h,int color,double width,Integer fill,boolean ellipse){paint.setStrokeJoin(Paint.Join.MITER);RectF r=new RectF((float)x,(float)y,(float)(x+w),(float)(y+h));if(fill!=null){paint.setColor(fill);paint.setStyle(Paint.Style.FILL);if(ellipse)canvas.drawOval(r,paint);else canvas.drawRect(r,paint);}if(width>0){paint.setColor(color);paint.setStyle(Paint.Style.STROKE);paint.setStrokeWidth((float)width);if(ellipse)canvas.drawOval(r,paint);else canvas.drawRect(r,paint);}}
    public void text(double x,double y,String text,int color,double size){paint.setColor(color);paint.setStyle(Paint.Style.FILL);paint.setTextSize((float)size);canvas.drawText(text,(float)x,(float)y,paint);}
    public void styledText(double x,double y,String text,int color,double size,String family,boolean bold,boolean italic,boolean underline,String alignment){paint.setColor(color);paint.setStyle(Paint.Style.FILL);paint.setTextSize((float)size);int style=(bold?Typeface.BOLD:0)|(italic?Typeface.ITALIC:0);paint.setTypeface(Typeface.create("serif".equals(family)?"serif":"monospace".equals(family)?"monospace":"sans-serif",style));float drawX=(float)x-("center".equals(alignment)?paint.measureText(text)/2:"right".equals(alignment)?paint.measureText(text):0);canvas.drawText(text,drawX,(float)y,paint);if(underline){paint.setStrokeWidth(1);paint.setStyle(Paint.Style.STROKE);canvas.drawLine(drawX,(float)(y+2),(float)(drawX+paint.measureText(text)),(float)(y+2),paint);}}
    public void image(double x,double y,double w,double h,String data){
        paint.setColor(Color.WHITE);
        Bitmap bitmap=null;
        try{
            int comma=data.indexOf(',');byte[] bytes=android.util.Base64.decode(data.substring(comma+1),android.util.Base64.DEFAULT);
            BitmapFactory.Options bounds=new BitmapFactory.Options();bounds.inJustDecodeBounds=true;BitmapFactory.decodeByteArray(bytes,0,bytes.length,bounds);
            if(bounds.outWidth<=0||bounds.outHeight<=0)throw new IllegalArgumentException("Could not decode image dimensions");
            BitmapFactory.Options options=new BitmapFactory.Options();options.inSampleSize=1;
            while(((bounds.outWidth+options.inSampleSize-1)/options.inSampleSize)*((bounds.outHeight+options.inSampleSize-1)/options.inSampleSize)>4*1024*1024)options.inSampleSize*=2;
            bitmap=BitmapFactory.decodeByteArray(bytes,0,bytes.length,options);if(bitmap==null)throw new IllegalArgumentException("Could not decode image pixels");
            canvas.drawBitmap(bitmap,null,new RectF((float)x,(float)y,(float)(x+w),(float)(y+h)),paint);
        }catch(IllegalArgumentException e){throw e;}catch(Exception e){throw new IllegalArgumentException("Could not render image",e);}finally{if(bitmap!=null)bitmap.recycle();}
    }
    public void svg(double x,double y,double w,double h,String source){
        synchronized(SVGS){
            com.caverock.androidsvg.SVG svg=SVGS.get(source);
            if(svg==null){try{svg=com.caverock.androidsvg.SVG.getFromString(SvgAsset.viewport(source));}catch(com.caverock.androidsvg.SVGParseException e){throw new IllegalArgumentException("Could not render SVG",e);}SVGS.put(source,svg);}
            canvas.save();try{canvas.translate((float)x,(float)y);canvas.clipRect(0,0,(float)w,(float)h);svg.renderToCanvas(canvas,new RectF(0,0,(float)w,(float)h));}finally{canvas.restore();}
        }
    }
    public void saveTransform(double cx,double cy,double rotation,boolean flipX,boolean flipY){transforms.push(canvas.save());canvas.translate((float)cx,(float)cy);canvas.rotate((float)rotation);canvas.scale(flipX?-1:1,flipY?-1:1);canvas.translate((float)-cx,(float)-cy);}
    public void restoreTransform(){canvas.restoreToCount(transforms.pop());}
}
