package com.notemaker;

import android.app.Instrumentation;
import android.os.*;
import android.view.*;
import java.util.*;
import java.util.concurrent.*;
import java.util.concurrent.atomic.*;
import java.util.function.BooleanSupplier;
import static com.notemaker.core.Json.*;

/** Draw timing for common viewport and toolbar changes on a disposable note. */
final class InteractionPerformance {
    static Map<String,Object> run(Instrumentation test,MainActivity app) throws Exception {
        Handler input=Handler.createAsync(Looper.getMainLooper());
        HandlerThread frames=new HandlerThread("interaction-frames");frames.start();
        Map<String,List<Double>> draw=new LinkedHashMap<>(),handler=new LinkedHashMap<>(),frame=new LinkedHashMap<>();
        Map<String,List<Double>> queue=new LinkedHashMap<>(),action=new LinkedHashMap<>(),drawWait=new LinkedHashMap<>(),uiDraw=new LinkedHashMap<>();
        Map<String,List<Double>> frameDraw=new LinkedHashMap<>(),frameCommand=new LinkedHashMap<>();
        List<Double> undoUiDraw=new ArrayList<>(),redoUiDraw=new ArrayList<>(),undoCanvasDraw=new ArrayList<>(),redoCanvasDraw=new ArrayList<>();
        for(String name:Arrays.asList("pan","zoom_redraw","tool_switch","layer_switch","document_reopen","history_toggle")){
            draw.put(name,new ArrayList<>());handler.put(name,new ArrayList<>());
            frame.put(name,Collections.synchronizedList(new ArrayList<>()));
            queue.put(name,new ArrayList<>());action.put(name,new ArrayList<>());drawWait.put(name,new ArrayList<>());uiDraw.put(name,new ArrayList<>());
            frameDraw.put(name,Collections.synchronizedList(new ArrayList<>()));
            frameCommand.put(name,Collections.synchronizedList(new ArrayList<>()));
        }
        AtomicReference<String> current=new AtomicReference<>(null);
        AtomicReference<Runnable> uiDrawAck=new AtomicReference<>(null);
        AtomicReference<BooleanSupplier> uiDrawReady=new AtomicReference<>(null);
        ViewTreeObserver.OnDrawListener uiListener=()->{
            Runnable ack=uiDrawAck.get();BooleanSupplier ready=uiDrawReady.get();
            if(ack!=null&&(ready==null||ready.getAsBoolean())&&uiDrawAck.compareAndSet(ack,null))ack.run();
        };
        Window.OnFrameMetricsAvailableListener listener=(window,metrics,dropped)->{
            String name=current.get();if(name!=null){frame.get(name).add(metrics.getMetric(FrameMetrics.TOTAL_DURATION)/1e6);
                frameDraw.get(name).add(metrics.getMetric(FrameMetrics.DRAW_DURATION)/1e6);
                frameCommand.get(name).add(metrics.getMetric(FrameMetrics.COMMAND_ISSUE_DURATION)/1e6);}
        };
        test.runOnMainSync(()->{app.pan=true;app.erase=false;app.canvas.zoom=1;app.canvas.offsetX=app.canvas.offsetY=0;
            app.getWindow().addOnFrameMetricsAvailableListener(listener,new Handler(frames.getLooper()));
            app.getWindow().getDecorView().getViewTreeObserver().addOnDrawListener(uiListener);app.canvas.invalidate();});
        test.waitForIdleSync();
        Map<String,Object> start=Performance.resources(0);
        Map<String,Object> modeResources=new LinkedHashMap<>();
        String originalLayer=app.selected,extraLayer="performance-"+UUID.randomUUID();
        String historyId="performance-history-"+UUID.randomUUID();
        try {
            for(String name:draw.keySet()){
                if(name.equals("layer_switch")){
                    app.replica.command(Arrays.asList("layer","put","-"),object("layer",extraLayer,"name","Benchmark layer","force",false));
                    Map<String,Object> state=app.replica.read();
                    test.runOnMainSync(()->{try{app.refreshShared(state);app.update();}catch(Exception error){throw new RuntimeException(error);}});
                    test.waitForIdleSync();
                }
                if(name.equals("history_toggle")){
                    app.replica.command(Arrays.asList("object","put","-"),object("kind","object","object",historyId,
                            "shape","rect","layer",app.selected,"page",app.page,"x",120,"y",140,"w",180,"h",110,
                            "color",android.graphics.Color.BLACK,"width",5,"fontSize",24,"text",""));
                    Map<String,Object> state=app.replica.read();
                    test.runOnMainSync(()->{try{app.refreshShared(state);app.undo.clear();app.redo.clear();
                        app.undo.add("object-action");app.save();app.update();}catch(Exception error){throw new RuntimeException(error);}});
                    test.waitForIdleSync();
                }
                Map<String,Object> modeStart=Performance.resources(0);
                current.set(name);
                int count=name.equals("pan")||name.equals("history_toggle")?60:name.equals("document_reopen")?12:40;
                int warmup=name.equals("document_reopen")?2:name.equals("history_toggle")?6:5;
                for(int i=0;i<count;i++){
                    int index=i;CountDownLatch painted=new CountDownLatch(1),handled=new CountDownLatch(1),uiPainted=new CountDownLatch(1);
                    AtomicLong drawn=new AtomicLong(),uiDrawn=new AtomicLong(),started=new AtomicLong(),completed=new AtomicLong();
                    long queued=System.nanoTime();
                    input.post(()->{
                        started.set(System.nanoTime());
                        boolean history=name.equals("history_toggle"),forward=index%2==1;
                        BooleanSupplier ready=()->!history||(!app.historyPending&&
                                (forward?!Boolean.FALSE.equals(app.model.objectActive.get(historyId)):
                                        Boolean.FALSE.equals(app.model.objectActive.get(historyId))));
                        uiDrawReady.set(ready);
                        uiDrawAck.set(()->{uiDrawn.set(System.nanoTime());uiPainted.countDown();});
                        app.canvas.afterDraw=()->{if(ready.getAsBoolean()){drawn.set(System.nanoTime());painted.countDown();}};
                        if(name.equals("pan")){
                            app.pan=true;float x=app.canvas.getWidth()/2f,y=app.canvas.getHeight()/2f;
                            long when=SystemClock.uptimeMillis();
                            MotionEvent down=MotionEvent.obtain(when,when,MotionEvent.ACTION_DOWN,x,y,0);
                            MotionEvent move=MotionEvent.obtain(when,when+8,MotionEvent.ACTION_MOVE,x+(index%2==0?8:-8),y,0);
                            app.canvas.dispatchTouchEvent(down);app.canvas.dispatchTouchEvent(move);
                            down.recycle();move.recycle();
                        }else if(name.equals("zoom_redraw")){
                            app.canvas.zoom=index%2==0?1.2f:1f;app.canvas.invalidate();
                        }else if(name.equals("tool_switch")){
                            app.setDrawingTool(index%2==0?"rectangle":null);
                        }else if(name.equals("layer_switch")){
                            app.selectLayer(index%2==0?extraLayer:originalLayer);
                        }else if(history){
                            app.history(forward);
                        }else{
                            app.load(app.type);app.configureProxy();app.update();
                        }
                        completed.set(System.nanoTime());handled.countDown();
                    });
                    if(!handled.await(5,TimeUnit.SECONDS)||!uiPainted.await(5,TimeUnit.SECONDS)||
                            (!name.equals("layer_switch")&&!painted.await(5,TimeUnit.SECONDS)))
                        throw new IllegalStateException("Interaction draw timed out: "+name+" #"+index);
                    if(index>=warmup){
                        handler.get(name).add((completed.get()-queued)/1e6);uiDraw.get(name).add((uiDrawn.get()-queued)/1e6);
                        queue.get(name).add((started.get()-queued)/1e6);action.get(name).add((completed.get()-started.get())/1e6);
                        if(!name.equals("layer_switch")){draw.get(name).add((drawn.get()-queued)/1e6);
                            drawWait.get(name).add((drawn.get()-completed.get())/1e6);}
                        if(name.equals("history_toggle")){
                            (index%2==0?undoUiDraw:redoUiDraw).add((uiDrawn.get()-queued)/1e6);
                            (index%2==0?undoCanvasDraw:redoCanvasDraw).add((drawn.get()-queued)/1e6);
                        }
                    }
                    Thread.sleep(25);
                }
                if(name.equals("history_toggle")){
                    ReplicaService.writes.submit(()->{}).get(30,TimeUnit.SECONDS);
                    Map<String,Object> durable=app.replica.read();
                    com.notemaker.core.Model saved=com.notemaker.core.Store.model(durable);
                    com.notemaker.core.Model.require(saved.objects.containsKey(historyId)&&
                            !Boolean.FALSE.equals(saved.objectActive.get(historyId)),"History toggle did not persist final object");
                    test.runOnMainSync(()->com.notemaker.core.Model.require(app.undo.size()==1&&app.redo.isEmpty(),
                            "History toggle did not restore editor history"));
                }
                test.waitForIdleSync();modeResources.put(name,object("before",modeStart,"after",Performance.resources(0)));
            }
            test.runOnMainSync(()->{app.canvas.afterDraw=null;app.pan=false;app.setDrawingTool(null);});
            test.waitForIdleSync();Thread.sleep(250);
        }finally{
            current.set(null);
            test.runOnMainSync(()->{app.canvas.afterDraw=null;uiDrawAck.set(null);uiDrawReady.set(null);
                app.getWindow().removeOnFrameMetricsAvailableListener(listener);
                app.getWindow().getDecorView().getViewTreeObserver().removeOnDrawListener(uiListener);});
            frames.quitSafely();
        }
        Map<String,Object> end=Performance.resources(0),modes=new LinkedHashMap<>();
        System.gc();Thread.sleep(500);
        Map<String,Object> postGc=Performance.resources(0);
        for(String name:draw.keySet())modes.put(name,object("handler",Performance.stats(handler.get(name)),
                "input_to_draw",Performance.stats(draw.get(name)),"input_to_ui_draw",Performance.stats(uiDraw.get(name)),
                "window_frame",Performance.stats(frame.get(name)),
                "queue",Performance.stats(queue.get(name)),"action",Performance.stats(action.get(name)),
                "handler_to_draw",Performance.stats(drawWait.get(name)),"frame_draw",Performance.stats(frameDraw.get(name)),
                "frame_command_issue",Performance.stats(frameCommand.get(name))));
        Map<String,Object> historyDirections=object(
                "undo",object("input_to_ui_draw",Performance.stats(undoUiDraw),"input_to_draw",Performance.stats(undoCanvasDraw)),
                "redo",object("input_to_ui_draw",Performance.stats(redoUiDraw),"input_to_draw",Performance.stats(redoCanvasDraw)));
        return object("modes",modes,"mode_resources",modeResources,"start_resources",start,"end_resources",end,"post_gc_resources",postGc,
                "history_directions",historyDirections,
                "latency_definition","enqueue to first updated Canvas/window draw; 5 warmup actions per mode except 2 for document reopen; synthetic pan and viewport zoom redraw; history alternates object undo and redo",
                "document",app.document,"state_revision",app.replica.read().get("revision"));
    }
}
