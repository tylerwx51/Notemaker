package com.notemaker.cli;
import com.notemaker.core.*;
import java.awt.*;
import java.awt.geom.*;
import java.awt.image.BufferedImage;
import java.io.*;
import java.util.Collections;
import javax.imageio.ImageIO;

public final class Png implements Render.Surface {
    private static final java.util.Map<String,com.github.weisj.jsvg.SVGDocument> SVGS=new java.util.LinkedHashMap<String,com.github.weisj.jsvg.SVGDocument>(16,.75f,true){protected boolean removeEldestEntry(java.util.Map.Entry<String,com.github.weisj.jsvg.SVGDocument> e){return size()>16;}};
    private final Graphics2D g;
    private Png(Graphics2D g){this.g=g;g.setRenderingHint(RenderingHints.KEY_ANTIALIASING,RenderingHints.VALUE_ANTIALIAS_ON);}
    public static void export(Model m,int page,double x,double y,int w,int h,File out)throws IOException {
        BufferedImage image=new BufferedImage(w,h,BufferedImage.TYPE_INT_ARGB);Graphics2D g=image.createGraphics();g.setColor(Color.WHITE);g.fillRect(0,0,w,h);g.translate(-x,-y);Render.draw(m,page,Collections.emptySet(),new Png(g));g.dispose();ImageIO.write(image,"png",out);
    }
    public void line(double x,double y,double ex,double ey,int color,double width){g.setColor(new Color(color,true));g.setStroke(new BasicStroke((float)width,BasicStroke.CAP_ROUND,BasicStroke.JOIN_ROUND));g.draw(new Line2D.Double(x,y,ex,ey));}
    public void rect(double x,double y,double w,double h,int color,double width,Integer fill,boolean ellipse){Shape shape=ellipse?new Ellipse2D.Double(x,y,w,h):new Rectangle2D.Double(x,y,w,h);if(fill!=null){g.setColor(new Color(fill,true));g.fill(shape);}if(width>0){g.setColor(new Color(color,true));g.setStroke(new BasicStroke((float)width));g.draw(shape);}}
    public void text(double x,double y,String text,int color,double size){g.setColor(new Color(color,true));g.setFont(new Font(Font.SANS_SERIF,Font.PLAIN,1).deriveFont((float)size));g.drawString(text,(float)x,(float)y);}
    public void svg(double x,double y,double w,double h,String source){
        synchronized(SVGS){
            com.github.weisj.jsvg.SVGDocument svg=SVGS.get(source);
            if(svg==null){svg=new com.github.weisj.jsvg.parser.SVGLoader().load(new ByteArrayInputStream(SvgAsset.viewport(source).getBytes(java.nio.charset.StandardCharsets.UTF_8)));Model.require(svg!=null,"Could not render SVG");SVGS.put(source,svg);}
            Graphics2D child=(Graphics2D)g.create();try{child.translate(x,y);child.clip(new Rectangle2D.Double(0,0,w,h));svg.render(null,child,new com.github.weisj.jsvg.attributes.ViewBox(0,0,(float)w,(float)h));}finally{child.dispose();}
        }
    }
}
