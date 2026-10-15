package com.notemaker;
import android.app.*;
import android.os.*;
import android.content.*;
import android.graphics.*;
import android.view.*;
import org.json.*;
import java.util.Map;

/** Drives the real Activity and Canvas with pressure-bearing input events. */
public class Integration extends Instrumentation {
    Bundle arguments;
    static void gesture(MainActivity app,float x,float y,float w,float h,boolean cancel){
        long start=SystemClock.uptimeMillis();
        for(int i=0;i<3;i++){
            int action=i==0?MotionEvent.ACTION_DOWN:i==1?MotionEvent.ACTION_MOVE:cancel?MotionEvent.ACTION_CANCEL:MotionEvent.ACTION_UP;
            MotionEvent event=MotionEvent.obtain(start,start+i*20,action,(x+(i==0?0:w))*app.canvas.scale(),(y+(i==0?0:h))*app.canvas.scale(),0);
            app.canvas.onTouchEvent(event);event.recycle();
        }
    }
    static void copy(java.io.InputStream in,java.io.OutputStream out){try{byte[] b=new byte[8192];int n;while((n=in.read(b))!=-1)out.write(b,0,n);}catch(java.io.IOException e){throw new RuntimeException(e);}}
    @Override public void onCreate(Bundle args){arguments=args;start();}
    @Override public void onStart(){
        android.util.Log.i("NoteMakerPerformance","Instrumentation starting");
        Bundle result=new Bundle();try{
            Intent intent=new Intent(getTargetContext(),MainActivity.class);intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
            MainActivity app=(MainActivity)startActivitySync(intent);android.util.Log.i("NoteMakerPerformance","Activity started");waitForIdleSync();
            String action=arguments.getString("action","state");
            if(action.equals("erase-performance")){result.putString("stream","PASS erase-performance\n"+com.notemaker.core.Json.write(ErasePerformance.run(this,app))+"\n");finish(Activity.RESULT_OK,result);return;}
            if(action.equals("coverage")){CoverageCheck.run(this,app);result.putString("stream","PASS coverage\n");finish(Activity.RESULT_OK,result);return;}
            if(action.equals("file-preview")){
                Map<String,Object> snapshot=app.replica.read();Bitmap preview=DocumentBrowser.thumbnail(snapshot);
                int ink=0;for(int y=0;y<preview.getHeight();y++)for(int x=0;x<preview.getWidth();x++)if(preview.getPixel(x,y)!=Color.WHITE)ink++;
                preview.recycle();if(ink==0)throw new IllegalStateException("Document preview contains no drawing");
                if(!snapshot.get("revision").equals(app.replica.read().get("revision")))throw new IllegalStateException("Preview changed document state");
                result.putString("stream","PASS file-preview\n");finish(Activity.RESULT_OK,result);return;
            }
            if(action.equals("performance")){
                String metrics=com.notemaker.core.Json.write(Performance.run(this,app));
                try(java.io.FileOutputStream out=app.openFileOutput("performance.json",0)){out.write(metrics.getBytes("UTF-8"));}
                result.putString("stream","PASS performance\n"+metrics+"\n");finish(Activity.RESULT_OK,result);return;
            }
            ReplicaService.writes.submit(()->{}).get(30,java.util.concurrent.TimeUnit.SECONDS);
            java.lang.Process child=null;
            if(action.equals("cli")||action.equals("cli-concurrent")){
                JSONArray argumentsJson=new JSONArray(new String(android.util.Base64.decode(arguments.getString("cli"),android.util.Base64.DEFAULT),"UTF-8"));String[] cliArgs=new String[argumentsJson.length()];for(int i=0;i<cliArgs.length;i++)cliArgs[i]=argumentsJson.getString(i);
                child=app.startCli(cliArgs);
                try(java.io.OutputStream input=child.getOutputStream()){input.write(android.util.Base64.decode(arguments.getString("input",""),android.util.Base64.DEFAULT));}
            }
            final String[] drawnStroke={null};final int[] maxGesturePoints={0};
            runOnMainSync(()->{
                synchronized(app.lock){
                    if(action.equals("configure")){
                        app.save();app.document=arguments.getString("document","integration");app.host=arguments.getString("host","http://10.0.2.2:8765");app.proxyUrl=arguments.getString("proxyUrl","");app.proxyToken=arguments.getString("proxyToken","");app.load(arguments.getString("type","fixed"));
                        app.configureProxy();app.getSharedPreferences("settings",0).edit().putString("host",app.host).putString("document",app.document).putString("proxyUrl",app.proxyUrl).putString("proxyToken",app.proxyToken).commit();
                    }
                    if(action.equals("draw")||action.equals("cli-concurrent")){
                        app.setDrawingTool(null);
                        app.color=Integer.parseInt(arguments.getString("color","-16777216"));app.width=10;app.erase=false;app.pan=false;
                        float shift=Float.parseFloat(arguments.getString("shift","0")),scale=app.canvas.scale();long start=SystemClock.uptimeMillis();
                        for(int i=0;i<12;i++){
                            MotionEvent.PointerProperties property=new MotionEvent.PointerProperties();property.id=0;property.toolType=arguments.getString("tool","stylus").equals("finger")?MotionEvent.TOOL_TYPE_FINGER:MotionEvent.TOOL_TYPE_STYLUS;
                            MotionEvent.PointerCoords coords=new MotionEvent.PointerCoords();coords.x=(100+i*30+shift)*scale;coords.y=(200+i*20+shift)*scale;coords.pressure=.2f+i*.06f;coords.size=1;
                            MotionEvent event=MotionEvent.obtain(start,start+i*20,i==0?MotionEvent.ACTION_DOWN:i==11?MotionEvent.ACTION_UP:MotionEvent.ACTION_MOVE,1,new MotionEvent.PointerProperties[]{property},new MotionEvent.PointerCoords[]{coords},0,0,1,1,0,0,property.toolType==MotionEvent.TOOL_TYPE_FINGER?android.view.InputDevice.SOURCE_TOUCHSCREEN:android.view.InputDevice.SOURCE_STYLUS,0);
                            app.canvas.onTouchEvent(event);if(i==0)drawnStroke[0]=app.canvas.current;if(app.canvas.points!=null)maxGesturePoints[0]=Math.max(maxGesturePoints[0],app.canvas.points.length());event.recycle();
                        }
                    }
                    if(action.equals("shape")){
                        app.setDrawingTool(arguments.getString("shape","triangle"));
                        app.objectFillColor=Color.LTGRAY;
                        float x=Float.parseFloat(arguments.getString("x","100")),y=Float.parseFloat(arguments.getString("y","100"));
                        float w=Float.parseFloat(arguments.getString("w","160")),h=Float.parseFloat(arguments.getString("h","120"));
                        boolean cancel=Boolean.parseBoolean(arguments.getString("cancel","false"));
                        gesture(app,x,y,w,h,cancel);
                        app.setDrawingTool(null);
                    }
                    if(action.equals("fill-tap")){
                        app.setDrawingTool(null);app.objectFillColor=Color.RED;
                        app.enqueue(MainActivity.obj("kind","object","object",MainActivity.id(),"shape","rect","layer",app.selected,"page",app.page,"x",100,"y",100,"w",240,"h",180,"color",Color.BLACK,"width",5,"fontSize",24,"text",""));
                        app.fillTapMode=true;
                        gesture(app,180,160,0,0,false);
                    }
                    if(action.equals("collaboration")){
                        app.setDrawingTool(null);app.enqueue(MainActivity.obj("kind","object","object","collaboration-object","shape","rect","layer",app.selected,"page",app.page,"x",120,"y",140,"w",160,"h",120,"color",Color.BLACK,"width",5,"fontSize",24,"text",""));
                    }
                    if(action.equals("remote-preview")){
                        app.setDrawingTool(null);app.enqueue(MainActivity.obj("kind","object","object","remote-preview-object","shape","rect","layer",app.selected,"page",app.page,"x",100,"y",120,"w",120,"h",90,"color",Color.BLACK,"width",5,"fontSize",24,"text",""));
                    }
                    if(action.equals("locked-undo")){
                        app.persistObject(MainActivity.obj("kind","object","object","locked-undo-object","shape","rect","layer",app.selected,"page",app.page,"x",100,"y",120,"w",120,"h",90,"color",Color.BLACK,"width",5,"fontSize",24,"text",""));
                    }
                    if(action.equals("lasso")){
                        app.setDrawingTool(null);
                        app.enqueue(MainActivity.obj("kind","object","object","lasso-object","shape","rect","layer",app.selected,"page",app.page,"x",100,"y",100,"w",150,"h",150,"color",Color.BLACK,"width",5,"fontSize",24,"text",""));
                        app.enqueue(MainActivity.obj("kind","stroke","stroke","lasso-stroke","layer",app.selected,"page",app.page,"color",Color.BLACK,"width",5,"brush","pen","points",new JSONArray().put(new JSONArray(java.util.Arrays.asList(400,100,1))).put(new JSONArray(java.util.Arrays.asList(400,400,1)))));
                    }
                    if(action.equals("freeform-lasso")){
                        app.setDrawingTool(null);app.freeformLasso=true;
                        app.enqueue(MainActivity.obj("kind","object","object","lasso-inside","shape","rect","layer",app.selected,"page",app.page,"x",130,"y",130,"w",20,"h",20,"color",Color.BLACK,"width",1,"fontSize",24,"text",""));
                        app.enqueue(MainActivity.obj("kind","object","object","lasso-outside-bounds","shape","rect","layer",app.selected,"page",app.page,"x",270,"y",270,"w",20,"h",20,"color",Color.BLACK,"width",1,"fontSize",24,"text",""));
                    }
                    if(action.equals("layer-order")){
                        app.setDrawingTool(null);
                        app.enqueue(MainActivity.obj("kind","layer","layer","template-0","name","Page template","force",false,"order",-10));
                        app.enqueue(MainActivity.obj("kind","layer","layer","order-low","name","Blue below red","force",false,"order",-2));
                        app.enqueue(MainActivity.obj("kind","layer","layer","order-high","name","Red above blue","force",false,"order",2));
                        app.enqueue(MainActivity.obj("kind","object","object","order-template-bg","shape","rect","layer","template-0","page",app.page,"x",500,"y",500,"w",120,"h",100,"color",Color.BLACK,"width",1,"fill",0xffdddddd,"fontSize",24,"text",""));
                        app.enqueue(MainActivity.obj("kind","object","object","order-blue","shape","rect","layer","order-low","page",app.page,"x",100,"y",100,"w",120,"h",100,"color",Color.BLACK,"width",1,"fill",Color.BLUE,"fontSize",24,"text",""));
                        app.enqueue(MainActivity.obj("kind","object","object","order-red","shape","rect","layer","order-high","page",app.page,"x",100,"y",100,"w",120,"h",100,"color",Color.BLACK,"width",1,"fill",Color.RED,"fontSize",24,"text",""));
                        app.enqueue(MainActivity.obj("kind","stroke","stroke","order-ink","layer","base","page",app.page,"color",Color.BLACK,"width",8,"brush","pen","points",new JSONArray().put(new JSONArray(java.util.Arrays.asList(510,550,1))).put(new JSONArray(java.util.Arrays.asList(600,550,1)))));
                    }
                    if(action.equals("tool-switch-selection")){
                        app.setDrawingTool(null);app.enqueue(MainActivity.obj("kind","object","object","tool-switch-item","shape","rect","layer",app.selected,"page",app.page,"x",120,"y",140,"w",150,"h",100,"color",Color.BLACK,"width",5,"fontSize",24,"text",""));app.enqueue(MainActivity.obj("kind","object","object","tool-switch-item-2","shape","ellipse","layer",app.selected,"page",app.page,"x",340,"y",220,"w",90,"h",70,"color",Color.BLACK,"width",5,"fontSize",24,"text",""));
                    }
                    if(action.equals("select-panel")){
                        if(!app.model.objects.containsKey("lasso-object"))app.enqueue(MainActivity.obj("kind","object","object","lasso-object","shape","rect","layer",app.selected,"page",app.page,"x",100,"y",100,"w",150,"h",150,"color",Color.BLACK,"width",5,"fontSize",24,"text",""));
                    }
                    if(action.equals("mixed-drawing")){
                        app.setDrawingTool("triangle");gesture(app,100,100,100,100,false);
                        app.setDrawingTool(null);gesture(app,300,100,100,100,false);
                        app.setDrawingTool("star");gesture(app,100,300,100,100,false);
                        app.setDrawingTool(null);gesture(app,300,300,100,100,false);
                        app.history(false); // Pending object saves must block this attempt.
                    }
                    if(action.equals("text"))app.placeText(arguments.getString("text","Editable text"),32,300,400);
                    if(action.equals("layer")){app.selected=arguments.getString("layer","test-layer");app.enqueue(MainActivity.obj("kind","layer","layer",app.selected,"name","Test layer","force",Boolean.parseBoolean(arguments.getString("force","false"))));app.save();}
                    if(action.equals("hide")){app.hidden.add(arguments.getString("layer","test-layer"));app.save();}
                    if(action.equals("undo"))app.history(false);
                    if(action.equals("redo"))app.history(true);
                    if(action.equals("page")){app.page=Integer.parseInt(arguments.getString("page","1"));app.save();}
                    if(action.equals("erase")){app.erase=true;float shift=Float.parseFloat(arguments.getString("shift","0"));app.canvas.eraseAt((200+shift)*app.canvas.scale(),(266+shift)*app.canvas.scale());app.save();}
                    app.update();
                }
            });
            if(action.equals("draw")){
                drainWrites();waitForIdleSync();org.json.JSONObject completed=app.strokes.get(drawnStroke[0]);int stored=completed==null||completed.optJSONArray("points")==null?0:completed.optJSONArray("points").length();
                // ACTION_UP appends the final sample and clears the in-flight list, so the
                // last observable in-gesture length is 11 while the persisted stroke has 12.
                if(maxGesturePoints[0]<11||stored<12)throw new IllegalStateException("Canvas stroke lost move samples: observed="+maxGesturePoints[0]+" stored="+stored+" stroke="+drawnStroke[0]);
            }
            if(action.equals("remote-preview")){
                drainWrites();waitForIdleSync();long revisionBefore=app.revision;java.util.ArrayList<String> undoBefore=new java.util.ArrayList<>(app.undo);Bitmap before=captureCanvas(app);
                String session="55555555-5555-4555-8555-555555555555";Map<String,Object> entity=com.notemaker.core.Json.object("kind","object","id","remote-preview-object","page",app.page);
                com.notemaker.cli.LocalProxy proxy=ReplicaService.ensure(app);proxy.execute(com.notemaker.core.Json.object("document",app.document,"actor","remote-editor","args",java.util.Arrays.asList("selection","select","-"),"input",com.notemaker.core.Json.object("session",session,"page",app.page,"entities",java.util.Collections.singletonList(entity))));
                Map<String,Object> transform=com.notemaker.core.Json.object("session",session,"entities",java.util.Collections.singletonList(entity),"transform",com.notemaker.core.Json.object("scale",1,"dx",180,"dy",40));
                proxy.execute(com.notemaker.core.Json.object("document",app.document,"actor","remote-editor","args",java.util.Arrays.asList("transform","preview","-"),"input",transform));
                waitUntil(()->app.transformPreviews.containsKey(session),10000,"foreign transform preview notification");waitForIdleSync();Bitmap previewed=captureCanvas(app);
                if(!imagesDiffer(before,previewed))throw new IllegalStateException("Foreign preview updated metadata but did not render a visual transform");
                if(app.revision!=revisionBefore||!app.undo.equals(undoBefore))throw new IllegalStateException("Ephemeral foreign preview changed revision or undo history");
                proxy.execute(com.notemaker.core.Json.object("document",app.document,"actor","remote-editor","args",java.util.Arrays.asList("transform","cancel",session)));
                waitUntil(()->!app.transformPreviews.containsKey(session),10000,"foreign preview cancellation notification");waitForIdleSync();Bitmap cancelled=captureCanvas(app);
                if(imagesDiffer(before,cancelled))throw new IllegalStateException("Cancelled foreign preview did not restore committed canvas pixels");
                if(app.revision!=revisionBefore||!app.undo.equals(undoBefore))throw new IllegalStateException("Foreign preview cancel changed revision or undo history");
                proxy.execute(com.notemaker.core.Json.object("document",app.document,"actor","remote-editor","args",java.util.Arrays.asList("selection","deselect",session)));
                before.recycle();previewed.recycle();cancelled.recycle();
            }
            if(action.equals("locked-undo")){
                drainWrites();waitForIdleSync();waitUntil(()->app.pendingObjectHistory.isEmpty(),10000,"object history save");
                java.util.ArrayList<String> undoBefore=new java.util.ArrayList<>(app.undo),redoBefore=new java.util.ArrayList<>(app.redo);String modelBefore=com.notemaker.core.Json.write(app.model.json());
                String session="66666666-6666-4666-8666-666666666666";Map<String,Object> entity=com.notemaker.core.Json.object("kind","object","id","locked-undo-object","page",app.page);com.notemaker.cli.LocalProxy proxy=ReplicaService.ensure(app);
                proxy.execute(com.notemaker.core.Json.object("document",app.document,"actor","remote-editor","args",java.util.Arrays.asList("selection","select","-"),"input",com.notemaker.core.Json.object("session",session,"page",app.page,"entities",java.util.Collections.singletonList(entity))));
                runOnMainSync(()->app.history(false));waitUntil(()->!app.historyPending,10000,"rejected locked undo");
                if(!app.undo.equals(undoBefore)||!app.redo.equals(redoBefore)||!modelBefore.equals(com.notemaker.core.Json.write(app.model.json())))throw new IllegalStateException("Rejected locked undo changed model or local history");
                proxy.execute(com.notemaker.core.Json.object("document",app.document,"actor","remote-editor","args",java.util.Arrays.asList("selection","deselect",session)));
                runOnMainSync(()->app.history(false));waitUntil(()->!app.historyPending&&app.undo.size()==undoBefore.size()-1,10000,"unlocked undo");
                if(!Boolean.FALSE.equals(app.model.objectActive.get("locked-undo-object"))||app.redo.size()!=redoBefore.size()+1)throw new IllegalStateException("Undo did not succeed after foreign lease release");
            }
            if(action.equals("conflict-preview"))waitForIdleSync();
            if(action.equals("locked-selection")){
                if(arguments.containsKey("cli")){
                    JSONArray argumentsJson=new JSONArray(new String(android.util.Base64.decode(arguments.getString("cli"),android.util.Base64.DEFAULT),"UTF-8"));java.util.ArrayList<String> cliArgs=new java.util.ArrayList<>();for(int i=0;i<argumentsJson.length();i++)cliArgs.add(argumentsJson.getString(i));
                    ReplicaService.ensure(app).execute(com.notemaker.core.Json.object("document",app.document,"actor","remote-editor","args",cliArgs,"input",com.notemaker.core.Json.parse(new String(android.util.Base64.decode(arguments.getString("input",""),android.util.Base64.DEFAULT),"UTF-8"))));
                }
                runOnMainSync(()->app.startSelection(java.util.Collections.singletonList(com.notemaker.core.Json.object("kind","object","id",arguments.getString("object","locked-object")))));
                waitUntil(()->app.selectionSkipped>0,10000,"locked selection result");drainWrites();waitForIdleSync();
                if(!app.selectedEntities.isEmpty()||app.selectionSkipped!=1)throw new IllegalStateException("Locked selection was acquired instead of skipped");
                runOnMainSync(app::deselect);drainWrites();
            }
            if(action.equals("clipboard")){
                runOnMainSync(()->app.enqueue(MainActivity.obj("kind","object","object","clipboard-object","shape","rect","layer",app.selected,"page",app.page,"x",120,"y",120,"w",80,"h",60,"color",Color.BLACK,"width",4,"fontSize",24,"text","")));
                drainWrites();waitForIdleSync();runOnMainSync(()->app.startSelection(java.util.Collections.singletonList(com.notemaker.core.Json.object("kind","object","id","clipboard-object"))));
                waitUntil(()->app.selectionSession!=null,10000,"clipboard selection");runOnMainSync(app::copySelection);waitUntil(()->!com.notemaker.core.Json.list(app.selectionClipboard.getOrDefault("entities",java.util.Collections.emptyList())).isEmpty(),10000,"selection copied to proxy clipboard");
                drainWrites();waitForIdleSync();runOnMainSync(app::deselect);drainWrites();
            }
            if(action.equals("lasso")){
                drainWrites();waitForIdleSync();runOnMainSync(()->{app.selecting=true;gesture(app,50,50,500,500,false);});
                waitUntil(()->app.selectedEntities.size()==2,10000,"lasso selection");drainWrites();waitForIdleSync();
                boolean objectSelected=false,strokeSelected=false;for(Map<String,Object> entity:app.selectedEntities){objectSelected|="lasso-object".equals(entity.get("id"));strokeSelected|="lasso-stroke".equals(entity.get("id"));}
                if(!objectSelected||!strokeSelected)throw new IllegalStateException("Lasso missed object or stroke: "+app.selectedEntities);
                runOnMainSync(app::deselect);drainWrites();runOnMainSync(()->app.selectAt(400,250));waitUntil(()->app.selectedEntities.size()==1,10000,"stroke midpoint selection");
                if(!"lasso-stroke".equals(app.selectedEntities.get(0).get("id")))throw new IllegalStateException("Stroke midpoint hit test failed: "+app.selectedEntities);
                runOnMainSync(app::deselect);drainWrites();
            }
            if(action.equals("freeform-lasso")){
                drainWrites();waitForIdleSync();long start=SystemClock.uptimeMillis();float[][] path={{100,100},{300,100},{100,300},{100,100}};
                runOnMainSync(()->{app.selecting=true;for(int i=0;i<path.length;i++){int mode=i==0?MotionEvent.ACTION_DOWN:i==path.length-1?MotionEvent.ACTION_UP:MotionEvent.ACTION_MOVE;MotionEvent event=MotionEvent.obtain(start,start+i*40,mode,path[i][0]*app.canvas.scale(),path[i][1]*app.canvas.scale(),0);app.canvas.onTouchEvent(event);event.recycle();}});
                waitUntil(()->app.selectedEntities.size()==1,10000,"freeform lasso selection");drainWrites();waitForIdleSync();
                if(!"lasso-inside".equals(app.selectedEntities.get(0).get("id")))throw new IllegalStateException("Freeform lasso selected outside its polygon: "+app.selectedEntities);
                runOnMainSync(app::deselect);drainWrites();
            }
            if(action.equals("layer-order")){
                drainWrites();waitForIdleSync();runOnMainSync(()->{try{app.refreshShared(app.replica.read());app.update();}catch(Exception e){throw new RuntimeException(e);}});waitForIdleSync();
                Bitmap first=captureCanvas(app);int[] before={canvasPixel(app,first,160,150),canvasPixel(app,first,550,550)};
                if(Color.red(before[0])<220||Color.green(before[0])>80||Color.blue(before[0])>80||Color.red(before[1])>60||Color.green(before[1])>60||Color.blue(before[1])>60)throw new IllegalStateException("Template/layer paint order is incorrect: "+java.util.Arrays.toString(before));
                com.notemaker.cli.LocalProxy proxy=ReplicaService.ensure(app);proxy.execute(com.notemaker.core.Json.object("document",app.document,"actor","human","args",java.util.Arrays.asList("layer","reorder","order-high","0")));
                Map<String,Object> state=app.replica.read();runOnMainSync(()->{try{app.refreshShared(state);app.update();}catch(Exception e){throw new RuntimeException(e);}});waitForIdleSync();Bitmap second=captureCanvas(app);int after=canvasPixel(app,second,160,150);
                if(Color.blue(after)<220||Color.red(after)>80)throw new IllegalStateException("Layer reorder did not update Android compositing: "+Color.red(after)+","+Color.green(after)+","+Color.blue(after));
                first.recycle();second.recycle();
            }
            if(action.equals("tool-switch-selection")){
                drainWrites();waitForIdleSync();final java.util.concurrent.CountDownLatch entered=new java.util.concurrent.CountDownLatch(1),release=new java.util.concurrent.CountDownLatch(1);
                ReplicaService.writes.execute(()->{entered.countDown();try{if(!release.await(15,java.util.concurrent.TimeUnit.SECONDS))throw new IllegalStateException("selection race gate timed out");}catch(InterruptedException e){Thread.currentThread().interrupt();}});
                if(!entered.await(5,java.util.concurrent.TimeUnit.SECONDS))throw new IllegalStateException("selection race gate did not start");
                runOnMainSync(()->{app.startSelection(java.util.Collections.singletonList(com.notemaker.core.Json.object("kind","object","id","tool-switch-item")));app.editor.openTool("pen",app.canvas);});release.countDown();drainWrites();waitForIdleSync();drainWrites();
                Map<String,Object> state=app.replica.read();Map<String,Object> leases=com.notemaker.core.Json.map(state.getOrDefault("leases",java.util.Collections.emptyMap()));String key=app.page+":object:tool-switch-item";
                if(app.selectionSession!=null||!app.selectedEntities.isEmpty()||leases.containsKey(key))throw new IllegalStateException("A late selection acquisition survived switching to Pen: session="+app.selectionSession+" leases="+leases);
                runOnMainSync(()->{app.startSelection(java.util.Collections.singletonList(com.notemaker.core.Json.object("kind","object","id","tool-switch-item")));});waitUntil(()->app.selectionSession!=null,10000,"active selection for Select mode");String session=app.selectionSession;
                runOnMainSync(()->app.editor.openTool("select",app.canvas));drainWrites();state=app.replica.read();leases=com.notemaker.core.Json.map(state.getOrDefault("leases",java.util.Collections.emptyMap()));if(!session.equals(app.selectionSession)||!leases.containsKey(key))throw new IllegalStateException("Opening Select mode discarded the current selection");
                String modelBefore=com.notemaker.core.Json.write(app.model.json());java.util.ArrayList<String> undoBefore=new java.util.ArrayList<>(app.undo);long revisionBefore=app.revision;
                runOnMainSync(()->app.previewSelection(1,120,20));waitUntil(()->app.transformPreviews.containsKey(session),10000,"active transform preview before tool switch");
                runOnMainSync(()->app.editor.openTool("pen",app.canvas));drainWrites();waitForIdleSync();drainWrites();state=app.replica.read();leases=com.notemaker.core.Json.map(state.getOrDefault("leases",java.util.Collections.emptyMap()));Map<String,Object> previews=com.notemaker.core.Json.map(state.getOrDefault("previews",java.util.Collections.emptyMap()));
                if(app.selectionSession!=null||!app.selectedEntities.isEmpty()||leases.containsKey(key)||previews.containsKey(session))throw new IllegalStateException("Switching to Pen retained a lease or transform preview: leases="+leases+" previews="+previews);
                if(!modelBefore.equals(com.notemaker.core.Json.write(app.model.json()))||!undoBefore.equals(app.undo)||revisionBefore!=app.revision)throw new IllegalStateException("Cancelling the preview on tool switch changed drawing state or history");
                for(String kind:new String[]{"eraser","move","shapes","text","fill"}){
                    runOnMainSync(()->app.startSelection(java.util.Collections.singletonList(com.notemaker.core.Json.object("kind","object","id","tool-switch-item"))));waitUntil(()->app.selectionSession!=null,10000,"selection before "+kind+" switch");
                    final String selectedTool=kind;runOnMainSync(()->app.editor.openTool(selectedTool,app.canvas));drainWrites();waitForIdleSync();drainWrites();state=app.replica.read();leases=com.notemaker.core.Json.map(state.getOrDefault("leases",java.util.Collections.emptyMap()));
                    if(app.selectionSession!=null||!app.selectedEntities.isEmpty()||leases.containsKey(key))throw new IllegalStateException("Switching to "+kind+" retained a selection lease: "+leases);
                }
                runOnMainSync(()->app.startSelection(java.util.Collections.singletonList(com.notemaker.core.Json.object("kind","object","id","tool-switch-item"))));waitUntil(()->app.selectionSession!=null,10000,"selection before shape setter");runOnMainSync(()->app.setDrawingTool("rect"));drainWrites();waitForIdleSync();drainWrites();state=app.replica.read();leases=com.notemaker.core.Json.map(state.getOrDefault("leases",java.util.Collections.emptyMap()));
                if(app.selectionSession!=null||leases.containsKey(key))throw new IllegalStateException("Direct shape selection retained a lease: "+leases);
                runOnMainSync(()->app.startSelection(java.util.Arrays.asList(com.notemaker.core.Json.object("kind","object","id","tool-switch-item"),com.notemaker.core.Json.object("kind","object","id","tool-switch-item-2"))));waitUntil(()->app.selectionSession!=null&&app.selectedEntities.size()==2,10000,"multi-item selection before deselect race");final java.util.concurrent.CountDownLatch deselectEntered=new java.util.concurrent.CountDownLatch(1),deselectRelease=new java.util.concurrent.CountDownLatch(1);
                ReplicaService.writes.execute(()->{deselectEntered.countDown();try{if(!deselectRelease.await(15,java.util.concurrent.TimeUnit.SECONDS))throw new IllegalStateException("deselect race gate timed out");}catch(InterruptedException e){Thread.currentThread().interrupt();}});if(!deselectEntered.await(5,java.util.concurrent.TimeUnit.SECONDS))throw new IllegalStateException("deselect race gate did not start");
                runOnMainSync(()->{app.deselectEntity("object","tool-switch-item");app.editor.openTool("move",app.canvas);});deselectRelease.countDown();drainWrites();waitForIdleSync();drainWrites();state=app.replica.read();leases=com.notemaker.core.Json.map(state.getOrDefault("leases",java.util.Collections.emptyMap()));
                if(app.selectionSession!=null||app.selecting||leases.containsKey(key)||leases.containsKey(app.page+":object:tool-switch-item-2"))throw new IllegalStateException("A delayed per-item deselect reacquired locks after switching tools: "+leases);
            }
            if(action.equals("select-panel")){
                drainWrites();waitForIdleSync();runOnMainSync(()->app.startSelection(java.util.Collections.singletonList(com.notemaker.core.Json.object("kind","object","id","lasso-object"))));waitUntil(()->app.selectionSession!=null,10000,"selection panel lease");runOnMainSync(()->app.editor.showSelectionPanel());waitForIdleSync();
            }
            if(action.equals("collaboration")){
                drainWrites();waitForIdleSync();
                runOnMainSync(()->app.startSelection(java.util.Collections.singletonList(com.notemaker.core.Json.object("kind","object","id","collaboration-object"))));
                waitUntil(()->app.selectionSession!=null,10000,"selection lease");drainWrites();waitForIdleSync();
                final String session=app.selectionSession;
                runOnMainSync(()->app.previewTransform(com.notemaker.core.Json.object("scale",1.5,"dx",20,"dy",30,"rotation",30,"flipX",true)));
                waitUntil(()->!app.transformPreviews.isEmpty(),10000,"transform preview");drainWrites();waitForIdleSync();
                if(!app.replica.read().containsKey("previews"))throw new IllegalStateException("Snapshot omitted transform previews");
                runOnMainSync(app::cancelTransformPreview);drainWrites();waitForIdleSync();
                if(Math.abs(((Number)app.model.objects.get("collaboration-object").get("x")).doubleValue()-120)>0.01||!app.transformPreviews.isEmpty())throw new IllegalStateException("Canceled preview changed durable state");
                runOnMainSync(()->app.previewTransform(com.notemaker.core.Json.object("scale",1.5,"dx",20,"dy",30,"rotation",30,"flipX",true)));
                drainWrites();waitForIdleSync();runOnMainSync(app::commitTransformPreview);drainWrites();waitForIdleSync();
                Map<String,Object> transformed=app.model.objects.get("collaboration-object");
                if(transformed==null||Math.abs(((Number)transformed.get("rotation")).doubleValue()-30)>0.01||!Boolean.TRUE.equals(transformed.get("flipX")))throw new IllegalStateException("Transform did not commit rotation/flip: "+transformed);
                runOnMainSync(app::duplicateSelection);drainWrites();waitForIdleSync();
                if(app.model.objects.size()<2)throw new IllegalStateException("Selection duplicate did not create an object");
                runOnMainSync(app::deleteSelection);drainWrites();waitForIdleSync();
                if(!Boolean.FALSE.equals(app.model.objectActive.get("collaboration-object")))throw new IllegalStateException("Selection delete did not deactivate original");
                runOnMainSync(app::deselect);drainWrites();
            }
            if(child!=null){
                final java.lang.Process process=child;java.io.ByteArrayOutputStream output=new java.io.ByteArrayOutputStream(),errors=new java.io.ByteArrayOutputStream();
                Thread stdout=new Thread(()->copy(process.getInputStream(),output)),stderr=new Thread(()->copy(process.getErrorStream(),errors));stdout.start();stderr.start();
                if(!child.waitFor(30,java.util.concurrent.TimeUnit.SECONDS)){child.destroyForcibly();throw new RuntimeException("CLI timed out");}stdout.join();stderr.join();
                if(child.exitValue()!=0)throw new RuntimeException("CLI failed: "+errors.toString("UTF-8"));
                try(java.io.FileOutputStream out=app.openFileOutput("cli-result.json",0)){out.write(output.toByteArray());}
                result.putString("cli",output.size()<32000?output.toString("UTF-8"):"See files/cli-result.json");
            }
            if(action.equals("conflict-preview")){
                makeConflictFixture(app);Map<String,Object> snapshot=app.replica.read();if(com.notemaker.core.Json.list(snapshot.getOrDefault("conflicts",java.util.Collections.emptyList())).size()!=1)throw new IllegalStateException("Proxy ingress did not materialize the expected real conflict: "+snapshot.get("conflicts"));
                runOnMainSync(()->{try{app.refreshShared(snapshot);app.update();app.editor.showConflictPanel();}catch(Exception error){throw new RuntimeException(error);}});
                waitForIdleSync();
            }
            ReplicaService.writes.submit(()->{}).get(30,java.util.concurrent.TimeUnit.SECONDS);
            Thread.sleep(800);
            runOnMainSync(()->{synchronized(app.lock){
                try{
                    if(action.equals("state")){
                        int width=app.canvas.getWidth(),height=app.canvas.getHeight();
                        Bitmap cached=Bitmap.createBitmap(width,height,Bitmap.Config.ARGB_8888),reference=Bitmap.createBitmap(width,height,Bitmap.Config.ARGB_8888);
                        app.canvas.draw(new Canvas(cached));app.canvas.useInkCache=false;
                        try{app.canvas.draw(new Canvas(reference));}finally{app.canvas.useInkCache=true;}
                        int[] a=new int[width*height],b=new int[a.length];cached.getPixels(a,0,width,0,0,width,height);reference.getPixels(b,0,width,0,0,width,height);int mismatched=0;
                        for(int i=0;i<a.length;i++)if(Math.abs(Color.red(a[i])-Color.red(b[i]))>8||Math.abs(Color.green(a[i])-Color.green(b[i]))>8||Math.abs(Color.blue(a[i])-Color.blue(b[i]))>8)mismatched++;
                        cached.recycle();reference.recycle();if(mismatched>a.length/1000)throw new IllegalStateException("Ink cache differs from uncached rendering: "+mismatched+" pixels");
                    }
                    JSONObject state=MainActivity.obj("document",app.document,"type",app.type,"cursor",app.cursor,"pending",app.pendingCount,"strokes",app.strokes.size(),"events",new JSONArray(com.notemaker.core.Json.list(app.store.read().get("events"))),"visible",app.canvas.visible("test-layer"),"page",app.page,"model",new JSONObject(com.notemaker.core.Json.write(app.model.json())));
                    Bitmap image=Bitmap.createBitmap(1000,1400,Bitmap.Config.ARGB_8888);Canvas c=new Canvas(image);c.drawColor(Color.WHITE);
                    com.notemaker.core.Render.draw(app.model,app.page,app.hidden,new com.notemaker.cli.Png(c));
                    try(java.io.FileOutputStream out=app.openFileOutput("integration.png",0)){image.compress(Bitmap.CompressFormat.PNG,100,out);}
                    try(java.io.FileOutputStream out=app.openFileOutput("integration.json",0)){out.write(state.toString().getBytes("UTF-8"));}
                    int[] pixels=new int[1000*1400];image.getPixels(pixels,0,1000,0,0,1000,1400);int count=0,minX=1000,minY=1400,maxX=-1,maxY=-1;for(int i=0;i<pixels.length;i++){int pixel=pixels[i];if(Color.red(pixel)<245||Color.green(pixel)<245||Color.blue(pixel)<245){count++;int x=i%1000,y=i/1000;minX=Math.min(minX,x);minY=Math.min(minY,y);maxX=Math.max(maxX,x);maxY=Math.max(maxY,y);}}JSONObject stats=MainActivity.obj("pixels",count,"bounds",new JSONArray(java.util.Arrays.asList(minX,minY,maxX,maxY)));try(java.io.FileOutputStream out=app.openFileOutput("render-stats.json",0)){out.write(stats.toString().getBytes("UTF-8"));}result.putString("state",state.toString().length()<32000?state.toString():"See files/integration.json");
                }catch(Exception e){throw new RuntimeException(e);}
            }});
            result.putString("stream","PASS "+action+"\n");finish(Activity.RESULT_OK,result);
        }catch(Throwable error){result.putString("stream","FAIL: "+error+"\n");finish(Activity.RESULT_CANCELED,result);}
    }
    private void drainWrites() throws Exception {ReplicaService.writes.submit(()->{}).get(30,java.util.concurrent.TimeUnit.SECONDS);}
    private void waitUntil(java.util.concurrent.Callable<Boolean> condition,long timeout,String label)throws Exception{long deadline=System.currentTimeMillis()+timeout;while(System.currentTimeMillis()<deadline){waitForIdleSync();if(condition.call())return;Thread.sleep(50);}throw new IllegalStateException("Timed out waiting for "+label);}
    private void makeConflictFixture(MainActivity app) throws Exception {
        String id="android-conflict-item";java.io.File root=new java.io.File(app.getCacheDir(),"drawing-conflict-fixture");
        com.notemaker.core.Store a=com.notemaker.core.Store.writer(new java.io.File(root,"a"),app.document);
        com.notemaker.core.Store b=com.notemaker.core.Store.writer(new java.io.File(root,"b"),app.document);
        Map<String,Object> seed=com.notemaker.core.Actions.drawing(com.notemaker.core.Json.object("object",id,"shape","rect","x",80,"y",90,"w",180,"h",120,"color",Color.BLACK,"width",5),"base");
        a.append(seed,"fixture-seed");Map<String,Object> common=com.notemaker.core.Json.map(com.notemaker.core.Json.list(a.read().get("pending")).get(0));
        b.accept(java.util.Collections.singletonList(common),null);
        a.commit("writer-a",model->{Map<String,Object> changed=new java.util.LinkedHashMap<>(model.objects.get(id));changed.put("x",100);return java.util.Collections.singletonList(changed);});
        b.commit("writer-b",model->{Map<String,Object> changed=new java.util.LinkedHashMap<>(model.objects.get(id));changed.put("shape","ellipse");changed.put("x",250);return java.util.Collections.singletonList(changed);});
        java.util.ArrayList<Object> incoming=new java.util.ArrayList<>(com.notemaker.core.Json.list(a.read().get("pending")));
        incoming.addAll(com.notemaker.core.Json.list(b.read().get("pending")));
        com.notemaker.cli.LocalProxy proxy=ReplicaService.ensure(app);Map<String,Object> descriptor=com.notemaker.core.Json.map(com.notemaker.core.Json.parse(new String(java.nio.file.Files.readAllBytes(new java.io.File(app.getFilesDir(),"proxy.json").toPath()),java.nio.charset.StandardCharsets.UTF_8)));
        java.net.HttpURLConnection request=(java.net.HttpURLConnection)new java.net.URL("http://127.0.0.1:"+proxy.port()+"/api/events").openConnection();request.setRequestMethod("POST");request.setConnectTimeout(5000);request.setReadTimeout(10000);request.setDoOutput(true);request.setRequestProperty("Authorization","Bearer "+descriptor.get("token"));request.setRequestProperty("Content-Type","application/json; charset=utf-8");
        byte[] body=com.notemaker.core.Json.write(com.notemaker.core.Json.object("document",app.document,"operations",incoming)).getBytes(java.nio.charset.StandardCharsets.UTF_8);try(java.io.OutputStream out=request.getOutputStream()){out.write(body);}
        if(request.getResponseCode()!=200){java.io.ByteArrayOutputStream error=new java.io.ByteArrayOutputStream();copy(request.getErrorStream(),error);throw new IllegalStateException("Proxy conflict fixture ingress failed: "+error.toString("UTF-8"));}
        try(java.io.InputStream response=request.getInputStream()){java.io.ByteArrayOutputStream received=new java.io.ByteArrayOutputStream();copy(response,received);com.notemaker.core.Json.parse(received.toString("UTF-8"));}finally{request.disconnect();}
    }
    private Bitmap captureCanvas(MainActivity app){final Bitmap[] output=new Bitmap[1];runOnMainSync(()->{Bitmap bitmap=Bitmap.createBitmap(app.canvas.getWidth(),app.canvas.getHeight(),Bitmap.Config.ARGB_8888);app.canvas.draw(new Canvas(bitmap));output[0]=bitmap;});return output[0];}
    private int canvasPixel(MainActivity app,Bitmap bitmap,float x,float y){int px=Math.max(0,Math.min(bitmap.getWidth()-1,Math.round(app.canvas.offsetX+x*app.canvas.scale()*app.canvas.zoom))),py=Math.max(0,Math.min(bitmap.getHeight()-1,Math.round(app.canvas.offsetY+y*app.canvas.scale()*app.canvas.zoom)));return bitmap.getPixel(px,py);}
    private boolean imagesDiffer(Bitmap a,Bitmap b){if(a.getWidth()!=b.getWidth()||a.getHeight()!=b.getHeight())return true;for(int y=0;y<a.getHeight();y+=4)for(int x=0;x<a.getWidth();x+=4)if(a.getPixel(x,y)!=b.getPixel(x,y))return true;return false;}
}
