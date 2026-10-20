package com.notemaker;

import android.app.Instrumentation;
import android.os.*;
import android.view.*;
import java.util.*;
import java.util.concurrent.*;
import java.util.concurrent.atomic.*;
import static com.notemaker.core.Json.*;

/** Draw timing for common viewport and toolbar changes on a disposable note. */
final class InteractionPerformance {
    static Map<String,Object> run(Instrumentation test,MainActivity app) throws Exception {
        Handler input=Handler.createAsync(Looper.getMainLooper());
        HandlerThread frames=new HandlerThread("interaction-frames");frames.start();
        Map<String,List<Double>> draw=new LinkedHashMap<>(),handler=new LinkedHashMap<>(),frame=new LinkedHashMap<>();
        for(String name:Arrays.asList("pan","zoom_redraw","tool_switch")){
            draw.put(name,new ArrayList<>());handler.put(name,new ArrayList<>());
            frame.put(name,Collections.synchronizedList(new ArrayList<>()));
        }
        AtomicReference<String> current=new AtomicReference<>(null);
        Window.OnFrameMetricsAvailableListener listener=(window,metrics,dropped)->{
            String name=current.get();if(name!=null)frame.get(name).add(metrics.getMetric(FrameMetrics.TOTAL_DURATION)/1e6);
        };
        test.runOnMainSync(()->{app.pan=true;app.erase=false;app.canvas.zoom=1;app.canvas.offsetX=app.canvas.offsetY=0;
            app.getWindow().addOnFrameMetricsAvailableListener(listener,new Handler(frames.getLooper()));app.canvas.invalidate();});
        test.waitForIdleSync();
        Map<String,Object> start=Performance.resources(0);
        try {
            for(String name:draw.keySet()){
                current.set(name);
                int count=name.equals("pan")?60:40;
                for(int i=0;i<count;i++){
                    int index=i;CountDownLatch painted=new CountDownLatch(1),handled=new CountDownLatch(1);
                    AtomicLong drawn=new AtomicLong(),completed=new AtomicLong();
                    long queued=System.nanoTime();
                    input.post(()->{
                        app.canvas.afterDraw=()->{drawn.set(System.nanoTime());painted.countDown();};
                        if(name.equals("pan")){
                            app.pan=true;float x=app.canvas.getWidth()/2f,y=app.canvas.getHeight()/2f;
                            long when=SystemClock.uptimeMillis();
                            MotionEvent down=MotionEvent.obtain(when,when,MotionEvent.ACTION_DOWN,x,y,0);
                            MotionEvent move=MotionEvent.obtain(when,when+8,MotionEvent.ACTION_MOVE,x+(index%2==0?8:-8),y,0);
                            app.canvas.dispatchTouchEvent(down);app.canvas.dispatchTouchEvent(move);
                            down.recycle();move.recycle();
                        }else if(name.equals("zoom_redraw")){
                            app.canvas.zoom=index%2==0?1.2f:1f;app.canvas.invalidate();
                        }else{
                            app.setDrawingTool(index%2==0?"rectangle":null);
                        }
                        completed.set(System.nanoTime());handled.countDown();
                    });
                    if(!handled.await(5,TimeUnit.SECONDS)||!painted.await(5,TimeUnit.SECONDS))throw new IllegalStateException("Interaction draw timed out: "+name+" #"+index);
                    if(index>=5){handler.get(name).add((completed.get()-queued)/1e6);draw.get(name).add((drawn.get()-queued)/1e6);}
                    Thread.sleep(25);
                }
            }
            test.runOnMainSync(()->{app.canvas.afterDraw=null;app.pan=false;app.setDrawingTool(null);});
            test.waitForIdleSync();Thread.sleep(250);
        }finally{
            current.set(null);
            test.runOnMainSync(()->{app.canvas.afterDraw=null;app.getWindow().removeOnFrameMetricsAvailableListener(listener);});
            frames.quitSafely();
        }
        Map<String,Object> end=Performance.resources(0),modes=new LinkedHashMap<>();
        for(String name:draw.keySet())modes.put(name,object("handler",Performance.stats(handler.get(name)),
                "input_to_draw",Performance.stats(draw.get(name)),"window_frame",Performance.stats(frame.get(name))));
        return object("modes",modes,"start_resources",start,"end_resources",end,
                "latency_definition","enqueue to end of Canvas draw; 5 warmup actions per mode; synthetic pan and viewport zoom redraw",
                "document",app.document,"state_revision",app.replica.read().get("revision"));
    }
}
