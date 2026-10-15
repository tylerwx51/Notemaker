import com.notemaker.core.*;
import com.notemaker.cli.*;
import java.util.*;
import static com.notemaker.core.Json.*;

/** Retained cache memory, including native paths; no source document is mutated. */
public final class CacheFootprintProbe {
    static Map<String,Object> stroke(int i,int count){
        List<Object> points=new ArrayList<>();
        for(int p=0;p<count;p++)points.add(Arrays.asList(p*2.0,Math.sin(p*.7)*10,0.5+(p%5)*.1));
        return object("kind","stroke","stroke","memory-"+i,"layer","base","page",0,"width",2+i*.0001,"color",-16777216,"points",points);
    }
    public static void main(String[] args)throws Exception{
        java.lang.reflect.Field cache=ItemCoverage.class.getDeclaredField("CACHE");cache.setAccessible(true);
        ItemCoverage.create(stroke(-1,20));((Map<?,?>)cache.get(null)).clear();
        LatencyProbe.memory("cache_empty");
        long start=System.nanoTime();int from=0;
        for(int count:new int[]{100,1000,10000}){
            for(int i=from;i<count;i++)ItemCoverage.create(stroke(i,20));
            System.out.println(write(object("items",count,"points_per_item",20,"cache_entries",((Map<?,?>)cache.get(null)).size(),"build_ms",(System.nanoTime()-start)/1e6)));
            LatencyProbe.memory("cache_"+count);from=count;
        }
        System.out.flush();System.exit(0);
    }
}
