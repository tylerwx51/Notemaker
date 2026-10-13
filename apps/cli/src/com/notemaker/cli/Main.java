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
        + "--proxy-file FILE          Send commands to a local proxy descriptor\n"
        + "--help                     Show this help without contacting a server\n\n"
        + "serve [PORT [SERVER_URL]]   Start local replica proxy (default port 8766)\n"
        + "document create [fixed|pages|infinite] | document list\ninspect | status | log\n"
        + "diagram add FILE|-         Atomic new layer and editable diagram objects\n"
        + "svg add FILE.svg|- [PLACEMENT.json]   Place SVG; optional {x,y,w,h,layer,page,object}\n"
        + "layer put FILE|-           {layer,name,force} (create or update)\nlayer hide ID | layer show ID\n"
        + "object put FILE|-          Create/replace an editable object\n"
        + "object update ID FILE|-    Patch geometry, label or styling\n"
        + "object hide ID | object show ID\n"
        + "stroke put FILE|- | stroke hide ID | stroke show ID\n"
        + "apply FILE|-               Atomic JSON array of protocol actions\n"
        + "undo | redo                History for the selected actor\n"
        + "export FILE.svg|FILE.png [PAGE [X Y WIDTH HEIGHT]]   Page is zero-based\n"
        + "sync SERVER_URL            Upload one batch and fetch all remote pages; repeat if pending > 0\n"
        + "watch SERVER_URL           Keep synchronizing until interrupted\n"
        + "FILE|- accepts JSON from a file or stdin (svg add reads raw SVG).\n"
        + "Edits use local state and work offline; document list lists local documents only.\n"
        + "JSON on stdout (except help); JSON errors on stderr and exit 1.\n"
        + "Mutations are not idempotent: inspect log before retrying a lost result.\n"
        + "sync retries are idempotent. See apps/cli/README.md for input schemas.\n\n"
        + "Examples:\n"
        + "  notemaker --document notes status\n"
        + "  notemaker --document notes sync http://127.0.0.1:8765\n";
    public static void main(String[] args){try{Object result=run(args);if(result!=null)System.out.println(write(result));}catch(Exception e){Throwable cause=e;while(cause.getCause()!=null)cause=cause.getCause();System.err.println(write(object("error",cause.toString())));System.exit(1);}}
    public static Object run(String[] args) throws Exception {
        List<String>a=new ArrayList<>(Arrays.asList(args));
        if(a.isEmpty()||a.contains("--help")||(a.size()==1&&a.get(0).equals("help"))){System.out.print(HELP);return null;}
        String state=System.getenv("NOTEMAKER_STATE");if(state==null){String xdg=System.getenv("XDG_STATE_HOME");state=(xdg==null?System.getProperty("user.home")+"/.local/state":xdg)+"/notemaker";}
        String doc="notes",actor=System.getenv("NOTEMAKER_ACTOR"),proxy=null;
        if(actor==null||actor.isEmpty())actor="cli";
        while(!a.isEmpty()&&a.get(0).startsWith("--")){String flag=a.remove(0);require(!a.isEmpty(),"Missing "+flag+" value");String value=a.remove(0);switch(flag){case "--state":state=value;break;case "--document":doc=value;break;case "--actor":actor=value;break;case "--proxy-file":proxy=value;break;default:throw new IllegalArgumentException("Unknown option "+flag);}}
        require(!a.isEmpty(),"Missing command");
        Model remoteModel=null;
        if(proxy!=null){
            Map<String,Object> descriptor=map(parse(new String(Files.readAllBytes(Paths.get(proxy)),StandardCharsets.UTF_8)));
            Object value=null;String cmd=a.get(0);
            if(cmd.equals("svg")){value=svgInput(a.subList(1,a.size()));a=new ArrayList<>(Arrays.asList("svg","add","-"));}
            else if(cmd.equals("apply")||cmd.equals("diagram")||((cmd.equals("object")||cmd.equals("layer")||cmd.equals("stroke"))&&(a.contains("put")||a.contains("update")))){
                int last=a.size()-1;value=input(a.get(last));a.set(last,"-");
            }
            if(cmd.equals("export"))remoteModel=Model.fromJson(map(LocalProxy.request(descriptor,doc,actor,Arrays.asList("inspect"),null)));
            else return LocalProxy.request(descriptor,doc,actor,a,value);
        }
        String command=a.remove(0);File directory=new File(state);Store store=remoteModel==null?new Store(directory,doc):null;
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
                require(a.size()>=1,"document create|list");String sub=a.remove(0);
                if(sub.equals("list")){arity(a,0);List<Object> documents=new ArrayList<>();File[] files=directory.listFiles((d,n)->n.startsWith("document-")&&n.endsWith(".json"));if(files!=null)for(File f:files){Map<String,Object>s=map(parse(new String(Files.readAllBytes(f.toPath()),StandardCharsets.UTF_8)));documents.add(s.getOrDefault("document",f.getName()));}return object("documents",documents);}
                require(sub.equals("create")&&a.size()<=1,"document create [fixed|pages|infinite]");String type=a.isEmpty()?"fixed":a.get(0);Map<String,Object> op=object("kind","document","type",type);Model.validate(op);
                store.edit(s->{require(list(s.get("events")).isEmpty()&&list(s.get("pending")).isEmpty(),"Document already exists");op.put("id",Actions.id());op.put("actor",actionActor);list(s.get("pending")).add(op);});return object("document",doc,"type",type);
            }
            case "inspect":arity(a,0);return store.model().json();
            case "status":{arity(a,0);Map<String,Object>s=store.read();return object("document",doc,"cursor",s.get("cursor"),"pending",list(s.get("pending")).size(),"revision",s.get("revision"),"serverId",s.get("serverId"));}
            case "log":arity(a,0);return store.read();
            case "sync":arity(a,1);return store.sync(a.get(0));
            case "watch":arity(a,1);while(true){try{System.out.println(write(store.sync(a.get(0))));}catch(Exception e){System.err.println(write(object("error",e.getMessage(),"retrying",true)));}Thread.sleep(1000);}
            case "undo":case "redo":arity(a,0);return store.history(actor,command.equals("redo"));
            case "diagram":arity(a,2);require(a.get(0).equals("add"),"diagram add FILE|-");Map<String,Object> diagram=map(input(a.get(1)));return store.commit(actor,m->Actions.diagram(m,diagram));
            case "svg":{
                Map<String,Object> raw=svgInput(a);raw.put("shape","svg");Map<String,Object> drawing=Actions.drawing(raw,"base");
                return store.commit(actor,m->{require(!m.objects.containsKey(drawing.get("object")),"Object ID already exists; use object update");return Arrays.asList(drawing);});
            }
            case "apply":arity(a,1);List<Object> ops=list(input(a.get(0)));return store.commit(actor,m->ops);
            case "layer":case "object":case "stroke":{
                require(!a.isEmpty(),"Missing subcommand");String sub=a.remove(0);String kind=command;
                if(sub.equals("hide")||sub.equals("show")){arity(a,1);String id=a.get(0);return store.commit(actor,m->{require((kind.equals("object")?m.objects:kind.equals("layer")?m.layers:m.strokes).containsKey(id),"Unknown "+kind+" ID");return Arrays.asList(object("kind",kind.equals("stroke")?"active":kind+"-active",kind,id,"active",sub.equals("show")));});}
                if(sub.equals("update")){require(kind.equals("object"),"Only object update is supported; use put for other kinds");arity(a,2);String id=a.get(0);Map<String,Object> patch=map(input(a.get(1)));require(!patch.containsKey("kind")&&!patch.containsKey("object")&&!patch.containsKey("id"),"Cannot patch identity");return store.commit(actor,m->{require(m.objects.containsKey(id),"Unknown object ID");Map<String,Object> o=map(copy(m.objects.get(id)));o.putAll(patch);return Arrays.asList(o);});}
                require(sub.equals("put"),"Unknown subcommand");arity(a,1);Map<String,Object> raw=map(input(a.get(0)));Map<String,Object> o=kind.equals("object")?Actions.drawing(raw,"base"):map(copy(raw));o.put("kind",kind);o.remove("id");return store.commit(actor,m->Arrays.asList(o));
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
    private static String readInput(String path)throws IOException{
        byte[] bytes;if(path.equals("-")){ByteArrayOutputStream b=new ByteArrayOutputStream();byte[] buffer=new byte[8192];int n;while((n=System.in.read(buffer))!=-1){b.write(buffer,0,n);require(b.size()<=3000000,"Input exceeds 3 MB");}bytes=b.toByteArray();}else{require(Files.size(Paths.get(path))<=3000000,"Input exceeds 3 MB");bytes=Files.readAllBytes(Paths.get(path));}return new String(bytes,StandardCharsets.UTF_8);
    }
}
