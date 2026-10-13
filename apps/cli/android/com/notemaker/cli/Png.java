package com.notemaker.cli;
import com.notemaker.core.*;
import android.graphics.*;
import java.io.*;
import java.util.Collections;

public final class Png implements Render.Surface {
    private static final java.util.Map<String,com.caverock.androidsvg.SVG> SVGS=new java.util.LinkedHashMap<String,com.caverock.androidsvg.SVG>(16,.75f,true){protected boolean removeEldestEntry(java.util.Map.Entry<String,com.caverock.androidsvg.SVG> e){return size()>16;}};
    private final Canvas canvas;
    private final Paint paint=new Paint(Paint.ANTI_ALIAS_FLAG);
    public Png(Canvas canvas){this.canvas=canvas;paint.setStrokeCap(Paint.Cap.ROUND);}
    public static void export(Model m,int page,double x,double y,int w,int h,File out)throws Exception {
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
        Bitmap image=Bitmap.createBitmap(w,h,Bitmap.Config.ARGB_8888);Canvas c=new Canvas(image);c.drawColor(Color.WHITE);c.translate((float)-x,(float)-y);Render.draw(m,page,Collections.emptySet(),new Png(c));try(FileOutputStream stream=new FileOutputStream(out)){if(!image.compress(Bitmap.CompressFormat.PNG,100,stream))throw new IOException("PNG encoding failed");}finally{image.recycle();}
    }
    public void line(double x,double y,double ex,double ey,int color,double width){paint.setColor(color);paint.setStyle(Paint.Style.STROKE);paint.setStrokeWidth((float)width);canvas.drawLine((float)x,(float)y,(float)ex,(float)ey,paint);}
    public void rect(double x,double y,double w,double h,int color,double width,Integer fill,boolean ellipse){RectF r=new RectF((float)x,(float)y,(float)(x+w),(float)(y+h));if(fill!=null){paint.setColor(fill);paint.setStyle(Paint.Style.FILL);if(ellipse)canvas.drawOval(r,paint);else canvas.drawRect(r,paint);}if(width>0){paint.setColor(color);paint.setStyle(Paint.Style.STROKE);paint.setStrokeWidth((float)width);if(ellipse)canvas.drawOval(r,paint);else canvas.drawRect(r,paint);}}
    public void text(double x,double y,String text,int color,double size){paint.setColor(color);paint.setStyle(Paint.Style.FILL);paint.setTextSize((float)size);canvas.drawText(text,(float)x,(float)y,paint);}
    public void svg(double x,double y,double w,double h,String source){
        synchronized(SVGS){
            com.caverock.androidsvg.SVG svg=SVGS.get(source);
            if(svg==null){try{svg=com.caverock.androidsvg.SVG.getFromString(SvgAsset.viewport(source));}catch(com.caverock.androidsvg.SVGParseException e){throw new IllegalArgumentException("Could not render SVG",e);}SVGS.put(source,svg);}
            canvas.save();try{canvas.translate((float)x,(float)y);canvas.clipRect(0,0,(float)w,(float)h);svg.renderToCanvas(canvas,new RectF(0,0,(float)w,(float)h));}finally{canvas.restore();}
        }
    }
}
