package com.notemaker;

import android.graphics.*;
import com.notemaker.core.*;
import java.util.*;
import java.util.function.*;
import static com.notemaker.core.Json.*;

/** Settled layer pixels; edits repaint only the damaged region in original stroke order. */
final class InkCache {
    static long rebuilds;
    final Bitmap bitmap;
    private LinkedHashMap<String,Map<String,Object>> drawn=new LinkedHashMap<>();
    private IdentityHashMap<Map<String,Object>,RectF> paintBounds=new IdentityHashMap<>();
    InkCache(int width,int height){bitmap=Bitmap.createBitmap(width,height,Bitmap.Config.ARGB_8888);}
    void draw(Canvas destination,LinkedHashMap<String,Map<String,Object>> visible,Consumer<Canvas> transform,BiConsumer<Canvas,Map<String,Object>> stroke){
        boolean changed=false,reordered=false;RectF dirty=new RectF();
        List<String> oldOrder=new ArrayList<>(),newOrder=new ArrayList<>();
        for(String id:drawn.keySet())if(visible.containsKey(id))oldOrder.add(id);
        boolean added=false;for(String id:visible.keySet())if(drawn.containsKey(id)){newOrder.add(id);if(added)changed=true;}else added=true;
        reordered|=!oldOrder.equals(newOrder);
        for(Map.Entry<String,Map<String,Object>> old:drawn.entrySet()){
            Map<String,Object> next=visible.get(old.getKey());
            if(next==null||!sameInk(old.getValue(),next)){
                // A new native clipping path can change antialias rounding along
                // the entire edited stroke, so repaint its full paint bounds.
                changed=true;dirty.union(bounds(old.getValue()));if(next!=null)dirty.union(bounds(next));
            }
        }
        IdentityHashMap<Map<String,Object>,RectF> nextBounds=new IdentityHashMap<>();
        for(Map<String,Object> value:visible.values())nextBounds.put(value,bounds(value));
        Canvas image=new Canvas(bitmap);transform.accept(image);
        if(reordered){rebuilds++;bitmap.eraseColor(Color.TRANSPARENT);for(Map<String,Object> value:visible.values())stroke.accept(image,value);}
        else if(changed){
            for(Map.Entry<String,Map<String,Object>> entry:visible.entrySet())if(!drawn.containsKey(entry.getKey()))dirty.union(bounds(entry.getValue()));
            // Native clipping can change edge coverage: repaint intersecting items
            // completely. Geometry bounds are cached, and reverse traversal follows
            // the usual newest-to-oldest chain without repeatedly scanning points.
            List<RectF> remaining=new ArrayList<>();for(Map<String,Object> value:visible.values())remaining.add(nextBounds.get(value));
            boolean grew;do{grew=false;for(int i=remaining.size()-1;i>=0;i--){RectF b=remaining.get(i);if(RectF.intersects(dirty,b)){if(!dirty.contains(b)){dirty.union(b);grew=true;}remaining.remove(i);}}}while(grew);
            Matrix matrix=image.getMatrix(),inverse=new Matrix();RectF pixels=new RectF(dirty);matrix.mapRect(pixels);
            pixels.set((float)Math.floor(pixels.left)-1,(float)Math.floor(pixels.top)-1,(float)Math.ceil(pixels.right)+1,(float)Math.ceil(pixels.bottom)+1);
            if(matrix.invert(inverse))inverse.mapRect(dirty,pixels);
            image.save();image.setMatrix(new Matrix());image.clipRect(pixels);image.drawColor(Color.TRANSPARENT,PorterDuff.Mode.CLEAR);image.setMatrix(matrix);
            for(Map<String,Object> value:visible.values())if(RectF.intersects(dirty,bounds(value)))stroke.accept(image,value);
            image.restore();
        }else for(Map.Entry<String,Map<String,Object>> entry:visible.entrySet())if(!drawn.containsKey(entry.getKey()))stroke.accept(image,entry.getValue());
        drawn=visible;paintBounds=nextBounds;destination.drawBitmap(bitmap,0,0,null);
    }
    private static boolean sameBase(Map<String,Object> a,Map<String,Object> b){
        if("object".equals(a.get("kind"))||"object".equals(b.get("kind")))return a.equals(b);
        for(String key:Arrays.asList("color","width","page","layer","brush","material","points"))if(!Objects.equals(a.get(key),b.get(key)))return false;return true;
    }
    private static boolean sameInk(Map<String,Object> a,Map<String,Object> b){return a==b||(sameBase(a,b)&&Objects.equals(a.get("erasures"),b.get("erasures")));}
    private RectF bounds(Map<String,Object> stroke){
        RectF cached=paintBounds.get(stroke);if(cached!=null)return cached;
        RectF result=computeBounds(stroke);paintBounds.put(stroke,result);return result;
    }
    private static RectF computeBounds(Map<String,Object> stroke){
        if("object".equals(stroke.get("kind"))){double[] b=new DrawItem(new Model(),stroke).paintBounds(com.notemaker.cli.ItemCoverage::create);return new RectF((float)b[0]-1,(float)b[1]-1,(float)b[2]+1,(float)b[3]+1);}
        float width=((Number)stroke.get("width")).floatValue();
        // Pen ribbons and round caps stay within half the nominal width. Using
        // the broad textured-brush margin falsely links nearby rows of handwriting.
        float pad="pen".equals(stroke.getOrDefault("brush","pen"))?Math.max(.3f,width/2)+1:width*2+1;
        float l=Float.POSITIVE_INFINITY,t=l,r=Float.NEGATIVE_INFINITY,b=r;
        for(Object raw:list(stroke.get("points"))){List<Object> p=list(raw);float x=((Number)p.get(0)).floatValue(),y=((Number)p.get(1)).floatValue();l=Math.min(l,x);t=Math.min(t,y);r=Math.max(r,x);b=Math.max(b,y);}
        return new RectF(l-pad,t-pad,r+pad,b+pad);
    }
}
