package com.notemaker;

import android.graphics.*;
import com.notemaker.core.*;
import java.util.*;
import static com.notemaker.core.Json.*;

/** Instrumented coverage checks run only on the explicitly launched test emulator. */
final class CoverageCheck {
    static void run(Integration test,MainActivity app) throws Exception {
        cachePixels(true);
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
        for(String layer:Arrays.asList("base","under"))for(boolean topmost:new boolean[]{true,false})
            for(double[][] region:new double[][][]{Geometry.rect(0,0,110,110),Geometry.rect(115,115,120,120),Geometry.rect(300,100,40,40)}){
                List<Object> expected=new LayerEditor(model,layer,0).select(region,topmost,com.notemaker.cli.ItemCoverage::create);
                List<Object> actual=LayerEditor.select(model,layer,0,region,topmost,com.notemaker.cli.ItemCoverage::create);
                require(expected.equals(actual),"Lazy selection changed ordered coverage hits");
            }
        Bitmap rendered=Bitmap.createBitmap(400,250,Bitmap.Config.ARGB_8888);Canvas c=new Canvas(rendered);c.drawColor(Color.WHITE);Render.draw(model,0,Collections.emptySet(),new com.notemaker.cli.Png(c));
        require(rendered.getPixel(50,50)==Color.RED,"Partial erase damaged or hid underlying layer");require(rendered.getPixel(20,50)==Color.BLACK,"Partial erase lost untouched pixels");rendered.recycle();
        test.runOnMainSync(()->{try{app.refreshShared(snapshot);}catch(Exception e){throw new RuntimeException(e);}});
        previewRollback(test,app);
        objectPreview(test,app);
        test.runOnMainSync(()->app.selectAt(120,170));
        long until=android.os.SystemClock.uptimeMillis()+10000;while(app.selectionSession==null&&android.os.SystemClock.uptimeMillis()<until)Thread.sleep(25);
        require(app.selectionSession!=null,"Activity did not select outline");
        test.runOnMainSync(()->app.selectLayer("under"));ReplicaService.writes.submit(()->{}).get();
        require(app.selectionSession==null&&app.selectedEntities.isEmpty(),"Layer switch retained selection");
        require(app.selecting,"Layer switch unexpectedly left the selection tool");
        require(map(app.replica.read().get("leases")).isEmpty(),"Layer switch retained leases");
    }
    private static List<Object> query(MainActivity app,double x,double y)throws Exception{return list(map(app.replica.command(Arrays.asList("selection","query","-"),object("layer","base","region",Geometry.json(Geometry.rect(x-.1,y-.1,.2,.2))))).get("items"));}
    private static void require(boolean condition,String message){if(!condition)throw new IllegalStateException(message);}
    private static void objectPreview(Integration test,MainActivity app)throws Exception{
        String id="preview-created-box";
        java.util.concurrent.CountDownLatch paused=new java.util.concurrent.CountDownLatch(1),release=new java.util.concurrent.CountDownLatch(1);
        java.util.concurrent.CountDownLatch committed=new java.util.concurrent.CountDownLatch(1),deliver=new java.util.concurrent.CountDownLatch(1);
        app.beforeObjectCommitCallback=()->{committed.countDown();try{if(!deliver.await(30,java.util.concurrent.TimeUnit.SECONDS))throw new IllegalStateException("Object callback gate timed out");}catch(InterruptedException e){Thread.currentThread().interrupt();throw new RuntimeException(e);}};
        ReplicaService.writes.execute(()->{paused.countDown();try{release.await();}catch(InterruptedException e){Thread.currentThread().interrupt();}});
        require(paused.await(5,java.util.concurrent.TimeUnit.SECONDS),"Object writer did not pause");
        long coalescedBefore=app.coalescedObjectFrames;
        Bitmap before=Bitmap.createBitmap(800,800,Bitmap.Config.ARGB_8888),during=Bitmap.createBitmap(800,800,Bitmap.Config.ARGB_8888),after=Bitmap.createBitmap(800,800,Bitmap.Config.ARGB_8888);
        try{
            test.runOnMainSync(()->{
                app.canvas.draw(new Canvas(before));
                app.persistObject(MainActivity.obj("kind","object","object",id,"shape","rect","layer","base","page",0,
                        "x",300,"y",300,"w",70,"h",70,"color",Color.BLACK,"fill",Color.BLACK,"width",4,"fontSize",24,"text",""));
                require(app.pendingObjectPreviews.containsKey(id),"Missing transient object preview");
                app.canvas.draw(new Canvas(during));
                require(!before.sameAs(during),"Pending object did not change canvas pixels");
                require(app.drawnObjectPreviews.contains(id),"Preview was not drawn");
                require(!app.model.objects.containsKey(id),"Preview entered model before commit");
            });
            require(!Store.model(app.replica.read()).objects.containsKey(id),"Preview changed durable state");
        }finally{release.countDown();}
        try{
            require(committed.await(30,java.util.concurrent.TimeUnit.SECONDS),"Object command did not commit");
            app.refreshFrom(app.replica);
            test.waitForIdleSync();
            require(app.objectRefreshDeferred,"Local listener did not reach the pending object before save callback");
            require(!app.model.objects.containsKey(id),"Local listener replaced the painted preview before save callback");
        }finally{deliver.countDown();app.beforeObjectCommitCallback=null;}
        ReplicaService.writes.submit(()->{}).get(10,java.util.concurrent.TimeUnit.SECONDS);test.waitForIdleSync();
        require(app.pendingObjectPreviews.isEmpty(),"Committed object retained transient preview");
        require(Store.model(app.replica.read()).objects.containsKey(id),"Previewed object was not durable");
        test.runOnMainSync(()->app.canvas.draw(new Canvas(after)));
        boolean samePixels=during.sameAs(after);
        before.recycle();during.recycle();after.recycle();
        require(samePixels,"Committed object changed preview pixels");
        require(app.coalescedObjectFrames==coalescedBefore+1,"Matching committed object still redrew the canvas");
    }
    private static void previewRollback(Integration test,MainActivity app)throws Exception{
        java.util.concurrent.CountDownLatch paused=new java.util.concurrent.CountDownLatch(1),release=new java.util.concurrent.CountDownLatch(1);
        ReplicaService.writes.execute(()->{paused.countDown();try{release.await();}catch(InterruptedException e){Thread.currentThread().interrupt();}});
        require(paused.await(5,java.util.concurrent.TimeUnit.SECONDS),"Writer did not pause");
        Model before=Store.model(app.replica.read());boolean locked=false;
        try{
            test.runOnMainSync(()->{try{app.applyErase("partial","base",0,new org.json.JSONArray("[[20,20]]"),8);}catch(Exception e){throw new RuntimeException(e);}});
            require(app.canvas.erasePreviews.size()==1,"Missing pending feedback");
            MainActivity.ErasePreview preview=app.canvas.erasePreviews.values().iterator().next();
            require(preview.layer.equals("base")&&preview.page==0&&preview.paths.size()==1,"Preview scope changed");
            Bitmap pixels=Bitmap.createBitmap(100,100,Bitmap.Config.ARGB_8888);Canvas canvas=new Canvas(pixels);canvas.drawColor(Color.RED);canvas.save();for(Path cut:preview.paths)canvas.clipOutPath(cut);canvas.drawColor(Color.BLACK);canvas.restore();
            require(pixels.getPixel(20,20)==Color.RED&&pixels.getPixel(40,20)==Color.BLACK,"Preview cleared underlying layer");pixels.recycle();
            require(before.objects.equals(Store.model(app.replica.read()).objects),"Preview mutated durable state");
            ReplicaService.ensure(app).execute(object("document",app.replica.document,"actor","racer","args",Arrays.asList("selection","select","-"),"input",object("session","preview-race","layer","base","page",0,"entities",Arrays.asList(object("kind","object","id","base-box")))));locked=true;
        }finally{release.countDown();}
        try{
            ReplicaService.writes.submit(()->{}).get(10,java.util.concurrent.TimeUnit.SECONDS);test.waitForIdleSync();
            require(app.canvas.erasePreviews.isEmpty(),"Rejected erase retained preview");
            require(app.saveError!=null,"Late lock did not reject erase");
            require(before.objects.equals(Store.model(app.replica.read()).objects),"Rejected erase changed source");
        }finally{
            if(locked)ReplicaService.ensure(app).execute(object("document",app.replica.document,"actor","racer","args",Arrays.asList("selection","deselect","preview-race"),"input",null));
            Map<String,Object> restored=app.replica.read();test.runOnMainSync(()->{try{app.saveError=null;app.refreshShared(restored);}catch(Exception e){throw new RuntimeException(e);}});
        }
    }
    static void cachePixels(){cachePixels(false);}
    private static void cachePixels(boolean fullSize){
        String svg="<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"100\" height=\"100\"><circle cx=\"50\" cy=\"50\" r=\"30\" fill=\"none\" stroke=\"black\"/><text x=\"5\" y=\"20\">Cache</text></svg>";
        Bitmap direct=Bitmap.createBitmap(200,200,Bitmap.Config.ARGB_8888),recorded=Bitmap.createBitmap(200,200,Bitmap.Config.ARGB_8888);
        com.notemaker.cli.Png.useSvgPictures=false;new com.notemaker.cli.Png(new Canvas(direct)).svg(5.25,10.5,150.5,160.25,svg);
        com.notemaker.cli.Png.useSvgPictures=true;new com.notemaker.cli.Png(new Canvas(recorded)).svg(5.25,10.5,150.5,160.25,svg);
        require(direct.sameAs(recorded),"SVG display-list pixels differ");direct.recycle();recorded.recycle();
        StringBuilder dots=new StringBuilder("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 714.2857142857143 1000.0000000000001\" width=\"714.2857142857143\" height=\"1000.0000000000001\"><g stroke=\"#999999\" stroke-opacity=\"1.0\" stroke-width=\"0.7142857142857143\">");
        for(int y=1;y<=43;y++)for(int x=1;x<=31;x++)dots.append("<circle cx=\"").append(x*22.857142857142858).append("\" cy=\"").append(y*22.857142857142858).append("\" r=\"1.4285714285714286\" fill=\"#999999\"/>");
        dots.append("</g></svg>");
        Bitmap fastDots=Bitmap.createBitmap(240,240,Bitmap.Config.ARGB_8888),domDots=Bitmap.createBitmap(240,240,Bitmap.Config.ARGB_8888);
        new com.notemaker.cli.Png(new Canvas(fastDots)).svg(5.25,10.5,210,210,dots.toString());
        new com.notemaker.cli.Png(new Canvas(domDots)).svg(5.25,10.5,210,210," "+dots);
        int changed=0,maxAlphaDifference=0,maxColorDifference=0;
        for(int y=0;y<240;y++)for(int x=0;x<240;x++){
            int fast=fastDots.getPixel(x,y),reference=domDots.getPixel(x,y);
            if(fast==reference)continue;
            changed++;maxAlphaDifference=Math.max(maxAlphaDifference,Math.abs(Color.alpha(fast)-Color.alpha(reference)));
            maxColorDifference=Math.max(maxColorDifference,Math.abs(Color.red(fast)*Color.alpha(fast)-Color.red(reference)*Color.alpha(reference))/255);
        }
        // The direct cubic path and AndroidSVG differ only at antialiased edges.
        require(changed<=1500&&maxAlphaDifference<=12&&maxColorDifference<=12,"Generated dot SVG fast path changed pixels: "+changed+", alpha "+maxAlphaDifference+", color "+maxColorDifference);
        fastDots.recycle();domDots.recycle();
        if(fullSize){Bitmap fullFast=Bitmap.createBitmap(1000,1400,Bitmap.Config.ARGB_8888),fullReference=Bitmap.createBitmap(1000,1400,Bitmap.Config.ARGB_8888);
        new com.notemaker.cli.Png(new Canvas(fullFast)).svg(0,0,1000,1400,dots.toString());
        new com.notemaker.cli.Png(new Canvas(fullReference)).svg(0,0,1000,1400," "+dots);
        int[] fastPixels=new int[1400000],referencePixels=new int[1400000];
        fullFast.getPixels(fastPixels,0,1000,0,0,1000,1400);fullReference.getPixels(referencePixels,0,1000,0,0,1000,1400);
        changed=0;maxAlphaDifference=0;maxColorDifference=0;
        for(int i=0;i<fastPixels.length;i++)if(fastPixels[i]!=referencePixels[i]){
            changed++;maxAlphaDifference=Math.max(maxAlphaDifference,Math.abs(Color.alpha(fastPixels[i])-Color.alpha(referencePixels[i])));
            maxColorDifference=Math.max(maxColorDifference,Math.abs(Color.red(fastPixels[i])*Color.alpha(fastPixels[i])-Color.red(referencePixels[i])*Color.alpha(referencePixels[i]))/255);
        }
        require(changed<=28000&&maxAlphaDifference<=12&&maxColorDifference<=12,"Full-size generated dot SVG changed pixels: "+changed+", alpha "+maxAlphaDifference+", color "+maxColorDifference);
        fullFast.recycle();fullReference.recycle();}
        String translucent=dots.toString().replace("#999999","#336699").replace("stroke-opacity=\"1.0\"","stroke-opacity=\"0.5\"");
        Bitmap alphaFast=Bitmap.createBitmap(240,240,Bitmap.Config.ARGB_8888),alphaReference=Bitmap.createBitmap(240,240,Bitmap.Config.ARGB_8888);
        new com.notemaker.cli.Png(new Canvas(alphaFast)).svg(5.25,10.5,210,210,translucent);
        new com.notemaker.cli.Png(new Canvas(alphaReference)).svg(5.25,10.5,210,210," "+translucent);
        changed=0;maxAlphaDifference=0;maxColorDifference=0;
        for(int y=0;y<240;y++)for(int x=0;x<240;x++){
            int fast=alphaFast.getPixel(x,y),reference=alphaReference.getPixel(x,y);
            if(fast==reference)continue;
            changed++;maxAlphaDifference=Math.max(maxAlphaDifference,Math.abs(Color.alpha(fast)-Color.alpha(reference)));
            maxColorDifference=Math.max(maxColorDifference,Math.abs(Color.red(fast)*Color.alpha(fast)-Color.red(reference)*Color.alpha(reference))/255);
        }
        require(changed<=1500&&maxAlphaDifference<=12&&maxColorDifference<=12,"Translucent generated dot SVG changed pixels: "+changed+", alpha "+maxAlphaDifference+", color "+maxColorDifference);
        alphaFast.recycle();alphaReference.recycle();
        LinkedHashMap<String,Map<String,Object>> visible=new LinkedHashMap<>();
        for(int i=0;i<3;i++){
            ArrayList<Object> points=new ArrayList<>();for(int p=0;p<40;p++)points.add(Arrays.asList(20+p*4,70+i*12+20*Math.sin(p*.3),.4+(p%5)*.1));
            visible.put("s"+i,object("kind","stroke","stroke","s"+i,"layer","base","page",0,"points",points,"width",12,"color",i==1?0x80660000:Color.BLACK,"brush",i==2?"square":"pen"));
        }
        visible.put("object:box",Actions.drawing(object("object","box","shape","rect","x",50,"y",50,"w",90,"h",90,"fill",0x80ff0000),"base"));
        Map<String,Object> original=visible.get("s1");InkCache cache=new InkCache(320,320);
        for(int step=0;step<10;step++){
            if(step==1)visible.put("s1",new DrawItem(new Model(),original).erase(Collections.singletonList(Geometry.rect(75,30,25,130))));
            if(step==2)visible.put("s1",original);
            if(step==3)visible.remove("s0");
            if(step==4){Map<String,Object> a=visible.remove("s1");visible.put("s1",a);}
            if(step==5){Map<String,Object> a=new LinkedHashMap<>(original);a.put("stroke","append");visible.put("append",a);}
            if(step==6){Map<String,Object> a=new LinkedHashMap<>(original);a.put("brush","marker");a.put("width",24);visible.put("s1",a);}
            if(step==7){Map<String,Object> box=visible.remove("object:box"),a=new LinkedHashMap<>(original);a.put("stroke","insert");visible.put("insert",a);visible.put("object:box",box);}
            if(step==9){Map<String,Object> a=new LinkedHashMap<>(visible.get("s1"));a.put("material",90);visible.put("s1",a);}
            if(step==8){Map<String,Object> box=new LinkedHashMap<>(visible.get("object:box"));box.put("x",80);box.put("rotation",33);visible.put("object:box",box);}
            Bitmap actual=Bitmap.createBitmap(320,320,Bitmap.Config.ARGB_8888),expected=Bitmap.createBitmap(320,320,Bitmap.Config.ARGB_8888);
            java.util.function.Consumer<Canvas> transform=c->{c.translate(10.5f,5.25f);c.scale(1.3f,1.3f);};
            cache.draw(new Canvas(actual),new LinkedHashMap<>(visible),transform,(c,s)->Render.drawItem(new Model(),s,new com.notemaker.cli.Png(c)));
            Canvas c=new Canvas(expected);transform.accept(c);for(Map<String,Object> s:visible.values())Render.drawItem(new Model(),s,new com.notemaker.cli.Png(c));
            if(!actual.sameAs(expected)){int differences=0,l=320,t=320,r=0,b=0;for(int y=0;y<320;y++)for(int x=0;x<320;x++)if(actual.getPixel(x,y)!=expected.getPixel(x,y)){differences++;l=Math.min(l,x);t=Math.min(t,y);r=Math.max(r,x);b=Math.max(b,y);}throw new IllegalStateException("Regional ink cache step "+step+": "+differences+" different pixels in "+l+","+t+"–"+r+","+b);}
            actual.recycle();expected.recycle();
        }
        cache.bitmap.recycle();
    }
}
