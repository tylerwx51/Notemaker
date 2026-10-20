package com.notemaker;

import android.app.Instrumentation;
import android.os.*;
import android.view.*;
import java.util.*;
import java.util.concurrent.*;
import com.notemaker.core.*;
import static com.notemaker.core.Json.*;

/** Real view input and window frame timing; run only in an isolated test package or emulator. */
final class Performance {
    static void position(MotionEvent.PointerCoords pc,int stroke,int point,float scale,float visibleWidth,float visibleHeight){
        int columns=Math.max(1,Math.min(20,(int)((visibleWidth-100)/45)+1));
        int rows=Math.max(1,Math.min(50,(int)((visibleHeight-100)/24)+1));
        int cell=stroke%(columns*rows);
        float x=30+(cell%columns)*45,y=40+(cell/columns)*24;
        if(stroke%100==99){double angle=point*2*Math.PI/499;pc.x=(x+20*(float)Math.sin(2*angle))*scale;pc.y=(y+20*(float)Math.sin(angle))*scale;}
        else {pc.x=(x+point*.45f)*scale;pc.y=(y+point*.15f)*scale;}
    }
    static int bucket(int stroke){return stroke<100?0:stroke>=400&&stroke<500?1:stroke>=900?2:-1;}
    static double percentile(List<Double> values,double fraction){List<Double> sorted=new ArrayList<>(values);Collections.sort(sorted);return sorted.isEmpty()?0:sorted.get(Math.min(sorted.size()-1,(int)(sorted.size()*fraction)));}
    static Map<String,Object> stats(List<Double> values){return object("over_30_ms",values.stream().filter(v->v>=30).count(),"samples",values.size(),"p50_ms",percentile(values,.50),"p95_ms",percentile(values,.95),"p99_ms",percentile(values,.99),"max_ms",percentile(values,1));}
    static Map<String,Object> run(Instrumentation test,MainActivity app) throws Exception {
        @SuppressWarnings("unchecked") List<Double>[][] samples=new List[3][4];
        for(int b=0;b<3;b++)for(int m=0;m<4;m++)samples[b][m]=Collections.synchronizedList(new ArrayList<>());
        List<Double> strokeLatency=Collections.synchronizedList(new ArrayList<>());
        List<Double> figureEightLatency=new ArrayList<>();
        @SuppressWarnings("unchecked") List<Double>[] phaseLatency=new List[3];
        for(int i=0;i<3;i++)phaseLatency[i]=Collections.synchronizedList(new ArrayList<>());
        // Main-thread pending events: a draw acknowledges every coalesced event, so
        // missing/slow MOVE and UP events cannot disappear from the latency budget.
        List<long[]> pendingDraw=new ArrayList<>();
        List<Object> slowEvents=new ArrayList<>();
        List<Object> dispatchStalls=new ArrayList<>();
        java.util.concurrent.atomic.AtomicInteger current=new java.util.concurrent.atomic.AtomicInteger(-1);
        // Android input messages bypass traversal synchronization barriers. A regular
        // runOnMainSync message does not, adding an artificial extra vsync of delay.
        Handler input=Handler.createAsync(Looper.getMainLooper());
        HandlerThread frames=new HandlerThread("performance-frames");frames.start();
        Window.OnFrameMetricsAvailableListener listener=(window,metrics,dropped)->{int b=bucket(current.get());if(current.get()>=10&&b>=0)samples[b][3].add(metrics.getMetric(FrameMetrics.TOTAL_DURATION)/1e6);};
        test.runOnMainSync(()->{
            app.document="performance-"+System.currentTimeMillis();app.host="http://127.0.0.1:1";app.load("fixed");app.configureProxy();app.update();
            app.getSharedPreferences("settings",0).edit().putString("document",app.document).putString("host",app.host).commit();
            app.canvas.afterDraw=()->{
                long drawn=System.nanoTime();
                for(long[] event:pendingDraw){
                    double ms=(drawn-event[0])/1e6;strokeLatency.add(ms);phaseLatency[(int)event[1]].add(ms);if(event[2]%100==99)figureEightLatency.add(ms);
                    if(ms>=30&&slowEvents.size()<20)slowEvents.add(object("recorded_wall_ms",System.currentTimeMillis(),"post_call_ms",event[6]==0?null:(event[6]-event[0])/1e6,"stroke",event[2]+1,"step",event[3],"total_ms",ms,"queue_ms",(event[4]-event[0])/1e6,"handler_ms",(event[5]-event[4])/1e6,"draw_wait_ms",(drawn-event[5])/1e6));
                }
                pendingDraw.clear();
            };
            app.canvas.drawLatency=n->{int b=bucket(current.get());if(current.get()>=10&&b>=0)samples[b][2].add(n/1e6);};
            app.getWindow().addOnFrameMetricsAvailableListener(listener,new Handler(frames.getLooper()));
        });
        Thread.sleep(1000);int maxSaving=0;long start=System.nanoTime();
        Map<String,Object> initialTileMetrics=tileMetrics(test,app);
        List<Object> resources=new ArrayList<>();resources.add(resources(0));
        for(int stroke=0;stroke<1000;stroke++){
            current.set(stroke);final int number=stroke,b=bucket(stroke);long down=SystemClock.uptimeMillis();
            for(int step=0;step<8;step++){
                final int index=step;CountDownLatch handled=new CountDownLatch(1);long queued=System.nanoTime();
                long[] eventTiming={queued,index==0?0:index==7?2:1,number,index,0,0,0};
                input.post(()->{
                    MotionEvent.PointerProperties pp=new MotionEvent.PointerProperties();pp.id=0;pp.toolType=MotionEvent.TOOL_TYPE_STYLUS;
                    MotionEvent.PointerCoords pc=new MotionEvent.PointerCoords();float scale=app.canvas.scale();
                    int batch=number%100==99?83:8;
                    int point=index==0?0:index==7?6*batch+1:1+(index-1)*batch;
                    float visibleWidth=app.canvas.getWidth()/scale,visibleHeight=app.canvas.getHeight()/scale;
                    position(pc,number,point,scale,visibleWidth,visibleHeight);pc.pressure=.5f;pc.size=1;
                    int action=index==0?MotionEvent.ACTION_DOWN:index==7?MotionEvent.ACTION_UP:MotionEvent.ACTION_MOVE;
                    MotionEvent e=MotionEvent.obtain(down,SystemClock.uptimeMillis(),action,1,new MotionEvent.PointerProperties[]{pp},new MotionEvent.PointerCoords[]{pc},0,0,1,1,0,0,InputDevice.SOURCE_STYLUS,0);
                    if(action==MotionEvent.ACTION_MOVE)for(int history=1;history<batch;history++){position(pc,number,point+history,scale,visibleWidth,visibleHeight);e.addBatch(SystemClock.uptimeMillis(),new MotionEvent.PointerCoords[]{pc},0);}
                    long before=System.nanoTime();app.canvas.dispatchTouchEvent(e);long after=System.nanoTime();e.recycle();
                    eventTiming[4]=before;eventTiming[5]=after;if(number>=10)pendingDraw.add(eventTiming);
                    if(number>=10&&b>=0){samples[b][0].add((after-before)/1e6);samples[b][1].add((after-queued)/1e6);}
                    handled.countDown();
                });
                eventTiming[6]=System.nanoTime();
                if(!handled.await(30,TimeUnit.MILLISECONDS)){
                    Map<String,Object> stacks=new LinkedHashMap<>();
                    for(Map.Entry<Thread,StackTraceElement[]> entry:Thread.getAllStackTraces().entrySet())if(entry.getKey()==Looper.getMainLooper().getThread()||entry.getKey().getName().equals("replica-writer"))stacks.put(entry.getKey().getName(),Arrays.toString(entry.getValue()));
                    dispatchStalls.add(object("stroke",number+1,"step",index,"wall_ms",System.currentTimeMillis(),"wait_ms",(System.nanoTime()-queued)/1e6,"stacks",stacks));
                    Model.require(handled.await(5,TimeUnit.SECONDS),"Input dispatch timed out");
                }
                Thread.sleep(16);
            }
            final int[] pending={0};test.runOnMainSync(()->pending[0]=app.saving);maxSaving=Math.max(maxSaving,pending[0]);
            if(stroke%100==99){resources.add(resources(stroke+1));android.util.Log.i("NoteMakerPerformance","strokes="+(stroke+1)+" saveQueue="+pending[0]);}
        }
        long drainStart=System.nanoTime();ReplicaService.writes.submit(()->{}).get(30,TimeUnit.SECONDS);test.waitForIdleSync();Thread.sleep(500);
        Map<String,Object> state=app.store.read();Model effective=Store.model(state);Model replay=new Model(state);
        Model.require(write(effective.json()).equals(write(replay.json())),"Incremental model differs from full replay");
        Model.require(effective.strokes.size()==1000,"Lost strokes: "+effective.strokes.size());
        @SuppressWarnings("unchecked") Map<String,Object>[] finalTileMetrics=new Map[]{null};
        test.runOnMainSync(()->{Model.require(pendingDraw.isEmpty(),"Input never reached a draw");finalTileMetrics[0]=app.canvas.tileCacheMetrics();app.canvas.afterDraw=null;app.canvas.drawLatency=null;app.getWindow().removeOnFrameMetricsAvailableListener(listener);});frames.quitSafely();
        Map<String,Object> tileCache=object("tile_renders",metricDelta(finalTileMetrics[0],initialTileMetrics,"tileRenders"),"item_renders",metricDelta(finalTileMetrics[0],initialTileMetrics,"itemRenders"),"geometry_renders",metricDelta(finalTileMetrics[0],initialTileMetrics,"geometryRenders"),"path_command_reuses",metricDelta(finalTileMetrics[0],initialTileMetrics,"pathCommandReuses"),"transient_path_command_reuses",metricDelta(finalTileMetrics[0],initialTileMetrics,"transientPathCommandReuses"),"path_command_misses",metricDelta(finalTileMetrics[0],initialTileMetrics,"pathCommandMisses"),"path_command_evictions",metricDelta(finalTileMetrics[0],initialTileMetrics,"pathCommandEvictions"),"reused_tiles",metricDelta(finalTileMetrics[0],initialTileMetrics,"reusedTiles"),"current_bytes",finalTileMetrics[0].get("currentBytes"),"peak_bytes",finalTileMetrics[0].get("peakBytes"),"max_bytes",finalTileMetrics[0].get("maxBytes"),"path_command_bytes_estimate",finalTileMetrics[0].get("pathCommandBytesEstimate"),"path_command_points_estimate",finalTileMetrics[0].get("pathCommandPointsEstimate"),"max_path_command_bytes_estimate",finalTileMetrics[0].get("maxPathCommandBytesEstimate"),"max_path_command_points_estimate",finalTileMetrics[0].get("maxPathCommandPointsEstimate"));
        try(java.io.FileOutputStream out=app.openFileOutput("performance-resources.json",0)){out.write(write(object("samples",resources,"sampling","Process counters sampled between every 100 strokes; cost is outside input latency but included in elapsed time. I/O counters are cumulative, not storage latency.")).getBytes("UTF-8"));}
        List<Object> windows=new ArrayList<>();String[] names={"1-100 (10 warmup)","401-500","901-1000"};
        for(int b=0;b<3;b++)windows.add(object("strokes",names[b],"handler",stats(samples[b][0]),"dispatch",stats(samples[b][1]),"input_to_draw",stats(samples[b][2]),"frame",stats(samples[b][3])));
        return object("dispatch_stalls",dispatchStalls,"slow_events",slowEvents,"latency_definition","UI enqueue to end of canvas draw; every event after 10 warmup strokes, including coalesced events", "stroke_latency",stats(strokeLatency),"figure_eight_latency",stats(figureEightLatency),"down_latency",stats(phaseLatency[0]),"move_latency",stats(phaseLatency[1]),"up_latency",stats(phaseLatency[2]),"document",app.document,"strokes",1000,"short_strokes",990,"short_stroke_points",50,"figure_eight_strokes",10,"figure_eight_points",500,"elapsed_seconds",(System.nanoTime()-start)/1e9,"drain_ms",(System.nanoTime()-drainStart)/1e6,"max_save_queue",maxSaving,"tile_cache",tileCache,"incremental_matches_replay",true,"windows",windows);
    }
    private static Map<String,Object> resources(int strokes) {
        long start=System.nanoTime();
        Debug.MemoryInfo memory=new Debug.MemoryInfo();Debug.getMemoryInfo(memory);
        Map<String,Object> io=new LinkedHashMap<>();
        try(java.io.BufferedReader reader=new java.io.BufferedReader(new java.io.FileReader("/proc/self/io"))){
            String line;while((line=reader.readLine())!=null){String[] parts=line.split(":");if(parts.length==2)io.put(parts[0],Long.parseLong(parts[1].trim()));}
        }catch(Exception error){io.put("unavailable",error.getClass().getSimpleName());}
        Runtime runtime=Runtime.getRuntime();
        return object("strokes",strokes,"pss_kib",memory.getTotalPss(),"private_dirty_kib",memory.getTotalPrivateDirty(),"java_used_bytes",runtime.totalMemory()-runtime.freeMemory(),"native_allocated_bytes",Debug.getNativeHeapAllocatedSize(),"process_cpu_ms",android.os.Process.getElapsedCpuTime(),"io",io,"sampling_ms",(System.nanoTime()-start)/1e6);
    }
    private static Map<String,Object> tileMetrics(Instrumentation test,MainActivity app){@SuppressWarnings("unchecked") Map<String,Object>[] result=new Map[]{null};test.runOnMainSync(()->result[0]=app.canvas.tileCacheMetrics());return result[0];}
    private static long metricDelta(Map<String,Object> after,Map<String,Object> before,String key){return ((Number)after.get(key)).longValue()-((Number)before.get(key)).longValue();}
}
