package com.notemaker;

import android.os.Handler;
import android.os.Looper;
import com.notemaker.cli.LocalProxy;
import java.util.*;
import java.util.concurrent.*;
import static com.notemaker.core.Json.*;
import static com.notemaker.core.Model.require;

/** Semantic controls for a live Activity. Register every new editor control here. */
final class UiActions implements LocalProxy.UiSession {
    private interface Action { void run(Map<String,Object> input) throws Exception; }
    private final MainActivity app;
    private final Handler main=new Handler(Looper.getMainLooper());
    private final LinkedHashMap<String,Action> actions=new LinkedHashMap<>();
    private final LinkedHashMap<String,String> descriptions=new LinkedHashMap<>();

    UiActions(MainActivity app){
        this.app=app;
        add("panel.open","{name: document|pen|eraser|template|layers|shapes|fill|select|text|insert|move|page|agent|more|history|conflicts|network|preferences|presets|brushes}",a->app.editor.openPanel(string(a,"name")));
        add("panel.close","{}",a->app.editor.dismissPopup());
        add("files.open","{}",a->app.editor.showFiles());
        add("files.close","{}",a->{if(app.browser!=null)app.browser.dismiss();});
        add("files.search","{query: string}",a->{require(app.browser!=null&&app.browser.showing(),"Files browser is closed");app.browser.search(string(a,"query"));});
        add("files.mode","{mode: grid|list}",a->{require(app.browser!=null&&app.browser.showing(),"Files browser is closed");app.browser.mode(string(a,"mode"));});
        add("files.refresh","{}",a->{require(app.browser!=null&&app.browser.showing(),"Files browser is closed");app.browser.refreshNow();});
        add("document.open","{name: string}",a->{String name=string(a,"name").trim();require(!name.isEmpty()&&name.length()<=200,"Document name must be 1–200 characters");if(app.browser!=null)app.browser.dismiss();app.editor.dismissPopup();app.openDocument(name,"fixed");});
        add("page.open","{page: integer, zero based}",a->{require(app.type.equals("pages"),"Document has no pages");int page=integer(a,"page",0,10000);app.deselect();app.resetDrawingGesture();app.page=page;app.save();app.update();});
        add("page.next","{}",a->{require(app.type.equals("pages")&&app.page<10000,"No next page");app.deselect();app.resetDrawingGesture();app.page++;app.save();app.update();});
        add("page.previous","{}",a->{require(app.type.equals("pages")&&app.page>0,"No previous page");app.deselect();app.resetDrawingGesture();app.page--;app.save();app.update();});
        add("layer.select","{id: layer ID}",a->{String id=string(a,"id");require(app.layers.containsKey(id)&&!Boolean.FALSE.equals(app.model.layerActive.get(id)),"Layer is unavailable");app.selectLayer(id);});
        add("layer.visibility","{id: layer ID, visible: boolean} — device view only",a->{String id=string(a,"id");require(app.layers.containsKey(id),"Unknown layer");boolean visible=bool(a,"visible");if(visible)app.hidden.remove(id);else app.hidden.add(id);app.save();app.update();});
        add("tool.select","{name: pen|eraser|move|shapes|text|fill|select|interact}",a->{String name=string(a,"name");require(Arrays.asList("pen","eraser","move","shapes","text","fill","select","interact").contains(name),"Unknown drawing tool");app.editor.openPanel(name);});
        add("shape.select","{id: shape ID}",a->{String id=string(a,"id");require(com.notemaker.core.ShapeCatalog.ids().contains(id),"Unknown shape");app.editor.dismissPopup();app.setDrawingTool(id);});
        add("pen.configure","{brush?, color?, width?, material?}",a->{if(a.containsKey("brush")){String brush=string(a,"brush");require(Arrays.asList("pen","pencil","marker","brush","oily","fountain","charcoal","neoBrush","latinCalligraphy","asiaCalligraphy","square").contains(brush),"Unknown brush");app.brush=brush;}if(a.containsKey("color"))app.color=integer(a,"color",Integer.MIN_VALUE,Integer.MAX_VALUE);if(a.containsKey("width"))app.width=integer(a,"width",1,30);if(a.containsKey("material")){app.material=integer(a,"material",0,100);app.getSharedPreferences("settings",0).edit().putInt("penMaterial",app.material).apply();}app.update();});
        add("pen.preset.save","{name: string}",a->app.editor.savePenPreset(string(a,"name")));
        add("pen.preset.apply","{name: string}",a->app.editor.applyPenPreset(string(a,"name")));
        add("shape.fill","{enabled: boolean}",a->{app.objectFillColor=bool(a,"enabled")?app.color:android.graphics.Color.TRANSPARENT;app.update();});
        add("fill.tap","{enabled: boolean}",a->{app.fillTapMode=bool(a,"enabled");if(app.fillTapMode){app.selecting=false;if(app.objectFillColor==android.graphics.Color.TRANSPARENT)app.objectFillColor=app.color;}app.editor.dismissPopup();app.update();});
        add("eraser.configure","{mode?, radius?, scribble?}",a->{if(a.containsKey("mode")){String mode=string(a,"mode");require(Arrays.asList("stroke","partial","area").contains(mode),"Unknown eraser mode");app.eraserMode=mode;}if(a.containsKey("radius"))app.eraserRadius=integer(a,"radius",1,80);if(a.containsKey("scribble")){app.scribbleToErase=bool(a,"scribble");app.getSharedPreferences("settings",0).edit().putBoolean("scribbleToErase",app.scribbleToErase).apply();}app.update();});
        add("selection.lasso","{enabled: boolean}",a->{app.freeformLasso=bool(a,"enabled");app.getSharedPreferences("settings",0).edit().putBoolean("freeformLasso",app.freeformLasso).apply();app.update();});
        add("selection.all","{}",a->app.selectAllVisible());
        add("selection.choose","{entities: [{kind: object|stroke, id: string}]}",a->{List<Object> raw=list(a.get("entities"));require(!raw.isEmpty()&&raw.size()<=100,"Choose 1–100 entities");ArrayList<Map<String,Object>> refs=new ArrayList<>();for(Object value:raw){Map<String,Object> ref=map(value);String kind=string(ref,"kind"),id=string(ref,"id");require(kind.equals("object")||kind.equals("stroke"),"Unknown entity kind");refs.add(object("kind",kind,"id",id,"page",app.page));}app.startSelection(refs);});
        add("selection.clear","{}",a->app.deselect());
        add("view.zoom","{factor: positive number}",a->{double factor=number(a,"factor");require(Double.isFinite(factor)&&factor>0,"Zoom factor must be positive");app.canvas.zoom=(float)Math.max(.2,Math.min(5,app.canvas.zoom*factor));app.canvas.invalidate();});
        add("view.pan","{dx: number, dy: number} — device pixels",a->{double dx=number(a,"dx"),dy=number(a,"dy");require(Double.isFinite(dx)&&Double.isFinite(dy),"Pan offsets must be finite");app.canvas.offsetX+=(float)dx;app.canvas.offsetY+=(float)dy;app.canvas.invalidate();});
        add("view.reset","{}",a->{app.canvas.zoom=1;app.canvas.offsetX=app.canvas.offsetY=0;app.canvas.invalidate();});
        add("toolbar.labels","{visible: boolean}",a->app.editor.showToolLabels(bool(a,"visible")));
        add("network.configure","{server: string, port: string, proxy: string, token: string}",a->{String server=string(a,"server"),port=string(a,"port"),proxy=string(a,"proxy"),token=string(a,"token");MainActivity.composeServerAddress(server,port,app.host);require(proxy.isEmpty()||(proxy.startsWith("http://")||proxy.startsWith("https://"))&&!token.isEmpty(),"Proxy URL requires an HTTP(S) URL and token");app.saveNetworkSettings(server,port,proxy,token);});
        add("history.undo","{}",a->app.history(false));
        add("history.redo","{}",a->app.history(true));
        add("template.insert","{name: ruled|grid|dots|planner|blank}",a->{String name=string(a,"name");require(Arrays.asList("ruled","grid","dots","planner","blank").contains(name),"Unknown template");app.insertTemplate(name);});
        add("agent.open","{}",a->app.agent.show());
        add("agent.close","{}",a->app.agent.dismiss());
        add("gif.player.open","{}",a->app.animationPlayer.show());
        add("gif.import.open","{}",a->app.chooseGif());
        add("gif.select","{id: GIF object ID}",a->app.animationPlayer.select(string(a,"id")));
        add("gif.play","{}",a->app.animationPlayer.play(true));
        add("gif.pause","{}",a->app.animationPlayer.play(false));
        add("gif.next","{}",a->app.animationPlayer.step(1));
        add("gif.previous","{}",a->app.animationPlayer.step(-1));
        add("gif.seek","{frame: zero-based integer}",a->app.animationPlayer.seek(integer(a,"frame",0,499)));
    }
    private void add(String name,String schema,Action action){actions.put(name,action);descriptions.put(name,schema);}
    private static int integer(Map<String,Object> a,String key,int min,int max){Object value=a.get(key);require(value instanceof Number&&((Number)value).doubleValue()==((Number)value).intValue(),key+" must be an integer");int n=((Number)value).intValue();require(n>=min&&n<=max,key+" is out of range");return n;}
    private static double number(Map<String,Object> a,String key){Object value=a.get(key);require(value instanceof Number,key+" must be a number");return ((Number)value).doubleValue();}
    private static boolean bool(Map<String,Object> a,String key){require(a.get(key) instanceof Boolean,key+" must be boolean");return (Boolean)a.get(key);}
    private Map<String,Object> state(){return object("gifPlayer",app.animationPlayer.state(),"session",app.appSessionId,"deviceId",app.deviceId,"deviceName",android.os.Build.MANUFACTURER+" "+android.os.Build.MODEL,"platform","Android","startedAt",app.appSessionStartedAt,"document",app.document,"documentType",app.type,"page",app.page,"layer",app.selected,"tool",app.editor.selectedTool(),"filesOpen",app.browser!=null&&app.browser.showing(),"files",app.browser!=null&&app.browser.showing()?app.browser.state():null,"panelOpen",app.editor.panelShowing(),"agentOpen",app.agent.showing(),"foreground",app.foreground,"zoom",app.canvas.zoom,"offsetX",app.canvas.offsetX,"offsetY",app.canvas.offsetY,"color",app.color,"brush",app.brush,"width",app.width,"material",app.material,"eraserMode",app.eraserMode,"eraserRadius",app.eraserRadius,"selectedCount",app.selectedEntities.size());}
    @Override public Object request(String verb,String action,Map<String,Object> input) throws Exception {
        Callable<Object> task=()->{
            require(app.running,"App session is closed");
            if(verb.equals("state"))return state();
            if(verb.equals("actions"))return object("session",app.appSessionId,"actions",new LinkedHashMap<>(descriptions));
            require(verb.equals("action"),"Unknown UI request");Action selected=actions.get(action);require(selected!=null,"Unknown UI action: "+action);
            selected.run(input);return object("action",action,"state",state());
        };
        if(Looper.myLooper()==Looper.getMainLooper())return task.call();
        FutureTask<Object> pending=new FutureTask<>(task);main.post(pending);
        try{return pending.get(30,TimeUnit.SECONDS);}catch(ExecutionException failed){Throwable cause=failed.getCause();if(cause instanceof Exception)throw (Exception)cause;throw failed;}catch(TimeoutException timeout){pending.cancel(false);throw new IllegalStateException("UI session did not respond within 30 seconds");}
    }
}
