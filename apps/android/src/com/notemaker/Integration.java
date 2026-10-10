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
    @Override public void onCreate(Bundle args){arguments=args;start();}
    @Override public void onStart(){
        Bundle result=new Bundle();try{
            Intent intent=new Intent(getTargetContext(),MainActivity.class);intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
            MainActivity app=(MainActivity)startActivitySync(intent);waitForIdleSync();
            String action=arguments.getString("action","state");
            runOnMainSync(()->{
                synchronized(app.lock){
                    if(action.equals("configure")){
                        app.save();app.document=arguments.getString("document","integration");app.host=arguments.getString("host","http://10.0.2.2:8765");app.load(arguments.getString("type","fixed"));
                        app.getSharedPreferences("settings",0).edit().putString("host",app.host).putString("document",app.document).commit();
                    }
                    if(action.equals("draw")){
                        app.color=Integer.parseInt(arguments.getString("color","-16777216"));app.width=10;app.erase=false;app.pan=false;
                        float shift=Float.parseFloat(arguments.getString("shift","0")),scale=app.canvas.scale();long start=SystemClock.uptimeMillis();
                        for(int i=0;i<12;i++){
                            MotionEvent.PointerProperties property=new MotionEvent.PointerProperties();property.id=0;property.toolType=arguments.getString("tool","stylus").equals("finger")?MotionEvent.TOOL_TYPE_FINGER:MotionEvent.TOOL_TYPE_STYLUS;
                            MotionEvent.PointerCoords coords=new MotionEvent.PointerCoords();coords.x=(100+i*30+shift)*scale;coords.y=(200+i*20+shift)*scale;coords.pressure=.2f+i*.06f;coords.size=1;
                            MotionEvent event=MotionEvent.obtain(start,start+i*20,i==0?MotionEvent.ACTION_DOWN:i==11?MotionEvent.ACTION_UP:MotionEvent.ACTION_MOVE,1,new MotionEvent.PointerProperties[]{property},new MotionEvent.PointerCoords[]{coords},0,0,1,1,0,0,property.toolType==MotionEvent.TOOL_TYPE_FINGER?android.view.InputDevice.SOURCE_TOUCHSCREEN:android.view.InputDevice.SOURCE_STYLUS,0);
                            app.canvas.onTouchEvent(event);event.recycle();
                        }
                    }
                    if(action.equals("layer")){app.selected=arguments.getString("layer","test-layer");app.enqueue(MainActivity.obj("kind","layer","layer",app.selected,"name","Test layer","force",Boolean.parseBoolean(arguments.getString("force","false"))));}
                    if(action.equals("hide")){app.hidden.add(arguments.getString("layer","test-layer"));app.save();}
                    if(action.equals("undo"))app.history(false);
                    if(action.equals("redo"))app.history(true);
                    if(action.equals("page")){app.page=Integer.parseInt(arguments.getString("page","1"));app.save();}
                    if(action.equals("erase")){app.erase=true;float shift=Float.parseFloat(arguments.getString("shift","0"));app.canvas.eraseAt((200+shift)*app.canvas.scale(),(266+shift)*app.canvas.scale());app.save();}
                    app.update();
                }
            });
            Thread.sleep(800);
            runOnMainSync(()->{synchronized(app.lock){
                try{
                    JSONObject state=MainActivity.obj("document",app.document,"type",app.type,"cursor",app.cursor,"pending",app.pending.length(),"strokes",app.strokes.size(),"events",app.events,"visible",app.canvas.visible("test-layer"),"page",app.page);
                    Bitmap image=Bitmap.createBitmap(1000,1400,Bitmap.Config.ARGB_8888);Canvas c=new Canvas(image);c.drawColor(Color.WHITE);
                    for(String layer:app.layers.keySet())if(app.canvas.visible(layer))for(java.util.Map.Entry<String,JSONObject> entry:app.strokes.entrySet())if(entry.getValue().optString("layer").equals(layer)&&entry.getValue().optInt("page")==app.page&&!Boolean.FALSE.equals(app.active.get(entry.getKey())))app.canvas.drawStroke(c,entry.getValue());
                    try(java.io.FileOutputStream out=app.openFileOutput("integration.png",0)){image.compress(Bitmap.CompressFormat.PNG,100,out);}
                    try(java.io.FileOutputStream out=app.openFileOutput("integration.json",0)){out.write(state.toString().getBytes("UTF-8"));}
                    int[] pixels=new int[1000*1400];image.getPixels(pixels,0,1000,0,0,1000,1400);int count=0,minX=1000,minY=1400,maxX=-1,maxY=-1;for(int i=0;i<pixels.length;i++){int pixel=pixels[i];if(Color.red(pixel)<245||Color.green(pixel)<245||Color.blue(pixel)<245){count++;int x=i%1000,y=i/1000;minX=Math.min(minX,x);minY=Math.min(minY,y);maxX=Math.max(maxX,x);maxY=Math.max(maxY,y);}}JSONObject stats=MainActivity.obj("pixels",count,"bounds",new JSONArray(java.util.Arrays.asList(minX,minY,maxX,maxY)));try(java.io.FileOutputStream out=app.openFileOutput("render-stats.json",0)){out.write(stats.toString().getBytes("UTF-8"));}result.putString("state",state.toString());
                }catch(Exception e){throw new RuntimeException(e);}
            }});
            result.putString("stream","PASS "+action+"\n");finish(Activity.RESULT_OK,result);
        }catch(Throwable error){result.putString("stream","FAIL: "+error+"\n");finish(Activity.RESULT_CANCELED,result);}
    }
}
