package com.notemaker;

import com.notemaker.core.*;
import java.util.*;
import java.util.concurrent.*;
import static com.notemaker.core.Json.*;

/** Real input-to-updated-draw measurements, restricted to the isolated test package. */
final class ErasePerformance {
    static Map<String,Object> run(Integration test,MainActivity app)throws Exception{
        Model.require(app.getPackageName().equals("com.notemaker.latency"),"Erase benchmark requires isolated latency package");
        CoverageCheck.cachePixels();
        ReplicaService.writes.submit(()->{}).get(30,TimeUnit.SECONDS);test.waitForIdleSync();
        Model original=Store.model(app.replica.read());List<Object> samples=new ArrayList<>();
        test.runOnMainSync(()->{app.setDrawingTool(null);app.erase=true;app.eraserMode="partial";app.eraserRadius=12;});
        for(DrawItem item:new LayerEditor(original,app.selected,app.page).items()){
            if(!item.kind.equals("stroke"))continue;
            List<Object> pts=list(item.content.get("points")),point=list(pts.get(pts.size()/2));
            float x=((Number)point.get(0)).floatValue(),y=((Number)point.get(1)).floatValue();
            if(!item.intersects(Geometry.capsule(new double[]{x,y},new double[]{x,y},12),com.notemaker.cli.ItemCoverage::create))continue;
            CountDownLatch drawn=new CountDownLatch(1);long[] start={0},elapsed={0},feedback={0};Object oldMasks=item.content.get("erasures");
            Map<String,Long> trace=new ConcurrentHashMap<>();
            test.runOnMainSync(()->{
                app.eraseTimings=trace;
                app.canvas.afterDraw=()->{if(feedback[0]==0&&!app.canvas.erasePreviews.isEmpty())feedback[0]=System.nanoTime()-start[0];Map<String,Object> current=app.model.strokes.get(item.id);if(current!=null&&!Objects.equals(oldMasks,current.get("erasures"))){elapsed[0]=System.nanoTime()-start[0];if(feedback[0]==0)feedback[0]=elapsed[0];app.canvas.afterDraw=null;drawn.countDown();}};
                start[0]=System.nanoTime();Integration.gesture(app,x,y,0,0,false);
            });
            Model.require(drawn.await(15,TimeUnit.SECONDS),"Updated content was not drawn");
            Map<String,Object> phases=object();for(Map.Entry<String,Long> e:trace.entrySet())phases.put(e.getKey(),(e.getValue()-start[0])/1e6);
            samples.add(object("points",pts.size(),"input_to_feedback_ms",feedback[0]/1e6,"input_to_updated_draw_ms",elapsed[0]/1e6,"phases_ms",phases));
            test.runOnMainSync(()->app.eraseTimings=null);
            ReplicaService.writes.submit(()->{}).get(30,TimeUnit.SECONDS);test.waitForIdleSync();
            app.replica.command(Arrays.asList("undo"),null);Map<String,Object> restored=app.replica.read();
            test.runOnMainSync(()->{try{app.refreshShared(restored);app.update();}catch(Exception e){throw new RuntimeException(e);}});
            test.waitForIdleSync();Thread.sleep(100);
        }
        Map<String,Object> report=object("samples",samples);
        // Undo advances version metadata, so compare visible entity geometry separately.
        Model after=Store.model(app.replica.read());report.put("geometry_restored",original.strokes.equals(after.strokes)&&original.active.equals(after.active));
        try(java.io.FileOutputStream out=app.openFileOutput("erase-performance.json",0)){out.write(write(report).getBytes("UTF-8"));}
        return report;
    }
}
