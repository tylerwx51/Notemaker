package com.notemaker;

import android.graphics.*;
import com.notemaker.core.*;
import java.util.*;
import static com.notemaker.core.Json.*;

/** Viewport culling and tile-cache checks against the editor's uncached draw path. */
final class ViewportCheck {
    private static void phase(String name){android.util.Log.i("ViewportCheck",name);}
    private static final class Frame {final Bitmap image;final Map<String,Object> metrics;Frame(Bitmap image,Map<String,Object> metrics){this.image=image;this.metrics=metrics;}}
    private static void require(boolean condition,String message){if(!condition)throw new IllegalStateException(message);}
    private static Bitmap snapshot(Integration test,MainActivity app){
        Bitmap[] image={null};test.runOnMainSync(()->{
            image[0]=Bitmap.createBitmap(app.canvas.getWidth(),app.canvas.getHeight(),Bitmap.Config.ARGB_8888);
            app.canvas.draw(new Canvas(image[0]));
        });return image[0];
    }
    private static Frame captureCold(Integration test,MainActivity app){
        Frame[] frame={null};test.runOnMainSync(()->{app.canvas.inkTransform="";Bitmap image=Bitmap.createBitmap(app.canvas.getWidth(),app.canvas.getHeight(),Bitmap.Config.ARGB_8888);app.canvas.draw(new Canvas(image));frame[0]=new Frame(image,app.canvas.tileCacheMetrics());});return frame[0];
    }
    private static int[] differences(Bitmap a,Bitmap b){
        require(a.getWidth()==b.getWidth()&&a.getHeight()==b.getHeight(),"Reference dimensions changed");
        int w=a.getWidth(),h=a.getHeight();int[] x=new int[w*h],y=new int[w*h];
        a.getPixels(x,0,w,0,0,w,h);b.getPixels(y,0,w,0,0,w,h);int[] out={0,0,0,0};
        for(int i=0;i<x.length;i++)if(x[i]!=y[i]){out[0]++;int px=i%w,py=i/w;out[1]=Math.max(out[1],Math.abs(Color.alpha(x[i])-Color.alpha(y[i])));out[2]=Math.max(out[2],Math.abs(Color.red(x[i])-Color.red(y[i])));out[3]=Math.max(out[3],Math.abs(Color.green(x[i])-Color.green(y[i])));}
        return out;
    }
    private static Map<String,Object> metrics(Integration test,MainActivity app){
        @SuppressWarnings("unchecked") Map<String,Object>[] result=new Map[1];test.runOnMainSync(()->result[0]=app.canvas.tileCacheMetrics());return result[0];
    }
    private static boolean hasItem(Map<String,Object> metrics,String id){for(Object value:list(metrics.get("lastRenderedItemIds")))if(id.equals(value))return true;return false;}
    private static long n(Map<String,Object> m,String key){return ((Number)m.get(key)).longValue();}
    private static void saveDebug(MainActivity app,String name,Bitmap bitmap){try(java.io.FileOutputStream out=app.openFileOutput(name,0)){bitmap.compress(Bitmap.CompressFormat.PNG,100,out);}catch(Exception e){throw new RuntimeException(e);}}
    private static void resetDocument(Integration test,MainActivity app,String type,float zoom,float ox,float oy)throws Exception{
        test.runOnMainSync(()->{
            app.document="viewport-"+UUID.randomUUID();app.host="";app.proxyUrl="";app.load(type);app.configureProxy();
            app.canvas.zoom=zoom;app.canvas.offsetX=ox;app.canvas.offsetY=oy;app.canvas.useInkCache=true;app.canvas.cullTileItems=true;app.canvas.monolithicTileProbe=null;
            app.setDrawingTool(null);app.erase=false;app.pan=false;app.selecting=false;app.scribbleToErase=false;
            app.page=0;app.selected="base";app.update();
        });
        ReplicaService.writes.submit(()->{}).get(30,java.util.concurrent.TimeUnit.SECONDS);test.waitForIdleSync();
    }
    private static List<Object> points(float scale,float zoom,float ox,float oy,float[][] screen){
        ArrayList<Object> result=new ArrayList<>();for(float[] p:screen)result.add(Arrays.asList((p[0]-ox)/(scale*zoom),(p[1]-oy)/(scale*zoom),.9));return result;
    }
    private static void addStroke(MainActivity app,String id,String brush,int color,double width,List<Object> pts)throws Exception{
        app.replica.command(Arrays.asList("stroke","put","-"),object("stroke",id,"layer","base","page",0,"color",color,"width",width,"brush",brush,"material",65,"points",pts));
    }
    private static void compare(Integration test,MainActivity app,String where){
        Bitmap cached=snapshot(test,app);test.runOnMainSync(()->app.canvas.cullTileItems=false);Bitmap direct=snapshot(test,app);test.runOnMainSync(()->app.canvas.cullTileItems=true);
        int[] diff=differences(cached,direct);int pixels=cached.getWidth()*cached.getHeight();if(diff[0]!=0){saveDebug(app,"viewport-compare-cached.png",cached);saveDebug(app,"viewport-compare-reference.png",direct);Frame cold=captureCold(test,app);int[] coldDiff=differences(cold.image,direct);saveDebug(app,"viewport-compare-cold.png",cold.image);try(java.io.FileOutputStream out=app.openFileOutput("viewport-compare-debug.txt",0)){out.write(("where="+where+" warmReferenceDiff="+java.util.Arrays.toString(diff)+" coldReferenceDiff="+java.util.Arrays.toString(coldDiff)+" coldMetrics="+com.notemaker.core.Json.write(cold.metrics)+" warmMetrics="+com.notemaker.core.Json.write(metrics(test,app))).getBytes("UTF-8"));}catch(Exception e){throw new RuntimeException(e);}cold.image.recycle();}cached.recycle();direct.recycle();
        require(diff[0]==0,where+" culled/unculled same-tier raster differs at "+diff[0]+" pixels (viewport "+pixels+"); per-channel maxima alpha/red/green="+diff[1]+"/"+diff[2]+"/"+diff[3]);
    }
    private static float nearbyTileBoundary(float center,float offset,float unit,float limit){
        float doc=(center-offset)/unit,boundary=(float)Math.floor(doc/256f)*256f,screen=offset+boundary*unit;
        if(screen<24||screen>limit-24){float next=boundary+256,projected=offset+next*unit;if(projected>=24&&projected<=limit-24)boundary=next;}
        return boundary;
    }
    private static void seamCompare(Integration test,MainActivity app,float zoom,float ox,float oy,float scale,float seamX,float seamY){
        Bitmap cached=snapshot(test,app);
        float unit=scale*zoom,probeX=48/unit,probeY=24/unit;
        test.runOnMainSync(()->app.canvas.monolithicTileProbe=new RectF(seamX-probeX,seamY-probeY,seamX+probeX,seamY+probeY));
        Bitmap monolithic=snapshot(test,app);test.runOnMainSync(()->app.canvas.monolithicTileProbe=null);
        test.runOnMainSync(()->app.canvas.useInkCache=false);Bitmap direct=snapshot(test,app);test.runOnMainSync(()->app.canvas.useInkCache=true);
        int w=cached.getWidth(),h=cached.getHeight(),sx=Math.round(ox+seamX*scale*zoom),sy=Math.round(oy+seamY*scale*zoom);
        int left=Math.max(0,sx-30),right=Math.min(w,sx+30),top=Math.max(0,sy-8),bottom=Math.min(h,sy+9),visible=0,worst=0,monoDiff=0,diffLeft=w,diffTop=h,diffRight=0,diffBottom=0;
        for(int y=top;y<bottom;y++)for(int x=left;x<right;x++){
            int a=cached.getPixel(x,y),b=direct.getPixel(x,y),m=monolithic.getPixel(x,y);if(m!=Color.WHITE)visible++;
            if(a!=m){monoDiff++;diffLeft=Math.min(diffLeft,x);diffTop=Math.min(diffTop,y);diffRight=Math.max(diffRight,x+1);diffBottom=Math.max(diffBottom,y+1);}
            worst=Math.max(worst,Math.max(Math.abs(Color.red(a)-Color.red(b)),Math.max(Math.abs(Color.green(a)-Color.green(b)),Math.abs(Color.blue(a)-Color.blue(b)))));
        }
        if(monoDiff>0){saveDebug(app,"viewport-seam-tiled.png",cached);saveDebug(app,"viewport-seam-monolithic.png",monolithic);saveDebug(app,"viewport-seam-direct.png",direct);try(java.io.FileOutputStream out=app.openFileOutput("viewport-seam-debug.txt",0)){out.write(("zoom="+zoom+" seam="+seamX+","+seamY+" screen="+sx+","+sy+" roi="+left+","+top+".."+right+","+bottom+" diffBounds="+diffLeft+","+diffTop+".."+diffRight+","+diffBottom+" monoDiff="+monoDiff+" worst="+worst+" unit="+(scale*zoom)+" tier="+app.canvas.rasterTier+" metrics="+com.notemaker.core.Json.write(metrics(test,app))).getBytes("UTF-8"));}catch(Exception e){throw new RuntimeException(e);}}
        cached.recycle();monolithic.recycle();direct.recycle();
        require(visible>4,"Translucent seam fixture is not visible at the selected zoom");
        require(monoDiff==0,"Tile seam differs from the monolithic same-tier reference at "+monoDiff+" ROI pixels");
        require(worst<=24,"Tile seam changed translucent pigment by "+worst+" channel levels");
    }
    private static int edgeHits(Bitmap image,String edge){
        int w=image.getWidth(),h=image.getHeight(),hits=0;
        if(edge.equals("top")){for(int y=0;y<12;y++)for(int x=12;x<w-12;x++)if(image.getPixel(x,y)!=Color.WHITE)hits++;}
        if(edge.equals("bottom")){for(int y=h-12;y<h;y++)for(int x=12;x<w-12;x++)if(image.getPixel(x,y)!=Color.WHITE)hits++;}
        if(edge.equals("left")){for(int x=0;x<12;x++)for(int y=12;y<h-12;y++)if(image.getPixel(x,y)!=Color.WHITE)hits++;}
        if(edge.equals("right")){for(int x=w-12;x<w;x++)for(int y=12;y<h-12;y++)if(image.getPixel(x,y)!=Color.WHITE)hits++;}
        return hits;
    }
    static void run(Integration test,MainActivity app)throws Exception{
        ReplicaService.writes.submit(()->{}).get(30,java.util.concurrent.TimeUnit.SECONDS);
        final float width=app.canvas.getWidth(),height=app.canvas.getHeight(),scale=app.canvas.scale();
        require(width>200&&height>200,"Canvas has no usable viewport");
        String[] brushes={"pen","marker","charcoal","oily","square","pencil"};
        for(String type:new String[]{"fixed","infinite"})for(float zoom:new float[]{.2f,1f,5f}){
            phase("begin "+type+" zoom="+zoom);
            float ox=13.25f,oy=-7.5f;resetDocument(test,app,type,zoom,ox,oy);
            // Each centerline is parallel to an edge and remains just beyond the
            // viewport. Its wide footprint, cap or textured brush must still show.
            float pad=Math.max(.5f,70*scale*zoom*.12f);
            float[][][] lines={{{-300,-pad},{width+300,-pad}},{{-300,height+pad},{width+300,height+pad}},{{-pad,-300},{-pad,height+300}},{{width+pad,-300},{width+pad,height+300}}};
            String[] sides={"top","bottom","left","right"};
            for(int i=0;i<4;i++)addStroke(app,"edge-"+i,brushes[(i+(int)(zoom*10))%brushes.length],0xff202020,70,points(scale,zoom,ox,oy,lines[i]));
            // Tile boundaries are at 256 document units. A translucent long
            // stroke crosses a visible tile corner, making a duplicated gutter visible.
            float unit=scale*zoom,seamX=type.equals("fixed")?0:nearbyTileBoundary(width/2,ox,unit,width),seamY=type.equals("fixed")?256:nearbyTileBoundary(height/2,oy,unit,height);
            addStroke(app,"tile-seam","charcoal",0x80660000,60,Arrays.asList(Arrays.asList((double)seamX-180,(double)seamY,.8),Arrays.asList((double)seamX+180,(double)seamY,.8)));
            // Include a large fully offscreen textured stroke to prove culling
            // happens before the expensive geometry renderer is entered.
            float[][] far={{width+2000,height+1700},{width+2600,height+1800}};
            addStroke(app,"offscreen-heavy","charcoal",Color.BLACK,100,points(scale,zoom,ox,oy,far));
            if(type.equals("fixed"))app.replica.command(Arrays.asList("object","put","-"),object("object","outside-page","layer","base","page",0,"shape","rect","x",1010,"y",300,"w",100,"h",100,"fill",0x800000ff,"color",Color.BLACK,"width",3));
            Map<String,Object> initial=app.replica.read();test.runOnMainSync(()->{try{app.refreshShared(initial);}catch(Exception e){throw new RuntimeException(e);}});
            test.waitForIdleSync();
            Frame firstFrame=captureCold(test,app);Bitmap rendered=firstFrame.image;Map<String,Object> firstDraw=firstFrame.metrics;
            phase("initial draw "+type+" zoom="+zoom);
            require(n(firstDraw,"itemRenders")>0,"Viewport draw did not execute any geometry rendering; the cull check would be vacuous");
            if(type.equals("infinite")){
                test.runOnMainSync(()->app.canvas.useInkCache=false);Bitmap uncullled=snapshot(test,app);test.runOnMainSync(()->app.canvas.useInkCache=true);
                for(String side:sides){int referenceHits=edgeHits(uncullled,side),cachedHits=edgeHits(rendered,side);if(referenceHits<=3||cachedHits<=3){int si=Arrays.asList(sides).indexOf(side);Map<String,Object> edge=app.model.strokes.get("edge-"+si);RectF bounds=edge==null?new RectF():new TileInkCache.Item("edge-"+si,edge).bounds;RectF viewport=app.canvas.documentViewport();saveDebug(app,"viewport-edge-cached.png",rendered);saveDebug(app,"viewport-edge-reference.png",uncullled);try(java.io.FileOutputStream out=app.openFileOutput("viewport-edge-debug.txt",0)){out.write(("side="+side+" zoom="+zoom+" width="+width+" height="+height+" scale="+scale+" unit="+unit+" pad="+pad+" screenLine="+Arrays.deepToString(lines[si])+" documentPoints="+points(scale,zoom,ox,oy,lines[si])+" itemBounds="+bounds+" documentViewport="+viewport+" tier="+app.canvas.rasterTier+" tileSize="+TileInkCache.tileSize(app.canvas.rasterTier)+" referenceHits="+referenceHits+" cachedHits="+cachedHits+" metrics="+com.notemaker.core.Json.write(metrics(test,app))).getBytes("UTF-8"));}catch(Exception e){throw new RuntimeException(e);}}require(referenceHits>3,"Direct uncached reference fixture did not paint across "+side+" edge at zoom="+zoom+" hits="+referenceHits+" viewport="+uncullled.getWidth()+"x"+uncullled.getHeight()+" scale="+scale+" unit="+unit+" pad="+pad);require(cachedHits>3,"Cached centerline outside "+side+" edge was culled despite reference paint");}
                uncullled.recycle();
                require(hasItem(firstDraw,"edge-0")||hasItem(firstDraw,"edge-1")||hasItem(firstDraw,"edge-2")||hasItem(firstDraw,"edge-3"),"No visible edge stroke reached the geometry renderer");
                require(!hasItem(firstDraw,"offscreen-heavy"),"Fully offscreen item reached the geometry renderer");
                seamCompare(test,app,zoom,ox,oy,scale,seamX,seamY);
                phase("seam passed "+type+" zoom="+zoom);
            }else{
                int x0=Math.max(0,Math.round(ox+scale*zoom*1008)),x1=Math.min(rendered.getWidth(),Math.round(ox+scale*zoom*1120));
                int y0=Math.max(0,Math.round(oy+scale*zoom*290)),y1=Math.min(rendered.getHeight(),Math.round(oy+scale*zoom*410));int blue=0;
                if(zoom==.2f)require(x1>x0&&y1>y0,"Fixed-page clipping probe did not intersect the viewport");
                for(int y=y0;y<y1;y++)for(int x=x0;x<x1;x++){int p=rendered.getPixel(x,y);if(Color.blue(p)>Color.red(p)+25&&Color.blue(p)>Color.green(p)+25)blue++;}
                require(blue==0,"Fixed-page rendering showed an object outside the paper clip");
            }
            rendered.recycle();
            compare(test,app,type+" zoom="+zoom);
            phase("uncull reference passed "+type+" zoom="+zoom);
            Map<String,Object> first=metrics(test,app);
            require(n(first,"culledItems")>0,"No fully offscreen items were culled");
            long itemsBefore=n(first,"itemRenders");compare(test,app,"repeat "+type+" zoom="+zoom);Map<String,Object> second=metrics(test,app);
            require(n(second,"itemRenders")==itemsBefore,"Repeated identical draw rerendered tile geometry");
            require(n(second,"currentBytes")<=n(second,"maxBytes"),"Tile cache exceeded its configured memory bound");
            require(n(second,"peakBytes")<=n(second,"maxBytes"),"Tile cache peak exceeded its configured memory bound");
            phase("reuse passed "+type+" zoom="+zoom);
            // A document-space object at the top edge exercises geometry clipping
            // through the same viewport cache; then edits must invalidate its tile.
            long beforeEdit=n(metrics(test,app),"tileRenders");
            app.replica.command(Arrays.asList("object","put","-"),object("object","edge-object","layer","base","page",0,"shape","rect","x",100,"y",100,"w",160,"h",90,"fill",0x804000ff,"color",Color.BLACK,"width",3));
            Map<String,Object> withObject=app.replica.read();test.runOnMainSync(()->{try{app.refreshShared(withObject);}catch(Exception e){throw new RuntimeException(e);}});test.waitForIdleSync();
            compare(test,app,"object insertion "+type+" zoom="+zoom);
            phase("object insertion passed "+type+" zoom="+zoom);
            require(n(metrics(test,app),"tileRenders")>beforeEdit,"Object edit did not invalidate and rerender affected viewport tiles");
            long stableItems=n(metrics(test,app),"itemRenders");compare(test,app,"stable object cache "+type+" zoom="+zoom);
            require(n(metrics(test,app),"itemRenders")==stableItems,"Stable object was rendered repeatedly");
            phase("stable object passed "+type+" zoom="+zoom);
            Bitmap beforeErase=snapshot(test,app);
            app.replica.command(Arrays.asList("erase","-"),object("layer","base","page",0,"mode","partial","radius",28,"points",Arrays.asList(Arrays.asList((double)seamX,(double)seamY))));
            Map<String,Object> masked=app.replica.read();test.runOnMainSync(()->{try{app.refreshShared(masked);}catch(Exception e){throw new RuntimeException(e);}});test.waitForIdleSync();
            Bitmap afterErase=snapshot(test,app);int erasedPixels=differences(beforeErase,afterErase)[0];beforeErase.recycle();afterErase.recycle();
            require(erasedPixels>0,"Partial erase fixture did not change visible seam pixels");
            compare(test,app,"masked stroke at tile seam "+type+" zoom="+zoom);
            phase("partial erase passed "+type+" zoom="+zoom);
            // Pan changes which document tiles are visible. Returning to the
            // original viewport should reuse resident tiles without a rebuild.
            if(type.equals("infinite")&&zoom==1f){
                test.runOnMainSync(()->{app.canvas.offsetX+=width*.18f;app.canvas.offsetY+=height*.16f;});snapshot(test,app).recycle();Map<String,Object> away=metrics(test,app);
                test.runOnMainSync(()->{app.canvas.offsetX=ox;app.canvas.offsetY=oy;});snapshot(test,app).recycle();Map<String,Object> returned=metrics(test,app);
                require(n(returned,"reusedTiles")>n(away,"reusedTiles"),"Returning to the original pan did not reuse resident tiles");
                require(n(returned,"tileRenders")==n(away,"tileRenders"),"Returning to the original pan rerendered resident tiles");
            }
        }
        // A connector's cached geometry depends on its attached nodes. Moving a
        // node changes the same connector map, so verify the editor re-resolves it.
        resetDocument(test,app,"infinite",1f,0,0);phase("begin attached arrow");
        app.replica.command(Arrays.asList("object","put","-"),object("object","node-a","layer","base","page",0,"shape","rect","x",300,"y",300,"w",100,"h",100,"fill",0x804000ff,"color",Color.BLACK,"width",3));
        app.replica.command(Arrays.asList("object","put","-"),object("object","node-b","layer","base","page",0,"shape","rect","x",700,"y",300,"w",100,"h",100,"fill",0x804000ff,"color",Color.BLACK,"width",3));
        app.replica.command(Arrays.asList("object","put","-"),object("object","connector","layer","base","page",0,"shape","arrowLine","x",350,"y",350,"w",400,"h",0,"from","node-a","to","node-b","color",Color.BLACK,"width",8));
        Map<String,Object> connected=app.replica.read();test.runOnMainSync(()->{try{app.refreshShared(connected);}catch(Exception e){throw new RuntimeException(e);}});test.waitForIdleSync();
        compare(test,app,"attached arrow initial");
        long beforeMove=n(metrics(test,app),"tileRenders");
        app.replica.command(Arrays.asList("object","put","-"),object("object","node-b","layer","base","page",0,"shape","rect","x",900,"y",450,"w",100,"h",100,"fill",0x804000ff,"color",Color.BLACK,"width",3));
        Map<String,Object> moved=app.replica.read();test.runOnMainSync(()->{try{app.refreshShared(moved);}catch(Exception e){throw new RuntimeException(e);}});test.waitForIdleSync();
        compare(test,app,"attached arrow endpoint moved");
        require(n(metrics(test,app),"tileRenders")>beforeMove,"Moving an attached node did not rerender tiles containing its arrow");
        phase("attached arrow passed");

        resetDocument(test,app,"infinite",1f,0,0);phase("begin zoom tier");
        addStroke(app,"zoom-stroke","pen",0xff202020,20,Arrays.asList(Arrays.asList(20d,120d,.8),Arrays.asList(900d,120d,.8)));
        Map<String,Object> zoomContent=app.replica.read();test.runOnMainSync(()->{try{app.refreshShared(zoomContent);}catch(Exception e){throw new RuntimeException(e);}});test.waitForIdleSync();
        snapshot(test,app).recycle();Map<String,Object> tierOne=metrics(test,app);long originalScaleRenders=n(tierOne,"tileRenders");
        require(hasItem(tierOne,"zoom-stroke"),"Zoom fixture did not populate a tile with visible geometry");
        float nextZoom=5f;test.runOnMainSync(()->app.canvas.zoom=nextZoom);snapshot(test,app).recycle();Map<String,Object> tierTwo=metrics(test,app);
        require(n(tierTwo,"densityTier")>n(tierOne,"densityTier"),"Zoom fixture did not cross a populated raster-density tier");
        require(n(tierTwo,"tileRenders")>originalScaleRenders,"Zoom density change did not rerasterize populated tiles");
        compare(test,app,"intermediate zoom tier");
        phase("zoom tier passed");
        // Exercise many viewport shifts to force LRU eviction while remaining
        // within the advertised bounded bitmap budget.
        resetDocument(test,app,"infinite",5f,0,0);phase("begin LRU");
        app.replica.command(Arrays.asList("object","put","-"),object("object","wide-cache-fill","layer","base","page",0,"shape","rect","x",-50000,"y",-50000,"w",100000,"h",100000,"fill",0x20660000,"color",Color.BLACK,"width",2));
        Map<String,Object> cacheFill=app.replica.read();test.runOnMainSync(()->{try{app.refreshShared(cacheFill);}catch(Exception e){throw new RuntimeException(e);}});test.waitForIdleSync();
        snapshot(test,app).recycle();require(n(metrics(test,app),"densityTier")==8,"High-zoom eviction fixture did not reach density tier 8");
        long evictionsBefore=n(metrics(test,app),"evictedTiles");
        for(int i=0;i<40;i++){
            final int shift=i;test.runOnMainSync(()->{app.canvas.offsetX=-shift*app.canvas.getWidth()*.8f;app.canvas.offsetY=-shift*app.canvas.getHeight()*.7f;});snapshot(test,app).recycle();
        }
        Map<String,Object> bounded=metrics(test,app);
        require(n(bounded,"currentBytes")<=n(bounded,"maxBytes")&&n(bounded,"peakBytes")<=n(bounded,"maxBytes"),"Panning grew tile cache beyond the memory budget");
        require(n(bounded,"evictedTiles")>evictionsBefore,"Populated high-density panning did not exercise bounded-cache eviction");
        phase("LRU passed");
    }
}
