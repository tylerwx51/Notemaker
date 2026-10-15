import com.notemaker.core.*;
import com.notemaker.cli.*;
import java.util.*;
import java.security.MessageDigest;
import static com.notemaker.core.Json.*;

/** Deterministic query digest: compare the installed union implementation and candidate. */
public final class CoverageParity {
    public static void main(String[] args)throws Exception{
        Random random=new Random(72931);MessageDigest hash=MessageDigest.getInstance("SHA-256");int queries=0,hits=0;
        for(String brush:Arrays.asList("pen","pencil","marker","brush","oily","fountain","charcoal","neoBrush","latinCalligraphy","asiaCalligraphy","square")){
            List<Object> points=new ArrayList<>();for(int i=0;i<90;i++)points.add(Arrays.asList(60+40*Math.sin(i*.3),60+40*Math.cos(i*.7),.1+random.nextDouble()*.9));
            Map<String,Object> stroke=object("kind","stroke","stroke",brush,"layer","base","page",0,"color",-16777216,"width",7,"brush",brush,"points",points);
            for(boolean masked:new boolean[]{false,true}){
                if(masked)stroke.put("erasures",Arrays.asList(Geometry.json(Geometry.rect(-45,-25,30,110)),Geometry.json(Geometry.capsule(new double[]{-40,-50},new double[]{30,0},9))));
                Model model=new Model();model.apply(stroke);DrawItem item=new DrawItem(model,stroke);
                for(int i=0;i<300;i++){
                    double x=random.nextDouble()*130,y=random.nextDouble()*130;
                    boolean hit=item.intersects(Geometry.rect(x,y,.4+random.nextDouble()*8,.4+random.nextDouble()*8),ItemCoverage::create);
                    hash.update((byte)(hit?1:0));queries++;if(hit)hits++;
                }
            }
        }
        for(String shape:Arrays.asList("rect","ellipse","triangle","line","arrow","text","svg")){
            Map<String,Object> input=object("object",shape,"shape",shape,"x",20,"y",30,"w",60,"h",40,"width",5,"color",-16777216,"fill",-65536,"text",shape.equals("text")?"O gj":"","fontSize",30);
            if(shape.equals("svg"))input.put("svg","<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"60\" height=\"40\"><circle cx=\"30\" cy=\"20\" r=\"18\" fill=\"none\" stroke=\"black\" stroke-width=\"4\"/></svg>");
            Map<String,Object> o=Actions.drawing(input,"base");
            o.put("rotation",33);o.put("flipX",true);o.put("erasures",Arrays.asList(Geometry.json(Geometry.rect(.3,-1,.2,3))));
            Model model=new Model();model.apply(o);DrawItem item=new DrawItem(model,o);
            for(int i=0;i<300;i++){boolean hit=item.intersects(Geometry.rect(random.nextDouble()*130,random.nextDouble()*130,2,2),ItemCoverage::create);hash.update((byte)(hit?1:0));queries++;if(hit)hits++;}
        }
        StringBuilder digest=new StringBuilder();for(byte b:hash.digest())digest.append(String.format("%02x",b&255));
        System.out.println(write(object("queries",queries,"hits",hits,"sha256",digest.toString())));System.out.flush();System.exit(0);
    }
}
