package com.notemaker;

import android.graphics.*;
import com.notemaker.core.*;
import java.util.*;
import static com.notemaker.core.Json.*;

/** Instrumented coverage checks run only on the explicitly launched test emulator. */
final class CoverageCheck {
    static void run(Integration test,MainActivity app) throws Exception {
        cachePixels();
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
    static void cachePixels(){
        LinkedHashMap<String,Map<String,Object>> visible=new LinkedHashMap<>();
        for(int i=0;i<3;i++){
            ArrayList<Object> points=new ArrayList<>();for(int p=0;p<40;p++)points.add(Arrays.asList(20+p*4,70+i*12+20*Math.sin(p*.3),.4+(p%5)*.1));
            visible.put("s"+i,object("kind","stroke","stroke","s"+i,"layer","base","page",0,"points",points,"width",12,"color",i==1?0x80660000:Color.BLACK,"brush",i==2?"square":"pen"));
        }
        Map<String,Object> original=visible.get("s1");InkCache cache=new InkCache(320,320);
        for(int step=0;step<7;step++){
            if(step==1)visible.put("s1",new DrawItem(new Model(),original).erase(Collections.singletonList(Geometry.rect(75,30,25,130))));
            if(step==2)visible.put("s1",original);
            if(step==3)visible.remove("s0");
            if(step==4){Map<String,Object> a=visible.remove("s1");visible.put("s1",a);}
            if(step==5){Map<String,Object> a=new LinkedHashMap<>(original);a.put("stroke","append");visible.put("append",a);}
            if(step==6){Map<String,Object> a=new LinkedHashMap<>(original);a.put("brush","marker");a.put("width",24);visible.put("s1",a);}
            Bitmap actual=Bitmap.createBitmap(320,320,Bitmap.Config.ARGB_8888),expected=Bitmap.createBitmap(320,320,Bitmap.Config.ARGB_8888);
            java.util.function.Consumer<Canvas> transform=c->{c.translate(10.5f,5.25f);c.scale(1.3f,1.3f);};
            cache.draw(new Canvas(actual),new LinkedHashMap<>(visible),transform,(c,s)->Render.drawStroke(s,new com.notemaker.cli.Png(c)));
            Canvas c=new Canvas(expected);transform.accept(c);for(Map<String,Object> s:visible.values())Render.drawStroke(s,new com.notemaker.cli.Png(c));
            if(!actual.sameAs(expected)){int differences=0,l=320,t=320,r=0,b=0;for(int y=0;y<320;y++)for(int x=0;x<320;x++)if(actual.getPixel(x,y)!=expected.getPixel(x,y)){differences++;l=Math.min(l,x);t=Math.min(t,y);r=Math.max(r,x);b=Math.max(b,y);}throw new IllegalStateException("Regional ink cache step "+step+": "+differences+" different pixels in "+l+","+t+"–"+r+","+b);}
            actual.recycle();expected.recycle();
        }
        cache.bitmap.recycle();
    }
}
