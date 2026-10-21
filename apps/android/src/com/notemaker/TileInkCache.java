package com.notemaker;

import android.graphics.*;
import com.notemaker.core.*;
import java.util.*;
import java.util.function.BiConsumer;
import static com.notemaker.core.Json.*;

/** Bounded document-space cache for settled layer tiles. */
final class TileInkCache {
    static final int GUTTER=2;
    static final long MAX_BYTES=96L*1024*1024;
    static final long MAX_PICTURE_BYTES=24L*1024*1024;
    static final int MAX_PICTURE_POINTS=100000;
    private static final int MAX_PICTURES=512;
    static long tileRenders,reusedTileRepaints,itemRenders,geometryRenders,pictureReuses,transientPictureReuses,pictureMisses,pictureEvictions,culledItems,allocatedTiles,reusedTiles,evictedTiles;
    private long currentBytes,peakBytes,cacheClears;
    private long pictureBytes,picturePoints;
    private final LinkedHashSet<String> renderedItemIds=new LinkedHashSet<>();
    private final LinkedHashSet<String> geometryItemIds=new LinkedHashSet<>();
    private static final class BoundRecord {final RectF bounds;final int pointCount;BoundRecord(RectF bounds,int pointCount){this.bounds=bounds;this.pointCount=pointCount;}}
    private static final IdentityHashMap<Map<String,Object>,BoundRecord> BOUNDS=new IdentityHashMap<>();
    private static int cachedBoundPoints;
    static final class Item {final String id;final Map<String,Object> value;final RectF bounds;final boolean cachePicture;
        Item(String id,Map<String,Object> value){this(id,value,true);}
        Item(String id,Map<String,Object> value,boolean cachePicture){this.id=id;this.value=value;this.bounds=cachePicture?bounds(value):computeBounds(value);this.cachePicture=cachePicture;}
    }
    private static final class Key {
        final String layer;final int page,x,y,tier;final boolean culling;
        Key(int page,String layer,int x,int y,int tier,boolean culling){this.page=page;this.layer=layer;this.x=x;this.y=y;this.tier=tier;this.culling=culling;}
        public int hashCode(){return Objects.hash(page,layer,x,y,tier,culling);}
        public boolean equals(Object other){if(!(other instanceof Key))return false;Key k=(Key)other;return page==k.page&&x==k.x&&y==k.y&&tier==k.tier&&culling==k.culling&&layer.equals(k.layer);}
    }
    private static final class Tile {final Bitmap bitmap;final BitmapShader shader;final Matrix bitmapMatrix;List<Item> items;final long bytes;
        Tile(Bitmap bitmap,List<Item> items,int tx,int ty,int tier,int tileSize){this.bitmap=bitmap;this.items=items;bytes=(long)bitmap.getRowBytes()*bitmap.getHeight();shader=shader(bitmap,tx*tileSize,ty*tileSize,tier);
            bitmapMatrix=new Matrix();float inv=1f/tier;bitmapMatrix.setValues(new float[]{inv,0,tx*tileSize-GUTTER*inv,0,inv,ty*tileSize-GUTTER*inv,0,0,1});}}
    private static final class PathEntry {final com.notemaker.cli.Png.StrokeCommands commands;final Map<String,Object> source;final long bytes,points;
        PathEntry(com.notemaker.cli.Png.StrokeCommands commands,Map<String,Object> source,long bytes,long points){this.commands=commands;this.source=source;this.bytes=bytes;this.points=points;}}
    private final LinkedHashMap<Key,Tile> tiles=new LinkedHashMap<>(32,.75f,true);
    private final LinkedHashMap<String,PathEntry> strokePaths=new LinkedHashMap<>(64,.75f,true);
    private final LinkedHashMap<String,PathEntry> transientStrokePaths=new LinkedHashMap<>();
    private final Paint bitmapPaint=new Paint(Paint.FILTER_BITMAP_FLAG|Paint.DITHER_FLAG);
    private final Paint strokePaint=strokePaint();
    private static Paint strokePaint(){Paint paint=new Paint(Paint.ANTI_ALIAS_FLAG);paint.setStrokeCap(Paint.Cap.ROUND);return paint;}

    static int tileSize(int tier){return tier>=8?64:tier>=4?128:256;}
    void beginFrame(){renderedItemIds.clear();geometryItemIds.clear();transientStrokePaths.clear();}
    private void recordRendered(String id){renderedItemIds.add(id);}
    private void recordGeometry(String id){geometryItemIds.add(id);}

    static int densityTier(float effectiveScale){if(!(effectiveScale>0))return 1;int tier=1;while(tier<8&&effectiveScale>tier*1.41421356f)tier*=2;while(tier>1&&effectiveScale<tier/1.41421356f)tier/=2;return Math.max(1,tier);}

    void draw(Canvas destination,int page,String layer,List<Item> items,RectF viewport,int tier,boolean cullItems,BiConsumer<Canvas,Map<String,Object>> renderer){
        int tileSize=tileSize(tier);
        int firstX=(int)Math.floor(viewport.left/tileSize),lastX=(int)Math.floor(Math.nextDown(viewport.right)/tileSize);
        int firstY=(int)Math.floor(viewport.top/tileSize),lastY=(int)Math.floor(Math.nextDown(viewport.bottom)/tileSize);
        Bitmap referenceBitmap=null;Canvas referenceCanvas=null;
        try{for(int ty=firstY;ty<=lastY;ty++)for(int tx=firstX;tx<=lastX;tx++){
            float left=tx*tileSize,top=ty*tileSize,gutter=GUTTER/(float)tier;RectF core=new RectF(left-gutter,top-gutter,left+tileSize+gutter,top+tileSize+gutter);
            ArrayList<Item> contributors=new ArrayList<>();if(cullItems){for(Item item:items)if(RectF.intersects(core,item.bounds))contributors.add(item);}else contributors.addAll(items);
            Key key=new Key(page,layer,tx,ty,tier,true);Tile tile=cullItems?tiles.get(key):null;
            if(contributors.isEmpty()){if(tile!=null)remove(key,tile);continue;}
            if(tile!=null&&same(tile.items,contributors)){reusedTiles++;drawTileBitmap(destination,tile,tx,ty,tileSize);continue;}
            // Full tile repaint preserves Skia's exact anti-aliasing at changed
            // item edges. Reuse a resident bitmap to avoid graphics allocation.
            if(cullItems)for(Item item:items)if(!RectF.intersects(core,item.bounds))culledItems++;
            int size=tileSize*tier+2*GUTTER;Bitmap bitmap;Canvas canvas;
            if(cullItems){if(tile!=null){bitmap=tile.bitmap;bitmap.eraseColor(Color.TRANSPARENT);reusedTileRepaints++;}else{trimFor((long)size*size*4);bitmap=Bitmap.createBitmap(size,size,Bitmap.Config.ARGB_8888);}canvas=new Canvas(bitmap);}
            else{if(referenceBitmap==null){referenceBitmap=Bitmap.createBitmap(size,size,Bitmap.Config.ARGB_8888);referenceCanvas=new Canvas(referenceBitmap);}else referenceBitmap.eraseColor(Color.TRANSPARENT);bitmap=referenceBitmap;canvas=referenceCanvas;canvas.setMatrix(new Matrix());}
            canvas.scale(tier,tier);canvas.translate(-left+gutter,-top+gutter);
            if(cullItems)canvas.clipRect(left-gutter,top-gutter,left+tileSize+gutter,top+tileSize+gutter);
            for(Item item:contributors){if(cullItems){itemRenders++;recordRendered(item.id);drawItem(canvas,item,renderer);}else renderer.accept(canvas,item.value);}
            if(cullItems){if(tile==null){tile=new Tile(bitmap,contributors,tx,ty,tier,tileSize);tiles.put(key,tile);currentBytes+=tile.bytes;allocatedTiles++;peakBytes=Math.max(peakBytes,currentBytes);}else tile.items=contributors;drawTileBitmap(destination,tile,tx,ty,tileSize);tileRenders++;}else{BitmapShader shader=shader(bitmap,left,top,tier);drawTile(destination,shader,tx,ty,tileSize);}
        }}finally{if(referenceBitmap!=null)referenceBitmap.recycle();}
    }
    void drawMonolithic(Canvas destination,List<Item> items,RectF probe,int tier,BiConsumer<Canvas,Map<String,Object>> renderer){
        int tileSize=tileSize(tier);int firstX=(int)Math.floor(probe.left/tileSize),lastX=(int)Math.floor(Math.nextDown(probe.right)/tileSize),firstY=(int)Math.floor(probe.top/tileSize),lastY=(int)Math.floor(Math.nextDown(probe.bottom)/tileSize);
        float left=firstX*tileSize,top=firstY*tileSize,right=(lastX+1)*tileSize,bottom=(lastY+1)*tileSize;int width=(int)Math.ceil((right-left)*tier)+2*GUTTER,height=(int)Math.ceil((bottom-top)*tier)+2*GUTTER;
        Bitmap bitmap=Bitmap.createBitmap(width,height,Bitmap.Config.ARGB_8888);Canvas canvas=new Canvas(bitmap);canvas.scale(tier,tier);canvas.translate(-left+((float)GUTTER/tier),-top+((float)GUTTER/tier));canvas.clipRect(left-GUTTER/(float)tier,top-GUTTER/(float)tier,right+GUTTER/(float)tier,bottom+GUTTER/(float)tier);
        for(Item item:items)renderer.accept(canvas,item.value);
        drawCore(destination,shader(bitmap,left,top,tier),left,top,right,bottom);bitmap.recycle();
    }
    private static BitmapShader shader(Bitmap bitmap,float originX,float originY,int tier){BitmapShader shader=new BitmapShader(bitmap,Shader.TileMode.CLAMP,Shader.TileMode.CLAMP);Matrix local=new Matrix();float inv=1f/tier,left=originX-GUTTER*inv,top=originY-GUTTER*inv;local.setValues(new float[]{inv,0,left,0,inv,top,0,0,1});shader.setLocalMatrix(local);return shader;}
    private void drawTile(Canvas destination,BitmapShader shader,int tx,int ty,int tileSize){float x=tx*tileSize,y=ty*tileSize;drawCore(destination,shader,x,y,x+tileSize,y+tileSize);}
    private void drawTileBitmap(Canvas destination,Tile tile,int tx,int ty,int tileSize){float x=tx*tileSize,y=ty*tileSize;
        destination.save();destination.clipRect(x,y,x+tileSize,y+tileSize);bitmapPaint.setShader(null);bitmapPaint.setAntiAlias(false);
        destination.drawBitmap(tile.bitmap,tile.bitmapMatrix,bitmapPaint);destination.restore();}
    private void drawCore(Canvas destination,BitmapShader shader,float left,float top,float right,float bottom){bitmapPaint.setShader(shader);bitmapPaint.setAntiAlias(false);destination.drawRect(left,top,right,bottom,bitmapPaint);bitmapPaint.setShader(null);}
    private static boolean same(List<Item> a,List<Item> b){if(a.size()!=b.size())return false;for(int i=0;i<a.size();i++){Map<String,Object> av=a.get(i).value,bv=b.get(i).value;if(av!=bv&&!av.equals(bv))return false;}return true;}
    private void trimFor(long incoming){Iterator<Map.Entry<Key,Tile>> it=tiles.entrySet().iterator();while(currentBytes+incoming>MAX_BYTES&&it.hasNext()){Map.Entry<Key,Tile> e=it.next();currentBytes-=e.getValue().bytes;e.getValue().bitmap.recycle();it.remove();evictedTiles++;}}
    private void remove(Key key,Tile tile){tiles.remove(key);currentBytes-=tile.bytes;tile.bitmap.recycle();}
    private void drawItem(Canvas canvas,Item item,BiConsumer<Canvas,Map<String,Object>> renderer){
        if(!"stroke".equals(item.value.get("kind"))){geometryRenders++;recordGeometry(item.id);renderer.accept(canvas,item.value);return;}
        PathEntry entry=item.cachePicture?strokePaths.get(item.id):transientStrokePaths.get(item.id);
        if(entry!=null&&(entry.source==item.value||entry.source.equals(item.value))){if(item.cachePicture)pictureReuses++;else transientPictureReuses++;play(canvas,entry);return;}
        if(item.cachePicture){if(entry!=null)removePath(item.id,entry);pictureMisses++;}else transientStrokePaths.remove(item.id);
        long points=pathPointEstimate(item.value),cost=4096+points*256;
        boolean retain=item.cachePicture&&points>0&&points<=20000&&cost<=MAX_PICTURE_BYTES;
        if(retain)trimPaths(cost,points);
        geometryRenders++;recordGeometry(item.id);
        PathEntry recorded=new PathEntry(com.notemaker.cli.Png.compileStroke(item.value),item.value,cost,points);
        if(retain&&pictureBytes+cost<=MAX_PICTURE_BYTES&&picturePoints+points<=MAX_PICTURE_POINTS&&strokePaths.size()<MAX_PICTURES){strokePaths.put(item.id,recorded);pictureBytes+=cost;picturePoints+=points;}
        else if(!item.cachePicture)transientStrokePaths.put(item.id,recorded);
        play(canvas,recorded);
    }
    private void play(Canvas canvas,PathEntry entry){if(entry!=null)com.notemaker.cli.Png.replayStroke(canvas,entry.commands,strokePaint);}
    private void trimPaths(long incoming,long points){Iterator<Map.Entry<String,PathEntry>> it=strokePaths.entrySet().iterator();while((pictureBytes+incoming>MAX_PICTURE_BYTES||picturePoints+points>MAX_PICTURE_POINTS||strokePaths.size()>=MAX_PICTURES)&&it.hasNext()){Map.Entry<String,PathEntry> e=it.next();pictureBytes-=e.getValue().bytes;picturePoints-=e.getValue().points;it.remove();pictureEvictions++;}}
    private void removePath(String id,PathEntry entry){strokePaths.remove(id);pictureBytes-=entry.bytes;picturePoints-=entry.points;}
    private static long pathPointEstimate(Map<String,Object> item){List<Object> raw=list(item.getOrDefault("points",Collections.emptyList()));if(raw.isEmpty())return 0;double length=0,px=0,py=0;for(int i=0;i<raw.size();i++){List<Object> p=list(raw.get(i));double x=((Number)p.get(0)).doubleValue(),y=((Number)p.get(1)).doubleValue();if(i>0)length+=Math.hypot(x-px,y-py);px=x;py=y;}double width=((Number)item.get("width")).doubleValue();String brush=String.valueOf(item.getOrDefault("brush","pen"));double factor="marker".equals(brush)?1.8:"neoBrush".equals(brush)?1.4:Arrays.asList("brush","charcoal").contains(brush)?1.35:Arrays.asList("pencil","oily").contains(brush)?.82:Arrays.asList("latinCalligraphy","asiaCalligraphy").contains(brush)?1.5:1;double step=Math.max(.75,Math.min(2.0,width*factor*.2));long estimate=Math.max(1,Math.min(8192,(long)Math.ceil(length/step)+1));for(Object erase:list(item.getOrDefault("erasures",Collections.emptyList())))estimate+=list(erase).size();return estimate;}
    void clear(){cacheClears++;for(Tile tile:tiles.values())tile.bitmap.recycle();currentBytes=0;tiles.clear();strokePaths.clear();transientStrokePaths.clear();pictureBytes=picturePoints=0;renderedItemIds.clear();geometryItemIds.clear();}
    long cacheClears(){return cacheClears;}
    int residentPageCount(){Set<Integer> pages=new HashSet<>();for(Key key:tiles.keySet())pages.add(key.page);return pages.size();}
    Map<String,Object> metrics(){ArrayList<String> rendered=new ArrayList<>(renderedItemIds),geometry=new ArrayList<>(geometryItemIds);return object("tileRenders",tileRenders,"itemRenders",itemRenders,"geometryRenders",geometryRenders,"pathCommandReuses",pictureReuses,"transientPathCommandReuses",transientPictureReuses,"pathCommandMisses",pictureMisses,"pathCommandEvictions",pictureEvictions,"pathCommandBytesEstimate",pictureBytes,"pathCommandPointsEstimate",picturePoints,"maxPathCommandBytesEstimate",MAX_PICTURE_BYTES,"maxPathCommandPointsEstimate",MAX_PICTURE_POINTS,"pictureReuses",pictureReuses,"transientPictureReuses",transientPictureReuses,"pictureMisses",pictureMisses,"pictureEvictions",pictureEvictions,"pictureBytes",pictureBytes,"picturePoints",picturePoints,"maxPictureBytes",MAX_PICTURE_BYTES,"maxPicturePoints",MAX_PICTURE_POINTS,"culledItems",culledItems,"allocatedTiles",allocatedTiles,"reusedTiles",reusedTiles,"evictedTiles",evictedTiles,"currentBytes",currentBytes,"peakBytes",peakBytes,"maxBytes",MAX_BYTES,"densityTier",0,"lastRenderedItemIds",rendered,"renderedItemIds",rendered,"lastGeometryItemIds",geometry);}

    private static synchronized RectF bounds(Map<String,Object> item){if("object".equals(item.get("kind")))return computeBounds(item);BoundRecord cached=BOUNDS.get(item);if(cached!=null)return new RectF(cached.bounds);RectF result=computeBounds(item);int pointCount=item.get("points") instanceof List?((List<?>)item.get("points")).size():0;if(pointCount<=10000){while((cachedBoundPoints+pointCount>100000||BOUNDS.size()>=4096)&&!BOUNDS.isEmpty()){Iterator<Map.Entry<Map<String,Object>,BoundRecord>> it=BOUNDS.entrySet().iterator();Map.Entry<Map<String,Object>,BoundRecord> e=it.next();cachedBoundPoints-=e.getValue().pointCount;it.remove();}BOUNDS.put(item,new BoundRecord(result,pointCount));cachedBoundPoints+=pointCount;}return new RectF(result);}
    private static RectF computeBounds(Map<String,Object> item){
        if("object".equals(item.get("kind"))){double[] b=new DrawItem(new Model(),item).paintBounds(com.notemaker.cli.ItemCoverage::create);return new RectF((float)b[0]-2,(float)b[1]-2,(float)b[2]+2,(float)b[3]+2);}
        float width=((Number)item.get("width")).floatValue();String brush=String.valueOf(item.getOrDefault("brush","pen"));float pad=("pen".equals(brush)?Math.max(.3f,width/2):Math.max(width*2,width/2))+2;
        float l=Float.POSITIVE_INFINITY,t=l,r=Float.NEGATIVE_INFINITY,b=r;
        for(Object raw:list(item.get("points"))){List<Object> p=list(raw);float x=((Number)p.get(0)).floatValue(),y=((Number)p.get(1)).floatValue();l=Math.min(l,x);t=Math.min(t,y);r=Math.max(r,x);b=Math.max(b,y);}
        if(l==Float.POSITIVE_INFINITY)return new RectF();return new RectF(l-pad,t-pad,r+pad,b+pad);
    }
}
