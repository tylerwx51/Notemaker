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
    private static final int SVG_PICK_REQUEST = 4512, IMAGE_PICK_REQUEST = 4513;
    final Object lock = new Object();
    int eventCount, pendingCount;
    final LinkedHashMap<String,JSONObject> optimistic=new LinkedHashMap<>();
    int saving;
    String saveError;
    final Map<String,Map<String,Object>> renderedEntities=new HashMap<>();
    Map<String,Object> leases=new LinkedHashMap<>();
    Map<String,Object> transformPreviews=new LinkedHashMap<>();
    List<Object> conflicts=new ArrayList<>();
    LinkedHashMap<String, JSONObject> layers = new LinkedHashMap<>(), strokes = new LinkedHashMap<>();
    HashMap<String, Boolean> active = new HashMap<>();
    HashSet<String> hidden = new HashSet<>();
    ArrayList<String> undo = new ArrayList<>(), redo = new ArrayList<>();
    final ArrayList<PendingObjectHistory> pendingObjectHistory = new ArrayList<>();
    String proxyUrl="",proxyToken="";
    AppReplica replica;
    String document="notes", host="", type="fixed", selected="base";
    long cursor=0;
    int page=0, color=Color.BLACK;
    float width=5;
    String brush="pen";
    boolean erase=false, pan=false, connected=false, documentSeen=false, fillTapMode=false;
    String eraserMode="stroke";
    float eraserRadius=12;
    boolean scribbleToErase=false;
    String drawingShape, pendingText;
    String selectionSession;
    volatile long selectionGeneration;
    volatile long selectionRequestSequence;
    final ArrayList<Map<String,Object>> selectedEntities=new ArrayList<>();
    int selectionSkipped;
    boolean selecting;
    Map<String,Object> pendingTransform;
    Map<String,Object> selectionClipboard=Json.object("entities",Collections.emptyList());
    final Handler selectionHandler=new Handler(Looper.getMainLooper());
    Runnable heartbeatTask;
    Runnable previewExpiryTask;
    Runnable previewDispatchTask;
    boolean previewInFlight,transformCommitQueued;
    boolean historyPending;
    boolean freeformLasso;
    float pendingTextSize=24;
    boolean pendingTextBold, pendingTextItalic, pendingTextUnderline;
    String pendingTextFamily="sans-serif", pendingTextAlignment="left";
    int objectFillColor=Color.TRANSPARENT;
    volatile boolean running=true;
    CanvasView canvas;
    EditorUi editor;
    AgentUi agent;
    boolean displayedConnected;
    Thread syncThread, localThread;
    Store store;
    Model model=new Model();
    long revision=-1;
    private static final class PendingObjectHistory { int index; PendingObjectHistory(int index){this.index=index;} }

    static JSONObject obj(Object... args) {
        JSONObject o=new JSONObject(); try {for(int i=0;i<args.length;i+=2)o.put((String)args[i],args[i+1]);}catch(Exception e){throw new RuntimeException(e);}return o;
    }
    static String id(){return UUID.randomUUID().toString();}
    @Override public void onCreate(Bundle b){
        super.onCreate(b);
        android.content.SharedPreferences prefs=getSharedPreferences("settings",0);
        scribbleToErase=prefs.getBoolean("scribbleToErase",false);
        freeformLasso=prefs.getBoolean("freeformLasso",false);
        try{String savedClipboard=prefs.getString("selectionClipboard","");if(!savedClipboard.isEmpty())selectionClipboard=Json.map(Json.parse(savedClipboard));}catch(Exception ignored){}
        host=prefs.getString("host",host);document=prefs.getString("document",document);proxyUrl=prefs.getString("proxyUrl","");proxyToken=prefs.getString("proxyToken","");
        load("fixed");
        agent=new AgentUi(this);
        editor=new EditorUi(this);
        View root=editor.build();
        setContentView(root);
        if(Build.VERSION.SDK_INT>=30)root.post(()->{WindowInsetsController controller=getWindow().getInsetsController();if(controller!=null){int appearance=WindowInsetsController.APPEARANCE_LIGHT_STATUS_BARS|WindowInsetsController.APPEARANCE_LIGHT_NAVIGATION_BARS;controller.setSystemBarsAppearance(appearance,appearance);}});
        else getWindow().getDecorView().setSystemUiVisibility(View.SYSTEM_UI_FLAG_LIGHT_STATUS_BAR|View.SYSTEM_UI_FLAG_LIGHT_NAVIGATION_BAR);
        update();
        configureProxy();

    }
    @Override public void onDestroy(){running=false;if(heartbeatTask!=null)selectionHandler.removeCallbacks(heartbeatTask);if(previewExpiryTask!=null)selectionHandler.removeCallbacks(previewExpiryTask);if(previewDispatchTask!=null)selectionHandler.removeCallbacks(previewDispatchTask);AppReplica departing=replica;if(departing!=null&&selectionSession!=null)ReplicaService.writes.execute(()->{try{departing.command(Arrays.asList("selection","disconnect"),null);}catch(Exception ignored){}});if(replica!=null)replica.close();if(agent!=null)agent.destroy();if(editor!=null)editor.dismissPopup();if(syncThread!=null)syncThread.interrupt();if(localThread!=null)localThread.interrupt();super.onDestroy();}
    File stateFile(){return store.file;}
    void configureProxy(){
        final AppReplica target=replica;final String targetType=type;
        if(!target.remote())startService(new Intent(this,ReplicaService.class).putExtra("document",document).putExtra("host",host));
        else ReplicaService.writes.execute(()->{try{
            Map<String,Object> initial=target.read();
            if(((Number)initial.get("eventCount")).intValue()==0&&((Number)initial.get("pendingCount")).intValue()==0)target.command(Arrays.asList("append","-"),Json.object("kind","document","type",targetType));
            refreshFrom(target);
        }catch(Exception e){runOnUiThread(()->{saveError=e.toString();update();});}});
        if(localThread!=null)localThread.interrupt();
        localThread=new Thread(()->localLoop(target),"local-state");localThread.start();
    }
    void persist(List<String> args,Object input,String optimisticId){
        final AppReplica target=replica;saving++;
        ReplicaService.writes.execute(()->{
            try{target.command(args,input);Map<String,Object> snapshot=target.read();runOnUiThread(()->{
                saving--;if(target!=replica)return;saveError=null;if(optimisticId!=null)optimistic.remove(optimisticId);
                try{refreshShared(snapshot);update();}catch(Exception e){saveError=e.toString();update();}
            });}catch(Exception e){android.util.Log.e("NoteMaker","Local save failed",e);runOnUiThread(()->{saving--;saveError=e.toString();update();Toast.makeText(this,"Could not save changes: "+e.getMessage(),Toast.LENGTH_LONG).show();});}
        });
    }
    void setDrawingTool(String shape){if(shape!=null&&(selecting||selectionSession!=null||!selectedEntities.isEmpty()))deselect();synchronized(lock){drawingShape=shape;erase=false;pan=false;fillTapMode=false;if(!"text".equals(shape))pendingText=null;if(canvas!=null){canvas.shapeDragging=false;canvas.invalidate();}update();}}
    void placeText(String text,float fontSize,float x,float y){
        if(text==null||text.trim().isEmpty()||Float.isNaN(fontSize)||Float.isInfinite(fontSize)||fontSize<1||fontSize>500||Float.isNaN(x)||Float.isInfinite(x)||Float.isNaN(y)||Float.isInfinite(y))return;
        float w=Math.max(100,text.length()*fontSize*.65f),h=Math.max(48,fontSize*1.6f);if(!type.equals("infinite")){x=Math.max(0,Math.min(999,x));y=Math.max(0,Math.min(1399,y));w=Math.max(1,Math.min(w,1000-x));h=Math.max(1,Math.min(h,1400-y));}
        JSONObject object=objectValue("text",x,y,w,h,color,width,Color.TRANSPARENT);
        try{object.put("text",text);object.put("fontSize",fontSize);object.put("bold",pendingTextBold);object.put("italic",pendingTextItalic);object.put("underline",pendingTextUnderline);object.put("fontFamily",pendingTextFamily);object.put("alignment",pendingTextAlignment);}catch(Exception e){throw new RuntimeException(e);}persistObject(object);
    }
    private JSONObject objectValue(String shape,float x,float y,float w,float h,int outline,float outlineWidth,int fill){
        JSONObject object=obj("kind","object","object",id(),"shape",shape,"layer",selected,"page",page,"x",x,"y",y,"w",w,"h",h,"color",outline,"width",outlineWidth,"fontSize",24,"text","");
        if(fill!=Color.TRANSPARENT)try{object.put("fill",fill);}catch(Exception e){throw new RuntimeException(e);}return object;
    }
    void persistObject(JSONObject object){
        final AppReplica target=replica;final PendingObjectHistory pending;
        synchronized(lock){pending=new PendingObjectHistory(undo.size());pendingObjectHistory.add(pending);saving++;update();}
        ReplicaService.writes.execute(()->{try{
            target.command(Arrays.asList("object","put","-"),Json.map(Json.parse(object.toString())));Map<String,Object> snapshot=null;try{snapshot=target.read();}catch(Exception readError){android.util.Log.w("NoteMaker","Object saved but snapshot refresh failed",readError);}final Map<String,Object> savedSnapshot=snapshot;runOnUiThread(()->{saving--;if(target!=replica)return;pendingObjectHistory.remove(pending);int index=Math.max(0,Math.min(pending.index,undo.size()));undo.add(index,"object-action");for(PendingObjectHistory later:pendingObjectHistory)if(later.index>=index)later.index++;saveError=null;redo.clear();if(savedSnapshot!=null)try{refreshShared(savedSnapshot);}catch(Exception e){saveError=e.toString();}save();update();if(savedSnapshot==null)ReplicaService.writes.execute(()->refreshFrom(target));});
        }catch(Exception e){android.util.Log.e("NoteMaker","Object save failed",e);runOnUiThread(()->{saving--;if(target!=replica)return;pendingObjectHistory.remove(pending);saveError=e.toString();update();Toast.makeText(this,"Could not save object: "+e.getMessage(),Toast.LENGTH_LONG).show();});}});
    }
    boolean objectHistoryPending(){synchronized(lock){return !pendingObjectHistory.isEmpty();}}
    void save(){
        if(replica.remote()&&revision<0)return;
        final Map<String,Object> preferences=Json.object("hidden",new ArrayList<>(hidden),"undo",new ArrayList<>(undo),"redo",new ArrayList<>(redo),"selected",selected,"page",page,"clipboard",selectionClipboard);
        persist(Arrays.asList("settings","-"),preferences,null);
    }
    void refreshShared(Map<String,Object> state) throws Exception {
        long next=((Number)state.get("revision")).longValue();if(next<revision)return;
        if(replica.remote()&&revision<0&&next>=0){
            for(String key:Arrays.asList("hidden","undo","redo")){java.util.Collection<String> values=key.equals("hidden")?hidden:key.equals("undo")?undo:redo;values.clear();for(Object value:Json.list(state.getOrDefault(key,Collections.emptyList())))values.add((String)value);}
            selected=(String)state.getOrDefault("selected","base");page=((Number)state.getOrDefault("page",0)).intValue();
        }
        revision=next;eventCount=state.containsKey("eventCount")?((Number)state.get("eventCount")).intValue():Json.list(state.get("events")).size();pendingCount=state.containsKey("pendingCount")?((Number)state.get("pendingCount")).intValue():Json.list(state.get("pending")).size();cursor=((Number)state.get("cursor")).longValue();
        if(state.containsKey("leases"))leases=Json.map(state.get("leases"));if(state.containsKey("previews"))transformPreviews=Json.map(state.get("previews"));if(state.containsKey("conflicts"))conflicts=Json.list(state.get("conflicts"));
        if(state.get("clipboard") instanceof Map){Map<String,Object> sharedClipboard=Json.map(state.get("clipboard"));long sharedAt=((Number)sharedClipboard.getOrDefault("copiedAt",0L)).longValue(),localAt=((Number)selectionClipboard.getOrDefault("copiedAt",0L)).longValue();boolean sharedHasItems=!Json.list(sharedClipboard.getOrDefault("entities",Collections.emptyList())).isEmpty(),localHasItems=!Json.list(selectionClipboard.getOrDefault("entities",Collections.emptyList())).isEmpty();if(sharedAt>=localAt&&(sharedHasItems||!localHasItems))selectionClipboard=sharedClipboard;}
        getSharedPreferences("settings",0).edit().putString("selectionClipboard",Json.write(selectionClipboard)).apply();schedulePreviewExpiry();
        model=Store.model(state);type=model.type;documentSeen=eventCount+pendingCount>0;
        LinkedHashMap<String,JSONObject> nextLayers=new LinkedHashMap<>(),nextStrokes=new LinkedHashMap<>();
        ArrayList<Map.Entry<String,Map<String,Object>>> orderedLayers=new ArrayList<>(model.layers.entrySet());
        orderedLayers.sort(java.util.Comparator.comparingDouble(entry -> entry.getValue().get("order") instanceof Number ? ((Number)entry.getValue().get("order")).doubleValue() : 0d));
        for(Map.Entry<String,Map<String,Object>> entry:orderedLayers)nextLayers.put(entry.getKey(),entity("layer:",entry,layers));
        for(Map.Entry<String,Map<String,Object>> entry:model.strokes.entrySet())nextStrokes.put(entry.getKey(),entity("stroke:",entry,strokes));
        layers=nextLayers;strokes=nextStrokes;active.clear();active.putAll(model.active);
        for(JSONObject op:optimistic.values())apply(op);
        if(!layers.containsKey(selected)||Boolean.FALSE.equals(model.layerActive.get(selected))){selected="base";deselect();resetDrawingGesture();}
    }
    JSONObject entity(String prefix,Map.Entry<String,Map<String,Object>> entry,Map<String,JSONObject> previous){
        Map<String,Object> old=renderedEntities.put(prefix+entry.getKey(),entry.getValue());JSONObject existing=previous.get(entry.getKey());
        return existing!=null&&(old==entry.getValue()||entry.getValue().equals(old))?existing:new JSONObject(entry.getValue());
    }
    void load(String newType){synchronized(lock){
        resetDrawingGesture();
        pendingObjectHistory.clear();
        historyPending=false;
        eventCount=pendingCount=0;optimistic.clear();renderedEntities.clear();cursor=0;revision=-1;hidden.clear();undo.clear();redo.clear();selected="base";page=0;type=newType;
        try{
            if(replica!=null)replica.close();replica=new AppReplica(this,document,proxyUrl,proxyToken);store=replica.local;Map<String,Object> state=replica.remote()?Json.object("effectiveModel",new Model().json(),"revision",-1,"cursor",0,"eventCount",0,"pendingCount",0):replica.read();JSONObject saved=new JSONObject(state);
            JSONArray h=saved.optJSONArray("hidden");if(h!=null)for(int i=0;i<h.length();i++)hidden.add(h.getString(i));
            h=saved.optJSONArray("undo");if(h!=null)for(int i=0;i<h.length();i++)undo.add(h.getString(i));
            h=saved.optJSONArray("redo");if(h!=null)for(int i=0;i<h.length();i++)redo.add(h.getString(i));
            selected=saved.optString("selected","base");page=saved.optInt("page",0);refreshShared(state);
            if(replica.remote())type=newType;
            if(!replica.remote()&&eventCount==0&&pendingCount==0)enqueue(obj("kind","document","type",newType));
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
        persist(Arrays.asList("append","-"),Json.map(Json.parse(o.toString())),token);
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
        EditText endpoint=new EditText(this);endpoint.setSingleLine();endpoint.setText(proxyUrl);endpoint.setHint("Editing proxy URL (blank: this device)");fields.addView(endpoint);
        EditText credential=new EditText(this);credential.setSingleLine();credential.setText(proxyToken);credential.setHint("Editing proxy token");credential.setInputType(129);fields.addView(credential);
        EditText name=new EditText(this);name.setSingleLine();name.setText(document);name.setHint("Document name (same on both clients)");fields.addView(name);
        Spinner types=new Spinner(this);types.setAdapter(new ArrayAdapter<String>(this,android.R.layout.simple_spinner_dropdown_item,new String[]{"Fixed page","Multiple pages","Infinite canvas"}));types.setSelection(type.equals("pages")?1:type.equals("infinite")?2:0);fields.addView(types);
        new AlertDialog.Builder(this).setTitle("Open or create document").setMessage("Type is used when creating a new document. Existing documents retain their type.").setView(fields).setPositiveButton("Open",(d,w)->{
            String selectedProxy=endpoint.getText().toString().trim().replaceAll("/+$",""),selectedToken=credential.getText().toString().trim();
            if(!selectedProxy.isEmpty()&&(!(selectedProxy.startsWith("http://")||selectedProxy.startsWith("https://"))||selectedToken.isEmpty())){Toast.makeText(this,"Enter a proxy HTTP(S) URL and token",1).show();return;}
            String next=name.getText().toString().trim(),url=server.getText().toString().trim().replaceAll("/+$","");
            if(next.isEmpty()||!(url.isEmpty()||url.startsWith("http://")||url.startsWith("https://"))){Toast.makeText(this,"Enter a document name and an optional HTTP(S) URL",1).show();return;}
            deselect();synchronized(lock){save();document=next;host=url;proxyUrl=selectedProxy;proxyToken=selectedToken;connected=false;load(new String[]{"fixed","pages","infinite"}[types.getSelectedItemPosition()]);canvas.zoom=1;canvas.offsetX=canvas.offsetY=0;}
            getSharedPreferences("settings",0).edit().putString("host",host).putString("document",document).putString("proxyUrl",proxyUrl).putString("proxyToken",proxyToken).apply();configureProxy();update();
        }).setNegativeButton("Cancel",null).show();
    }
    void pageDialog(){if(!type.equals("pages"))return;EditText field=new EditText(this);field.setInputType(2);field.setText(""+(page+1));new AlertDialog.Builder(this).setTitle("Go to / create page").setView(field).setPositiveButton("Open",(d,w)->{try{int next=Math.max(0,Math.min(10000,Integer.parseInt(field.getText().toString())-1));deselect();resetDrawingGesture();page=next;save();update();}catch(Exception ignored){}}).show();}
    void history(boolean forward){
        final ArrayList<String> from=forward?redo:undo,to=forward?undo:redo;final String entry;final AppReplica target=replica;
        synchronized(lock){if(historyPending||!pendingObjectHistory.isEmpty()||from.isEmpty())return;entry=from.get(from.size()-1);historyPending=true;saving++;update();}
        ReplicaService.writes.execute(()->{try{
            if(entry.equals("object-action"))target.command(Arrays.asList(forward?"redo":"undo"),null);
            else{boolean erased=entry.startsWith("erase:");String stroke=erased?entry.substring(6):entry;target.command(Arrays.asList("append","-"),Json.object("kind","active","stroke",stroke,"active",erased?!forward:forward));}
            Map<String,Object> snapshot=target.read();runOnUiThread(()->{synchronized(lock){saving--;if(target!=replica)return;historyPending=false;if(from.isEmpty()||!entry.equals(from.get(from.size()-1))){saveError="History changed before the operation completed";update();return;}from.remove(from.size()-1);to.add(entry);saveError=null;try{refreshShared(snapshot);}catch(Exception error){saveError=error.toString();}save();update();}});
        }catch(Exception error){runOnUiThread(()->{synchronized(lock){saving--;if(target!=replica)return;historyPending=false;saveError=error.toString();update();Toast.makeText(this,"Could not "+(forward?"redo":"undo")+": "+error.getMessage(),Toast.LENGTH_LONG).show();}});}});
    }
    void refreshFrom(AppReplica target){
        try{
            Map<String,Object> snapshot=target.read();
            runOnUiThread(()->{if(target!=replica)return;try{
                boolean wasConnected=connected;connected=target.remote()||(ReplicaService.proxy!=null&&Boolean.TRUE.equals(ReplicaService.proxy.connected.get(document)));
                if(((Number)snapshot.get("revision")).longValue()>revision){refreshShared(snapshot);update();}
                else {
                    Map<String,Object> nextLeases=Json.map(snapshot.getOrDefault("leases",Collections.emptyMap()));
                    Map<String,Object> nextPreviews=Json.map(snapshot.getOrDefault("previews",Collections.emptyMap()));
                    boolean coordinationChanged=!leases.equals(nextLeases)||!transformPreviews.equals(nextPreviews);
                    if(coordinationChanged){leases=nextLeases;transformPreviews=nextPreviews;schedulePreviewExpiry();if(canvas!=null)canvas.invalidate();editor.refresh();}
                    if(wasConnected!=connected)update();
                }
            }catch(Exception e){android.util.Log.e("NoteMaker","Refresh failed",e);}});
        }catch(Exception e){throw new RuntimeException(e);}
    }
    void localLoop(AppReplica target){
        long observed=-1,backoff=500;
        while(running&&target==replica)try{
            if(target.remote())target.listen(()->refreshFrom(target));
            else {com.notemaker.cli.LocalProxy proxy=ReplicaService.ensure(this);observed=proxy.awaitChange(observed);refreshFrom(target);}
            backoff=500;
        }catch(InterruptedException e){return;}catch(Exception e){android.util.Log.e("NoteMaker","Local state refresh failed",e);runOnUiThread(()->{if(target==replica&&connected){connected=false;update();}});try{Thread.sleep(backoff);backoff=Math.min(30000,backoff*2);}catch(InterruptedException stop){return;}}
    }

    /** Launches the compiled CLI under this app's UID, using the installed APK as classpath. */
    public java.lang.Process startCli(String... arguments) throws IOException {
        ArrayList<String> command=new ArrayList<>(Arrays.asList("/system/bin/app_process","/system/bin","com.notemaker.cli.Main","--proxy-file",replica.descriptorFile.getAbsolutePath(),"--document",document));
        command.addAll(Arrays.asList(arguments));ProcessBuilder builder=new ProcessBuilder(command);builder.environment().put("CLASSPATH",getApplicationInfo().sourceDir);return builder.start();
    }
    void objectDialog(){synchronized(lock){
        ArrayList<String> ids=new ArrayList<>();for(DrawItem item:new LayerEditor(model,selected,page,true).items())if(item.kind.equals("object"))ids.add(item.id);String[] labels=new String[ids.size()];for(int i=0;i<ids.size();i++){Map<String,Object> o=model.objects.get(ids.get(i));labels[i]=o.get("shape")+" · "+o.getOrDefault("text",ids.get(i));}
        new AlertDialog.Builder(this).setTitle("Edit diagram object").setItems(labels,(d,index)->editObject(ids.get(index))).setNegativeButton("Close",null).show();
    }}
    void choosePicture(){
        Intent picker=new Intent(Intent.ACTION_OPEN_DOCUMENT);picker.setType("image/png");picker.putExtra(Intent.EXTRA_MIME_TYPES,new String[]{"image/png","image/jpeg"});picker.addCategory(Intent.CATEGORY_OPENABLE);startActivityForResult(picker,IMAGE_PICK_REQUEST);
    }
    void chooseSvgPicture(){
        Intent picker=new Intent(Intent.ACTION_OPEN_DOCUMENT);picker.setType("image/svg+xml");picker.addCategory(Intent.CATEGORY_OPENABLE);startActivityForResult(picker,SVG_PICK_REQUEST);
    }
    @Override protected void onActivityResult(int requestCode,int resultCode,Intent data){
        super.onActivityResult(requestCode,resultCode,data);
        if((requestCode!=SVG_PICK_REQUEST&&requestCode!=IMAGE_PICK_REQUEST)||resultCode!=RESULT_OK||data==null||data.getData()==null)return;
        android.net.Uri uri=data.getData();
        ReplicaService.writes.execute(()->{
            try(java.io.InputStream input=getContentResolver().openInputStream(uri)){
                if(input==null)throw new IllegalArgumentException("Could not open image");
                java.io.ByteArrayOutputStream bytes=new java.io.ByteArrayOutputStream();byte[] buffer=new byte[8192];int count;
                int maxBytes=requestCode==SVG_PICK_REQUEST?com.notemaker.core.SvgAsset.MAX_BYTES:com.notemaker.core.ImageAsset.MAX_BYTES;
                while((count=input.read(buffer))!=-1){if(bytes.size()+count>maxBytes)throw new IllegalArgumentException("Image exceeds the supported size");bytes.write(buffer,0,count);}
                byte[] raw=bytes.toByteArray();
                if(requestCode==SVG_PICK_REQUEST){String source=new String(raw,StandardCharsets.UTF_8);double[] size=com.notemaker.core.SvgAsset.validate(source);runOnUiThread(()->insertSvg(source,size[0],size[1]));}
                else {String uriData=com.notemaker.core.ImageAsset.fromBytes(raw);double[] size=com.notemaker.core.ImageAsset.validate(uriData);runOnUiThread(()->insertImage(uriData,size[0],size[1]));}
            }catch(Exception error){runOnUiThread(()->Toast.makeText(this,"Could not import picture: "+error.getMessage(),Toast.LENGTH_LONG).show());}
        });
    }
    void insertTemplateSvg(String source){
        try{com.notemaker.core.SvgAsset.validate(source);insertSvg(source,1000,1400,true);}
        catch(Exception error){Toast.makeText(this,"Template could not be added: "+error.getMessage(),Toast.LENGTH_LONG).show();}
    }
    void insertTemplate(String template){
        if(!Arrays.asList("ruled","grid","dots","planner","blank").contains(template))return;
        AppReplica target=replica;final PendingObjectHistory pending;
        synchronized(lock){pending=new PendingObjectHistory(undo.size());pendingObjectHistory.add(pending);saving++;update();}
        Map<String,Object> input=Json.object("template",template,"layer",selected,"page",page,"x",0,"y",0,"w",1000,"h",1400,"spacing",32,"color",0xffc8ccc8);
        ReplicaService.writes.execute(()->{try{
            target.command(Arrays.asList("template","add","-"),input);Map<String,Object> snapshot=target.read();
            runOnUiThread(()->{saving--;if(target!=replica)return;pendingObjectHistory.remove(pending);int index=Math.max(0,Math.min(pending.index,undo.size()));undo.add(index,"object-action");for(PendingObjectHistory later:pendingObjectHistory)if(later.index>=index)later.index++;saveError=null;try{refreshShared(snapshot);}catch(Exception e){saveError=e.toString();}save();update();});
        }catch(Exception error){runOnUiThread(()->{saving--;if(target!=replica)return;pendingObjectHistory.remove(pending);saveError=error.toString();update();Toast.makeText(this,"Could not insert template: "+error.getMessage(),Toast.LENGTH_LONG).show();});}});
    }
    void fillEntity(String kind,String id,int fillColor){
        if(!"object".equals(kind)&&!"stroke".equals(kind))return;
        AppReplica target=replica;final PendingObjectHistory pending;
        synchronized(lock){pending=new PendingObjectHistory(undo.size());pendingObjectHistory.add(pending);saving++;update();}
        Map<String,Object> input=Json.object(kind,id,"color",fillColor,"layer",selected,"page",page);
        ReplicaService.writes.execute(()->{try{
            target.command(Arrays.asList("fill","-"),input);Map<String,Object> snapshot=target.read();
            runOnUiThread(()->{saving--;if(target!=replica)return;pendingObjectHistory.remove(pending);int index=Math.max(0,Math.min(pending.index,undo.size()));undo.add(index,"object-action");for(PendingObjectHistory later:pendingObjectHistory)if(later.index>=index)later.index++;saveError=null;try{refreshShared(snapshot);}catch(Exception e){saveError=e.toString();}save();update();});
        }catch(Exception error){runOnUiThread(()->{saving--;if(target!=replica)return;pendingObjectHistory.remove(pending);saveError=error.toString();update();Toast.makeText(this,"Could not fill area: "+error.getMessage(),Toast.LENGTH_LONG).show();});}});
    }
    void renameLayer(String id){
        JSONObject old=layers.get(id);if(old==null)return;EditText name=new EditText(this);name.setSingleLine(true);name.setText(old.optString("name"));
        new AlertDialog.Builder(this).setTitle("Rename layer").setView(name).setNegativeButton("Cancel",null).setPositiveButton("Save",(dialog,which)->{String value=name.getText().toString().trim();if(value.isEmpty())return;synchronized(lock){enqueue(obj("kind","layer","layer",id,"name",value,"force",old.optBoolean("force")));}}).show();
    }
    void layerCommand(String command,String id,String argument){
        ArrayList<String> args=new ArrayList<>(Arrays.asList("layer",command,id));if(argument!=null)args.add(argument);
        AppReplica target=replica;final PendingObjectHistory pending;
        synchronized(lock){pending=new PendingObjectHistory(undo.size());pendingObjectHistory.add(pending);saving++;update();}
        ReplicaService.writes.execute(()->{try{
            target.command(args,null);Map<String,Object> snapshot=target.read();
            runOnUiThread(()->{saving--;if(target!=replica)return;pendingObjectHistory.remove(pending);int index=Math.max(0,Math.min(pending.index,undo.size()));undo.add(index,"object-action");for(PendingObjectHistory later:pendingObjectHistory)if(later.index>=index)later.index++;saveError=null;try{refreshShared(snapshot);}catch(Exception e){saveError=e.toString();}save();update();});
        }catch(Exception error){runOnUiThread(()->{saving--;if(target!=replica)return;pendingObjectHistory.remove(pending);saveError=error.toString();update();Toast.makeText(this,"Could not update layer: "+error.getMessage(),Toast.LENGTH_LONG).show();});}});
    }
    void startSelection(List<Map<String,Object>> entities){
        if(entities==null||entities.isEmpty())return;selecting=true;final long generation=selectionGeneration;AppReplica target=replica;String prior=selectionSession;final long requestSequence=prior==null?++selectionRequestSequence:selectionRequestSequence;Map<String,Object> input=Json.object("layer",selected,"page",page,"entities",entities);if(prior!=null)input.put("session",prior);
        ReplicaService.writes.execute(()->{try{Map<String,Object> result=Json.map(target.command(Arrays.asList("selection","select","-"),input));runOnUiThread(()->{
            String acquired=String.valueOf(result.get("session"));if(target!=replica||generation!=selectionGeneration||!selecting||(prior==null&&requestSequence!=selectionRequestSequence)){ReplicaService.writes.execute(()->{try{target.command(Arrays.asList("selection","deselect",acquired),null);}catch(Exception error){android.util.Log.w("NoteMaker","Could not release stale selection",error);}});return;}selectionSession=acquired;
            for(Object raw:Json.list(result.getOrDefault("selected",Collections.emptyList()))){Map<String,Object> entity=Json.map(raw);boolean exists=false;for(Map<String,Object> old:selectedEntities)if(Objects.equals(old.get("kind"),entity.get("kind"))&&Objects.equals(old.get("id"),entity.get("id")))exists=true;if(!exists)selectedEntities.add(entity);}
            List<Object> skipped=Json.list(result.getOrDefault("skipped",Collections.emptyList()));selectionSkipped+=skipped.size();if(!skipped.isEmpty())Toast.makeText(this,skipped.size()+" item(s) are locked by another editor",Toast.LENGTH_LONG).show();scheduleHeartbeat();update();
        });}catch(Exception error){runOnUiThread(()->Toast.makeText(this,"Could not select entities: "+error.getMessage(),Toast.LENGTH_LONG).show());}});
    }
    com.notemaker.core.LayerEditor layerEditor(){return new com.notemaker.core.LayerEditor(model,selected,page);}
    void selectRegion(double[][] region,boolean topmost){synchronized(lock){if(canvas==null||!canvas.visible(selected))return;List<Object> hits=layerEditor().select(region,topmost,com.notemaker.cli.ItemCoverage::create);ArrayList<Map<String,Object>> refs=new ArrayList<>();for(Object hit:hits)refs.add(Json.map(hit));startSelection(refs);}}
    void selectAt(float x,float y){double radius=canvas==null?1:6/Math.max(.01,canvas.scale()*canvas.zoom);selectRegion(com.notemaker.core.Geometry.capsule(new double[]{x,y},new double[]{x,y},radius),true);}
    void selectAllVisible(){selectRegion(null,false);}
    void selectWithin(float x0,float y0,float x1,float y1){selectRegion(com.notemaker.core.Geometry.rect(Math.min(x0,x1),Math.min(y0,y1),Math.abs(x1-x0),Math.abs(y1-y0)),false);}
    private void addLassoPoint(float x,float y){if(canvas==null)return;ArrayList<float[]> path=canvas.lassoPath;if(path.isEmpty()||Math.hypot(path.get(path.size()-1)[0]-x,path.get(path.size()-1)[1]-y)>=3.5f)path.add(new float[]{x,y});}
    void selectLasso(List<float[]> polygon){if(polygon.size()<3)return;double[][] p=new double[polygon.size()][2];for(int i=0;i<p.length;i++){p[i][0]=polygon.get(i)[0];p[i][1]=polygon.get(i)[1];}selectRegion(p,false);}
    Map<String,Object> selectedEntityAt(float x,float y){double radius=6/Math.max(.01,canvas.scale()*canvas.zoom);double[][] region=com.notemaker.core.Geometry.capsule(new double[]{x,y},new double[]{x,y},radius);com.notemaker.core.LayerEditor editor=layerEditor();for(Map<String,Object> ref:selectedEntities){try{if(editor.resolve(String.valueOf(ref.get("kind")),String.valueOf(ref.get("id"))).intersects(region,com.notemaker.cli.ItemCoverage::create))return ref;}catch(IllegalArgumentException stale){}}return null;}
    void selectLayer(String layer){if(!Objects.equals(selected,layer)){boolean selectionTool=selecting;deselect();resetDrawingGesture();selected=layer;selecting=selectionTool;}save();update();}
    void deselectEntity(String kind,String id){if(selectionSession==null)return;ArrayList<Map<String,Object>> remaining=new ArrayList<>();for(Map<String,Object> entity:selectedEntities)if(!(kind.equals(entity.get("kind"))&&id.equals(entity.get("id"))))remaining.add(entity);String session=selectionSession;AppReplica target=replica;final long generation=selectionGeneration;selectionSession=null;selectedEntities.clear();pendingTransform=null;selecting=true;if(heartbeatTask!=null)selectionHandler.removeCallbacks(heartbeatTask);ReplicaService.writes.execute(()->{try{target.command(Arrays.asList("selection","deselect",session),null);if(!remaining.isEmpty()){if(generation!=selectionGeneration||!selecting||target!=replica)return;Map<String,Object> request=Json.object("layer",selected,"page",page,"entities",remaining);Map<String,Object> result=Json.map(target.command(Arrays.asList("selection","select","-"),request));String acquired=String.valueOf(result.get("session"));runOnUiThread(()->{if(target!=replica||generation!=selectionGeneration||!selecting){ReplicaService.writes.execute(()->{try{target.command(Arrays.asList("selection","deselect",acquired),null);}catch(Exception error){android.util.Log.w("NoteMaker","Could not release stale selection",error);}});return;}selectionSession=acquired;for(Object raw:Json.list(result.getOrDefault("selected",Collections.emptyList())))selectedEntities.add(Json.map(raw));selectionSkipped+=Json.list(result.getOrDefault("skipped",Collections.emptyList())).size();scheduleHeartbeat();update();});}else runOnUiThread(this::update);}catch(Exception error){runOnUiThread(()->Toast.makeText(this,"Could not release item lock: "+error.getMessage(),Toast.LENGTH_LONG).show());}});update();}
    private void scheduleHeartbeat(){
        if(heartbeatTask!=null)selectionHandler.removeCallbacks(heartbeatTask);
        if(selectionSession==null)return;final String session=selectionSession;final AppReplica target=replica;
        heartbeatTask=()->{if(target!=replica||!session.equals(selectionSession)||!running)return;ReplicaService.writes.execute(()->{try{target.command(Arrays.asList("selection","heartbeat",session),null);}catch(Exception error){android.util.Log.w("NoteMaker","Selection heartbeat failed",error);}});selectionHandler.postDelayed(heartbeatTask,5000);};
        selectionHandler.postDelayed(heartbeatTask,5000);
    }
    private void schedulePreviewExpiry(){
        if(previewExpiryTask!=null){selectionHandler.removeCallbacks(previewExpiryTask);previewExpiryTask=null;}
        if(transformPreviews.isEmpty()||canvas==null)return;
        previewExpiryTask=()->{previewExpiryTask=null;if(canvas!=null)canvas.invalidate();long now=System.currentTimeMillis();for(Object raw:transformPreviews.values())try{if(((Number)Json.map(raw).getOrDefault("expires",0)).longValue()>now){schedulePreviewExpiry();break;}}catch(Exception ignored){}};
        selectionHandler.postDelayed(previewExpiryTask,500);
    }
    void deselect(){
        ++selectionGeneration;++selectionRequestSequence;final String session=selectionSession;final boolean clearPreview=selectionSession!=null&&(pendingTransform!=null||previewInFlight);AppReplica target=replica;if(heartbeatTask!=null)selectionHandler.removeCallbacks(heartbeatTask);heartbeatTask=null;if(previewDispatchTask!=null)selectionHandler.removeCallbacks(previewDispatchTask);previewDispatchTask=null;selectionSession=null;selectedEntities.clear();selectionSkipped=0;pendingTransform=null;transformCommitQueued=false;selecting=false;if(session!=null)transformPreviews.remove(session);if(canvas!=null){canvas.lassoing=false;canvas.lassoPath.clear();canvas.transforming=false;}update();
        if(session!=null)ReplicaService.writes.execute(()->{try{if(clearPreview)target.command(Arrays.asList("transform","cancel",session),null);target.command(Arrays.asList("selection","deselect",session),null);}catch(Exception error){android.util.Log.w("NoteMaker","Could not release selection",error);}});
    }
    void previewSelection(double scale,double dx,double dy){
        previewTransform(Json.object("scale",scale,"dx",dx,"dy",dy));
    }
    void previewTransform(Map<String,Object> specification){
        String session=selectionSession;if(session==null||selectedEntities.isEmpty()||transformCommitQueued)return;pendingTransform=Json.object("session",session,"entities",new ArrayList<>(selectedEntities),"transform",new LinkedHashMap<>(specification));update();
        if(!previewInFlight&&previewDispatchTask==null){previewDispatchTask=()->{previewDispatchTask=null;dispatchLatestPreview();};selectionHandler.postDelayed(previewDispatchTask,100);}
    }
    private void dispatchLatestPreview(){if(previewInFlight||pendingTransform==null||selectionSession==null||transformCommitQueued)return;final Map<String,Object> sent=new LinkedHashMap<>(pendingTransform);final AppReplica target=replica;final long generation=selectionGeneration;previewInFlight=true;ReplicaService.writes.execute(()->{try{target.command(Arrays.asList("transform","preview","-"),sent);Map<String,Object> snapshot=target.read();runOnUiThread(()->{previewInFlight=false;if(target==replica&&generation==selectionGeneration&&Objects.equals(selectionSession,sent.get("session")))try{refreshShared(snapshot);update();}catch(Exception error){saveError=error.toString();update();}if(generation==selectionGeneration&&!transformCommitQueued&&pendingTransform!=null&&!pendingTransform.equals(sent))dispatchLatestPreview();});}catch(Exception error){runOnUiThread(()->{previewInFlight=false;if(generation==selectionGeneration)Toast.makeText(this,"Could not preview transform: "+error.getMessage(),Toast.LENGTH_LONG).show();if(generation==selectionGeneration&&!transformCommitQueued&&pendingTransform!=null&&!pendingTransform.equals(sent))dispatchLatestPreview();});}});}
    void transformSelection(double scale,double dx,double dy,boolean commit){
        if(!commit){previewSelection(scale,dx,dy);return;}previewSelection(scale,dx,dy);commitTransformPreview();
    }
    void commitTransformPreview(){
        if(pendingTransform==null||selectionSession==null)return;if(previewDispatchTask!=null){selectionHandler.removeCallbacks(previewDispatchTask);previewDispatchTask=null;}transformCommitQueued=true;final Map<String,Object> transform=new LinkedHashMap<>(pendingTransform);AppReplica target=replica;final PendingObjectHistory pending;
        synchronized(lock){pending=new PendingObjectHistory(undo.size());pendingObjectHistory.add(pending);saving++;update();}
        ReplicaService.writes.execute(()->{try{
            target.command(Arrays.asList("transform","preview","-"),transform);target.command(Arrays.asList("transform","commit","-"),transform);Map<String,Object> snapshot=target.read();
            runOnUiThread(()->{saving--;if(target!=replica)return;pendingObjectHistory.remove(pending);int index=Math.max(0,Math.min(pending.index,undo.size()));undo.add(index,"object-action");for(PendingObjectHistory later:pendingObjectHistory)if(later.index>=index)later.index++;pendingTransform=null;transformCommitQueued=false;saveError=null;try{refreshShared(snapshot);}catch(Exception error){saveError=error.toString();}save();update();});
        }catch(Exception error){runOnUiThread(()->{saving--;if(target!=replica)return;pendingObjectHistory.remove(pending);transformCommitQueued=false;saveError=error.toString();update();Toast.makeText(this,"Could not transform selection: "+error.getMessage(),Toast.LENGTH_LONG).show();});}});
    }
    void cancelTransformPreview(){if(selectionSession==null||pendingTransform==null)return;String session=selectionSession;AppReplica target=replica;if(previewDispatchTask!=null){selectionHandler.removeCallbacks(previewDispatchTask);previewDispatchTask=null;}pendingTransform=null;transformCommitQueued=false;ReplicaService.writes.execute(()->{try{target.command(Arrays.asList("transform","cancel",session),null);Map<String,Object> snapshot=target.read();runOnUiThread(()->{if(target==replica)try{refreshShared(snapshot);update();}catch(Exception error){saveError=error.toString();update();}});}catch(Exception error){runOnUiThread(()->Toast.makeText(this,"Could not cancel preview: "+error.getMessage(),Toast.LENGTH_SHORT).show());}});update();}
    void resolveConflict(String id,String choice){
        if(!Arrays.asList("local","remote","both").contains(choice))return;AppReplica target=replica;saving++;update();
        ReplicaService.writes.execute(()->{try{target.command(Arrays.asList("conflict","resolve",id,choice),null);Map<String,Object> snapshot=target.read();runOnUiThread(()->{saving--;if(target!=replica)return;saveError=null;try{refreshShared(snapshot);}catch(Exception error){saveError=error.toString();}update();});}catch(Exception error){runOnUiThread(()->{saving--;if(target!=replica)return;saveError=error.toString();update();Toast.makeText(this,"Could not resolve conflict: "+error.getMessage(),Toast.LENGTH_LONG).show();});}});
    }
    void duplicateSelection(){
        if(selectionSession!=null)selectionCommand("duplicate");
    }
    void deleteSelection(){
        if(selectionSession!=null)selectionCommand("delete");
    }
    void copySelection(){selectionClipboardCommand("copy");}
    void cutSelection(){selectionClipboardCommand("cut");}
    void pasteSelection(){
        List<Object> entities=Json.list(selectionClipboard.getOrDefault("entities",Collections.emptyList()));if(entities.isEmpty()){Toast.makeText(this,"Clipboard is empty",Toast.LENGTH_SHORT).show();return;}Map<String,Object> input=Json.object("entities",entities,"layer",selected,"page",page,"dx",24,"dy",24);AppReplica target=replica;saving++;update();ReplicaService.writes.execute(()->{try{target.command(Arrays.asList("selection","paste","-"),input);Map<String,Object> snapshot=target.read();runOnUiThread(()->{saving--;if(target!=replica)return;int index=undo.size();undo.add(index,"object-action");redo.clear();saveError=null;try{refreshShared(snapshot);}catch(Exception error){saveError=error.toString();}save();update();});}catch(Exception error){runOnUiThread(()->{saving--;if(target!=replica)return;saveError=error.toString();update();Toast.makeText(this,"Could not paste selection: "+error.getMessage(),Toast.LENGTH_LONG).show();});}});
    }
    private void selectionClipboardCommand(String command){
        String session=selectionSession;if(session==null||selectedEntities.isEmpty())return;AppReplica target=replica;saving++;update();ReplicaService.writes.execute(()->{try{Map<String,Object> result=Json.map(target.command(Arrays.asList("selection",command,session),null));Map<String,Object> clipboard=Json.object("entities",new ArrayList<>(Json.list(result.getOrDefault("entities",Collections.emptyList()))),"copiedAt",System.currentTimeMillis());Map<String,Object> snapshot=target.read();runOnUiThread(()->{saving--;if(target!=replica)return;selectionClipboard=clipboard;getSharedPreferences("settings",0).edit().putString("selectionClipboard",Json.write(clipboard)).apply();saveError=null;if(command.equals("cut")){try{refreshShared(snapshot);}catch(Exception error){saveError=error.toString();}undo.add("object-action");redo.clear();save();deselect();}else save();update();});}catch(Exception error){runOnUiThread(()->{saving--;if(target!=replica)return;saveError=error.toString();update();Toast.makeText(this,"Could not "+command+" selection: "+error.getMessage(),Toast.LENGTH_LONG).show();});}});
    }
    private void selectionCommand(String command){
        String session=selectionSession;if(session==null)return;AppReplica target=replica;final PendingObjectHistory pending;synchronized(lock){pending=new PendingObjectHistory(undo.size());pendingObjectHistory.add(pending);saving++;update();}
        ReplicaService.writes.execute(()->{try{target.command(Arrays.asList("selection",command,session),null);Map<String,Object> snapshot=target.read();runOnUiThread(()->{saving--;if(target!=replica)return;pendingObjectHistory.remove(pending);int index=Math.max(0,Math.min(pending.index,undo.size()));undo.add(index,"object-action");for(PendingObjectHistory later:pendingObjectHistory)if(later.index>=index)later.index++;saveError=null;try{refreshShared(snapshot);}catch(Exception error){saveError=error.toString();}save();if(command.equals("delete"))deselect();update();});}
            catch(Exception error){runOnUiThread(()->{saving--;if(target!=replica)return;pendingObjectHistory.remove(pending);saveError=error.toString();update();Toast.makeText(this,"Could not update selection: "+error.getMessage(),Toast.LENGTH_LONG).show();});}});
    }
    Map<String,Long> eraseTimings;
    void applyErase(String mode,String layer,int erasePage,JSONArray path,float radius){
        final Map<String,Long> trace=eraseTimings;if(trace!=null)trace.put("entered",System.nanoTime());
        AppReplica target=replica;final PendingObjectHistory pending;
        synchronized(lock){pending=new PendingObjectHistory(undo.size());pendingObjectHistory.add(pending);saving++;update();}
        if(trace!=null)trace.put("queued",System.nanoTime());
        Map<String,Object> input=Json.object("mode",mode,"layer",layer,"page",erasePage,"points",path==null?Collections.emptyList():Json.parse(path.toString()),"radius",radius);
        ReplicaService.writes.execute(()->{try{
            if(trace!=null)trace.put("worker_start",System.nanoTime());
            target.command(Arrays.asList("erase","-"),input);if(trace!=null)trace.put("command_done",System.nanoTime());Map<String,Object> snapshot=target.read();if(trace!=null)trace.put("snapshot_done",System.nanoTime());
            runOnUiThread(()->{if(trace!=null)trace.put("ui_start",System.nanoTime());saving--;if(target!=replica)return;pendingObjectHistory.remove(pending);int index=Math.max(0,Math.min(pending.index,undo.size()));undo.add(index,"object-action");for(PendingObjectHistory later:pendingObjectHistory)if(later.index>=index)later.index++;saveError=null;try{refreshShared(snapshot);}catch(Exception e){saveError=e.toString();}if(trace!=null)trace.put("refresh_done",System.nanoTime());save();update();if(trace!=null)trace.put("ui_done",System.nanoTime());});
        }catch(Exception error){runOnUiThread(()->{saving--;if(target!=replica)return;pendingObjectHistory.remove(pending);saveError=error.toString();update();Toast.makeText(this,"Could not erase: "+error.getMessage(),Toast.LENGTH_LONG).show();});}});
    }
    void fillAt(float x,float y){
        final AppReplica target=replica;final PendingObjectHistory pending;
        synchronized(lock){pending=new PendingObjectHistory(undo.size());pendingObjectHistory.add(pending);saving++;update();}
        Map<String,Object> input=Json.object("x",x,"y",y,"page",page,"layer",selected,"color",objectFillColor==Color.TRANSPARENT?color:objectFillColor);
        ReplicaService.writes.execute(()->{try{
            target.command(Arrays.asList("fill","-"),input);Map<String,Object> snapshot=target.read();
            runOnUiThread(()->{saving--;if(target!=replica)return;pendingObjectHistory.remove(pending);int index=Math.max(0,Math.min(pending.index,undo.size()));undo.add(index,"object-action");for(PendingObjectHistory later:pendingObjectHistory)if(later.index>=index)later.index++;saveError=null;try{refreshShared(snapshot);}catch(Exception e){saveError=e.toString();}save();update();});
        }catch(Exception error){runOnUiThread(()->{saving--;if(target!=replica)return;pendingObjectHistory.remove(pending);saveError=error.toString();update();Toast.makeText(this,"Could not fill area: "+error.getMessage(),Toast.LENGTH_LONG).show();});}});
    }
    private void insertSvg(String source,double intrinsicWidth,double intrinsicHeight){insertSvg(source,intrinsicWidth,intrinsicHeight,false);}
    private void insertSvg(String source,double intrinsicWidth,double intrinsicHeight,boolean template){
        double scale=template?1:Math.min(800/intrinsicWidth,1000/intrinsicHeight);float w=(float)(intrinsicWidth*scale),h=(float)(intrinsicHeight*scale);
        JSONObject object=obj("kind","object","object",id(),"shape","svg","layer",selected,"page",page,"x",(1000-w)/2,"y",(1400-h)/2,"w",w,"h",h,"color",color,"width",width,"fontSize",24,"text","","svg",source);
        persistObject(object);
    }
    private void insertImage(String data,double intrinsicWidth,double intrinsicHeight){
        double scale=Math.min(800/intrinsicWidth,1000/intrinsicHeight);float w=(float)(intrinsicWidth*scale),h=(float)(intrinsicHeight*scale);
        JSONObject object=obj("kind","object","object",id(),"shape","image","image",data,"layer",selected,"page",page,"x",(1000-w)/2,"y",(1400-h)/2,"w",w,"h",h,"color",color,"width",width,"fontSize",24,"text","");persistObject(object);
    }
    void textPlacementDialog(){
        EditText content=new EditText(this);content.setHint("Text to place");content.setMinLines(2);content.setGravity(Gravity.TOP);
        EditText size=new EditText(this);size.setSingleLine(true);size.setInputType(8194|4096);size.setText("24");size.setHint("Font size");
        CheckBox bold=new CheckBox(this);bold.setText("Bold");bold.setChecked(pendingTextBold);
        CheckBox italic=new CheckBox(this);italic.setText("Italic");italic.setChecked(pendingTextItalic);
        CheckBox underline=new CheckBox(this);underline.setText("Underline");underline.setChecked(pendingTextUnderline);
        Spinner family=new Spinner(this);family.setAdapter(new ArrayAdapter<String>(this,android.R.layout.simple_spinner_dropdown_item,new String[]{"sans-serif","serif","monospace"}));family.setSelection(pendingTextFamily.equals("serif")?1:pendingTextFamily.equals("monospace")?2:0);
        Spinner alignment=new Spinner(this);alignment.setAdapter(new ArrayAdapter<String>(this,android.R.layout.simple_spinner_dropdown_item,new String[]{"left","center","right"}));alignment.setSelection(pendingTextAlignment.equals("center")?1:pendingTextAlignment.equals("right")?2:0);
        LinearLayout box=new LinearLayout(this);box.setOrientation(1);box.setPadding(24,8,24,8);box.addView(content);box.addView(size);box.addView(bold);box.addView(italic);box.addView(underline);box.addView(new TextView(this){{setText("Font");}});box.addView(family);box.addView(new TextView(this){{setText("Alignment");}});box.addView(alignment);
        new AlertDialog.Builder(this).setTitle("Place text").setMessage("Enter text, then tap the canvas to position it.").setView(box).setNegativeButton("Cancel",null).setPositiveButton("Continue",(d,w)->{try{String value=content.getText().toString();float font=Float.parseFloat(size.getText().toString());if(value.trim().isEmpty()||Float.isNaN(font)||Float.isInfinite(font)||font<1||font>500)throw new IllegalArgumentException("Enter text and a font size from 1 to 500");pendingText=value;pendingTextSize=font;pendingTextBold=bold.isChecked();pendingTextItalic=italic.isChecked();pendingTextUnderline=underline.isChecked();pendingTextFamily=family.getSelectedItem().toString();pendingTextAlignment=alignment.getSelectedItem().toString();setDrawingTool("text");}catch(Exception e){Toast.makeText(this,e.getMessage(),Toast.LENGTH_LONG).show();}}).show();
    }
    void resetDrawingGesture(){drawingShape=null;pendingText=null;if(canvas!=null){canvas.shapeDragging=false;canvas.points=null;canvas.current=null;canvas.eraserPoints=null;canvas.invalidate();}}
    void editObject(String id){synchronized(lock){
        Map<String,Object> original=new LayerEditor(model,selected,page,true).resolve("object",id).content;final String editLayer=selected;final int editPage=page;
        LinearLayout form=new LinearLayout(this);form.setOrientation(1);form.setPadding(24,12,24,12);Map<String,EditText> fields=new LinkedHashMap<>();
        boolean svg="svg".equals(original.get("shape"));
        String[] keys=svg?new String[]{"x","y","w","h"}:new String[]{"text","x","y","w","h","fontSize"};
        String[] labels=svg?new String[]{"X position","Y position","Width","Height"}:new String[]{"Label","X position","Y position","Width","Height","Font size"};
        for(int index=0;index<keys.length;index++){String key=keys[index];TextView label=new TextView(this);label.setText(labels[index]);form.addView(label);EditText field=new EditText(this);field.setText(String.valueOf(original.getOrDefault(key,key.equals("text")?"":24)));if(!key.equals("text"))field.setInputType(8194|4096);form.addView(field);fields.put(key,field);}
        CheckBox bold=null,italic=null,underline=null;Spinner family=null,alignment=null;
        if("text".equals(original.get("shape"))){
            bold=new CheckBox(this);bold.setText("Bold");bold.setChecked(Boolean.TRUE.equals(original.get("bold")));form.addView(bold);
            italic=new CheckBox(this);italic.setText("Italic");italic.setChecked(Boolean.TRUE.equals(original.get("italic")));form.addView(italic);
            underline=new CheckBox(this);underline.setText("Underline");underline.setChecked(Boolean.TRUE.equals(original.get("underline")));form.addView(underline);
            family=new Spinner(this);family.setAdapter(new ArrayAdapter<String>(this,android.R.layout.simple_spinner_dropdown_item,new String[]{"sans-serif","serif","monospace"}));String ff=String.valueOf(original.getOrDefault("fontFamily","sans-serif"));family.setSelection(ff.equals("serif")?1:ff.equals("monospace")?2:0);form.addView(family);
            alignment=new Spinner(this);alignment.setAdapter(new ArrayAdapter<String>(this,android.R.layout.simple_spinner_dropdown_item,new String[]{"left","center","right"}));String al=String.valueOf(original.getOrDefault("alignment","left"));alignment.setSelection(al.equals("center")?1:al.equals("right")?2:0);form.addView(alignment);
        }
        final CheckBox boldField=bold,italicField=italic,underlineField=underline;final Spinner familyField=family,alignmentField=alignment;
        new AlertDialog.Builder(this).setTitle("Edit object").setView(form).setPositiveButton("Save",(d,w)->{
            try{synchronized(lock){Map<String,Object> patch=Json.object();for(Map.Entry<String,EditText> f:fields.entrySet())patch.put(f.getKey(),f.getKey().equals("text")?f.getValue().getText().toString():Double.parseDouble(f.getValue().getText().toString()));
                if(boldField!=null){patch.put("bold",boldField.isChecked());patch.put("italic",italicField.isChecked());patch.put("underline",underlineField.isChecked());patch.put("fontFamily",familyField.getSelectedItem().toString());patch.put("alignment",alignmentField.getSelectedItem().toString());}
                if(!editLayer.equals(selected)||editPage!=page)throw new IllegalStateException("Editing layer changed");patch.put("scope",Json.object("layer",editLayer,"page",editPage));persist(Arrays.asList("object","update",id,"-"),patch,null);undo.add("object-action");redo.clear();save();update();}
            }catch(Exception e){Toast.makeText(this,e.getMessage(),Toast.LENGTH_LONG).show();}
        }).setNeutralButton(Boolean.FALSE.equals(model.objectActive.get(id))?"Restore":"Erase",(d,w)->{
            try{synchronized(lock){if(!editLayer.equals(selected)||editPage!=page)throw new IllegalStateException("Editing layer changed");persist(Arrays.asList("object",Boolean.FALSE.equals(model.objectActive.get(id))?"show":"hide",id,editLayer,String.valueOf(editPage)),null,null);undo.add("object-action");redo.clear();save();update();}}catch(Exception e){Toast.makeText(this,e.getMessage(),1).show();}
        }).setNegativeButton("Cancel",null).show();
    }}

    class CanvasView extends View {
        Paint paint=new Paint(Paint.ANTI_ALIAS_FLAG);
        float zoom=1,offsetX=0,offsetY=0,lastX,lastY;
        JSONArray points,eraserPoints;String current;long lastSent;
        ScaleGestureDetector scaling;
        CanvasView(Context context){super(context);setBackgroundColor(Color.WHITE);scaling=new ScaleGestureDetector(context,new ScaleGestureDetector.SimpleOnScaleGestureListener(){@Override public boolean onScale(ScaleGestureDetector d){if(!pan)return false;float old=zoom;zoom=Math.max(.2f,Math.min(5,zoom*d.getScaleFactor()));offsetX=d.getFocusX()-(d.getFocusX()-offsetX)*zoom/old;offsetY=d.getFocusY()-(d.getFocusY()-offsetY)*zoom/old;invalidate();return true;}});}
        float scale(){return Math.min(getWidth()/1000f,getHeight()/1400f);}
        float x(float x){return (x-offsetX)/(scale()*zoom);}float y(float y){return (y-offsetY)/(scale()*zoom);}
        boolean visible(String layer){JSONObject l=layers.get(layer);return l!=null&&!Boolean.FALSE.equals(model.layerActive.get(layer))&&(l.optBoolean("force")||!hidden.contains(layer));}
        void drawStroke(Canvas c,JSONObject stroke){Render.drawStroke(Json.map(Json.parse(stroke.toString())),new com.notemaker.cli.Png(c));}
        // Cache settled ink at display resolution; extending a page draws only the new strokes.
        final LinkedHashMap<String,InkCache> ink=new LinkedHashMap<String,InkCache>(4,.75f,true);
        String inkTransform="";
        long inputStarted;
        java.util.function.LongConsumer drawLatency;
        Runnable afterDraw;
        boolean useInkCache=true;
        float shapeStartX,shapeStartY,shapeEndX,shapeEndY;boolean shapeDragging;
        boolean transforming;float transformStartX,transformStartY,transformDeltaX,transformDeltaY;long lastPreviewAt;
        boolean lassoing;float lassoStartX,lassoStartY,lassoEndX,lassoEndY;final ArrayList<float[]> lassoPath=new ArrayList<>();
        void transform(Canvas c){c.translate(offsetX,offsetY);c.scale(scale()*zoom,scale()*zoom);if(!type.equals("infinite"))c.clipRect(0,0,1000,1400);}
        void settledInk(Canvas c,String layer){
            InkCache cache=ink.get(layer);if(cache==null){if(ink.size()>=3){String oldest=ink.keySet().iterator().next();ink.remove(oldest).bitmap.recycle();}cache=new InkCache(getWidth(),getHeight());ink.put(layer,cache);}
            LinkedHashMap<String,JSONObject> visible=new LinkedHashMap<>();
            for(Map.Entry<String,JSONObject> e:strokes.entrySet()){JSONObject stroke=e.getValue();if(!e.getKey().equals(current)&&layer.equals(stroke.optString("layer"))&&stroke.optInt("page")==page&&!Boolean.FALSE.equals(active.get(e.getKey())))visible.put(e.getKey(),stroke);}
            cache.draw(c,visible,this::transform,this::drawStroke);
        }
        @Override protected void onDraw(Canvas c){super.onDraw(c);synchronized(lock){
            if(eraseTimings!=null)eraseTimings.put("draw_start",System.nanoTime());
            if(getWidth()==0||getHeight()==0)return;
            String key=document+"/"+page+"/"+type+"/"+getWidth()+"/"+getHeight()+"/"+zoom+"/"+offsetX+"/"+offsetY;
            if(!key.equals(inkTransform)){for(InkCache item:ink.values())item.bitmap.recycle();ink.clear();inkTransform=key;}
            c.save();transform(c);if(!type.equals("infinite")){paint.setColor(0xffaaaaaa);paint.setStyle(Paint.Style.STROKE);paint.setStrokeWidth(1);c.drawRect(0,0,1000,1400,paint);paint.setStyle(Paint.Style.FILL);}c.restore();
            boolean previewing=false;for(Object raw:transformPreviews.values())try{if(((Number)Json.map(raw).getOrDefault("expires",0)).longValue()>System.currentTimeMillis()){previewing=true;break;}}catch(Exception ignored){}
            for(String layer:layers.keySet())if(visible(layer)){
                if(useInkCache&&!previewing)settledInk(c,layer);
                else {c.save();transform(c);for(Map.Entry<String,JSONObject> e:strokes.entrySet()){JSONObject s=e.getValue();if(!e.getKey().equals(current)&&layer.equals(s.optString("layer"))&&s.optInt("page")==page&&!Boolean.FALSE.equals(active.get(e.getKey()))&&!(previewing&&previewed("stroke",e.getKey())))drawStroke(c,s);}c.restore();}
                c.save();transform(c);
                for(Map.Entry<String,Map<String,Object>> e:model.objects.entrySet()){Map<String,Object> o=e.getValue();if(layer.equals(o.get("layer"))&&((Number)o.get("page")).intValue()==page&&!Boolean.FALSE.equals(model.objectActive.get(e.getKey()))&&!(previewing&&previewed("object",e.getKey())))Render.drawObject(model,o,new com.notemaker.cli.Png(c));}
                if(layer.equals(selected)&&points!=null&&current!=null)drawStroke(c,stroke());c.restore();
            }
            drawTransformPreviews(c);drawSelection(c);
            if(lassoing){c.save();transform(c);paint.setColor(Color.BLACK);paint.setStyle(Paint.Style.STROKE);paint.setStrokeWidth(2/Math.max(.2f,zoom));paint.setPathEffect(new DashPathEffect(new float[]{7,5},0));if(freeformLasso&&lassoPath.size()>1){Path outline=new Path();outline.moveTo(lassoPath.get(0)[0],lassoPath.get(0)[1]);for(int i=1;i<lassoPath.size();i++)outline.lineTo(lassoPath.get(i)[0],lassoPath.get(i)[1]);c.drawPath(outline,paint);}else c.drawRect(lassoStartX,lassoStartY,lassoEndX,lassoEndY,paint);paint.setPathEffect(null);paint.setStyle(Paint.Style.FILL);c.restore();}
            if(inputStarted!=0){if(drawLatency!=null)drawLatency.accept(System.nanoTime()-inputStarted);inputStarted=0;}
            if(shapeDragging&&drawingShape!=null&&!drawingShape.equals("text")){float dx=shapeEndX-shapeStartX,dy=shapeEndY-shapeStartY,left=Math.min(shapeStartX,shapeEndX),top=Math.min(shapeStartY,shapeEndY),w=Math.abs(dx),h=Math.abs(dy);if(drawingShape.equals("line")||drawingShape.equals("arrow")){left=shapeStartX;top=shapeStartY;w=dx;h=dy;if(Math.abs(w)+Math.abs(h)<1){w=1;h=1;}}else{w=Math.max(12,w);h=Math.max(12,h);if(!type.equals("infinite")){left=Math.max(0,Math.min(1000-w,left));top=Math.max(0,Math.min(1400-h,top));}}JSONObject preview=objectValue(drawingShape,left,top,w,h,color,width,objectFillColor);c.save();transform(c);Render.drawObject(model,Json.map(Json.parse(preview.toString())),new com.notemaker.cli.Png(c));c.restore();}
            if(eraseTimings!=null)eraseTimings.put("draw_done",System.nanoTime());if(afterDraw!=null)afterDraw.run();
        }}
        JSONObject stroke(){return obj("kind","stroke","stroke",current,"layer",selected,"page",page,"color",color,"width",width,"brush",brush,"points",points);}
        boolean previewed(String kind,String id){long now=System.currentTimeMillis();for(Object raw:transformPreviews.values())try{Map<String,Object> preview=Json.map(raw);if(((Number)preview.getOrDefault("expires",0)).longValue()<=now)continue;for(Object item:Json.list(preview.get("entities"))){Map<String,Object> entity=Json.map(item);if(kind.equals(entity.get("kind"))&&id.equals(entity.get("id")))return true;}}catch(Exception ignored){}return false;}
        void drawTransformPreviews(Canvas c){
            long now=System.currentTimeMillis();
            ArrayList<Object> previews=new ArrayList<>();for(Object raw:transformPreviews.values()){if(pendingTransform!=null&&Objects.equals(Json.map(raw).get("session"),pendingTransform.get("session")))continue;previews.add(raw);}if(pendingTransform!=null){Map<String,Object> local=new LinkedHashMap<>(pendingTransform);local.put("expires",now+250);previews.add(local);}
            for(Object raw:previews)try{
                Map<String,Object> preview=Json.map(raw);if(((Number)preview.getOrDefault("expires",0)).longValue()<=now)continue;Map<String,Object> tr=Json.map(preview.get("transform"));double factor=((Number)tr.getOrDefault("scale",1)).doubleValue(),dx=((Number)tr.getOrDefault("dx",0)).doubleValue(),dy=((Number)tr.getOrDefault("dy",0)).doubleValue(),rotation=((Number)tr.getOrDefault("rotation",0)).doubleValue();boolean flipX=Boolean.TRUE.equals(tr.get("flipX")),flipY=Boolean.TRUE.equals(tr.get("flipY"));List<Object> entities=Json.list(preview.get("entities"));double minX=Double.POSITIVE_INFINITY,minY=minX,maxX=Double.NEGATIVE_INFINITY,maxY=maxX;
                for(Object rawEntity:entities){Map<String,Object> entity=Json.map(rawEntity);String kind=String.valueOf(entity.get("kind")),id=String.valueOf(entity.get("id"));if(kind.equals("object")){Map<String,Object> o=model.objects.get(id);if(o!=null){double x0=((Number)o.get("x")).doubleValue(),y0=((Number)o.get("y")).doubleValue();minX=Math.min(minX,x0);maxX=Math.max(maxX,x0+((Number)o.get("w")).doubleValue());minY=Math.min(minY,y0);maxY=Math.max(maxY,y0+((Number)o.get("h")).doubleValue());}}else{JSONObject s=strokes.get(id);if(s!=null){JSONArray ps=s.optJSONArray("points");for(int i=0;i<ps.length();i++){JSONArray p=ps.optJSONArray(i);minX=Math.min(minX,p.optDouble(0));maxX=Math.max(maxX,p.optDouble(0));minY=Math.min(minY,p.optDouble(1));maxY=Math.max(maxY,p.optDouble(1));}}}}
                if(minX==Double.POSITIVE_INFINITY)continue;double px=((Number)tr.getOrDefault("pivotX",(minX+maxX)/2)).doubleValue(),py=((Number)tr.getOrDefault("pivotY",(minY+maxY)/2)).doubleValue();
                int layerSave=c.saveLayerAlpha(null,96);c.save();transform(c);c.translate((float)(px+dx),(float)(py+dy));c.rotate((float)rotation);c.scale((float)(factor*(flipX?-1:1)),(float)(factor*(flipY?-1:1)));c.translate((float)-px,(float)-py);
                for(Object rawEntity:entities){Map<String,Object> entity=Json.map(rawEntity);String kind=String.valueOf(entity.get("kind")),id=String.valueOf(entity.get("id"));
                    if(kind.equals("object")){Map<String,Object> o=model.objects.get(id);if(o!=null)Render.drawObject(model,o,new com.notemaker.cli.Png(c));}
                    else if(kind.equals("stroke")){JSONObject s=strokes.get(id);if(s!=null)drawStroke(c,s);}
                }
                c.restore();c.restoreToCount(layerSave);
            }catch(Exception ignored){}
        }
        void drawSelection(Canvas c){if(selectedEntities.isEmpty())return;c.save();transform(c);paint.setColor(Color.BLACK);paint.setStyle(Paint.Style.STROKE);paint.setStrokeWidth(2/Math.max(.2f,zoom));paint.setPathEffect(new DashPathEffect(new float[]{8,5},0));for(Map<String,Object> entity:selectedEntities){String kind=String.valueOf(entity.get("kind")),id=String.valueOf(entity.get("id"));if(kind.equals("object")){Map<String,Object> o=model.objects.get(id);if(o!=null){float x=((Number)o.get("x")).floatValue(),y=((Number)o.get("y")).floatValue(),w=((Number)o.get("w")).floatValue(),h=((Number)o.get("h")).floatValue();c.save();c.translate(x+w/2,y+h/2);c.rotate(((Number)o.getOrDefault("rotation",0)).floatValue());c.scale(Boolean.TRUE.equals(o.get("flipX"))?-1:1,Boolean.TRUE.equals(o.get("flipY"))?-1:1);c.drawRect(-w/2-4,-h/2-4,w/2+4,h/2+4,paint);c.restore();}}else{JSONObject s=strokes.get(id);if(s!=null){JSONArray ps=s.optJSONArray("points");float l=Float.MAX_VALUE,t=Float.MAX_VALUE,r=-Float.MAX_VALUE,b=-Float.MAX_VALUE;for(int i=0;i<ps.length();i++){JSONArray p=ps.optJSONArray(i);l=Math.min(l,(float)p.optDouble(0));r=Math.max(r,(float)p.optDouble(0));t=Math.min(t,(float)p.optDouble(1));b=Math.max(b,(float)p.optDouble(1));}c.drawRect(l-8,t-8,r+8,b+8,paint);}}}paint.setPathEffect(null);paint.setStyle(Paint.Style.FILL);c.restore();}
        void emit(){if(points!=null&&points.length()>0){try{enqueue(new JSONObject(stroke().toString()));}catch(Exception e){throw new RuntimeException(e);}lastSent=System.currentTimeMillis();}}
        void point(float sx,float sy,float pressure){float dx=x(sx),dy=y(sy);if(!type.equals("infinite")&&(dx<0||dy<0||dx>1000||dy>1400))return;if(points.length()>=8000){JSONArray reduced=new JSONArray();for(int i=0;i<points.length();i+=2)reduced.put(points.optJSONArray(i));points=reduced;}points.put(new JSONArray(Arrays.asList(dx,dy,Math.max(0,Math.min(1,pressure)))));}
        void erasePoint(float sx,float sy){float dx=x(sx),dy=y(sy);if(type.equals("infinite")||(dx>=0&&dy>=0&&dx<=1000&&dy<=1400))eraserPoints.put(new JSONArray(Arrays.asList(dx,dy)));}
        void eraseAt(float sx,float sy){JSONArray path=new JSONArray();path.put(new JSONArray(Arrays.asList(x(sx),y(sy))));applyErase("stroke",selected,page,path,eraserRadius);}
        boolean scribble(JSONArray path){if(path==null||path.length()<7)return false;double minX=Double.MAX_VALUE,minY=Double.MAX_VALUE,maxX=-1,maxY=-1;int changes=0,previous=0;double spanX=0,spanY=0;for(int i=0;i<path.length();i++){JSONArray p=path.optJSONArray(i);double px=p.optDouble(0),py=p.optDouble(1);minX=Math.min(minX,px);maxX=Math.max(maxX,px);minY=Math.min(minY,py);maxY=Math.max(maxY,py);if(i>0){JSONArray a=path.optJSONArray(i-1);double dx=px-a.optDouble(0),dy=py-a.optDouble(1);if(Math.abs(dx)>Math.abs(dy)){int sign=dx>2?1:dx< -2?-1:0;if(sign!=0){if(previous!=0&&sign!=previous)changes++;previous=sign;}}else{int sign=dy>2?1:dy< -2?-1:0;if(sign!=0){if(previous!=0&&sign!=previous)changes++;previous=sign;}}}}spanX=maxX-minX;spanY=maxY-minY;return changes>=3&&Math.max(spanX,spanY)>=Math.max(30,eraserRadius*2);}
        @Override public boolean onTouchEvent(MotionEvent event){
            if(inputStarted==0)inputStarted=System.nanoTime();
            synchronized(lock){
                scaling.onTouchEvent(event);int action=event.getActionMasked();float sx=event.getX(),sy=event.getY();
                if(pan){if(action==MotionEvent.ACTION_DOWN){lastX=sx;lastY=sy;}if(action==MotionEvent.ACTION_MOVE&&!scaling.isInProgress()&&event.getPointerCount()==1){offsetX+=sx-lastX;offsetY+=sy-lastY;invalidate();}lastX=sx;lastY=sy;return true;}
                if(event.getPointerCount()>1){if(shapeDragging)shapeDragging=false;if(eraserPoints!=null)eraserPoints=null;invalidate();return true;}
                if(action==MotionEvent.ACTION_DOWN){
                    if(!visible(selected)){Toast.makeText(MainActivity.this,"Show the selected layer to draw on it",0).show();return true;}
                    if(fillTapMode){fillTapMode=false;fillAt(x(sx),y(sy));invalidate();return true;}
                    if(selecting){if(selectedEntities.isEmpty()){lassoing=true;lassoStartX=lassoEndX=x(sx);lassoStartY=lassoEndY=y(sy);lassoPath.clear();lassoPath.add(new float[]{lassoStartX,lassoStartY});}else{Map<String,Object> hit=selectedEntityAt(x(sx),y(sy));if(hit!=null){deselectEntity(String.valueOf(hit.get("kind")),String.valueOf(hit.get("id")));return true;}transforming=true;transformStartX=x(sx);transformStartY=y(sy);transformDeltaX=transformDeltaY=0;}return true;}
                    if(drawingShape!=null){float dx=x(sx),dy=y(sy);if(!type.equals("infinite")&&(dx<0||dy<0||dx>1000||dy>1400))return true;shapeStartX=shapeEndX=dx;shapeStartY=shapeEndY=dy;shapeDragging=true;invalidate();return true;}
                    if(erase){eraserPoints=new JSONArray();erasePoint(sx,sy);invalidate();}
                    else{current=id();points=new JSONArray();point(sx,sy,event.getPressure());if(!scribbleToErase)emit();}
                }
                if(action==MotionEvent.ACTION_MOVE){
                    if(lassoing){lassoEndX=x(sx);lassoEndY=y(sy);if(freeformLasso){for(int i=0;i<event.getHistorySize();i++)addLassoPoint(x(event.getHistoricalX(i)),y(event.getHistoricalY(i)));addLassoPoint(lassoEndX,lassoEndY);}invalidate();return true;}
                    if(transforming){transformDeltaX=x(sx)-transformStartX;transformDeltaY=y(sy)-transformStartY;if(System.currentTimeMillis()-lastPreviewAt>100){previewSelection(1,transformDeltaX,transformDeltaY);lastPreviewAt=System.currentTimeMillis();}invalidate();return true;}
                    if(shapeDragging){shapeEndX=x(sx);shapeEndY=y(sy);if(!type.equals("infinite")){shapeEndX=Math.max(0,Math.min(1000,shapeEndX));shapeEndY=Math.max(0,Math.min(1400,shapeEndY));}invalidate();return true;}
                    if(erase&&eraserPoints!=null){for(int i=0;i<event.getHistorySize();i++)erasePoint(event.getHistoricalX(i),event.getHistoricalY(i));erasePoint(sx,sy);invalidate();}
                    else if(points!=null){for(int i=0;i<event.getHistorySize();i++)point(event.getHistoricalX(i),event.getHistoricalY(i),event.getHistoricalPressure(i));point(sx,sy,event.getPressure());if(!scribbleToErase&&System.currentTimeMillis()-lastSent>100)emit();invalidate();}
                }
                if(action==MotionEvent.ACTION_CANCEL){
                    if(lassoing){lassoing=false;lassoPath.clear();invalidate();return true;}
                    if(transforming){transforming=false;cancelTransformPreview();}
                    else if(shapeDragging)shapeDragging=false;
                    else if(eraserPoints!=null)eraserPoints=null;
                    else if(points!=null&&!scribbleToErase){point(sx,sy,event.getPressure());emit();undo.add(current);redo.clear();save();}
                    points=null;current=null;invalidate();return true;
                }
                if(action==MotionEvent.ACTION_UP){
                    if(lassoing){lassoEndX=x(sx);lassoEndY=y(sy);if(freeformLasso)addLassoPoint(lassoEndX,lassoEndY);ArrayList<float[]> polygon=new ArrayList<>(lassoPath);boolean drag;if(freeformLasso&&polygon.size()>1){float minX=Float.POSITIVE_INFINITY,minY=minX,maxX=Float.NEGATIVE_INFINITY,maxY=maxX;for(float[] p:polygon){minX=Math.min(minX,p[0]);minY=Math.min(minY,p[1]);maxX=Math.max(maxX,p[0]);maxY=Math.max(maxY,p[1]);}drag=Math.max(maxX-minX,maxY-minY)>12;}else drag=Math.hypot(lassoEndX-lassoStartX,lassoEndY-lassoStartY)>12;float lx=lassoStartX,ly=lassoStartY,rx=lassoEndX,ry=lassoEndY;lassoing=false;lassoPath.clear();if(drag){if(freeformLasso&&polygon.size()>=3)selectLasso(polygon);else selectWithin(lx,ly,rx,ry);}else selectAt(rx,ry);invalidate();return true;}
                    if(transforming){transformDeltaX=x(sx)-transformStartX;transformDeltaY=y(sy)-transformStartY;transforming=false;if(Math.abs(transformDeltaX)+Math.abs(transformDeltaY)>1)transformSelection(1,transformDeltaX,transformDeltaY,true);else cancelTransformPreview();return true;}
                    if(shapeDragging){shapeEndX=x(sx);shapeEndY=y(sy);if(!type.equals("infinite")){shapeEndX=Math.max(0,Math.min(1000,shapeEndX));shapeEndY=Math.max(0,Math.min(1400,shapeEndY));}shapeDragging=false;String shape=drawingShape;if(shape.equals("text")){if(pendingText!=null){placeText(pendingText,pendingTextSize,shapeStartX,shapeStartY);pendingText=null;}drawingShape=null;}else{float dx=shapeEndX-shapeStartX,dy=shapeEndY-shapeStartY;float left=Math.min(shapeStartX,shapeEndX),top=Math.min(shapeStartY,shapeEndY),w=Math.abs(dx),h=Math.abs(dy);if(shape.equals("line")||shape.equals("arrow")||shape.equals("arrowLine")){left=shapeStartX;top=shapeStartY;w=dx;h=dy;if(Math.abs(w)+Math.abs(h)<1){w=1;h=1;}}else{w=Math.max(12,w);h=Math.max(12,h);if(!type.equals("infinite")){left=Math.max(0,Math.min(1000-w,left));top=Math.max(0,Math.min(1400-h,top));}}persistObject(objectValue(shape,left,top,w,h,color,width,objectFillColor));}invalidate();return true;}
                    if(eraserPoints!=null){erasePoint(sx,sy);JSONArray path=eraserPoints;eraserPoints=null;if(!"page".equals(eraserMode))applyErase(eraserMode,selected,page,path,eraserRadius);}
                    else if(points!=null){point(sx,sy,event.getPressure());if(scribbleToErase&&scribble(points))applyErase("area",selected,page,points,eraserRadius);else{emit();undo.add(current);redo.clear();save();}points=null;current=null;}
                }
                invalidate();return true;
            }
        }
    }
}
