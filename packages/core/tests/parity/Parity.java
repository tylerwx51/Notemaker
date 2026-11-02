import com.notemaker.core.*;
import java.io.*;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.util.*;
import java.util.regex.*;
import static com.notemaker.core.Json.*;

/**
 * Test-only parity driver for the Java core. Prints canonical sections that
 * tests/parity/parity.rs prints identically from the Rust port.
 *
 * Usage: java Parity FIXTURES SECTION [ARGS...]
 *   all                      every fixture-driven section
 *   read DIR DOC...          Store reads of an existing state directory
 *   write DIR                deterministic journal write sequence, then dump files
 *   mutate DIR DOC           edits on an existing replica (cross-implementation check)
 */
public final class Parity {
    static final StringBuilder section=new StringBuilder();
    static final PrintStream out=new PrintStream(new FileOutputStream(FileDescriptor.out),true);
    static String header;
    static Path fixtures;

    static void begin(String name){flush();header=name;}
    static void line(String text){section.append(text).append('\n');}
    static void flush(){
        if(header==null)return;
        String text=canonical(section.toString());
        byte[] bytes=("== "+header+"\n"+text).getBytes(StandardCharsets.UTF_8);
        out.write(bytes,0,bytes.length);out.flush();
        section.setLength(0);header=null;
    }
    static final Pattern UUID_PATTERN=Pattern.compile("[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}");
    static String canonical(String text){
        Map<String,String> ids=new LinkedHashMap<>();Matcher m=UUID_PATTERN.matcher(text);StringBuffer b=new StringBuffer();
        while(m.find()){String id=m.group();String r=ids.get(id);if(r==null){r="<id-"+ids.size()+">";ids.put(id,r);}m.appendReplacement(b,Matcher.quoteReplacement(r));}
        m.appendTail(b);return b.toString();
    }
    static String err(Throwable e){
        String type=e instanceof IllegalArgumentException?"IAE":e instanceof IOException?"IO":e instanceof UnsupportedOperationException?"UNSUPPORTED":e.getClass().getSimpleName();
        return "ERR "+type+" "+e.getMessage();
    }
    interface Body{Object run() throws Exception;}
    static void attempt(String label,Body body){
        try{Object v=body.run();line(label+" "+show(v));}catch(Throwable e){line(label+" "+err(e));}
    }
    static String show(Object v){
        if(v==null)return "null";
        if(v instanceof double[])return Arrays.toString((double[])v);
        if(v instanceof double[][]){StringBuilder b=new StringBuilder("[");for(double[] p:(double[][])v){if(b.length()>1)b.append(", ");b.append(Arrays.toString(p));}return b.append(']').toString();}
        if(v instanceof String)return (String)v;
        if(v instanceof Map||v instanceof List||v instanceof Number||v instanceof Boolean)return write(v);
        return String.valueOf(v);
    }
    static String read(Path p) throws IOException {return new String(Files.readAllBytes(p),StandardCharsets.UTF_8);}

    /** Deterministic xorshift64 shared with the Rust driver. */
    static long seed=0x9E3779B97F4A7C15L;
    static long next(){seed^=seed<<13;seed^=seed>>>7;seed^=seed<<17;return seed;}

    public static void main(String[] args) throws Exception {
        fixtures=Paths.get(args[0]);String command=args[1];
        switch(command){
            case "all":jsonSections();validate();assets();svg();geometry();scenes();actions();break;
            case "read":for(int i=3;i<args.length;i++)readStore(Paths.get(args[2]),args[i]);break;
            case "write":writeSequence(Paths.get(args[2]));break;
            case "mutate":mutate(Paths.get(args[2]),args[3]);break;
            default:throw new IllegalArgumentException("Unknown command "+command);
        }
        flush();
    }

    // ------------------------------------------------------------------ JSON
    static void jsonSections() throws Exception {
        begin("json/cases");
        for(String raw:Files.readAllLines(fixtures.resolve("json/cases.txt"),StandardCharsets.UTF_8)){
            String text=raw.replace("\\n","\n");attempt(raw,()->write(parse(text)));
        }
        begin("json/equivalent");
        for(String raw:Files.readAllLines(fixtures.resolve("json/equivalent.txt"),StandardCharsets.UTF_8)){
            String[] pair=raw.split("\t");attempt(raw,()->equivalent(parse(pair[0]),parse(pair[1])));
        }
        begin("json/doubles");
        for(int i=0;i<60000;i++){
            long bits=next();double d;
            switch(i%6){
                case 0:d=Double.longBitsToDouble(bits);break;
                case 1:d=(bits%2000001)/1000.0;break;
                case 2:d=(bits%100000)*0.1;break;
                case 3:d=Math.sqrt(Math.abs((double)(bits%1000000)))/7;break;
                case 4:d=(bits>>>11)*0x1.0p-53*Double.parseDouble("1e"+((int)((bits>>>3)%40)-20));break;
                default:d=(int)(bits%100000)/(double)(1+(bits>>>40)%999);break;
            }
            line((Double.isNaN(d)?"nan":Long.toHexString(Double.doubleToRawLongBits(d)))+" "+d);
        }
        begin("json/floats");
        for(int i=0;i<20000;i++){long bits=next();float f=i%2==0?Float.intBitsToFloat((int)bits):(float)((bits%2000001)/1000.0);line((Float.isNaN(f)?"nan":Integer.toHexString(Float.floatToRawIntBits(f)))+" "+f);}
        begin("json/objects");
        line(write(object("a",1,"b",1.0,"c",-0.0,"d",1e-5,"e",Long.MAX_VALUE,"f",Arrays.asList(1,2.5,"x",null,true),"g",object("h",object()))));
        attempt("nonfinite",()->write(Arrays.asList(Double.NaN)));
    }
    static void validate() throws Exception {
        begin("validate");
        for(String raw:Files.readAllLines(fixtures.resolve("validate.jsonl"),StandardCharsets.UTF_8)){
            String label=raw.length()>80?raw.substring(0,80):raw;
            attempt(label,()->{Model.validate(map(parse(raw)));return "OK";});
        }
    }

    // ------------------------------------------------------------------ assets
    static void assets() throws Exception {
        List<Path> files=new ArrayList<>();try(DirectoryStream<Path> d=Files.newDirectoryStream(fixtures.resolve("assets"))){for(Path p:d)files.add(p);}
        Collections.sort(files);
        for(Path p:files){
            String name=p.getFileName().toString();if(name.equals("uris.txt"))continue;byte[] bytes=Files.readAllBytes(p);
            begin("assets/"+name);
            String b64=Base64.getEncoder().encodeToString(bytes);
            if(name.endsWith(".gif")){
                attempt("bytes",()->gifInfo(GifAsset.validate(bytes)));
                attempt("uri",()->gifInfo(GifAsset.validate("data:image/gif;base64,"+b64)));
                attempt("fromBytes",()->GifAsset.fromBytes(bytes).length());
            }else{
                String mime=name.endsWith(".png")?"image/png":"image/jpeg";
                attempt("uri",()->ImageAsset.validate("data:"+mime+";base64,"+b64));
                attempt("wrongMime",()->ImageAsset.validate("data:"+(mime.equals("image/png")?"image/jpeg":"image/png")+";base64,"+b64));
                attempt("fromBytes",()->ImageAsset.fromBytes(bytes).substring(0,30));
            }
        }
        begin("assets/uris");
        for(String uri:Files.readAllLines(fixtures.resolve("assets/uris.txt"),StandardCharsets.UTF_8)){
            attempt("image "+uri,()->ImageAsset.validate(uri));attempt("gif "+uri,()->gifInfo(GifAsset.validate(uri)));
        }
        attempt("image null",()->ImageAsset.validate(null));attempt("gif null",()->GifAsset.validate((String)null));
    }
    static String gifInfo(GifAsset.Info info){return info.width+"x"+info.height+" delays="+Arrays.toString(info.delays)+" offsets="+Arrays.toString(info.offsets)+" duration="+info.duration+" t1="+(info.offsets.length>1?info.timeForFrame(1):-1);}
    static void svg() throws Exception {
        List<Path> files=new ArrayList<>();try(DirectoryStream<Path> d=Files.newDirectoryStream(fixtures.resolve("svg"))){for(Path p:d)files.add(p);}
        Collections.sort(files);
        for(Path p:files){
            String source=read(p);begin("svg/"+p.getFileName());
            attempt("size",()->SvgAsset.validate(source));
            attempt("viewport",()->SvgAsset.viewport(source));
            attempt("dots",()->{SvgAsset.DotGrid g=SvgAsset.dotGrid(source);return g==null?"none":g.width+" "+g.height+" "+g.strokeWidth+" "+g.strokeOpacity+" "+g.color+" "+Arrays.toString(g.circles());});
        }
    }

    // ------------------------------------------------------------------ geometry
    static double coord(){return ((next()>>>11)%200000-100000)/100.0;}
    static void geometry() throws Exception {
        begin("geometry/capsule");
        for(double radius:new double[]{0.05,0.5,1,3.3,12,100,10000})for(int i=0;i<4;i++){double[] a={coord(),coord()},b=i==0?a.clone():new double[]{coord(),coord()};line(radius+" "+show(Geometry.capsule(a,b,radius)));}
        begin("geometry/predicates");
        for(int i=0;i<400;i++){
            int n=3+(int)((next()>>>1)%5),m=3+(int)((next()>>>1)%5);double[][] a=new double[n][],b=new double[m][];
            double ox=coord(),oy=coord();for(int k=0;k<n;k++)a[k]=new double[]{ox+coord()/50,oy+coord()/50};for(int k=0;k<m;k++)b[k]=new double[]{ox+coord()/50,oy+coord()/50};
            double px=ox+coord()/40,py=oy+coord()/40;
            line(i+" "+Geometry.intersects(a,b)+" "+Geometry.contains(a,px,py)+" "+show(Geometry.bounds(a))+" "+show(Geometry.rect(px,py,ox,oy)));
        }
        begin("geometry/polygon");
        for(String raw:new String[]{"[[0,0],[1,0],[0,1]]","[[0,0],[1,0]]","[[0,0],[1,0],[0,\"1\"]]","[[0,0],[1,0],[0,1,2]]","[[0,0],[1,0],[0,1e10]]","{}","[[0.5,1.25],[2,3],[4,5],[6,7]]"})
            attempt(raw,()->Geometry.json(Geometry.polygon(parse(raw))));
        begin("shapes");
        line(write(ShapeCatalog.json()));
        for(String id:ShapeCatalog.ids()){attempt(id+" polygon="+ShapeCatalog.polygon(id),()->ShapeCatalog.vertices(id));attempt(id+" svg",()->ShapeCatalog.drawingSvg(id));}
        attempt("name nope",()->ShapeCatalog.name("nope"));attempt("known svg",()->ShapeCatalog.known("svg"));attempt("known template",()->ShapeCatalog.known("template"));
        begin("ink");
        InkGeometry ink=new InkGeometry();line(show(ink.bounds())+" "+ink.size());
        for(int i=0;i<40;i++){if(i%3==0)ink.dot(coord(),coord(),Math.abs(coord()),Math.abs(coord()),i%2==0);else ink.line(coord(),coord(),coord(),coord(),Math.abs(coord())/10);}
        ink.finish();line(show(ink.bounds())+" "+ink.size());
        for(int i=0;i<ink.size();i++){double l=coord(),t=coord();line(i+" "+ink.value(i,0)+" "+ink.value(i,5)+" "+ink.overlaps(i,l,t,l+300,t+300));}
        begin("coverage/tiles");
        Coverage tiles=new Coverage.Tiles(new double[]{-70,-10,130,90},(x,y,size)->{int[] px=new int[size*size];for(int j=0;j<size;j++)for(int i=0;i<size;i++){double cx=x+i+.5,cy=y+j+.5;double d=(cx-30)*(cx-30)/4900+(cy-40)*(cy-40)/1600;px[j*size+i]=d<=1?0xff000000:(d<=1.2?0x01000000:0);}return px;});
        for(int i=0;i<200;i++){double x=coord()/5,y=coord()/5;double[][] poly=Geometry.capsule(new double[]{x,y},new double[]{x+coord()/20,y+coord()/20},1+Math.abs(coord())/100);attempt(i+"",()->tiles.intersects(poly));}
        attempt("huge",()->tiles.intersects(Geometry.rect(-1e6,-1e6,2e6,2e6)));
        Coverage big=new Coverage.Tiles(new double[]{-1e6,-1e6,1e6,1e6},(x,y,size)->new int[size*size]);
        attempt("too large",()->big.intersects(Geometry.rect(-1e5,-1e5,2e5,2e5)));
    }

    // ------------------------------------------------------------------ scenes
    static final Coverage.Factory FACTORY=Parity::coverage;
    static Coverage coverage(Map<String,Object> local){
        double width=number(local,"width");
        if("stroke".equals(local.get("kind"))){
            List<double[]> pts=new ArrayList<>();for(Object v:list(local.get("points"))){List<Object> p=list(v);pts.add(new double[]{((Number)p.get(0)).doubleValue(),((Number)p.get(1)).doubleValue()});}
            double[] b={Double.POSITIVE_INFINITY,Double.POSITIVE_INFINITY,Double.NEGATIVE_INFINITY,Double.NEGATIVE_INFINITY};for(double[] p:pts){b[0]=Math.min(b[0],p[0]);b[1]=Math.min(b[1],p[1]);b[2]=Math.max(b[2],p[0]);b[3]=Math.max(b[3],p[1]);}
            b[0]-=width;b[1]-=width;b[2]+=width;b[3]+=width;double rr=width/2+.5;
            return new Coverage.Tiles(b,(x,y,size)->{int[] px=new int[size*size];for(int j=0;j<size;j++)for(int i=0;i<size;i++){double cx=x+i+.5,cy=y+j+.5;boolean hit=false;
                for(int k=0;k<pts.size()&&!hit;k++){double[] a=pts.get(k),c=pts.get(Math.min(k+1,pts.size()-1));double dx=c[0]-a[0],dy=c[1]-a[1],q=dx*dx+dy*dy,t=q==0?0:Math.max(0,Math.min(1,((cx-a[0])*dx+(cy-a[1])*dy)/q)),ex=cx-a[0]-t*dx,ey=cy-a[1]-t*dy;hit=ex*ex+ey*ey<=rr*rr;}
                px[j*size+i]=hit?0xff000000:0;}return px;});
        }
        double w=number(local,"w"),h=number(local,"h");double[] b={Math.min(0,w)-width,Math.min(0,h)-width,Math.max(0,w)+width,Math.max(0,h)+width};
        return new Coverage.Tiles(b,(x,y,size)->{int[] px=new int[size*size];for(int j=0;j<size;j++)for(int i=0;i<size;i++){double cx=x+i+.5,cy=y+j+.5;px[j*size+i]=cx>=Math.min(0,w)&&cx<=Math.max(0,w)&&cy>=Math.min(0,h)&&cy<=Math.max(0,h)?0xff000000:0;}return px;});
    }
    static final Map<String,Model> SCENES=new LinkedHashMap<>();
    static Model scene(String name) throws Exception {
        Model cached=SCENES.get(name);if(cached!=null)return new Model(cached);
        Map<String,Object> s=map(parse(read(fixtures.resolve("scenes/"+name+".json"))));Model m=new Model();
        if(!"fixed".equals(s.get("type")))m.apply(object("kind","document","type",s.get("type")));
        boolean raw=Boolean.TRUE.equals(s.get("raw"));
        for(Object op:list(s.get("ops"))){try{if(!raw)Model.validate(map(op));m.apply(map(op));}catch(Exception e){if(header!=null)line("apply "+err(e));}}
        SCENES.put(name,m);return new Model(m);
    }
    static void scenes() throws Exception {
        List<Path> files=new ArrayList<>();try(DirectoryStream<Path> d=Files.newDirectoryStream(fixtures.resolve("scenes"))){for(Path p:d)files.add(p);}
        Collections.sort(files);
        for(Path p:files){
            String name=p.getFileName().toString().replace(".json","");begin("scene/"+name);
            SCENES.remove(name);Model m=scene(name);Map<String,Object> s=map(parse(read(p)));
            line("model "+write(m.json()));
            line("fromJson "+write(Model.fromJson(m.json()).json()));
            for(String layer:m.layers.keySet())for(int page=0;page<3;page++){List<Map<String,Object>> items=m.layerItems(layer,page);if(items.isEmpty())continue;StringBuilder ids=new StringBuilder();for(Map<String,Object> o:items)ids.append(o.get("kind")).append(':').append(o.get(o.get("kind"))).append(' ');line("items "+layer+" "+page+" "+ids);}
            for(Object rv:list(s.get("renders"))){
                Map<String,Object> render=map(rv);List<Object> vp=list(render.get("viewport"));double x=((Number)vp.get(0)).doubleValue(),y=((Number)vp.get(1)).doubleValue(),w=((Number)vp.get(2)).doubleValue(),h=((Number)vp.get(3)).doubleValue();
                Set<String> hidden=new HashSet<>();for(Object v:list(render.getOrDefault("hidden",Collections.emptyList())))hidden.add((String)v);
                attempt("render "+write(render),()->{Render.Svg svg=new Render.Svg(x,y,w,h);Render.draw(m,((Number)render.get("page")).intValue(),hidden,svg);return svg.toString();});
            }
            for(Map<String,Object> o:allItems(m)){
                String id=o.get("kind")+":"+o.get(o.get("kind"));
                attempt("item "+id,()->{DrawItem item=new DrawItem(m,o);StringBuilder b=new StringBuilder();b.append(item.active()).append(' ').append(show(item.local(10,20))).append(' ').append(write(item.ref())).append(" erasures=").append(DrawItem.erasures(o).size());
                    b.append(" geometry=").append(write(item.geometry())).append(" local=").append(write(item.localContent()));
                    try{b.append(" paint=").append(show(item.paintBounds(FACTORY))).append(" has=").append(item.hasCoverage(FACTORY));}catch(Exception e){b.append(" paint=").append(err(e));}
                    return b.toString();});
                if("object".equals(o.get("kind")))attempt("endpoints "+id,()->Render.endpoints(m,o));
            }
        }
    }
    static List<Map<String,Object>> allItems(Model m){List<Map<String,Object>> out=new ArrayList<>(m.strokes.values());out.addAll(m.objects.values());return out;}

    // ------------------------------------------------------------------ actions
    static void actions() throws Exception {
        begin("actions");
        List<Object> clipboard=null;int n=0;
        for(String raw:Files.readAllLines(fixtures.resolve("actions.jsonl"),StandardCharsets.UTF_8)){
            Map<String,Object> call=map(parse(raw));Model m=scene((String)call.get("scene"));String fn=(String)call.get("fn");String label=(n++)+" "+fn;
            final List<Object> copied=clipboard;
            Body body;
            switch(fn){
                case "copySelection":body=()->DrawingActions.copySelection(m,list(call.get("entities")));break;
                case "paste":body=()->{Map<String,Object> input=map(copy(call.get("input")));if("COPY".equals(input.get("entities")))input.put("entities",copied);return DrawingActions.paste(m,input);};break;
                case "duplicateLayer":body=()->DrawingActions.duplicateLayer(m,(String)call.get("id"));break;
                case "mergeLayer":body=()->DrawingActions.mergeLayer(m,(String)call.get("source"),(String)call.get("target"));break;
                case "reorderLayer":body=()->DrawingActions.reorderLayer(m,(String)call.get("id"),((Number)call.get("index")).intValue());break;
                case "template":body=()->DrawingActions.template(m,map(call.get("input")));break;
                case "fill":body=()->DrawingActions.fill(m,map(call.get("input")));break;
                case "erase":body=()->DrawingActions.erase(m,map(call.get("input")),FACTORY);break;
                case "eraseRegions":body=()->DrawingActions.eraseRegions(map(call.get("input"))).toArray(new double[0][][]).length+" "+show(DrawingActions.eraseRegions(map(call.get("input"))).get(0));break;
                case "transform":body=()->Collaboration.transform(m,list(call.get("entities")),map(call.get("spec")));break;
                case "transformSimple":body=()->Collaboration.transform(m,list(call.get("entities")),((Number)call.get("scale")).doubleValue(),((Number)call.get("dx")).doubleValue(),((Number)call.get("dy")).doubleValue());break;
                case "diagram":body=()->Actions.diagram(m,map(call.get("input")));break;
                case "inverse":body=()->{List<Object> r=new ArrayList<>();for(Object op:list(call.get("ops"))){try{r.add(Actions.inverse(m,map(op)));}catch(Exception e){r.add(err(e));}}return r;};break;
                case "validateReferences":body=()->{Model copy=new Model(m);for(Object op:list(call.get("ops")))copy.apply(map(op));Actions.validateReferences(copy,list(call.get("ops")));return "OK";};break;
                case "select":body=()->{double[][] region=call.get("region")==null?null:Geometry.polygon(call.get("region"));String layer=(String)call.get("layer");int page=((Number)call.get("page")).intValue();boolean top=Boolean.TRUE.equals(call.get("topmost"));
                    return write(LayerEditor.select(m,layer,page,region,top,FACTORY))+" | "+write(new LayerEditor(m,layer,page).select(region,top,FACTORY))+" | "+write(new LayerEditor(m,layer,page,true).select(region,top,FACTORY));};break;
                case "paintBounds":body=()->{StringBuilder b=new StringBuilder();for(DrawItem item:new LayerEditor(m,(String)call.get("layer"),((Number)call.get("page")).intValue()).items()){b.append(item.kind).append(':').append(item.id).append('=');try{b.append(show(item.paintBounds(FACTORY)));}catch(Exception e){b.append(err(e));}b.append(' ');}return b.toString();};break;
                case "requireReferences":body=()->{LayerEditor.requireReferences(m,(String)call.get("layer"),((Number)call.get("page")).intValue(),list(call.get("refs")));return "OK";};break;
                case "locks":body=()->{
                    StringBuilder b=new StringBuilder();Map<String,Object> state=map(copy(call.get("state")));Store.Snapshot snap=new Store.Snapshot(m);snap.putAll(state);
                    try{Collaboration.requireUnlocked(snap,"me",list(call.get("ops")),1000);b.append("unlocked");}catch(Exception e){b.append(err(e));}
                    b.append(" | leases=").append(write(snap.get("leases")));
                    try{Collaboration.requireIncomingUnlocked(snap,map(call.get("incoming")),1000);b.append(" | incoming ok");}catch(Exception e){b.append(" | ").append(err(e));}
                    b.append(" | affected=").append(write(Collaboration.affected(list(call.get("ops")))));
                    b.append(" | conflicts=").append(write(Collaboration.conflicts(snap,"me",Arrays.asList(object("kind","object","id","app"),object("kind","object","id","state","page",1)))));
                    b.append(" | key=").append(Collaboration.key("object","x",null)).append(',').append(Collaboration.key("object","x",2.0));
                    try{Collaboration.requireSelection(snap,m,"other","s2",Arrays.asList(object("kind","object","id","app")));b.append(" | selection ok");}catch(Exception e){b.append(" | ").append(err(e));}
                    try{Collaboration.requireSelection(snap,m,"me","s1",Arrays.asList(object("kind","object","id","app")));b.append(" | selection ok");}catch(Exception e){b.append(" | ").append(err(e));}
                    return b.toString();};break;
                default:throw new IllegalArgumentException("Unknown fn "+fn);
            }
            try{Object v=body.run();line(label+" "+show(v));if(fn.equals("copySelection")&&clipboard==null)clipboard=list(v);}catch(Throwable e){line(label+" "+err(e));}
        }
    }

    // ------------------------------------------------------------------ store
    static void readStore(Path dir,String document) throws Exception {
        begin("store/"+document);
        Store store=new Store(dir.toFile(),document);
        attempt("file",()->store.file.getName());
        attempt("read",()->store.read());
        attempt("model",()->store.model().json());
        attempt("replica",()->store.replica(0,0));
        attempt("replica-tail",()->store.replica(1,2));
        attempt("replica-bad",()->store.replica(-1,0));
        attempt("needsUpload",()->store.needsUpload("peer"));
        attempt("conflicts",()->store.conflicts());
        attempt("page",()->Store.page(list(store.read().get("events")),0).size());
        attempt("writeRequiresProxy",()->store.append(object("kind","active","stroke","x","active",true),"me"));
    }
    static void dump(Path dir) throws Exception {
        List<Path> files=new ArrayList<>();try(DirectoryStream<Path> d=Files.newDirectoryStream(dir)){for(Path p:d)files.add(p);}
        Collections.sort(files);
        for(Path p:files){
            String name=p.getFileName().toString();
            if(name.endsWith(".journal")){
                byte[] b=Files.readAllBytes(p);line("file "+name+" bytes="+b.length);int i=0;
                while(i+4<=b.length){int len=((b[i]&255)<<24)|((b[i+1]&255)<<16)|((b[i+2]&255)<<8)|(b[i+3]&255);if(i+8+len>b.length){line("  torn tail at "+i);break;}
                    java.util.zip.CRC32 crc=new java.util.zip.CRC32();crc.update(b,i+4,len);int stored=((b[i+4+len]&255)<<24)|((b[i+5+len]&255)<<16)|((b[i+6+len]&255)<<8)|(b[i+7+len]&255);
                    line("  record len="+len+" crcok="+((int)crc.getValue()==stored)+" "+new String(b,i+4,len,StandardCharsets.UTF_8));i+=8+len;}
            }else if(name.endsWith(".json"))line("file "+name+" "+read(p));
            else line("file "+name+" bytes="+Files.size(p));
        }
    }
    static void writeSequence(Path dir) throws Exception {
        begin("write");
        Store w=Store.writer(dir.toFile(),"parity-doc");
        Map<String,Object> diagram=map(parse(read(fixtures.resolve("../../../../../apps/cli/examples/diagram.json"))));
        attempt("accept",()->w.accept(Arrays.asList(object("kind","document","type","pages","id","op-doc","actor","server"),
            object("kind","stroke","stroke","seed","layer","base","page",0,"width",3,"color",-16777216,"points",Arrays.asList(Arrays.asList(1,2,0.5),Arrays.asList(3.25,4,1)),"id","op-seed","actor","server")),"root-1"));
        attempt("accept-again",()->w.accept(Arrays.asList(object("kind","document","type","pages","id","op-doc","actor","server")),"root-1"));
        attempt("accept-conflict",()->w.accept(Arrays.asList(object("kind","document","type","fixed","id","op-doc")),null));
        attempt("accept-root",()->w.accept(Collections.emptyList(),"root-2"));
        attempt("append",()->w.append(object("kind","stroke","stroke","s1","layer","base","page",0,"width",3,"color",-16777216,"points",Arrays.asList(Arrays.asList(1,2,0.5))),"alice").get("revision"));
        attempt("commit",()->w.commit("alice",m->Actions.diagram(m,diagram)));
        attempt("commit-bob",()->w.commit("bob",m->Arrays.<Object>asList(Actions.drawing(object("object","bob-1","shape","star","x",5,"y",6,"fill",-256),"base"))));
        attempt("undo",()->w.history("alice",false));attempt("redo",()->w.history("alice",true));attempt("undo-bob",()->w.history("bob",false));
        attempt("undo-empty",()->w.history("carol",false));attempt("redo-bob",()->w.history("bob",true));attempt("redo-bob-2",()->w.history("bob",true));
        attempt("settings",()->w.appendWithSettings(object("kind","active","stroke","s1","active",false),"alice",object("hidden",Arrays.asList("x"),"undo",Arrays.asList("u1"),"redo",Collections.emptyList(),"selected","base","page",0)).get("revision"));
        attempt("settings-bad",()->w.appendWithSettings(object("kind","active","stroke","s1","active",false),"alice",object("color",1)));
        attempt("lists",()->w.edit(s->{s.put("undo",Arrays.asList("u1","u2"));s.put("redo",Arrays.asList("r"));}).get("revision"));
        attempt("meta",()->w.edit(s->{s.put("custom",object("k",1.5));}).get("revision"));
        attempt("lease",()->w.editTransient(s->{map(s.get("leases")).put("0:object:bob-1",object("actor","alice","session","s9","expires",Long.MAX_VALUE));map(s.get("activeSessions")).put("alice","s9");}).get("leases"));
        attempt("locked",()->w.commit("bob",m->Arrays.<Object>asList(Actions.drawing(object("object","bob-1","shape","star","x",9,"y",6),"base"))));
        attempt("selected",()->w.commitSelected("alice","s9",Arrays.asList(object("kind","object","id","bob-1")),m->Collaboration.transform(m,Arrays.asList(object("kind","object","id","bob-1")),2,1,1),Collections.emptyList()));
        attempt("unlease",()->w.editTransient(s->{s.put("leases",object());}).get("leases"));
        for(int i=0;i<6;i++){final int k=i;List<Object> pts=new ArrayList<>();for(int j=0;j<10+k*6;j++)pts.add(Arrays.asList(j*1.5,(j*7%13)/4.0,0.5));
            attempt("grow-"+i,()->w.append(object("kind","stroke","stroke","grow","layer","base","page",0,"width",4,"color",-65536,"points",pts),"alice").get("revision"));}
        attempt("commit-move",()->w.edit(s->{List<Object> pending=list(s.get("pending"));Object first=pending.remove(1);list(s.get("events")).add(first);s.put("cursor",1);}).get("revision"));
        attempt("checkpoint",()->w.checkpoint());
        attempt("checkpoint-again",()->w.checkpoint());
        attempt("conflicting",()->w.accept(Arrays.asList(object("kind","batch","id","op-remote","actor","server","actions",Arrays.asList(object("kind","object","object","bob-1","layer","base","page",0,"shape","star","x",50,"y",60,"w",160,"h",80,"color",-16777216,"width",3,"fontSize",24,"text","")),"bases",object("object:bob-1","op-old"))),null));
        attempt("conflicts",()->w.conflicts());
        attempt("resolve",()->{List<Object> c=w.conflicts();return c.isEmpty()?"none":w.conflict(string(map(c.get(0)),"id"),"both","alice");});
        attempt("resolve-bad",()->w.conflict("nope","local","alice"));
        attempt("resolve-choice",()->w.conflict("nope","mine","alice"));
        attempt("model",()->w.model().json());
        attempt("delete",()->w.deleteDocument("alice"));attempt("delete-again",()->w.deleteDocument("alice"));
        attempt("after-delete",()->w.commit("alice",m->Arrays.<Object>asList(Actions.drawing(object("shape","rect"),"base"))));
        attempt("read",()->w.read());
        attempt("invalid-doc",()->Store.writer(dir.toFile(),""));
        flush();begin("write/files");dump(dir);
    }
    static void mutate(Path dir,String document) throws Exception {
        begin("mutate/"+document);
        Store w=Store.writer(dir.toFile(),document);
        attempt("commit",()->w.commit("cross",m->Arrays.<Object>asList(Actions.drawing(object("object","cross-1","shape","hexagon","x",11,"y",12,"text","from mutate"),"base"))));
        attempt("append",()->w.append(object("kind","stroke","stroke","cross-s","layer","base","page",0,"width",3,"color",-16777216,"points",Arrays.asList(Arrays.asList(1,2,0.5),Arrays.asList(5,6,0.7))),"cross").get("revision"));
        attempt("undo",()->w.history("cross",false));
        attempt("checkpoint",()->w.checkpoint().get("journalBytesAfter"));
        attempt("redo",()->w.history("cross",true));
        attempt("model",()->w.model().json());
    }
}
