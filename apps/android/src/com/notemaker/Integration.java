package com.notemaker;
import android.app.*;
import android.os.*;
import android.content.*;
import android.graphics.*;
import android.view.*;
import org.json.*;

/** Drives the real Activity and Canvas with pressure-bearing input events. */
public class Integration extends Instrumentation {
    Bundle arguments;
    static void copy(java.io.InputStream in,java.io.OutputStream out){try{byte[] b=new byte[8192];int n;while((n=in.read(b))!=-1)out.write(b,0,n);}catch(java.io.IOException e){throw new RuntimeException(e);}}
    @Override public void onCreate(Bundle args){arguments=args;start();}
    @Override public void onStart(){
        android.util.Log.i("NoteMakerPerformance","Instrumentation starting");
        Bundle result=new Bundle();try{
            Intent intent=new Intent(getTargetContext(),MainActivity.class);intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
            MainActivity app=(MainActivity)startActivitySync(intent);android.util.Log.i("NoteMakerPerformance","Activity started");waitForIdleSync();
            String action=arguments.getString("action","state");
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
            runOnMainSync(()->{
                synchronized(app.lock){
                    if(action.equals("configure")){
                        app.save();app.document=arguments.getString("document","integration");app.host=arguments.getString("host","http://10.0.2.2:8765");app.proxyUrl=arguments.getString("proxyUrl","");app.proxyToken=arguments.getString("proxyToken","");app.load(arguments.getString("type","fixed"));
                        app.configureProxy();app.getSharedPreferences("settings",0).edit().putString("host",app.host).putString("document",app.document).putString("proxyUrl",app.proxyUrl).putString("proxyToken",app.proxyToken).commit();
                    }
                    if(action.equals("draw")||action.equals("cli-concurrent")){
                        app.color=Integer.parseInt(arguments.getString("color","-16777216"));app.width=10;app.erase=false;app.pan=false;
                        float shift=Float.parseFloat(arguments.getString("shift","0")),scale=app.canvas.scale();long start=SystemClock.uptimeMillis();
                        for(int i=0;i<12;i++){
                            MotionEvent.PointerProperties property=new MotionEvent.PointerProperties();property.id=0;property.toolType=arguments.getString("tool","stylus").equals("finger")?MotionEvent.TOOL_TYPE_FINGER:MotionEvent.TOOL_TYPE_STYLUS;
                            MotionEvent.PointerCoords coords=new MotionEvent.PointerCoords();coords.x=(100+i*30+shift)*scale;coords.y=(200+i*20+shift)*scale;coords.pressure=.2f+i*.06f;coords.size=1;
                            MotionEvent event=MotionEvent.obtain(start,start+i*20,i==0?MotionEvent.ACTION_DOWN:i==11?MotionEvent.ACTION_UP:MotionEvent.ACTION_MOVE,1,new MotionEvent.PointerProperties[]{property},new MotionEvent.PointerCoords[]{coords},0,0,1,1,0,0,property.toolType==MotionEvent.TOOL_TYPE_FINGER?android.view.InputDevice.SOURCE_TOUCHSCREEN:android.view.InputDevice.SOURCE_STYLUS,0);
                            app.canvas.onTouchEvent(event);event.recycle();
                        }
                    }
                    if(action.equals("layer")){app.selected=arguments.getString("layer","test-layer");app.enqueue(MainActivity.obj("kind","layer","layer",app.selected,"name","Test layer","force",Boolean.parseBoolean(arguments.getString("force","false"))));app.save();}
                    if(action.equals("hide")){app.hidden.add(arguments.getString("layer","test-layer"));app.save();}
                    if(action.equals("undo"))app.history(false);
                    if(action.equals("redo"))app.history(true);
                    if(action.equals("page")){app.page=Integer.parseInt(arguments.getString("page","1"));app.save();}
                    if(action.equals("erase")){app.erase=true;float shift=Float.parseFloat(arguments.getString("shift","0"));app.canvas.eraseAt((200+shift)*app.canvas.scale(),(266+shift)*app.canvas.scale());app.save();}
                    app.update();
                }
            });
            if(child!=null){
                final java.lang.Process process=child;java.io.ByteArrayOutputStream output=new java.io.ByteArrayOutputStream(),errors=new java.io.ByteArrayOutputStream();
                Thread stdout=new Thread(()->copy(process.getInputStream(),output)),stderr=new Thread(()->copy(process.getErrorStream(),errors));stdout.start();stderr.start();
                if(!child.waitFor(30,java.util.concurrent.TimeUnit.SECONDS)){child.destroyForcibly();throw new RuntimeException("CLI timed out");}stdout.join();stderr.join();
                if(child.exitValue()!=0)throw new RuntimeException("CLI failed: "+errors.toString("UTF-8"));
                try(java.io.FileOutputStream out=app.openFileOutput("cli-result.json",0)){out.write(output.toByteArray());}
                result.putString("cli",output.size()<32000?output.toString("UTF-8"):"See files/cli-result.json");
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
}
