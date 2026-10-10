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

public class MainActivity extends Activity {
    final Object lock = new Object();
    JSONArray events = new JSONArray(), pending = new JSONArray();
    LinkedHashMap<String, JSONObject> layers = new LinkedHashMap<>(), strokes = new LinkedHashMap<>();
    HashMap<String, Boolean> active = new HashMap<>();
    HashSet<String> hidden = new HashSet<>();
    ArrayList<String> undo = new ArrayList<>(), redo = new ArrayList<>();
    String document="notes", host="http://10.0.2.2:8765", type="fixed", selected="base";
    long cursor=0;
    int page=0, color=Color.BLACK;
    float width=5;
    boolean erase=false, pan=false, connected=false, documentSeen=false;
    volatile boolean running=true;
    CanvasView canvas;
    EditorUi editor;
    boolean displayedConnected;
    Thread syncThread;

    static JSONObject obj(Object... args) {
        JSONObject o=new JSONObject(); try {for(int i=0;i<args.length;i+=2)o.put((String)args[i],args[i+1]);}catch(Exception e){throw new RuntimeException(e);}return o;
    }
    static String id(){return UUID.randomUUID().toString();}
    @Override public void onCreate(Bundle b){
        super.onCreate(b);
        android.content.SharedPreferences prefs=getSharedPreferences("settings",0);
        host=prefs.getString("host",host);document=prefs.getString("document",document);
        load("fixed");
        editor=new EditorUi(this);
        View root=editor.build();
        setContentView(root);
        if(Build.VERSION.SDK_INT>=30)root.post(()->{WindowInsetsController controller=getWindow().getInsetsController();if(controller!=null){int appearance=WindowInsetsController.APPEARANCE_LIGHT_STATUS_BARS|WindowInsetsController.APPEARANCE_LIGHT_NAVIGATION_BARS;controller.setSystemBarsAppearance(appearance,appearance);}});
        else getWindow().getDecorView().setSystemUiVisibility(View.SYSTEM_UI_FLAG_LIGHT_STATUS_BAR|View.SYSTEM_UI_FLAG_LIGHT_NAVIGATION_BAR);
        update();
        syncThread=new Thread(this::syncLoop,"document-sync");syncThread.start();
    }
    @Override public void onDestroy(){running=false;if(editor!=null)editor.dismissPopup();if(syncThread!=null)syncThread.interrupt();super.onDestroy();}
    File stateFile(){return new File(getFilesDir(),"document-"+UUID.nameUUIDFromBytes(document.getBytes(StandardCharsets.UTF_8))+".json");}
    void save(){
        synchronized(lock){try{
            JSONObject state=obj("events",events,"pending",pending,"cursor",cursor,"hidden",new JSONArray(hidden),"undo",new JSONArray(undo),"redo",new JSONArray(redo),"selected",selected,"page",page);
            File target=stateFile(),tmp=new File(target.getPath()+".tmp");
            try(FileOutputStream out=new FileOutputStream(tmp)){out.write(state.toString().getBytes(StandardCharsets.UTF_8));out.getFD().sync();}
            if(!tmp.renameTo(target))throw new IOException("Cannot save document");
        }catch(Exception e){runOnUiThread(()->Toast.makeText(this,"Could not save: "+e.getMessage(),Toast.LENGTH_LONG).show());}}
    }
    void load(String newType){synchronized(lock){
        events=new JSONArray();pending=new JSONArray();cursor=0;hidden.clear();undo.clear();redo.clear();selected="base";page=0;type=newType;
        try{File f=stateFile();if(f.exists()){
            JSONObject state=new JSONObject(new String(java.nio.file.Files.readAllBytes(f.toPath()),StandardCharsets.UTF_8));
            events=state.getJSONArray("events");pending=state.getJSONArray("pending");cursor=state.getLong("cursor");
            JSONArray h=state.getJSONArray("hidden");for(int i=0;i<h.length();i++)hidden.add(h.getString(i));
            h=state.getJSONArray("undo");for(int i=0;i<h.length();i++)undo.add(h.getString(i));
            h=state.getJSONArray("redo");for(int i=0;i<h.length();i++)redo.add(h.getString(i));
            selected=state.optString("selected","base");page=state.optInt("page",0);
        }}catch(Exception e){throw new RuntimeException("Cannot read saved document",e);}
        rebuild();
        if(events.length()==0&&pending.length()==0)enqueue(obj("kind","document","type",newType));
    }}
    void apply(JSONObject o){String kind=o.optString("kind");
        if(kind.equals("document")&&!documentSeen){type=o.optString("type","fixed");documentSeen=true;}
        if(kind.equals("layer"))layers.put(o.optString("layer"),o);
        if(kind.equals("stroke"))strokes.put(o.optString("stroke"),o);
        if(kind.equals("active"))active.put(o.optString("stroke"),o.optBoolean("active"));
    }
    void rebuild(){documentSeen=false;layers.clear();strokes.clear();active.clear();layers.put("base",obj("name","Layer 1","force",false,"layer","base"));
        for(int i=0;i<events.length();i++)apply(events.optJSONObject(i));for(int i=0;i<pending.length();i++)apply(pending.optJSONObject(i));
        if(!layers.containsKey(selected))selected="base";
    }
    void enqueue(JSONObject o){synchronized(lock){try{o.put("id",id());pending.put(o);apply(o);save();}catch(Exception e){throw new RuntimeException(e);}}}
    void update(){synchronized(lock){
        if(editor==null)return;
        displayedConnected=connected;
        editor.refresh();
        canvas.invalidate();
    }}
    void documentDialog(){
        LinearLayout fields=new LinearLayout(this);fields.setOrientation(1);fields.setPadding(editor.dp(24),editor.dp(8),editor.dp(24),editor.dp(8));
        EditText server=new EditText(this);server.setSingleLine();server.setText(host);server.setHint("Server URL, e.g. http://192.168.50.180:8765");fields.addView(server);
        EditText name=new EditText(this);name.setSingleLine();name.setText(document);name.setHint("Document name (same on both clients)");fields.addView(name);
        Spinner types=new Spinner(this);types.setAdapter(new ArrayAdapter<String>(this,android.R.layout.simple_spinner_dropdown_item,new String[]{"Fixed page","Multiple pages","Infinite canvas"}));types.setSelection(type.equals("pages")?1:type.equals("infinite")?2:0);fields.addView(types);
        new AlertDialog.Builder(this).setTitle("Open or create document").setMessage("Type is used when creating a new document. Existing documents retain their type.").setView(fields).setPositiveButton("Open",(d,w)->{
            String next=name.getText().toString().trim(),url=server.getText().toString().trim().replaceAll("/+$","");
            if(next.isEmpty()||!(url.startsWith("http://")||url.startsWith("https://"))){Toast.makeText(this,"Enter document name and HTTP(S) URL",1).show();return;}
            synchronized(lock){save();document=next;host=url;connected=false;load(new String[]{"fixed","pages","infinite"}[types.getSelectedItemPosition()]);canvas.zoom=1;canvas.offsetX=canvas.offsetY=0;}
            getSharedPreferences("settings",0).edit().putString("host",host).putString("document",document).apply();update();
        }).setNegativeButton("Cancel",null).show();
    }
    void pageDialog(){if(!type.equals("pages"))return;EditText field=new EditText(this);field.setInputType(2);field.setText(""+(page+1));new AlertDialog.Builder(this).setTitle("Go to / create page").setView(field).setPositiveButton("Open",(d,w)->{try{page=Math.max(0,Math.min(10000,Integer.parseInt(field.getText().toString())-1));save();update();}catch(Exception ignored){}}).show();}
    void history(boolean forward){synchronized(lock){ArrayList<String> from=forward?redo:undo,to=forward?undo:redo;if(from.isEmpty())return;String entry=from.remove(from.size()-1);to.add(entry);boolean erased=entry.startsWith("erase:");String stroke=erased?entry.substring(6):entry;enqueue(obj("kind","active","stroke",stroke,"active",erased?!forward:forward));update();}}
    JSONObject http(String method,String url,JSONObject body) throws Exception{
        HttpURLConnection connection=(HttpURLConnection)new URL(url).openConnection();connection.setConnectTimeout(2000);connection.setReadTimeout(3000);connection.setRequestMethod(method);
        try{if(body!=null){connection.setDoOutput(true);connection.setRequestProperty("Content-Type","application/json");try(OutputStream out=connection.getOutputStream()){out.write(body.toString().getBytes(StandardCharsets.UTF_8));}}
            if(connection.getResponseCode()!=200)throw new IOException("HTTP "+connection.getResponseCode());
            try(InputStream in=connection.getInputStream();ByteArrayOutputStream out=new ByteArrayOutputStream()){byte[] buf=new byte[8192];int n;while((n=in.read(buf))!=-1)out.write(buf,0,n);return new JSONObject(new String(out.toByteArray(),StandardCharsets.UTF_8));}
        }finally{connection.disconnect();}
    }
    void syncLoop(){while(running){String doc,url;long after;JSONArray batch=new JSONArray();
        synchronized(lock){doc=document;url=host;after=cursor;int bytes=0;for(int i=0;i<Math.min(100,pending.length());i++){JSONObject operation=pending.optJSONObject(i);int size=operation.toString().length();if(bytes+size>1000000&&i>0)break;batch.put(operation);bytes+=size;}}
        try{
            HashSet<String> ack=new HashSet<>();if(batch.length()>0){JSONArray accepted=http("POST",url+"/api/events",obj("document",doc,"operations",batch)).getJSONArray("accepted");for(int i=0;i<accepted.length();i++)ack.add(accepted.getString(i));}
            JSONArray received=new JSONArray();long last=after; // consume all pages before removing acknowledged optimistic operations
            while(true){JSONArray rows=http("GET",url+"/api/events?document="+URLEncoder.encode(doc,"UTF-8")+"&after="+last,null).getJSONArray("events");for(int i=0;i<rows.length();i++){JSONObject e=rows.getJSONObject(i);received.put(e);last=e.getLong("seq");}if(rows.length()<1000)break;}
            synchronized(lock){if(document.equals(doc)&&host.equals(url)){
                for(int i=0;i<received.length();i++)events.put(received.getJSONObject(i).getJSONObject("op"));cursor=last;
                JSONArray next=new JSONArray();for(int i=0;i<pending.length();i++)if(!ack.contains(pending.getJSONObject(i).getString("id")))next.put(pending.getJSONObject(i));pending=next;connected=true;
                if(received.length()>0||ack.size()>0){rebuild();save();runOnUiThread(this::update);}else runOnUiThread(()->{if(displayedConnected!=connected)update();});
            }}
        }catch(Exception e){synchronized(lock){connected=false;}runOnUiThread(()->{if(displayedConnected!=connected)update();});}
        try{Thread.sleep(200);}catch(InterruptedException e){return;}
    }}

    class CanvasView extends View {
        Paint paint=new Paint(Paint.ANTI_ALIAS_FLAG);
        float zoom=1,offsetX=0,offsetY=0,lastX,lastY;
        JSONArray points;String current;long lastSent;
        ScaleGestureDetector scaling;
        CanvasView(Context context){super(context);setBackgroundColor(Color.WHITE);scaling=new ScaleGestureDetector(context,new ScaleGestureDetector.SimpleOnScaleGestureListener(){@Override public boolean onScale(ScaleGestureDetector d){if(!pan)return false;float old=zoom;zoom=Math.max(.2f,Math.min(5,zoom*d.getScaleFactor()));offsetX=d.getFocusX()-(d.getFocusX()-offsetX)*zoom/old;offsetY=d.getFocusY()-(d.getFocusY()-offsetY)*zoom/old;invalidate();return true;}});}
        float scale(){return Math.min(getWidth()/1000f,getHeight()/1400f);}
        float x(float x){return (x-offsetX)/(scale()*zoom);}float y(float y){return (y-offsetY)/(scale()*zoom);}
        boolean visible(String layer){JSONObject l=layers.get(layer);return l!=null&&(l.optBoolean("force")||!hidden.contains(layer));}
        void drawStroke(Canvas c,JSONObject stroke){JSONArray ps=stroke.optJSONArray("points");paint.setColor(stroke.optInt("color"));paint.setStrokeCap(Paint.Cap.ROUND);float px=0,py=0,pw=0;for(int i=0;i<ps.length();i++){JSONArray p=ps.optJSONArray(i);float x=(float)p.optDouble(0),y=(float)p.optDouble(1),w=(float)(stroke.optDouble("width")*(.25+.75*p.optDouble(2)));if(i>0){paint.setStrokeWidth((pw+w)/2);c.drawLine(px,py,x,y,paint);}c.drawCircle(x,y,w/2,paint);px=x;py=y;pw=w;}}
        @Override protected void onDraw(Canvas c){super.onDraw(c);synchronized(lock){c.save();c.translate(offsetX,offsetY);c.scale(scale()*zoom,scale()*zoom);if(!type.equals("infinite")){c.clipRect(0,0,1000,1400);paint.setColor(0xffaaaaaa);paint.setStyle(Paint.Style.STROKE);paint.setStrokeWidth(1);c.drawRect(0,0,1000,1400,paint);paint.setStyle(Paint.Style.FILL);}
            for(String layer:layers.keySet()){if(!visible(layer))continue;for(Map.Entry<String,JSONObject> e:strokes.entrySet()){JSONObject s=e.getValue();if(!e.getKey().equals(current)&&layer.equals(s.optString("layer"))&&s.optInt("page")==page&&!Boolean.FALSE.equals(active.get(e.getKey())))drawStroke(c,s);}if(layer.equals(selected)&&points!=null&&current!=null)drawStroke(c,stroke());}
            c.restore();}}
        JSONObject stroke(){return obj("kind","stroke","stroke",current,"layer",selected,"page",page,"color",color,"width",width,"points",points);}
        void emit(){if(points!=null&&points.length()>0){try{enqueue(new JSONObject(stroke().toString()));}catch(Exception e){throw new RuntimeException(e);}lastSent=System.currentTimeMillis();}}
        void point(float sx,float sy,float pressure){float dx=x(sx),dy=y(sy);if(!type.equals("infinite")&&(dx<0||dy<0||dx>1000||dy>1400))return;if(points.length()>=8000){JSONArray reduced=new JSONArray();for(int i=0;i<points.length();i+=2)reduced.put(points.optJSONArray(i));points=reduced;}points.put(new JSONArray(Arrays.asList(dx,dy,Math.max(0,Math.min(1,pressure)))));}
        void eraseAt(float sx,float sy){float dx=x(sx),dy=y(sy);for(Map.Entry<String,JSONObject> e:new ArrayList<>(strokes.entrySet())){JSONObject s=e.getValue();if(!s.optString("layer").equals(selected)||s.optInt("page")!=page||!visible(selected)||Boolean.FALSE.equals(active.get(e.getKey())))continue;JSONArray ps=s.optJSONArray("points");for(int i=0;i<ps.length();i++){JSONArray p=ps.optJSONArray(i);double distance=Math.hypot(p.optDouble(0)-dx,p.optDouble(1)-dy);if(i>0){JSONArray a=ps.optJSONArray(i-1);double ax=a.optDouble(0),ay=a.optDouble(1),vx=p.optDouble(0)-ax,vy=p.optDouble(1)-ay,len=vx*vx+vy*vy,t=len==0?0:Math.max(0,Math.min(1,((dx-ax)*vx+(dy-ay)*vy)/len));distance=Math.min(distance,Math.hypot(dx-ax-t*vx,dy-ay-t*vy));}if(distance<18+s.optDouble("width")/2){String sid=e.getKey();enqueue(obj("kind","active","stroke",sid,"active",false));undo.add("erase:"+sid);redo.clear();save();break;}}}}
        @Override public boolean onTouchEvent(MotionEvent event){synchronized(lock){scaling.onTouchEvent(event);int action=event.getActionMasked();float sx=event.getX(),sy=event.getY();if(pan){if(action==MotionEvent.ACTION_DOWN){lastX=sx;lastY=sy;}if(action==MotionEvent.ACTION_MOVE&&!scaling.isInProgress()&&event.getPointerCount()==1){offsetX+=sx-lastX;offsetY+=sy-lastY;invalidate();}lastX=sx;lastY=sy;return true;}
            if(event.getPointerCount()>1)return true;
            if(action==MotionEvent.ACTION_DOWN){if(!visible(selected)){Toast.makeText(MainActivity.this,"Show the selected layer to draw on it",0).show();return true;}if(erase){eraseAt(sx,sy);}else{current=id();points=new JSONArray();point(sx,sy,event.getPressure());emit();}}
            if(action==MotionEvent.ACTION_MOVE){if(erase){eraseAt(sx,sy);}else if(points!=null){for(int i=0;i<event.getHistorySize();i++)point(event.getHistoricalX(i),event.getHistoricalY(i),event.getHistoricalPressure(i));point(sx,sy,event.getPressure());if(System.currentTimeMillis()-lastSent>100)emit();}}
            if(action==MotionEvent.ACTION_UP||action==MotionEvent.ACTION_CANCEL){if(points!=null){point(sx,sy,event.getPressure());emit();undo.add(current);redo.clear();points=null;current=null;save();}}
            update();return true;
        }}
    }
}
