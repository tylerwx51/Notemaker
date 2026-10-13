package com.notemaker;

import android.app.*;
import android.os.*;
import android.content.*;
import android.graphics.*;
import android.view.*;
import android.widget.*;
import org.json.*;
import java.io.*;
import java.net.*;
import java.nio.charset.StandardCharsets;
import java.util.*;
import com.notemaker.core.*;

public class MainActivity extends Activity {
    final Object lock = new Object();
    int eventCount, pendingCount;
    final LinkedHashMap<String,JSONObject> optimistic=new LinkedHashMap<>();
    int saving;
    String saveError;
    final Map<String,Map<String,Object>> renderedEntities=new HashMap<>();
    LinkedHashMap<String, JSONObject> layers = new LinkedHashMap<>(), strokes = new LinkedHashMap<>();
    HashMap<String, Boolean> active = new HashMap<>();
    HashSet<String> hidden = new HashSet<>();
    ArrayList<String> undo = new ArrayList<>(), redo = new ArrayList<>();
    String document="notes", host="", type="fixed", selected="base";
    long cursor=0;
    int page=0, color=Color.BLACK;
    float width=5;
    boolean erase=false, pan=false, connected=false, documentSeen=false;
    volatile boolean running=true;
    CanvasView canvas;
    EditorUi editor;
    AgentUi agent;
    boolean displayedConnected;
    Thread syncThread, localThread;
    Store store;
    Model model=new Model();
    long revision=-1;

    static JSONObject obj(Object... args) {
        JSONObject o=new JSONObject(); try {for(int i=0;i<args.length;i+=2)o.put((String)args[i],args[i+1]);}catch(Exception e){throw new RuntimeException(e);}return o;
    }
    static String id(){return UUID.randomUUID().toString();}
    @Override public void onCreate(Bundle b){
        super.onCreate(b);
        android.content.SharedPreferences prefs=getSharedPreferences("settings",0);
        host=prefs.getString("host",host);document=prefs.getString("document",document);
        load("fixed");
        agent=new AgentUi(this);
        editor=new EditorUi(this);
        View root=editor.build();
        setContentView(root);
        if(Build.VERSION.SDK_INT>=30)root.post(()->{WindowInsetsController controller=getWindow().getInsetsController();if(controller!=null){int appearance=WindowInsetsController.APPEARANCE_LIGHT_STATUS_BARS|WindowInsetsController.APPEARANCE_LIGHT_NAVIGATION_BARS;controller.setSystemBarsAppearance(appearance,appearance);}});
        else getWindow().getDecorView().setSystemUiVisibility(View.SYSTEM_UI_FLAG_LIGHT_STATUS_BAR|View.SYSTEM_UI_FLAG_LIGHT_NAVIGATION_BAR);
        update();
        configureProxy();
        localThread=new Thread(this::localLoop,"local-state");localThread.start();
    }
    @Override public void onDestroy(){running=false;if(agent!=null)agent.destroy();if(editor!=null)editor.dismissPopup();if(syncThread!=null)syncThread.interrupt();if(localThread!=null)localThread.interrupt();super.onDestroy();}
    File stateFile(){return store.file;}
    void configureProxy(){startService(new Intent(this,ReplicaService.class).putExtra("document",document).putExtra("host",host));}
    interface Work {void run(Store target) throws Exception;}
    void persist(Work work,String optimisticId){
        final Store target=store;saving++;
        ReplicaService.writes.execute(()->{
            try{work.run(target);Map<String,Object> snapshot=target.read();runOnUiThread(()->{
                saving--;if(target!=store)return;if(optimisticId!=null)optimistic.remove(optimisticId);
                try{refreshShared(snapshot);update();}catch(Exception e){saveError=e.toString();update();}
            });}catch(Exception e){android.util.Log.e("NoteMaker","Local save failed",e);runOnUiThread(()->{saving--;saveError=e.toString();update();Toast.makeText(this,"Could not save changes: "+e.getMessage(),Toast.LENGTH_LONG).show();});}
        });
    }
    void save(){
        final Map<String,Object> preferences=Json.object("hidden",new ArrayList<>(hidden),"undo",new ArrayList<>(undo),"redo",new ArrayList<>(redo),"selected",selected,"page",page);
        persist(target->target.edit(state->state.putAll(preferences)),null);
    }
    void refreshShared(Map<String,Object> state) throws Exception {
        long next=((Number)state.get("revision")).longValue();if(next<revision)return;
        revision=next;eventCount=Json.list(state.get("events")).size();pendingCount=Json.list(state.get("pending")).size();cursor=((Number)state.get("cursor")).longValue();
        model=Store.model(state);type=model.type;documentSeen=eventCount+pendingCount>0;
        LinkedHashMap<String,JSONObject> nextLayers=new LinkedHashMap<>(),nextStrokes=new LinkedHashMap<>();
        for(Map.Entry<String,Map<String,Object>> entry:model.layers.entrySet())nextLayers.put(entry.getKey(),entity("layer:",entry,layers));
        for(Map.Entry<String,Map<String,Object>> entry:model.strokes.entrySet())nextStrokes.put(entry.getKey(),entity("stroke:",entry,strokes));
        layers=nextLayers;strokes=nextStrokes;active.clear();active.putAll(model.active);
        for(JSONObject op:optimistic.values())apply(op);
        if(!layers.containsKey(selected)||Boolean.FALSE.equals(model.layerActive.get(selected)))selected="base";
    }
    JSONObject entity(String prefix,Map.Entry<String,Map<String,Object>> entry,Map<String,JSONObject> previous){
        Map<String,Object> old=renderedEntities.put(prefix+entry.getKey(),entry.getValue());JSONObject existing=previous.get(entry.getKey());
        return existing!=null&&(old==entry.getValue()||entry.getValue().equals(old))?existing:new JSONObject(entry.getValue());
    }
    void load(String newType){synchronized(lock){
        eventCount=pendingCount=0;optimistic.clear();renderedEntities.clear();cursor=0;revision=-1;hidden.clear();undo.clear();redo.clear();selected="base";page=0;type=newType;
        try{
            store=new Store(getFilesDir(),document);Map<String,Object> state=store.read();JSONObject saved=new JSONObject(state);
            JSONArray h=saved.optJSONArray("hidden");if(h!=null)for(int i=0;i<h.length();i++)hidden.add(h.getString(i));
            h=saved.optJSONArray("undo");if(h!=null)for(int i=0;i<h.length();i++)undo.add(h.getString(i));
            h=saved.optJSONArray("redo");if(h!=null)for(int i=0;i<h.length();i++)redo.add(h.getString(i));
            selected=saved.optString("selected","base");page=saved.optInt("page",0);refreshShared(state);
            if(eventCount==0&&pendingCount==0)enqueue(obj("kind","document","type",newType));
        }catch(Exception e){throw new RuntimeException("Cannot read saved document",e);}
    }}
    void apply(JSONObject o){String kind=o.optString("kind");
        if(kind.equals("batch")){JSONArray a=o.optJSONArray("actions");for(int i=0;i<a.length();i++)apply(a.optJSONObject(i));return;}
        model.apply(Json.map(Json.parse(o.toString())));
        if(kind.equals("document")&&!documentSeen){type=o.optString("type","fixed");documentSeen=true;}
        if(kind.equals("layer"))layers.put(o.optString("layer"),o);
        if(kind.equals("stroke"))strokes.put(o.optString("stroke"),o);
        if(kind.equals("active"))active.put(o.optString("stroke"),o.optBoolean("active"));
    }
    void enqueue(JSONObject o){
        String token=id();optimistic.put(token,o);apply(o);
        persist(target->target.append(Json.map(Json.parse(o.toString())),"human"),token);
    }
    void update(){synchronized(lock){
        if(editor==null)return;
        displayedConnected=connected;
        editor.refresh();
        canvas.invalidate();
    }}
    void documentDialog(){
        LinearLayout fields=new LinearLayout(this);fields.setOrientation(1);fields.setPadding(editor.dp(24),editor.dp(8),editor.dp(24),editor.dp(8));
        EditText server=new EditText(this);server.setSingleLine();server.setText(host);server.setHint("Server URL (optional), e.g. http://192.168.50.180:8765");fields.addView(server);
        EditText name=new EditText(this);name.setSingleLine();name.setText(document);name.setHint("Document name (same on both clients)");fields.addView(name);
        Spinner types=new Spinner(this);types.setAdapter(new ArrayAdapter<String>(this,android.R.layout.simple_spinner_dropdown_item,new String[]{"Fixed page","Multiple pages","Infinite canvas"}));types.setSelection(type.equals("pages")?1:type.equals("infinite")?2:0);fields.addView(types);
        new AlertDialog.Builder(this).setTitle("Open or create document").setMessage("Type is used when creating a new document. Existing documents retain their type.").setView(fields).setPositiveButton("Open",(d,w)->{
            String next=name.getText().toString().trim(),url=server.getText().toString().trim().replaceAll("/+$","");
            if(next.isEmpty()||!(url.isEmpty()||url.startsWith("http://")||url.startsWith("https://"))){Toast.makeText(this,"Enter a document name and an optional HTTP(S) URL",1).show();return;}
            synchronized(lock){save();document=next;host=url;connected=false;load(new String[]{"fixed","pages","infinite"}[types.getSelectedItemPosition()]);canvas.zoom=1;canvas.offsetX=canvas.offsetY=0;}
            getSharedPreferences("settings",0).edit().putString("host",host).putString("document",document).apply();configureProxy();update();
        }).setNegativeButton("Cancel",null).show();
    }
    void pageDialog(){if(!type.equals("pages"))return;EditText field=new EditText(this);field.setInputType(2);field.setText(""+(page+1));new AlertDialog.Builder(this).setTitle("Go to / create page").setView(field).setPositiveButton("Open",(d,w)->{try{page=Math.max(0,Math.min(10000,Integer.parseInt(field.getText().toString())-1));save();update();}catch(Exception ignored){}}).show();}
    void history(boolean forward){synchronized(lock){ArrayList<String> from=forward?redo:undo,to=forward?undo:redo;if(from.isEmpty())return;String entry=from.remove(from.size()-1);to.add(entry);if(entry.equals("object-action")){persist(target->target.history("human",forward),null);save();update();return;}boolean erased=entry.startsWith("erase:");String stroke=erased?entry.substring(6):entry;enqueue(obj("kind","active","stroke",stroke,"active",erased?!forward:forward));save();update();}}
    void localLoop(){while(running){try{
        final Store target=store;final Map<String,Object> snapshot=target.read();
        runOnUiThread(()->{if(target!=store)return;try{boolean wasConnected=connected;connected=ReplicaService.proxy!=null&&Boolean.TRUE.equals(ReplicaService.proxy.connected.get(document));if(((Number)snapshot.get("revision")).longValue()>revision){refreshShared(snapshot);update();}else if(wasConnected!=connected)update();}catch(Exception e){android.util.Log.e("NoteMaker","Refresh failed",e);}});
        Thread.sleep(100);
    }catch(InterruptedException e){return;}catch(Exception e){android.util.Log.e("NoteMaker","Local state reload failed",e);try{Thread.sleep(1000);}catch(InterruptedException stop){return;}}}}

    /** Launches the compiled CLI under this app's UID, using the installed APK as classpath. */
    public java.lang.Process startCli(String... arguments) throws IOException {
        ArrayList<String> command=new ArrayList<>(Arrays.asList("/system/bin/app_process","/system/bin","com.notemaker.cli.Main","--proxy-file",new File(getFilesDir(),"proxy.json").getAbsolutePath(),"--document",document));
        command.addAll(Arrays.asList(arguments));ProcessBuilder builder=new ProcessBuilder(command);builder.environment().put("CLASSPATH",getApplicationInfo().sourceDir);return builder.start();
    }
    void objectDialog(){synchronized(lock){
        ArrayList<String> ids=new ArrayList<>(model.objects.keySet());String[] labels=new String[ids.size()];for(int i=0;i<ids.size();i++){Map<String,Object> o=model.objects.get(ids.get(i));labels[i]=o.get("shape")+" · "+o.getOrDefault("text",ids.get(i));}
        new AlertDialog.Builder(this).setTitle("Edit diagram object").setItems(labels,(d,index)->editObject(ids.get(index))).setNegativeButton("Close",null).show();
    }}
    void editObject(String id){synchronized(lock){
        Map<String,Object> original=model.objects.get(id);if(original==null)return;
        LinearLayout form=new LinearLayout(this);form.setOrientation(1);form.setPadding(24,12,24,12);Map<String,EditText> fields=new LinkedHashMap<>();
        boolean svg="svg".equals(original.get("shape"));
        String[] keys=svg?new String[]{"x","y","w","h"}:new String[]{"text","x","y","w","h","fontSize"};
        String[] labels=svg?new String[]{"X position","Y position","Width","Height"}:new String[]{"Label","X position","Y position","Width","Height","Font size"};
        for(int index=0;index<keys.length;index++){String key=keys[index];TextView label=new TextView(this);label.setText(labels[index]);form.addView(label);EditText field=new EditText(this);field.setText(String.valueOf(original.getOrDefault(key,key.equals("text")?"":24)));if(!key.equals("text"))field.setInputType(8194|4096);form.addView(field);fields.put(key,field);}
        new AlertDialog.Builder(this).setTitle("Edit object").setView(form).setPositiveButton("Save",(d,w)->{
            try{synchronized(lock){Map<String,Object> patch=Json.object();for(Map.Entry<String,EditText> f:fields.entrySet())patch.put(f.getKey(),f.getKey().equals("text")?f.getValue().getText().toString():Double.parseDouble(f.getValue().getText().toString()));
                persist(target->target.commit("human",m->{Map<String,Object> next=Json.map(Json.copy(m.objects.get(id)));next.putAll(patch);return Arrays.asList(next);}),null);undo.add("object-action");redo.clear();save();update();}
            }catch(Exception e){Toast.makeText(this,e.getMessage(),Toast.LENGTH_LONG).show();}
        }).setNeutralButton(Boolean.FALSE.equals(model.objectActive.get(id))?"Restore":"Erase",(d,w)->{
            try{synchronized(lock){persist(target->target.commit("human",m->Arrays.asList(Json.object("kind","object-active","object",id,"active",Boolean.FALSE.equals(m.objectActive.get(id))))),null);undo.add("object-action");redo.clear();save();update();}}catch(Exception e){Toast.makeText(this,e.getMessage(),1).show();}
        }).setNegativeButton("Cancel",null).show();
    }}

    class CanvasView extends View {
        Paint paint=new Paint(Paint.ANTI_ALIAS_FLAG);
        float zoom=1,offsetX=0,offsetY=0,lastX,lastY;
        JSONArray points;String current;long lastSent;
        ScaleGestureDetector scaling;
        CanvasView(Context context){super(context);setBackgroundColor(Color.WHITE);scaling=new ScaleGestureDetector(context,new ScaleGestureDetector.SimpleOnScaleGestureListener(){@Override public boolean onScale(ScaleGestureDetector d){if(!pan)return false;float old=zoom;zoom=Math.max(.2f,Math.min(5,zoom*d.getScaleFactor()));offsetX=d.getFocusX()-(d.getFocusX()-offsetX)*zoom/old;offsetY=d.getFocusY()-(d.getFocusY()-offsetY)*zoom/old;invalidate();return true;}});}
        float scale(){return Math.min(getWidth()/1000f,getHeight()/1400f);}
        float x(float x){return (x-offsetX)/(scale()*zoom);}float y(float y){return (y-offsetY)/(scale()*zoom);}
        boolean visible(String layer){JSONObject l=layers.get(layer);return l!=null&&!Boolean.FALSE.equals(model.layerActive.get(layer))&&(l.optBoolean("force")||!hidden.contains(layer));}
        void drawStroke(Canvas c,JSONObject stroke){JSONArray ps=stroke.optJSONArray("points");paint.setColor(stroke.optInt("color"));paint.setStrokeCap(Paint.Cap.ROUND);float px=0,py=0,pw=0;for(int i=0;i<ps.length();i++){JSONArray p=ps.optJSONArray(i);float x=(float)p.optDouble(0),y=(float)p.optDouble(1),w=(float)(stroke.optDouble("width")*(.25+.75*p.optDouble(2)));if(i>0){paint.setStrokeWidth((pw+w)/2);c.drawLine(px,py,x,y,paint);}c.drawCircle(x,y,w/2,paint);px=x;py=y;pw=w;}}
        // Cache settled ink at display resolution; extending a page draws only the new strokes.
        final LinkedHashMap<String,InkCache> ink=new LinkedHashMap<String,InkCache>(4,.75f,true);
        String inkTransform="";
        long inputStarted;
        java.util.function.LongConsumer drawLatency;
        boolean useInkCache=true;
        void transform(Canvas c){c.translate(offsetX,offsetY);c.scale(scale()*zoom,scale()*zoom);if(!type.equals("infinite"))c.clipRect(0,0,1000,1400);}
        void settledInk(Canvas c,String layer){
            InkCache cache=ink.get(layer);if(cache==null){if(ink.size()>=3){String oldest=ink.keySet().iterator().next();ink.remove(oldest).bitmap.recycle();}cache=new InkCache(getWidth(),getHeight());ink.put(layer,cache);}
            LinkedHashMap<String,JSONObject> visible=new LinkedHashMap<>();
            for(Map.Entry<String,JSONObject> e:strokes.entrySet()){JSONObject stroke=e.getValue();if(!e.getKey().equals(current)&&layer.equals(stroke.optString("layer"))&&stroke.optInt("page")==page&&!Boolean.FALSE.equals(active.get(e.getKey())))visible.put(e.getKey(),stroke);}
            cache.draw(c,visible,this::transform,this::drawStroke);
        }
        @Override protected void onDraw(Canvas c){super.onDraw(c);synchronized(lock){
            if(getWidth()==0||getHeight()==0)return;
            String key=document+"/"+page+"/"+type+"/"+getWidth()+"/"+getHeight()+"/"+zoom+"/"+offsetX+"/"+offsetY;
            if(!key.equals(inkTransform)){for(InkCache item:ink.values())item.bitmap.recycle();ink.clear();inkTransform=key;}
            c.save();transform(c);if(!type.equals("infinite")){paint.setColor(0xffaaaaaa);paint.setStyle(Paint.Style.STROKE);paint.setStrokeWidth(1);c.drawRect(0,0,1000,1400,paint);paint.setStyle(Paint.Style.FILL);}c.restore();
            for(String layer:layers.keySet())if(visible(layer)){
                if(useInkCache)settledInk(c,layer);
                else {c.save();transform(c);for(Map.Entry<String,JSONObject> e:strokes.entrySet()){JSONObject s=e.getValue();if(!e.getKey().equals(current)&&layer.equals(s.optString("layer"))&&s.optInt("page")==page&&!Boolean.FALSE.equals(active.get(e.getKey())))drawStroke(c,s);}c.restore();}
                c.save();transform(c);
                for(Map.Entry<String,Map<String,Object>> e:model.objects.entrySet()){Map<String,Object> o=e.getValue();if(layer.equals(o.get("layer"))&&((Number)o.get("page")).intValue()==page&&!Boolean.FALSE.equals(model.objectActive.get(e.getKey())))Render.drawObject(model,o,new com.notemaker.cli.Png(c));}
                if(layer.equals(selected)&&points!=null&&current!=null)drawStroke(c,stroke());c.restore();
            }
            if(inputStarted!=0){if(drawLatency!=null)drawLatency.accept(System.nanoTime()-inputStarted);inputStarted=0;}
        }}
        JSONObject stroke(){return obj("kind","stroke","stroke",current,"layer",selected,"page",page,"color",color,"width",width,"points",points);}
        void emit(){if(points!=null&&points.length()>0){try{enqueue(new JSONObject(stroke().toString()));}catch(Exception e){throw new RuntimeException(e);}lastSent=System.currentTimeMillis();}}
        void point(float sx,float sy,float pressure){float dx=x(sx),dy=y(sy);if(!type.equals("infinite")&&(dx<0||dy<0||dx>1000||dy>1400))return;if(points.length()>=8000){JSONArray reduced=new JSONArray();for(int i=0;i<points.length();i+=2)reduced.put(points.optJSONArray(i));points=reduced;}points.put(new JSONArray(Arrays.asList(dx,dy,Math.max(0,Math.min(1,pressure)))));}
        void eraseAt(float sx,float sy){float dx=x(sx),dy=y(sy);for(Map.Entry<String,JSONObject> e:new ArrayList<>(strokes.entrySet())){JSONObject s=e.getValue();if(!s.optString("layer").equals(selected)||s.optInt("page")!=page||!visible(selected)||Boolean.FALSE.equals(active.get(e.getKey())))continue;JSONArray ps=s.optJSONArray("points");for(int i=0;i<ps.length();i++){JSONArray p=ps.optJSONArray(i);double distance=Math.hypot(p.optDouble(0)-dx,p.optDouble(1)-dy);if(i>0){JSONArray a=ps.optJSONArray(i-1);double ax=a.optDouble(0),ay=a.optDouble(1),vx=p.optDouble(0)-ax,vy=p.optDouble(1)-ay,len=vx*vx+vy*vy,t=len==0?0:Math.max(0,Math.min(1,((dx-ax)*vx+(dy-ay)*vy)/len));distance=Math.min(distance,Math.hypot(dx-ax-t*vx,dy-ay-t*vy));}if(distance<18+s.optDouble("width")/2){String sid=e.getKey();enqueue(obj("kind","active","stroke",sid,"active",false));undo.add("erase:"+sid);redo.clear();save();break;}}}}
        @Override public boolean onTouchEvent(MotionEvent event){if(inputStarted==0)inputStarted=System.nanoTime();synchronized(lock){scaling.onTouchEvent(event);int action=event.getActionMasked();float sx=event.getX(),sy=event.getY();if(pan){if(action==MotionEvent.ACTION_DOWN){lastX=sx;lastY=sy;}if(action==MotionEvent.ACTION_MOVE&&!scaling.isInProgress()&&event.getPointerCount()==1){offsetX+=sx-lastX;offsetY+=sy-lastY;invalidate();}lastX=sx;lastY=sy;return true;}
            if(event.getPointerCount()>1)return true;
            if(action==MotionEvent.ACTION_DOWN){if(!visible(selected)){Toast.makeText(MainActivity.this,"Show the selected layer to draw on it",0).show();return true;}if(erase){eraseAt(sx,sy);}else{current=id();points=new JSONArray();point(sx,sy,event.getPressure());emit();}}
            if(action==MotionEvent.ACTION_MOVE){if(erase){eraseAt(sx,sy);}else if(points!=null){for(int i=0;i<event.getHistorySize();i++)point(event.getHistoricalX(i),event.getHistoricalY(i),event.getHistoricalPressure(i));point(sx,sy,event.getPressure());if(System.currentTimeMillis()-lastSent>100)emit();}}
            if(action==MotionEvent.ACTION_UP||action==MotionEvent.ACTION_CANCEL){if(points!=null){point(sx,sy,event.getPressure());emit();undo.add(current);redo.clear();points=null;current=null;save();}}
            canvas.invalidate();return true;
        }}
    }
}
