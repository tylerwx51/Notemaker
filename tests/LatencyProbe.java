import com.notemaker.core.*;
import com.notemaker.cli.*;
import java.io.*;
import java.util.*;
import static com.notemaker.core.Json.*;

/** Run only against a disposable replica copy; emits phase timings as JSON lines. */
public final class LatencyProbe {
    interface Work { Object run() throws Exception; }
    static void memory(String phase) throws Exception {
        System.gc();Thread.sleep(100);System.gc();
        Runtime runtime=Runtime.getRuntime();Map<String,Object> out=object("phase",phase,"heap_bytes",runtime.totalMemory()-runtime.freeMemory());
        try {out.put("native_bytes",Class.forName("android.os.Debug").getMethod("getNativeHeapAllocatedSize").invoke(null));}catch(ClassNotFoundException ignored){}
        try(BufferedReader reader=new BufferedReader(new FileReader("/proc/self/status"))){String line;while((line=reader.readLine())!=null)if(line.startsWith("VmRSS:"))out.put("rss_kib",Long.parseLong(line.trim().split("\\s+")[1]));}
        System.out.println(write(out));
    }
    static Object timed(String phase, Work work) throws Exception {
        long start=System.nanoTime();Object result=work.run();
        System.out.println(write(object("phase",phase,"ms",(System.nanoTime()-start)/1e6)));
        System.out.flush();return result;
    }
    public static void main(String[] args) throws Exception {
        File dir=new File(args[0]);
        if(!new File(dir,"LATENCY-SCRATCH").isFile())throw new IllegalArgumentException("Requires marked disposable replica copy");
        String doc=args[1];Store store=Store.writer(dir,doc);
        Map<String,Object> state=map(timed("cold_read",()->store.read()));
        Model model=Store.model(state);
        String layer=(String)state.getOrDefault("selected","base");int page=((Number)state.getOrDefault("page",0)).intValue();
        LayerEditor editor=new LayerEditor(model,layer,page);
        int points=0;for(DrawItem item:editor.items())if(item.kind.equals("stroke"))points+=list(item.content.get("points")).size();
        System.out.println(write(object("items",editor.items().size(),"points",points,"layer",layer,"page",page)));
        memory("memory_before_coverage");
        for(int i=0;i<3;i++)timed("warm_read",()->store.read());
        // Use a real ink point, retaining the note's item sizes and brush geometry.
        double x=200,y=200;
        for(DrawItem item:editor.items())if(item.kind.equals("stroke")){List<Object> p=list(list(item.content.get("points")).get(0));x=((Number)p.get(0)).doubleValue();y=((Number)p.get(1)).doubleValue();break;}
        final double[][] region=Geometry.capsule(new double[]{x,y},new double[]{x,y},12);
        final Map<String,Object> input=object("mode","partial","layer",layer,"page",page,"radius",12,"points",Arrays.asList(Arrays.asList(x,y)));
        List<Object> swipe=new ArrayList<>();for(int i=0;i<120;i++)swipe.add(Arrays.asList(x+600*i/119.0,y+400*i/119.0));
        final Map<String,Object> swipeInput=object("mode","partial","layer",layer,"page",page,"radius",12,"points",swipe);
        List<Map<String,Object>> costs=new ArrayList<>();
        long[] factoryCost={0,0};Coverage.Factory measured=o->{long start=System.nanoTime();Coverage result=ItemCoverage.create(o);factoryCost[0]+=System.nanoTime()-start;factoryCost[1]++;return result;};
        timed("all_item_hit_tests",()->{for(DrawItem item:editor.items()){long start=System.nanoTime();boolean hit=item.intersects(region,measured);costs.add(object("id",item.id,"kind",item.kind,"points",item.kind.equals("stroke")?list(item.content.get("points")).size():0,"ms",(System.nanoTime()-start)/1e6,"hit",hit));}return null;});
        System.out.println(write(object("coverage_factory_ms",factoryCost[0]/1e6,"coverage_factory_calls",factoryCost[1])));
        memory("memory_after_coverage");
        costs.sort((a,b)->Double.compare(number(b,"ms"),number(a,"ms")));
        System.out.println(write(object("slowest",costs.subList(0,Math.min(10,costs.size())))));
        for(int i=0;i<3;i++)timed("erase_geometry",()->DrawingActions.erase(model,input,ItemCoverage::create));
        // Diagnostic only: conservative ink-point bounds before exact coverage.
        // Production still uses the original query; this isolates avoidable work.
        java.lang.reflect.Field cache=ItemCoverage.class.getDeclaredField("CACHE");cache.setAccessible(true);((Map<?,?>)cache.get(null)).clear();
        timed("experimental_ink_prefilter_cold",()->{
            int candidates=0,hits=0;double[] q=Geometry.bounds(region);
            for(DrawItem item:new LayerEditor(model,layer,page).items()){
                if(item.kind.equals("stroke")){
                    double l=Double.POSITIVE_INFINITY,t=l,r=Double.NEGATIVE_INFINITY,b=r;
                    for(Object raw:list(item.content.get("points"))){List<Object> p=list(raw);double px=((Number)p.get(0)).doubleValue(),py=((Number)p.get(1)).doubleValue();l=Math.min(l,px);t=Math.min(t,py);r=Math.max(r,px);b=Math.max(b,py);}
                    double pad=number(item.content,"width")*2;
                    if(r+pad<q[0]||l-pad>q[2]||b+pad<q[1]||t-pad>q[3])continue;
                }
                candidates++;if(item.intersects(region,ItemCoverage::create))hits++;
            }
            int expected=0;for(Map<String,Object> cost:costs)if(Boolean.TRUE.equals(cost.get("hit")))expected++;
            if(hits!=expected)throw new IllegalStateException("Prefilter changed hit count");
            System.out.println(write(object("prefilter_candidates",candidates,"prefilter_hits",hits)));return null;
        });
        timed("render_png",()->{Png.export(model,page,0,0,1000,1400,new File(dir,"render.png"));return null;});
        try(LocalProxy proxy=new LocalProxy(dir,0)){
            for(int i=0;i<3;i++){
                timed("proxy_erase",()->proxy.execute(object("document",doc,"actor","latency-probe","args",Arrays.asList("erase","-"),"input",input)));
                Map<String,Object> snap=map(timed("proxy_snapshot",()->proxy.execute(object("document",doc,"actor","latency-probe","args",Arrays.asList("snapshot")))));
                timed("snapshot_to_model",()->Store.model(snap));
                timed("proxy_undo",()->proxy.execute(object("document",doc,"actor","latency-probe","args",Arrays.asList("undo")))) ;
            }
            timed("swipe_geometry",()->DrawingActions.erase(model,swipeInput,ItemCoverage::create));
            timed("proxy_swipe",()->proxy.execute(object("document",doc,"actor","latency-probe","args",Arrays.asList("erase","-"),"input",swipeInput)));
            timed("post_swipe_snapshot",()->proxy.execute(object("document",doc,"actor","latency-probe","args",Arrays.asList("snapshot"))));
            try{timed("post_swipe_tap_geometry",()->DrawingActions.erase(store.model(),input,ItemCoverage::create));}catch(IllegalArgumentException expected){System.out.println(write(object("post_swipe_tap",expected.getMessage())));}
            proxy.execute(object("document",doc,"actor","latency-probe","args",Arrays.asList("undo")));
            java.util.concurrent.ExecutorService writer=java.util.concurrent.Executors.newSingleThreadExecutor();
            List<java.util.concurrent.Future<?>> jobs=new ArrayList<>();
            for(int i=0;i<5;i++){
                final int index=i;final long submitted=System.nanoTime();
                jobs.add(writer.submit(()->{long start=System.nanoTime();String error=null;try{
                    proxy.execute(object("document",doc,"actor","latency-probe","args",Arrays.asList("erase","-"),"input",input));
                    proxy.execute(object("document",doc,"actor","latency-probe","args",Arrays.asList("snapshot")));
                }catch(Exception e){error=e.getMessage();}
                System.out.println(write(object("phase","queued_erase","index",index,"queue_ms",(start-submitted)/1e6,"work_ms",(System.nanoTime()-start)/1e6,"total_ms",(System.nanoTime()-submitted)/1e6,"error",error)));}));
            }
            for(java.util.concurrent.Future<?> job:jobs)job.get();writer.shutdown();
        }
        System.out.flush();System.exit(0);
    }
}
