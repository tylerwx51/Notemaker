package com.notemaker.cli;

import com.notemaker.core.*;
import java.io.*;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.util.*;
import static com.notemaker.core.Json.*;
import static com.notemaker.core.Model.require;

/** Deterministic local actions. No implicit network calls or AI provider dependency. */
public final class Main {
    private Main() {}
    static final ThreadLocal<Object> proxyInput=new ThreadLocal<>();
    static final ThreadLocal<Boolean> inProxy=new ThreadLocal<>();
    public static final String HELP = "NoteMaker CLI\nUsage: notemaker [OPTIONS] COMMAND [ARGS]\n"
        + "Options precede the command:\n"
        + "--state DIR                Local replica; default NOTEMAKER_STATE, otherwise\n"
        + "                           $XDG_STATE_HOME/notemaker or ~/.local/state/notemaker\n"
        + "--document NAME            Case-sensitive document name (default: notes)\n"
        + "--actor ID                 Stable editing identity; default NOTEMAKER_ACTOR, otherwise cli\n"
        + "--proxy-file FILE          Selected proxy; default NOTEMAKER_PROXY_FILE\n"
        + "--help                     Show this help without contacting a server\n\n"
        + "ui sessions | ui state SESSION | ui actions SESSION\n"
        + "ui action SESSION NAME FILE|-   Control one running app session with JSON arguments\n"
        + "serve [PORT [UPSTREAM]]     Start replica proxy; upstream URL or descriptor\n"
        + "document create [fixed|pages|infinite] | document delete | document list\ninspect | status | log\n"
        + "diagram add FILE|-         Atomic new layer and editable diagram objects\n"
        + "svg add FILE.svg|- [PLACEMENT.json]   Place SVG; optional {x,y,w,h,layer,page,object}\n"
        + "shape list                   List supported editable shapes\n"
        + "layer put FILE|-           {layer,name,force} (create or update)\nlayer hide ID | layer show ID\n"
        + "layer duplicate ID | layer merge SOURCE TARGET | layer reorder ID INDEX\n"
        + "object put [snapshot] FILE|-  Create/replace an editable object\n"
        + "object update ID FILE|-    Patch geometry, label or styling\n"
        + "object hide ID | object show ID\n"
        + "selection query FILE|- | selection select FILE|- | selection heartbeat SESSION | selection deselect SESSION\n"
        + "selection copy SESSION | selection cut SESSION | selection paste FILE|-\n"
        + "selection duplicate SESSION | selection delete SESSION | selection disconnect\n"
        + "transform preview FILE|- | transform commit FILE|- | transform cancel SESSION\n"
        + "conflicts | conflict resolve ID local|remote|both\n"
        + "stroke put FILE|- | stroke hide ID | stroke show ID\n"
        + "erase FILE|- | fill FILE|- | template add FILE|- | image add FILE [PLACEMENT.json]\n"
        + "apply FILE|-               Atomic JSON array of protocol actions\n"
        + "append [snapshot] FILE|-   Append one raw action; optionally return post-edit snapshot\n"
        + "settings FILE|-            Update local editor preferences\n"
        + "snapshot                   Materialized model and local editor metadata\n"
        + "checkpoint                 Fold the local journal into its durable base\n"
        + "undo | redo [snapshot]     History for the selected actor; optional post-edit snapshot\n"
        + "export FILE.svg|FILE.png [PAGE [X Y WIDTH HEIGHT]]   Page is zero-based\n"
        + "sync [UPSTREAM]            Request proxy synchronization (URL or proxy descriptor)\n"
        + "configure UPSTREAM         Set sole upstream; empty string disconnects\n"
        + "FILE|- accepts JSON from a file or stdin (svg add reads raw SVG).\n"
        + "Edits require a running selected proxy and work offline; document list lists local documents only.\n"
        + "JSON on stdout (except help); JSON errors on stderr and exit 1.\n"
        + "Mutations are not idempotent: inspect log before retrying a lost result.\n"
        + "sync and document delete retries are idempotent. Deletion is permanent; names cannot be reused.\n"
        + "See apps/cli/README.md for input schemas.\n\n"
        + "Examples:\n"
        + "  notemaker --document notes status\n"
        + "  notemaker --proxy-file /path/to/replica/proxy.json --document notes sync http://127.0.0.1:8765\n";
    public static void main(String[] args){try{Object result=run(args);if(result!=null)System.out.println(write(result));}catch(Exception e){Throwable cause=e;while(cause.getCause()!=null)cause=cause.getCause();System.err.println(write(object("error",cause.toString())));System.exit(1);}}
    public static Object run(String[] args) throws Exception {
        List<String>a=new ArrayList<>(Arrays.asList(args));
        if(a.isEmpty()||a.contains("--help")||(a.size()==1&&a.get(0).equals("help"))){System.out.print(HELP);return null;}
        String state=System.getenv("NOTEMAKER_STATE");if(state==null){String xdg=System.getenv("XDG_STATE_HOME");state=(xdg==null?System.getProperty("user.home")+"/.local/state":xdg)+"/notemaker";}
        String doc="notes",actor=System.getenv("NOTEMAKER_ACTOR"),proxy=System.getenv("NOTEMAKER_PROXY_FILE");
        if(actor==null||actor.isEmpty())actor="cli";
        while(!a.isEmpty()&&a.get(0).startsWith("--")){String flag=a.remove(0);require(!a.isEmpty(),"Missing "+flag+" value");String value=a.remove(0);switch(flag){case "--state":state=value;break;case "--document":doc=value;break;case "--actor":actor=value;break;case "--proxy-file":proxy=value;break;default:throw new IllegalArgumentException("Unknown option "+flag);}}
        require(!a.isEmpty(),"Missing command");
        if(a.get(0).equals("serve"))proxy=null;
        Model remoteModel=null;
        if(Boolean.TRUE.equals(inProxy.get()))proxy=null;
        if(proxy!=null){
            Map<String,Object> descriptor=map(parse(new String(Files.readAllBytes(Paths.get(proxy)),StandardCharsets.UTF_8)));
            Object value=null;String cmd=a.get(0);
            if(cmd.equals("svg")){value=svgInput(a.subList(1,a.size()));a=new ArrayList<>(Arrays.asList("svg","add","-"));}
            else if(cmd.equals("image")){Map<String,Object> placement=imagePlacement(a);placement.put("shape","image");placement.put("image",ImageAsset.fromBytes(imageBytes(a.get(2))));value=placement;a=new ArrayList<>(Arrays.asList("image","add","-"));}
            else if((cmd.equals("selection")&&a.size()==3&&Arrays.asList("select","paste","query").contains(a.get(1)))||(cmd.equals("transform")&&a.size()==3&&Arrays.asList("preview","commit").contains(a.get(1)))){int last=a.size()-1;value=input(a.get(last));a.set(last,"-");}
            else if(cmd.equals("ui")&&a.size()==5&&a.get(1).equals("action")){value=input(a.get(4));a.set(4,"-");}
            else if(cmd.equals("append")||cmd.equals("settings")||cmd.equals("apply")||cmd.equals("diagram")||cmd.equals("erase")||cmd.equals("fill")||cmd.equals("template")||((cmd.equals("object")||cmd.equals("layer")||cmd.equals("stroke")||cmd.equals("image"))&&(a.contains("put")||a.contains("update")||a.contains("add")))){
                int last=a.size()-1;value=input(a.get(last));a.set(last,"-");
            }
            if(cmd.equals("export"))remoteModel=Model.fromJson(map(LocalProxy.request(descriptor,doc,actor,Arrays.asList("inspect"),null)));
            else return LocalProxy.request(descriptor,doc,actor,a,value);
        }
        String command=a.remove(0);if(command.equals("ui"))throw new IllegalArgumentException("UI commands require the running tablet's local --proxy-file");
        File directory=new File(state);Store store=remoteModel==null?(Boolean.TRUE.equals(inProxy.get())?Store.writer(directory,doc):new Store(directory,doc)):null;
        boolean readOnly=Arrays.asList("inspect","snapshot","status","log","export","serve").contains(command)||(command.equals("document")&&a.equals(Arrays.asList("list")))||(command.equals("shape")&&a.equals(Arrays.asList("list")));
        require(readOnly||Boolean.TRUE.equals(inProxy.get()),"Writes and synchronization require --proxy-file (or NOTEMAKER_PROXY_FILE); start the selected proxy with serve");
        final String actionActor=actor;
        switch(command){
            case "serve":{
                require(a.size()<=2,"serve [PORT [SERVER_URL]]");
                try(LocalProxy service=new LocalProxy(directory,a.isEmpty()?8766:Integer.parseInt(a.get(0)))){
                    if(a.size()==2)service.configure(doc,a.get(1));
                    System.out.println(write(object("port",service.port(),"descriptor",new File(directory,"proxy.json").getAbsolutePath())));System.out.flush();service.await();
                }return null;
            }

            case "document":{
                require(a.size()>=1,"document create|delete|list");String sub=a.remove(0);
                if(sub.equals("list")){arity(a,0);List<Object> documents=new ArrayList<>();File[] files=directory.listFiles((d,n)->n.startsWith("document-")&&n.endsWith(".json"));if(files!=null)for(File f:files){Map<String,Object>s=map(parse(new String(Files.readAllBytes(f.toPath()),StandardCharsets.UTF_8)));String name=String.valueOf(s.getOrDefault("document",f.getName()));if(!new Store(directory,name).model().deleted)documents.add(name);}return object("documents",documents);}
                if(sub.equals("delete")){arity(a,0);return store.deleteDocument(actor);}
                require(sub.equals("create")&&a.size()<=1,"document create [fixed|pages|infinite]");String type=a.isEmpty()?"fixed":a.get(0);Map<String,Object> op=object("kind","document","type",type);Model.validate(op);
                store.edit(s->{require(list(s.get("events")).isEmpty()&&list(s.get("pending")).isEmpty(),"Document already exists");op.put("id",Actions.id());op.put("actor",actionActor);list(s.get("pending")).add(op);});return object("document",doc,"type",type);
            }
            case "inspect":arity(a,0);return store.model().json();
            case "snapshot":{
                arity(a,0);return snapshot(store.read());
            }
            case "append":{
                boolean withSnapshot=a.size()==2&&a.get(0).equals("snapshot");if(!withSnapshot)arity(a,1);
                Map<String,Object> s=store.append(map(input(a.get(a.size()-1))),actor);List<Object> pending=list(s.get("pending"));
                Map<String,Object> result=object("operation",map(pending.get(pending.size()-1)).get("id"));
                if(withSnapshot)result.put("snapshot",snapshot(s));return result;
            }
            case "settings":{
                arity(a,1);Map<String,Object> preferences=map(input(a.get(0)));
                Store.validateSettings(preferences);
                store.edit(s->s.putAll(preferences));return object("saved",true);
            }
            case "status":{arity(a,0);Map<String,Object>s=store.read();return object("document",doc,"deleted",Store.model(s).deleted,"cursor",s.get("cursor"),"pending",list(s.get("pending")).size(),"revision",s.get("revision"),"serverId",s.get("serverId"));}
            case "log":arity(a,0);return store.read();
            case "checkpoint":arity(a,0);return store.checkpoint();
            case "undo":case "redo":{
                boolean withSnapshot=a.size()==1&&a.get(0).equals("snapshot");if(!withSnapshot)arity(a,0);
                Map<String,Object> result=store.history(actor,command.equals("redo"),withSnapshot);
                if(withSnapshot)result.put("snapshot",snapshot(map(result.remove("state"))));return result;
            }
            case "diagram":arity(a,2);require(a.get(0).equals("add"),"diagram add FILE|-");Map<String,Object> diagram=map(input(a.get(1)));return store.commit(actor,m->Actions.diagram(m,diagram));
            case "svg":{
                Map<String,Object> raw=svgInput(a);raw.put("shape","svg");Map<String,Object> drawing=Actions.drawing(raw,"base");
                return store.commit(actor,m->{require(!m.objects.containsKey(drawing.get("object")),"Object ID already exists; use object update");return Arrays.asList(drawing);});
            }
            case "shape":{arity(a,1);require(a.get(0).equals("list"),"shape list");return object("shapes",ShapeCatalog.json());}
            case "transform":{
                if(a.size()==2&&a.get(0).equals("cancel"))return object("cancelled",true,"session",a.get(1));
                require(a.size()==2&&Arrays.asList("preview","commit").contains(a.get(0)),"transform preview|commit FILE|-");Map<String,Object> request=map(input(a.get(1)));String session=string(request,"session");
                List<Object> entities=list(request.get("entities"));Map<String,Object> transform=map(request.get("transform"));for(String key:transform.keySet())require(Arrays.asList("scale","dx","dy","rotation","flipX","flipY","pivotX","pivotY").contains(key),"Unknown transform field: "+key);
                if(a.get(0).equals("preview"))return object("preview",true,"session",session);
                LocalProxy.assertSession(store,actionActor,session,entities);
                return store.commitSelected(actionActor,session,entities,m->Collaboration.transform(m,entities,transform),entities);
            }
            case "conflicts":arity(a,0);return object("conflicts",store.conflicts());
            case "conflict":{require(a.size()==3&&a.get(0).equals("resolve"),"conflict resolve ID local|remote|both");return store.conflict(a.get(1),a.get(2),actionActor);}
            case "erase":{arity(a,1);Map<String,Object> request=map(input(a.get(0)));return store.commit(actionActor,m->DrawingActions.erase(m,request,ItemCoverage::create));}
            case "fill":{arity(a,1);Map<String,Object> request=map(input(a.get(0)));Model model=store.model();List<Object> dependencies=new ArrayList<>();if(request.containsKey("object"))dependencies.add(object("kind","object","id",request.get("object"),"page",request.getOrDefault("page",0)));else if(request.containsKey("stroke"))dependencies.add(object("kind","stroke","id",request.get("stroke"),"page",request.getOrDefault("page",0)));else{int page=LayerEditor.page(request);String layer=String.valueOf(request.getOrDefault("layer","base"));for(Map<String,Object> entity:model.strokes.values())if(page==((Number)entity.getOrDefault("page",0)).intValue()&&layer.equals(entity.get("layer"))&&!Boolean.FALSE.equals(model.layerActive.get(layer))&&!Boolean.FALSE.equals(model.active.get(entity.get("stroke"))))dependencies.add(object("kind","stroke","id",entity.get("stroke"),"page",page));for(Map<String,Object> entity:model.objects.values())if(page==((Number)entity.getOrDefault("page",0)).intValue()&&layer.equals(entity.get("layer"))&&!Boolean.FALSE.equals(model.layerActive.get(layer))&&!Boolean.FALSE.equals(model.objectActive.get(entity.get("object"))))dependencies.add(object("kind","object","id",entity.get("object"),"page",page));}return store.commit(actionActor,m->DrawingActions.fill(m,request),dependencies);}
            case "template":{arity(a,2);require(a.get(0).equals("add"),"template add FILE|-");Map<String,Object> request=map(input(a.get(1)));return store.commit(actionActor,m->DrawingActions.template(m,request));}
            case "image":{if(Boolean.TRUE.equals(inProxy.get())){arity(a,2);Map<String,Object> placement=map(input(a.get(1)));return store.commit(actionActor,m->Arrays.asList(Actions.drawing(placement,"base")));}require(a.size()>=2&&a.get(0).equals("add"),"image add FILE [PLACEMENT.json]");Map<String,Object> placement=imagePlacement(a);byte[] bytes=imageBytes(a.get(1));return store.commit(actionActor,m->DrawingActions.image(m,bytes,placement));}
            case "apply":arity(a,1);List<Object> ops=list(input(a.get(0)));return store.commit(actor,m->ops);
            case "selection":{
                require(a.size()>=1,"selection select|heartbeat|deselect|disconnect|duplicate|delete|copy|cut|paste");String sub=a.remove(0);
                if(sub.equals("query")){arity(a,1);Map<String,Object> request=map(input(a.get(0)));Model m=store.model();LayerEditor editor=new LayerEditor(m,(String)request.getOrDefault("layer","base"),LayerEditor.page(request));return object("items",editor.select(request.containsKey("region")?Geometry.polygon(request.get("region")):null,Boolean.TRUE.equals(request.get("topmost")),ItemCoverage::create));}
                if(sub.equals("select")){arity(a,1);Map<String,Object> selection=map(input(a.get(0)));return LocalProxy.select(store,actionActor,selection);}
                if(sub.equals("paste")){arity(a,1);Map<String,Object> request=map(input(a.get(0)));if(!request.containsKey("entities"))request.put("entities",list(map(store.read().getOrDefault("clipboard",object())).getOrDefault("entities",Collections.emptyList())));return store.commit(actionActor,m->DrawingActions.paste(m,request));}
                if(sub.equals("disconnect")){arity(a,0);return LocalProxy.releaseActor(store,actionActor);}
                if(sub.equals("copy")||sub.equals("cut")){arity(a,1);String session=a.get(0);List<Object> entities=LocalProxy.selected(store,actionActor,session);LocalProxy.assertSession(store,actionActor,session,entities);Model model=store.model();List<Object> snapshots=DrawingActions.copySelection(model,entities);long copiedAt=System.currentTimeMillis();store.edit(s->s.put("clipboard",object("copiedAt",copiedAt,"entities",copy(snapshots))));List<Object> dependencies=new ArrayList<>(entities);Map<String,Object> cutResult=null;if(sub.equals("cut")){cutResult=store.commitSelected(actionActor,session,entities,m->{List<Object> generated=new ArrayList<>();for(Object value:entities){Map<String,Object> e=map(value);String kind=string(e,"kind");generated.add(object("kind",kind.equals("object")?"object-active":"active",kind,string(e,"id"),"active",false));}return generated;},dependencies);LocalProxy.release(store,actionActor,session,false);}Map<String,Object> response=object("copiedAt",copiedAt,"entities",snapshots);if(cutResult!=null)response.put("operation",cutResult.get("operation"));return response;}
                if(sub.equals("duplicate")||sub.equals("delete")){arity(a,1);String session=a.get(0);List<Object> entities=LocalProxy.selected(store,actionActor,session);LocalProxy.assertSession(store,actionActor,session,entities);List<Object> dependencies=new ArrayList<>(entities);Object result;
                    if(sub.equals("delete")){result=store.commitSelected(actionActor,session,entities,m->{List<Object> generated=new ArrayList<>();for(Object value:entities){Map<String,Object> e=map(value);String kind=string(e,"kind");generated.add(object("kind",kind.equals("object")?"object-active":"active",kind,string(e,"id"),"active",false));}return generated;},dependencies);LocalProxy.release(store,actionActor,session,false);}
                    else{Model model=store.model();result=store.commitSelected(actionActor,session,entities,m->{List<Object> generated=new ArrayList<>();Map<String,String> ids=new HashMap<>();for(Object value:entities){Map<String,Object> e=map(value);String kind=string(e,"kind"),id=string(e,"id");ids.put(kind+":"+id,Actions.id());}for(Object value:entities){Map<String,Object> e=map(value);String kind=string(e,"kind"),id=string(e,"id");Map<String,Object> old=kind.equals("object")?m.objects.get(id):m.strokes.get(id);Map<String,Object> n=map(copy(old));n.put(kind,ids.get(kind+":"+id));if(kind.equals("object")){n.put("x",number(n,"x")+24);n.put("y",number(n,"y")+24);for(String key:Arrays.asList("from","to"))if(n.containsKey(key)&&ids.containsKey("object:"+n.get(key)))n.put(key,ids.get("object:"+n.get(key)));}else{List<Object> points=new ArrayList<>();for(Object p:list(n.get("points"))){List<Object> xyz=list(p);points.add(Arrays.asList(((Number)xyz.get(0)).doubleValue()+24,((Number)xyz.get(1)).doubleValue()+24,xyz.get(2)));}n.put("points",points);}generated.add(n);}return generated;},dependencies);}return result;}
                arity(a,1);return LocalProxy.release(store,actionActor,a.get(0),sub.equals("heartbeat"));
            }
            case "layer":case "object":case "stroke":{
                require(!a.isEmpty(),"Missing subcommand");String sub=a.remove(0);String kind=command;
                if(kind.equals("layer")&&sub.equals("duplicate")){arity(a,1);String id=a.get(0);Model model=store.model();List<Object> dependencies=new ArrayList<>();for(Map<String,Object> entity:model.objects.values())if(id.equals(entity.get("layer")))dependencies.add(object("kind","object","id",entity.get("object"),"page",entity.getOrDefault("page",0)));for(Map<String,Object> entity:model.strokes.values())if(id.equals(entity.get("layer")))dependencies.add(object("kind","stroke","id",entity.get("stroke"),"page",entity.getOrDefault("page",0)));return store.commit(actor,m->DrawingActions.duplicateLayer(m,id),dependencies);}
                if(kind.equals("layer")&&sub.equals("merge")){arity(a,2);String source=a.get(0),target=a.get(1);return store.commit(actor,m->DrawingActions.mergeLayer(m,source,target));}
                if(kind.equals("layer")&&sub.equals("reorder")){arity(a,2);String id=a.get(0);int index=Integer.parseInt(a.get(1));return store.commit(actor,m->DrawingActions.reorderLayer(m,id,index));}
                if(sub.equals("hide")||sub.equals("show")){require(a.size()>=1&&a.size()<=3,"hide/show ID [LAYER [PAGE]]");String id=a.get(0),scopeLayer=a.size()>1?a.get(1):"base";int scopePage=a.size()>2?Integer.parseInt(a.get(2)):0;return store.commit(actor,m->{if(!kind.equals("layer"))new LayerEditor(m,scopeLayer,scopePage,true).resolve(kind,id);require((kind.equals("object")?m.objects:kind.equals("layer")?m.layers:m.strokes).containsKey(id),"Unknown "+kind+" ID");return Arrays.asList(object("kind",kind.equals("stroke")?"active":kind+"-active",kind,id,"active",sub.equals("show")));});}
                if(sub.equals("update")){require(kind.equals("object"),"Only object update is supported; use put for other kinds");arity(a,2);String id=a.get(0);Map<String,Object> patch=map(input(a.get(1)));Map<String,Object> scope=map(patch.getOrDefault("scope",object("layer","base","page",0)));patch.remove("scope");require(!patch.containsKey("kind")&&!patch.containsKey("object")&&!patch.containsKey("id"),"Cannot patch identity");return store.commit(actor,m->{LayerEditor editor=new LayerEditor(m,(String)scope.getOrDefault("layer","base"),LayerEditor.page(scope),true);Map<String,Object> o=map(copy(editor.resolve("object",id).content));require(!patch.containsKey("layer")||editor.layer.equals(patch.get("layer")),"Use layer management or cut/paste to move layers");o.putAll(patch);return Arrays.asList(o);});}
                require(sub.equals("put"),"Unknown subcommand");boolean withSnapshot=kind.equals("object")&&a.size()==2&&a.get(0).equals("snapshot");if(withSnapshot)a.remove(0);arity(a,1);Map<String,Object> raw=map(input(a.get(0)));Map<String,Object> o=kind.equals("object")?Actions.drawing(raw,"base"):map(copy(raw));o.put("kind",kind);o.remove("id");Actions.Build build=m->{if(!kind.equals("layer")&&(kind.equals("object")?m.objects:m.strokes).containsKey(o.get(kind)))new LayerEditor(m,string(o,"layer"),((Number)o.get("page")).intValue(),true).resolve(kind,string(o,kind));return Arrays.asList(o);};Map<String,Object> result=withSnapshot?store.commitWithState(actor,build):store.commit(actor,build);if(withSnapshot)result.put("snapshot",snapshot(map(result.remove("state"))));return result;
            }
            case "export":{
                require(a.size()==1||a.size()==2||a.size()==6,"export FILE [PAGE [X Y WIDTH HEIGHT]]");File out=new File(a.get(0));int page=a.size()>1?Integer.parseInt(a.get(1)):0;require(page>=0&&page<=10000,"Invalid page");double x=0,y=0,w=1000,h=1400;if(a.size()==6){x=Double.parseDouble(a.get(2));y=Double.parseDouble(a.get(3));w=Double.parseDouble(a.get(4));h=Double.parseDouble(a.get(5));}require(Double.isFinite(x)&&Double.isFinite(y)&&Double.isFinite(w)&&Double.isFinite(h)&&w>=1&&h>=1&&w<=8192&&h<=8192&&w*h<=16000000,"Invalid export viewport (max 16 million pixels)");Model model=remoteModel==null?store.model():remoteModel;
                if(out.getName().endsWith(".svg")){Render.Svg svg=new Render.Svg(x,y,w,h);Render.draw(model,page,Collections.emptySet(),svg);Files.write(out.toPath(),svg.toString().getBytes(StandardCharsets.UTF_8));}
                else if(out.getName().endsWith(".png"))Class.forName("com.notemaker.cli.Png").getMethod("export",Model.class,int.class,double.class,double.class,int.class,int.class,File.class).invoke(null,model,page,x,y,(int)w,(int)h,out);
                else throw new IllegalArgumentException("Export filename must end in .svg or .png");return object("file",out.getAbsolutePath(),"page",page);
            }
            default:throw new IllegalArgumentException("Unknown command: "+command);
        }
    }
    private static void arity(List<String>a,int n){require(a.size()==n,"Expected "+n+" arguments; see --help");}
    private static Map<String,Object> svgInput(List<String> a)throws IOException{
        require((a.size()==2||a.size()==3)&&a.get(0).equals("add"),"svg add FILE.svg|- [PLACEMENT.json]");
        if(Boolean.TRUE.equals(inProxy.get())){arity(a,2);return map(input(a.get(1)));}
        require(a.size()!=3||!a.get(1).equals("-")||!a.get(2).equals("-"),"SVG and placement cannot both read stdin");
        Map<String,Object> placement=a.size()==3?map(input(a.get(2))):object();
        for(String key:placement.keySet())require(Arrays.asList("x","y","w","h","layer","page","object").contains(key),"Unknown SVG placement field: "+key);
        placement.put("svg",readInput(a.get(1)));return placement;
    }
    private static Object input(String path) throws IOException {
        if(Boolean.TRUE.equals(inProxy.get())){require(path.equals("-")&&proxyInput.get()!=null,"Proxy input must be supplied in the input field with FILE=-");return copy(proxyInput.get());}

        return parse(readInput(path));
    }
    private static Map<String,Object> imagePlacement(List<String> args)throws IOException{
        int offset=args.get(0).equals("image")?2:1;require(args.size()==offset+1||args.size()==offset+2,"image add FILE [PLACEMENT.json]");require(args.get(offset-1).equals("add"),"image add FILE [PLACEMENT.json]");Map<String,Object> placement=args.size()==offset+2?map(parse(readInput(args.get(offset+1)))):object();
        for(String key:placement.keySet())require(Arrays.asList("x","y","w","h","layer","page","object").contains(key),"Unknown image placement field: "+key);return placement;
    }
    static Map<String,Object> snapshot(Map<String,Object> s){
        Model model=Store.model(s);Map<String,Object> out=new Store.Snapshot(model);out.putAll(object("effectiveModel",model.json(),"conflicts",model.visibleConflicts(),"revision",s.get("revision"),"cursor",s.get("cursor"),"eventCount",list(s.get("events")).size(),"pendingCount",list(s.get("pending")).size()));
        for(String key:Arrays.asList("hidden","undo","redo","selected","page","clipboard"))if(s.containsKey(key))out.put(key,s.get(key));return out;
    }
    private static byte[] imageBytes(String path)throws IOException{
        byte[] bytes;if(path.equals("-")){ByteArrayOutputStream out=new ByteArrayOutputStream();byte[] buffer=new byte[8192];int count;while((count=System.in.read(buffer))!=-1){out.write(buffer,0,count);require(out.size()<=ImageAsset.MAX_BYTES,"Image exceeds 2 MiB");}bytes=out.toByteArray();}else{require(Files.size(Paths.get(path))<=ImageAsset.MAX_BYTES,"Image exceeds 2 MiB");bytes=Files.readAllBytes(Paths.get(path));}ImageAsset.fromBytes(bytes);return bytes;
    }
    private static String readInput(String path)throws IOException{
        byte[] bytes;if(path.equals("-")){ByteArrayOutputStream b=new ByteArrayOutputStream();byte[] buffer=new byte[8192];int n;while((n=System.in.read(buffer))!=-1){b.write(buffer,0,n);require(b.size()<=3000000,"Input exceeds 3 MB");}bytes=b.toByteArray();}else{require(Files.size(Paths.get(path))<=3000000,"Input exceeds 3 MB");bytes=Files.readAllBytes(Paths.get(path));}return new String(bytes,StandardCharsets.UTF_8);
    }
}
