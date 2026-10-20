package com.notemaker;

import android.graphics.*;
import android.os.SystemClock;
import android.view.*;
import com.notemaker.core.*;
import java.util.*;
import java.util.concurrent.*;
import static com.notemaker.core.Json.*;

/** Pixel contracts for the real editor, including asynchronous local persistence. */
final class StrokeCheck {
    static void onMain(Integration test,Runnable task){
        RuntimeException[] failure={null};
        test.runOnMainSync(()->{try{task.run();}catch(RuntimeException e){failure[0]=e;}});
        if(failure[0]!=null)throw failure[0];
    }
    static Bitmap snapshot(MainActivity app){
        Bitmap image=Bitmap.createBitmap(app.canvas.getWidth(),app.canvas.getHeight(),Bitmap.Config.ARGB_8888);
        app.canvas.draw(new Canvas(image));return image;
    }
    static void equal(MainActivity app,Bitmap before,Bitmap after,String stage){
        if(before.sameAs(after))return;
        int[] a=new int[before.getWidth()*before.getHeight()],b=a.clone();
        before.getPixels(a,0,before.getWidth(),0,0,before.getWidth(),before.getHeight());
        after.getPixels(b,0,after.getWidth(),0,0,after.getWidth(),after.getHeight());
        int differences=0;for(int i=0;i<a.length;i++)if(a[i]!=b[i])differences++;
        try{for(int i=0;i<2;i++)try(java.io.FileOutputStream out=app.openFileOutput("stroke-"+(i==0?"before":"after")+".png",0)){(i==0?before:after).compress(Bitmap.CompressFormat.PNG,100,out);}}catch(Exception e){throw new RuntimeException(e);}
        throw new IllegalStateException(stage+": "+differences+" canvas pixels changed");
    }
    static void input(MainActivity app,long down,int action,float x,float y,float pressure){
        MotionEvent.PointerProperties property=new MotionEvent.PointerProperties();property.id=0;property.toolType=MotionEvent.TOOL_TYPE_STYLUS;
        MotionEvent.PointerCoords coords=new MotionEvent.PointerCoords();float scale=app.canvas.scale()*app.canvas.zoom;
        coords.x=x*scale+app.canvas.offsetX;coords.y=y*scale+app.canvas.offsetY;coords.pressure=pressure;coords.size=1;
        MotionEvent event=MotionEvent.obtain(down,SystemClock.uptimeMillis(),action,1,new MotionEvent.PointerProperties[]{property},new MotionEvent.PointerCoords[]{coords},0,0,1,1,0,0,InputDevice.SOURCE_STYLUS,0);
        try{app.canvas.dispatchTouchEvent(event);}finally{event.recycle();}
    }
    static void run(Integration test,MainActivity app)throws Exception{
        String[] brushes={"pen","marker","pencil","brush","oily","fountain","charcoal","neoBrush","latinCalligraphy","asiaCalligraphy","square"};
        for(String brush:brushes)for(boolean transformed:new boolean[]{false,true}){
            ReplicaService.writes.submit(()->{}).get(30,TimeUnit.SECONDS);test.waitForIdleSync();
            onMain(test,()->{
                app.document="stroke-pixels-"+UUID.randomUUID();app.host="";app.proxyUrl="";app.load("fixed");app.configureProxy();
                app.setDrawingTool(null);app.erase=false;app.pan=false;app.selecting=false;app.scribbleToErase=false;app.brush=brush;app.width=12;app.color=0xff202020;app.material=50;
                app.canvas.zoom=transformed?1.17f:1;app.canvas.offsetX=transformed?13.25f:0;app.canvas.offsetY=transformed?-7.5f:0;
                // Existing content exercises compositing and stable stacking, not just blank paper.
                app.enqueue(new org.json.JSONObject(object("kind","stroke","stroke","background","layer","base","page",0,"width",25,"color",0x80770000,"brush","pen","points",Arrays.asList(Arrays.asList(20,20,1),Arrays.asList(900,1100,1)))));
                app.enqueue(new org.json.JSONObject(Actions.drawing(object("object","box","shape","rect","x",170,"y",20,"w",35,"h",1100,"fill",0x800000ff),"base")));
                app.update();
            });
            ReplicaService.writes.submit(()->{}).get(30,TimeUnit.SECONDS);test.waitForIdleSync();
            CountDownLatch blocked=new CountDownLatch(1),release=new CountDownLatch(1);
            ReplicaService.writes.execute(()->{blocked.countDown();try{release.await();}catch(InterruptedException e){Thread.currentThread().interrupt();}});
            Model.require(blocked.await(10,TimeUnit.SECONDS),"Could not pause persistence");
            Bitmap[] before={null};String[] id={null};
            try{
                onMain(test,()->{
                    long down=SystemClock.uptimeMillis();
                    input(app,down,MotionEvent.ACTION_DOWN,80,80,.7f);
                    float[][] points={{280,230},{80,230},{280,80},{300,100}};
                    for(float[] p:points){input(app,down,MotionEvent.ACTION_MOVE,p[0],p[1],.7f);Bitmap prefix=snapshot(app);app.canvas.emit();Bitmap emitted=snapshot(app);equal(app,prefix,emitted,brush+" live emission");prefix.recycle();emitted.recycle();}
                    id[0]=app.canvas.current;before[0]=snapshot(app);
                    input(app,down,MotionEvent.ACTION_UP,300,100,.7f);
                    Bitmap up=snapshot(app);equal(app,before[0],up,brush+" pen-up transformed="+transformed);up.recycle();
                });
            }finally{release.countDown();}
            ReplicaService.writes.submit(()->{}).get(30,TimeUnit.SECONDS);test.waitForIdleSync();
            onMain(test,()->{
                Model.require(app.saveError==null,"Stroke save failed: "+app.saveError);
                Bitmap saved=snapshot(app);equal(app,before[0],saved,brush+" persisted");saved.recycle();
                // Rebuilding the raster cache must preserve the complete canvas too.
                app.canvas.inkTransform="";Bitmap rebuilt=snapshot(app);equal(app,before[0],rebuilt,brush+" cache rebuild");rebuilt.recycle();before[0].recycle();
            });
            Model.require(Store.model(app.replica.read()).strokes.containsKey(id[0]),"Finished stroke missing from local replica");
        }
        StrokeCrossings.run();
    }
}
