package com.notemaker;

import android.graphics.*;
import org.json.JSONObject;
import java.util.*;
import java.util.function.*;

/** Rasterize only newly appended settled strokes; invalidate replacements and reordering. */
final class InkCache {
    static long rebuilds;
    final Bitmap bitmap;
    private LinkedHashMap<String,JSONObject> drawn=new LinkedHashMap<>();
    InkCache(int width,int height){bitmap=Bitmap.createBitmap(width,height,Bitmap.Config.ARGB_8888);}
    void draw(Canvas destination,LinkedHashMap<String,JSONObject> visible,Consumer<Canvas> transform,BiConsumer<Canvas,JSONObject> stroke){
        boolean reset=false;
        for(Map.Entry<String,JSONObject> old:drawn.entrySet()){
            JSONObject value=visible.get(old.getKey());
            if(value==null||!sameInk(old.getValue(),value)){reset=true;break;}
        }
        if(!reset){Iterator<String> previous=drawn.keySet().iterator();for(String key:visible.keySet()){if(!previous.hasNext())break;if(!key.equals(previous.next())){reset=true;break;}}}
        if(reset){rebuilds++;bitmap.eraseColor(Color.TRANSPARENT);drawn.clear();}
        if(drawn.size()!=visible.size()){
            Canvas image=new Canvas(bitmap);transform.accept(image);
            for(Map.Entry<String,JSONObject> entry:visible.entrySet())if(!drawn.containsKey(entry.getKey()))stroke.accept(image,entry.getValue());
        }
        drawn=visible;destination.drawBitmap(bitmap,0,0,null);
    }
    private static boolean sameInk(JSONObject a,JSONObject b){
        // Persisted IDs/actors and other log metadata do not change pixels.
        return a==b||(a.optInt("color")==b.optInt("color")&&a.optDouble("width")==b.optDouble("width")
            &&a.optInt("page")==b.optInt("page")&&a.optString("layer").equals(b.optString("layer"))
            &&a.optJSONArray("points").toString().equals(b.optJSONArray("points").toString()));
    }
}
