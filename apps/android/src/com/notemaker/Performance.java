package com.notemaker;

import android.app.Instrumentation;
import android.os.*;
import android.view.*;
import java.util.*;
import java.util.concurrent.*;
import com.notemaker.core.*;
import static com.notemaker.core.Json.*;

/** Real view input and window frame timing; run only in an isolated test emulator. */
final class Performance {
    static int bucket(int stroke){return stroke<100?0:stroke>=400&&stroke<500?1:stroke>=900?2:-1;}
    static double percentile(List<Double> values,double fraction){List<Double> sorted=new ArrayList<>(values);Collections.sort(sorted);return sorted.isEmpty()?0:sorted.get(Math.min(sorted.size()-1,(int)(sorted.size()*fraction)));}
    static Map<String,Object> stats(List<Double> values){return object("samples",values.size(),"p50_ms",percentile(values,.50),"p95_ms",percentile(values,.95),"p99_ms",percentile(values,.99),"max_ms",percentile(values,1));}
    static Map<String,Object> run(Instrumentation test,MainActivity app) throws Exception {
        @SuppressWarnings("unchecked") List<Double>[][] samples=new List[3][4];
        for(int b=0;b<3;b++)for(int m=0;m<4;m++)samples[b][m]=Collections.synchronizedList(new ArrayList<>());
        java.util.concurrent.atomic.AtomicInteger current=new java.util.concurrent.atomic.AtomicInteger(-1);
        HandlerThread frames=new HandlerThread("performance-frames");frames.start();
        Window.OnFrameMetricsAvailableListener listener=(window,metrics,dropped)->{int b=bucket(current.get());if(current.get()>=10&&b>=0)samples[b][3].add(metrics.getMetric(FrameMetrics.TOTAL_DURATION)/1e6);};
        test.runOnMainSync(()->{
            app.document="performance-"+System.currentTimeMillis();app.host="http://127.0.0.1:1";app.load("fixed");app.configureProxy();app.update();
            app.getSharedPreferences("settings",0).edit().putString("document",app.document).putString("host",app.host).commit();
            app.canvas.drawLatency=n->{int b=bucket(current.get());if(current.get()>=10&&b>=0)samples[b][2].add(n/1e6);};
            app.getWindow().addOnFrameMetricsAvailableListener(listener,new Handler(frames.getLooper()));
        });
        Thread.sleep(1000);int maxSaving=0;long start=System.nanoTime(),initialRebuilds=InkCache.rebuilds;
        for(int stroke=0;stroke<1000;stroke++){
            current.set(stroke);final int number=stroke,b=bucket(stroke);long down=SystemClock.uptimeMillis();
            for(int step=0;step<8;step++){
                final int index=step;long queued=System.nanoTime();
                test.runOnMainSync(()->{
                    MotionEvent.PointerProperties pp=new MotionEvent.PointerProperties();pp.id=0;pp.toolType=MotionEvent.TOOL_TYPE_STYLUS;
                    MotionEvent.PointerCoords pc=new MotionEvent.PointerCoords();float scale=app.canvas.scale();
                    pc.x=(30+(number%20)*45+index*3)*scale;pc.y=(40+(number/20)*24+index)*scale;pc.pressure=.5f;pc.size=1;
                    int action=index==0?MotionEvent.ACTION_DOWN:index==7?MotionEvent.ACTION_UP:MotionEvent.ACTION_MOVE;
                    MotionEvent e=MotionEvent.obtain(down,SystemClock.uptimeMillis(),action,1,new MotionEvent.PointerProperties[]{pp},new MotionEvent.PointerCoords[]{pc},0,0,1,1,0,0,InputDevice.SOURCE_STYLUS,0);
                    if(action==MotionEvent.ACTION_MOVE)for(int history=1;history<=7;history++){pc.x+=.25f*scale;pc.y+=.1f*scale;e.addBatch(SystemClock.uptimeMillis(),new MotionEvent.PointerCoords[]{pc},0);}
                    long before=System.nanoTime();app.canvas.dispatchTouchEvent(e);long after=System.nanoTime();e.recycle();
                    if(number>=10&&b>=0){samples[b][0].add((after-before)/1e6);samples[b][1].add((after-queued)/1e6);}
                });
                Thread.sleep(16);
            }
            final int[] pending={0};test.runOnMainSync(()->pending[0]=app.saving);maxSaving=Math.max(maxSaving,pending[0]);
            if(stroke%100==99)android.util.Log.i("NoteMakerPerformance","strokes="+(stroke+1)+" saveQueue="+pending[0]);
        }
        long drainStart=System.nanoTime();ReplicaService.writes.submit(()->{}).get(30,TimeUnit.SECONDS);test.waitForIdleSync();Thread.sleep(500);
        Map<String,Object> state=app.store.read();Model effective=Store.model(state);Model replay=new Model(state);
        Model.require(write(effective.json()).equals(write(replay.json())),"Incremental model differs from full replay");
        Model.require(effective.strokes.size()==1000,"Lost strokes: "+effective.strokes.size());
        test.runOnMainSync(()->{app.canvas.drawLatency=null;app.getWindow().removeOnFrameMetricsAvailableListener(listener);});frames.quitSafely();
        List<Object> windows=new ArrayList<>();String[] names={"1-100 (10 warmup)","401-500","901-1000"};
        for(int b=0;b<3;b++)windows.add(object("strokes",names[b],"handler",stats(samples[b][0]),"dispatch",stats(samples[b][1]),"input_to_draw",stats(samples[b][2]),"frame",stats(samples[b][3])));
        return object("document",app.document,"strokes",1000,"points_per_stroke",50,"elapsed_seconds",(System.nanoTime()-start)/1e9,"drain_ms",(System.nanoTime()-drainStart)/1e6,"max_save_queue",maxSaving,"ink_cache_rebuilds",InkCache.rebuilds-initialRebuilds,"incremental_matches_replay",true,"windows",windows);
    }
}
