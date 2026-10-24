package com.notemaker;

import android.app.AlertDialog;
import android.graphics.*;
import android.os.*;
import android.view.*;
import android.widget.*;
import com.notemaker.core.GifAsset;
import com.notemaker.core.Json;
import java.util.*;

/** Playback belongs to one Activity session; document snapshots contain only GIF bytes. */
final class AnimationPlayer {
    private static final class Clip {
        final String source;final GifAsset.Info info;final Movie movie;int frame;boolean playing;long nextAt;
        Clip(String data){source=data;byte[] bytes=android.util.Base64.decode(data.substring(22),android.util.Base64.DEFAULT);info=GifAsset.validate(bytes);movie=Movie.decodeByteArray(bytes,0,bytes.length);if(movie==null)throw new IllegalArgumentException("Could not decode GIF");}
    }
    private final MainActivity app;
    private final Handler handler=new Handler(Looper.getMainLooper());
    private final Map<String,Clip> clips=new HashMap<>();
    private String selected;
    private AlertDialog dialog;
    private TextView label;
    private SeekBar timeline;
    private Button toggle;
    private final Paint controlPaint=new Paint(Paint.ANTI_ALIAS_FLAG);
    private String pressedId,pressedControl;
    private static final float BAR_HEIGHT=76,ROW_HEIGHT=39;
    private final Runnable tick=new Runnable(){public void run(){long now=SystemClock.uptimeMillis();boolean more=false;for(Map.Entry<String,Clip> entry:clips.entrySet()){Clip clip=entry.getValue();if(!clip.playing)continue;Map<String,Object> object=app.model.objects.get(entry.getKey());if(!available(object)||!clip.source.equals(object.get("gif"))){clip.playing=false;continue;}if(now>=clip.nextAt){clip.frame=(clip.frame+1)%clip.info.delays.length;clip.nextAt=now+clip.info.delays[clip.frame];invalidate(object);}more=true;}refresh();if(more)handler.postDelayed(this,50);}};
    AnimationPlayer(MainActivity app){this.app=app;}
    private boolean available(Map<String,Object> object){return object!=null&&"gif".equals(object.get("shape"))&&((Number)object.get("page")).intValue()==app.page&&!Boolean.FALSE.equals(app.model.objectActive.get(object.get("object")))&&!Boolean.FALSE.equals(app.model.layerActive.get(object.get("layer")));}
    private Clip clip(String id){Map<String,Object> o=app.model.objects.get(id);if(!available(o))throw new IllegalArgumentException("GIF is unavailable on this page");Clip result=clips.get(id);String source=(String)o.get("gif");if(result==null||!result.source.equals(source)){if(result!=null)result.playing=false;result=new Clip(source);clips.put(id,result);schedule();}return result;}
    private void invalidate(Map<String,Object> o){
        if(app.canvas==null||o==null)return;
        float x=((Number)o.get("x")).floatValue(),y=((Number)o.get("y")).floatValue();
        float w=((Number)o.get("w")).floatValue(),h=((Number)o.get("h")).floatValue();
        float scale=app.canvas.scale()*app.canvas.zoom;
        if(scale<=0){app.canvas.invalidate();return;}
        float radius=(float)Math.hypot(w,h)/2+4,cx=x+w/2,cy=y+h/2;
        RectF bar=controls(o);
        float left=Math.min(cx-radius,bar.left)-4,top=Math.min(cy-radius,bar.top)-4;
        float right=Math.max(cx+radius,bar.right)+4,bottom=Math.max(cy+radius,bar.bottom)+4;
        app.canvas.invalidate((int)(app.canvas.offsetX+left*scale),(int)(app.canvas.offsetY+top*scale),
                (int)Math.ceil(app.canvas.offsetX+right*scale),(int)Math.ceil(app.canvas.offsetY+bottom*scale));
    }
    boolean loaded(String id){Clip clip=clips.get(id);Map<String,Object> object=app.model.objects.get(id);return clip!=null&&object!=null&&clip.source.equals(object.get("gif"));}
    void prepareVisible(){for(Map<String,Object> o:app.model.objects.values())if(available(o)&&app.canvas.visible(String.valueOf(o.get("layer"))))clip(String.valueOf(o.get("object")));}
    void select(String id){clip(id);if(!id.equals(selected)){selected=id;invalidate(app.model.objects.get(id));}refresh();}
    private void schedule(){handler.removeCallbacks(tick);for(Clip c:clips.values())if(c.playing){handler.postDelayed(tick,50);break;}}
    void play(boolean value){Clip c=clip(requireSelected());c.playing=value;c.nextAt=SystemClock.uptimeMillis()+c.info.delays[c.frame];schedule();refresh();}
    void step(int delta){Clip c=clip(requireSelected());c.playing=false;c.frame=Math.floorMod(c.frame+delta,c.info.delays.length);schedule();invalidate(app.model.objects.get(selected));refresh();}
    void seek(int frame){Clip c=clip(requireSelected());if(frame<0||frame>=c.info.delays.length)throw new IllegalArgumentException("Frame is out of range");boolean changed=c.frame!=frame;c.frame=frame;c.nextAt=SystemClock.uptimeMillis()+c.info.delays[frame];if(changed)invalidate(app.model.objects.get(selected));refresh();}
    private String requireSelected(){if(selected==null)throw new IllegalArgumentException("Select a GIF first");return selected;}
    Map<String,Object> state(){List<Object> entries=new ArrayList<>();for(Map.Entry<String,Clip> e:clips.entrySet()){Clip c=e.getValue();entries.add(Json.object("id",e.getKey(),"frame",c.frame,"frames",c.info.delays.length,"playing",c.playing,"durationMs",c.info.duration));}return Json.object("selected",selected,"clips",entries);}
    void draw(Canvas canvas,String layer){for(Map.Entry<String,Clip> entry:clips.entrySet()){Clip c=entry.getValue();Map<String,Object> o=app.model.objects.get(entry.getKey());if(!available(o)||!c.source.equals(o.get("gif"))||!layer.equals(o.get("layer")))continue;float x=((Number)o.get("x")).floatValue(),y=((Number)o.get("y")).floatValue(),w=((Number)o.get("w")).floatValue(),h=((Number)o.get("h")).floatValue();canvas.save();float cx=x+w/2,cy=y+h/2;canvas.translate(cx,cy);canvas.rotate(((Number)o.getOrDefault("rotation",0)).floatValue());canvas.scale(Boolean.TRUE.equals(o.get("flipX"))?-1:1,Boolean.TRUE.equals(o.get("flipY"))?-1:1);canvas.translate(-cx,-cy);for(double[][] polygon:com.notemaker.core.DrawItem.erasures(o)){Path path=new Path();for(int i=0;i<polygon.length;i++){if(i==0)path.moveTo((float)polygon[i][0],(float)polygon[i][1]);else path.lineTo((float)polygon[i][0],(float)polygon[i][1]);}path.close();canvas.clipOutPath(path);}canvas.clipRect(x,y,x+w,y+h);canvas.translate(x,y);canvas.scale(w/c.info.width,h/c.info.height);c.movie.setTime(c.info.timeForFrame(c.frame));c.movie.draw(canvas,0,0);canvas.restore();}}
    private RectF controls(Map<String,Object> o){
        float x=((Number)o.get("x")).floatValue(),y=((Number)o.get("y")).floatValue();
        float w=((Number)o.get("w")).floatValue(),h=((Number)o.get("h")).floatValue();
        double angle=Math.toRadians(((Number)o.getOrDefault("rotation",0)).doubleValue());
        float visualW=(float)(Math.abs(w*Math.cos(angle))+Math.abs(h*Math.sin(angle)));
        float visualH=(float)(Math.abs(w*Math.sin(angle))+Math.abs(h*Math.cos(angle)));
        float width=Math.max(220,Math.min(360,Math.max(w,visualW)));
        float left=x+w/2-width/2,top=y+h/2+visualH/2+8;
        if(!app.type.equals("infinite")){
            left=Math.max(0,Math.min(1000-width,left));
            if(top+BAR_HEIGHT>1400)top=Math.max(0,y+h/2-visualH/2-BAR_HEIGHT-8);
        }
        return new RectF(left,top,left+width,top+BAR_HEIGHT);
    }
    private static String controlAt(RectF bar,float x,float y){
        if(x<bar.left||x>bar.right||y<bar.top||y>bar.bottom)return null;
        if(y>bar.top+ROW_HEIGHT)return "timeline";
        float part=(x-bar.left)/bar.width();
        return part<.24f?"previous":part<.58f?"toggle":part<.82f?"next":null;
    }
    private static final class Hit {final String id,control;final RectF bar;Hit(String id,String control,RectF bar){this.id=id;this.control=control;this.bar=bar;}}
    private Hit hit(float x,float y){
        ArrayList<Map<String,Object>> objects=new ArrayList<>(app.model.objects.values());
        for(int i=objects.size()-1;i>=0;i--){Map<String,Object> o=objects.get(i);
            if(!available(o)||!app.canvas.visible(String.valueOf(o.get("layer"))))continue;
            RectF bar=controls(o);String control=controlAt(bar,x,y);
            if(control!=null)return new Hit(String.valueOf(o.get("object")),control,bar);
        }
        return null;
    }
    private void seekAt(String id,RectF bar,float x){select(id);Clip c=clip(id);float fraction=Math.max(0,Math.min(1,(x-bar.left-12)/(bar.width()-24)));seek(Math.round(fraction*(c.info.delays.length-1)));}
    boolean touch(int action,float x,float y){
        if(action==MotionEvent.ACTION_DOWN){Hit hit=hit(x,y);pressedId=hit==null?null:hit.id;pressedControl=hit==null?null:hit.control;
            if(hit!=null&&"timeline".equals(hit.control))seekAt(hit.id,hit.bar,x);return true;}
        if(action==MotionEvent.ACTION_MOVE){if(pressedId!=null&&"timeline".equals(pressedControl)){Map<String,Object> o=app.model.objects.get(pressedId);if(available(o))seekAt(pressedId,controls(o),x);}return true;}
        if(action==MotionEvent.ACTION_UP){String id=pressedId,control=pressedControl;pressedId=pressedControl=null;
            if(id==null)return true;Map<String,Object> o=app.model.objects.get(id);if(!available(o))return true;
            if("timeline".equals(control)){seekAt(id,controls(o),x);return true;}
            Hit hit=hit(x,y);if(hit==null||!id.equals(hit.id)||!control.equals(hit.control))return true;
            select(id);if("previous".equals(control))step(-1);else if("next".equals(control))step(1);else play(!clip(id).playing);return true;}
        if(action==MotionEvent.ACTION_CANCEL){pressedId=pressedControl=null;return true;}
        return true;
    }
    private void centered(Canvas canvas,String value,float left,float right,float baseline){controlPaint.setStyle(Paint.Style.FILL);controlPaint.setColor(Color.BLACK);controlPaint.setTextSize(17);canvas.drawText(value,(left+right-controlPaint.measureText(value))/2,baseline,controlPaint);}
    void drawControls(Canvas canvas){
        for(Map<String,Object> o:app.model.objects.values()){
            if(!available(o)||!app.canvas.visible(String.valueOf(o.get("layer"))))continue;
            String id=String.valueOf(o.get("object"));Clip clip=clips.get(id);if(clip==null)continue;
            RectF bar=controls(o);float unit=bar.width();
            controlPaint.setStyle(Paint.Style.FILL);controlPaint.setColor(Color.WHITE);canvas.drawRect(bar,controlPaint);
            controlPaint.setStyle(Paint.Style.STROKE);controlPaint.setStrokeWidth(1.5f);controlPaint.setColor(Color.BLACK);
            canvas.drawRect(bar,controlPaint);canvas.drawLine(bar.left,bar.top+ROW_HEIGHT,bar.right,bar.top+ROW_HEIGHT,controlPaint);
            for(float fraction:new float[]{.24f,.58f,.82f})canvas.drawLine(bar.left+unit*fraction,bar.top,bar.left+unit*fraction,bar.top+ROW_HEIGHT,controlPaint);
            centered(canvas,"Prev",bar.left,bar.left+unit*.24f,bar.top+26);
            centered(canvas,clip.playing?"Pause":"Play",bar.left+unit*.24f,bar.left+unit*.58f,bar.top+26);
            centered(canvas,"Next",bar.left+unit*.58f,bar.left+unit*.82f,bar.top+26);
            centered(canvas,(clip.frame+1)+"/"+clip.info.delays.length,bar.left+unit*.82f,bar.right,bar.top+26);
            float trackLeft=bar.left+12,trackRight=bar.right-12,trackY=bar.top+58;
            controlPaint.setStyle(Paint.Style.STROKE);controlPaint.setStrokeWidth(2);canvas.drawLine(trackLeft,trackY,trackRight,trackY,controlPaint);
            if(clip.info.delays.length<=20)for(int i=0;i<clip.info.delays.length;i++){float tx=trackLeft+(trackRight-trackLeft)*i/Math.max(1,clip.info.delays.length-1);canvas.drawLine(tx,trackY-5,tx,trackY+5,controlPaint);}
            float knob=trackLeft+(trackRight-trackLeft)*clip.frame/Math.max(1,clip.info.delays.length-1);
            controlPaint.setStyle(Paint.Style.FILL);canvas.drawCircle(knob,trackY,6,controlPaint);
        }
    }
    void show(){ArrayList<String> ids=new ArrayList<>(),names=new ArrayList<>();for(Map<String,Object> o:app.model.objects.values())if(available(o)){ids.add((String)o.get("object"));String name=(String)o.getOrDefault("text","");names.add((name.isEmpty()?"GIF":name)+" · "+ids.get(ids.size()-1));}if(ids.isEmpty()){Toast.makeText(app,"No GIF animations on this page",Toast.LENGTH_SHORT).show();return;}if(selected==null||!ids.contains(selected))selected=ids.get(0);String[] choices=names.toArray(new String[0]);new AlertDialog.Builder(app).setTitle("Choose animation").setItems(choices,(d,which)->showControls(ids.get(which))).setNegativeButton("Close",null).show();}
    private void showControls(String id){select(id);LinearLayout box=new LinearLayout(app);box.setOrientation(LinearLayout.VERTICAL);box.setPadding(24,12,24,12);label=new TextView(app);box.addView(label);timeline=new SeekBar(app);timeline.setMax(clip(id).info.delays.length-1);timeline.setOnSeekBarChangeListener(new SeekBar.OnSeekBarChangeListener(){public void onProgressChanged(SeekBar bar,int value,boolean user){if(user)seek(value);}public void onStartTrackingTouch(SeekBar bar){}public void onStopTrackingTouch(SeekBar bar){}});box.addView(timeline);LinearLayout row=new LinearLayout(app);Button previous=new Button(app);previous.setText("◀ Frame");previous.setOnClickListener(v->step(-1));row.addView(previous);toggle=new Button(app);toggle.setOnClickListener(v->play(!clip(requireSelected()).playing));row.addView(toggle);Button next=new Button(app);next.setText("Frame ▶");next.setOnClickListener(v->step(1));row.addView(next);box.addView(row);dialog=new AlertDialog.Builder(app).setTitle("GIF player").setView(box).setNegativeButton("Close",null).create();dialog.setOnDismissListener(d->{dialog=null;label=null;timeline=null;toggle=null;});refresh();dialog.show();}
    private void refresh(){if(label==null||selected==null)return;Clip c=clips.get(selected);if(c==null)return;label.setText("Frame "+(c.frame+1)+" / "+c.info.delays.length+" · "+(c.playing?"Playing":"Paused"));timeline.setProgress(c.frame);toggle.setText(c.playing?"Pause":"Play");}
    void pauseAll(){handler.removeCallbacks(tick);for(Clip c:clips.values())c.playing=false;refresh();}
    void stop(){pauseAll();if(dialog!=null)dialog.dismiss();}
    void reset(){stop();clips.clear();selected=null;}
}
