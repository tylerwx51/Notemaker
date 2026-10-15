package com.notemaker;

import android.graphics.*;
import com.notemaker.core.*;
import java.util.*;
import static com.notemaker.core.Json.*;

/** Instrumented coverage checks run only on the explicitly launched test emulator. */
final class CoverageCheck {
    static void run(Integration test,MainActivity app) throws Exception {
        app.replica.command(Arrays.asList("layer","put","-"),object("layer","under","name","Under","force",false,"order",-1));
        for(String layer:Arrays.asList("base","under"))app.replica.command(Arrays.asList("object","put","-"),object("object",layer+"-box","layer",layer,"shape","rect","x",0,"y",0,"w",100,"h",100,"fill",layer.equals("base")?Color.BLACK:Color.RED));
        app.replica.command(Arrays.asList("object","put","-"),object("object","ring","shape","ellipse","x",120,"y",120,"w",100,"h",100,"width",4));
        app.replica.command(Arrays.asList("stroke","put","-"),object("stroke","ink","layer","base","page",0,"width",4,"color",Color.BLACK,"points",Arrays.asList(Arrays.asList(0,50,1),Arrays.asList(100,50,1))));
        Bitmap source=Bitmap.createBitmap(4,4,Bitmap.Config.ARGB_8888);source.eraseColor(Color.WHITE);source.setPixel(1,1,Color.TRANSPARENT);source.setPixel(2,1,Color.TRANSPARENT);source.setPixel(1,2,Color.TRANSPARENT);source.setPixel(2,2,Color.TRANSPARENT);
        java.io.ByteArrayOutputStream bytes=new java.io.ByteArrayOutputStream();source.compress(Bitmap.CompressFormat.PNG,100,bytes);source.recycle();
        app.replica.command(Arrays.asList("object","put","-"),object("object","image","shape","image","image",ImageAsset.fromBytes(bytes.toByteArray()),"x",300,"y",100,"w",40,"h",40));
        require(query(app,320,120).isEmpty(),"Transparent image hole selected");require(query(app,302,102).size()==1,"Opaque white image pixel missed");
        require(query(app,170,170).isEmpty(),"Hollow shape interior selected");require(query(app,120,170).size()==1,"Shape outline missed");
        app.replica.command(Arrays.asList("erase","-"),object("layer","base","page",0,"mode","partial","radius",10,"points",Arrays.asList(Arrays.asList(50,0),Arrays.asList(50,100))));
        require(query(app,50,50).isEmpty(),"Erased area still selects");require(query(app,20,50).size()==2,"Remaining coverage lost");
        Map<String,Object> snapshot=app.replica.read();Model model=Store.model(snapshot);
        Bitmap rendered=Bitmap.createBitmap(400,250,Bitmap.Config.ARGB_8888);Canvas c=new Canvas(rendered);c.drawColor(Color.WHITE);Render.draw(model,0,Collections.emptySet(),new com.notemaker.cli.Png(c));
        require(rendered.getPixel(50,50)==Color.RED,"Partial erase damaged or hid underlying layer");require(rendered.getPixel(20,50)==Color.BLACK,"Partial erase lost untouched pixels");rendered.recycle();
        test.runOnMainSync(()->{try{app.refreshShared(snapshot);}catch(Exception e){throw new RuntimeException(e);}app.selectAt(120,170);});
        long until=android.os.SystemClock.uptimeMillis()+10000;while(app.selectionSession==null&&android.os.SystemClock.uptimeMillis()<until)Thread.sleep(25);
        require(app.selectionSession!=null,"Activity did not select outline");
        test.runOnMainSync(()->app.selectLayer("under"));ReplicaService.writes.submit(()->{}).get();
        require(app.selectionSession==null&&app.selectedEntities.isEmpty(),"Layer switch retained selection");
        require(app.selecting,"Layer switch unexpectedly left the selection tool");
        require(map(app.replica.read().get("leases")).isEmpty(),"Layer switch retained leases");
    }
    private static List<Object> query(MainActivity app,double x,double y)throws Exception{return list(map(app.replica.command(Arrays.asList("selection","query","-"),object("layer","base","region",Geometry.json(Geometry.rect(x-.1,y-.1,.2,.2))))).get("items"));}
    private static void require(boolean condition,String message){if(!condition)throw new IllegalStateException(message);}
}
