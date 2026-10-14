package com.notemaker.core;

import java.util.*;
import static com.notemaker.core.Json.*;
import static com.notemaker.core.Model.require;

/** Bounded flood selection of an enclosed region formed by native ink and shapes. */
public final class RegionFill {
    private RegionFill() {}
    public static List<Object> create(Model model,Map<String,Object> input){
        double x=number(input,"x"),y=number(input,"y");Model.color(input,"color");
        double requestedPage=((Number)input.getOrDefault("page",0)).doubleValue();require(requestedPage==Math.rint(requestedPage)&&requestedPage>=0&&requestedPage<=10000,"Invalid fill page");int page=(int)requestedPage;String layer=(String)input.getOrDefault("layer","base");
        require(Double.isFinite(x)&&Double.isFinite(y)&&Math.abs(x)<=1000000&&Math.abs(y)<=1000000,"Invalid fill point");
        List<double[]> lines=new ArrayList<>();double[] bounds={x,y,x,y};
        for(Map<String,Object> s:model.strokes.values())if(visible(model,s,page,layer,true)){
            List<Object> points=list(s.get("points"));for(int i=1;i<points.size();i++){List<Object> a=list(points.get(i-1)),b=list(points.get(i));add(lines,bounds,((Number)a.get(0)).doubleValue(),((Number)a.get(1)).doubleValue(),((Number)b.get(0)).doubleValue(),((Number)b.get(1)).doubleValue(),number(s,"width"));}
        }
        for(Map<String,Object> o:model.objects.values())if(visible(model,o,page,layer,false)){
            String shape=string(o,"shape");double ox=number(o,"x"),oy=number(o,"y"),w=number(o,"w"),h=number(o,"h"),width=number(o,"width");double[][] vertices;
            if(ShapeCatalog.polygon(shape))vertices=ShapeCatalog.vertices(shape);
            else if(Arrays.asList("rect","rectangle").contains(shape))vertices=new double[][]{{0,0},{1,0},{1,1},{0,1}};
            else if(Arrays.asList("ellipse","circle").contains(shape)){vertices=new double[128][2];for(int i=0;i<128;i++){double a=i*Math.PI/64;vertices[i]=new double[]{.5+.5*Math.cos(a),.5+.5*Math.sin(a)};}}
            else if(Arrays.asList("line","arrow","arrowLine").contains(shape))vertices=new double[][]{{0,0},{1,1}};
            else continue;
            double rotation=((Number)o.getOrDefault("rotation",0)).doubleValue()*Math.PI/180,cos=Math.cos(rotation),sin=Math.sin(rotation);
            double[][] transformed=new double[vertices.length][2];for(int i=0;i<vertices.length;i++){double vx=(vertices[i][0]-.5)*w*(Boolean.TRUE.equals(o.get("flipX"))?-1:1),vy=(vertices[i][1]-.5)*h*(Boolean.TRUE.equals(o.get("flipY"))?-1:1);transformed[i]=new double[]{ox+w/2+vx*cos-vy*sin,oy+h/2+vx*sin+vy*cos};}
            boolean closed=!Arrays.asList("line","arrow","arrowLine","polyline").contains(shape);for(int i=1;i<vertices.length+(closed?1:0);i++){double[] a=transformed[i-1],b=transformed[i%vertices.length];add(lines,bounds,a[0],a[1],b[0],b[1],width);}
        }
        require(!lines.isEmpty(),"No enclosing ink on this layer");
        double pad=20,scale=Math.max(1,Math.max(bounds[2]-bounds[0]+2*pad,bounds[3]-bounds[1]+2*pad)/1000),left=bounds[0]-pad,top=bounds[1]-pad;
        int width=(int)Math.ceil((bounds[2]-bounds[0]+2*pad)/scale)+1,height=(int)Math.ceil((bounds[3]-bounds[1]+2*pad)/scale)+1;byte[] mask=new byte[width*height];
        long work=0;
        for(double[] l:lines){double ax=(l[0]-left)/scale,ay=(l[1]-top)/scale,bx=(l[2]-left)/scale,by=(l[3]-top)/scale,r=Math.max(.7,l[4]/2/scale);int x0=Math.max(0,(int)Math.floor(Math.min(ax,bx)-r)),x1=Math.min(width-1,(int)Math.ceil(Math.max(ax,bx)+r)),y0=Math.max(0,(int)Math.floor(Math.min(ay,by)-r)),y1=Math.min(height-1,(int)Math.ceil(Math.max(ay,by)+r));double dx=bx-ax,dy=by-ay,q=dx*dx+dy*dy;
            work+=(long)(x1-x0+1)*(y1-y0+1);require(work<=20000000,"Fill boundary is too complex; use a smaller layer or closed outline");
            for(int yy=y0;yy<=y1;yy++)for(int xx=x0;xx<=x1;xx++){double t=q==0?0:Math.max(0,Math.min(1,((xx-ax)*dx+(yy-ay)*dy)/q));if(Math.hypot(xx-ax-t*dx,yy-ay-t*dy)<=r)mask[yy*width+xx]=1;}}
        int sx=(int)Math.round((x-left)/scale),sy=(int)Math.round((y-top)/scale);require(mask[sy*width+sx]==0,"Tap inside the outline, away from its edge");
        int[] queue=new int[mask.length];int head=0,tail=0;queue[tail++]=sy*width+sx;mask[sy*width+sx]=2;
        while(head<tail){int p=queue[head++],xx=p%width,yy=p/width;require(xx>0&&xx<width-1&&yy>0&&yy<height-1,"Area is open; fill requires a closed boundary");for(int next:new int[]{p-1,p+1,p-width,p+width})if(mask[next]==0){mask[next]=2;queue[tail++]=next;}}
        StringBuilder path=new StringBuilder();for(int yy=1;yy<height-1;yy++)for(int xx=1;xx<width-1;xx++)if(mask[yy*width+xx]==2){int start=xx;while(xx+1<width&&mask[yy*width+xx+1]==2)xx++;path.append('M').append(start-.5).append(' ').append(yy-.5).append('h').append(xx-start+1).append("v1h-").append(xx-start+1).append('Z');}
        int color=((Number)input.get("color")).intValue();String svg="<svg xmlns=\"http://www.w3.org/2000/svg\" width=\""+width+"\" height=\""+height+"\" viewBox=\"0 0 "+width+" "+height+"\"><path d=\""+path+"\" fill=\""+String.format(Locale.ROOT,"#%06x",color&0xffffff)+"\" fill-opacity=\""+(color>>>24)/255.0+"\"/></svg>";
        return Arrays.asList(Actions.drawing(object("shape","svg","svg",svg,"x",left,"y",top,"w",width*scale,"h",height*scale,"page",page),layer));
    }
    private static boolean visible(Model m,Map<String,Object> o,int page,String layer,boolean stroke){return number(o,"page")==page&&layer.equals(o.get("layer"))&&!Boolean.FALSE.equals(m.layerActive.get(layer))&&!Boolean.FALSE.equals((stroke?m.active:m.objectActive).get(o.get(stroke?"stroke":"object")));}
    private static void add(List<double[]> lines,double[] bounds,double x,double y,double xx,double yy,double width){lines.add(new double[]{x,y,xx,yy,width});bounds[0]=Math.min(bounds[0],Math.min(x,xx));bounds[1]=Math.min(bounds[1],Math.min(y,yy));bounds[2]=Math.max(bounds[2],Math.max(x,xx));bounds[3]=Math.max(bounds[3],Math.max(y,yy));}
}
