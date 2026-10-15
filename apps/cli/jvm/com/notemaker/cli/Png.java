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
    private final java.util.List<Shape> clips=new java.util.ArrayList<>();
    public static int[] coverageTile(Model m,java.util.Map<String,Object> item,int x,int y,int size){BufferedImage image=new BufferedImage(size,size,BufferedImage.TYPE_INT_ARGB);Graphics2D g=image.createGraphics();try{g.translate(-x,-y);Render.drawItem(m,item,new Png(g));return image.getRGB(0,0,size,size,null,0,size);}finally{g.dispose();}}
    public void beginErase(java.util.List<double[][]> polygons){clips.add(g.getClip());for(double[][] p:polygons){Area keep=g.getClip()==null?new Area(new Rectangle2D.Double(-1e9,-1e9,2e9,2e9)):new Area(g.getClip());keep.subtract(new Area(ItemCoverage.path(p,true)));g.setClip(keep);}}
    public void endErase(){g.setClip(clips.remove(clips.size()-1));}
    public void polygon(double[][] points,int color,double width,Integer fill,boolean closed){Shape p=ItemCoverage.path(points,closed);if(fill!=null){g.setColor(new Color(fill,true));g.fill(p);}if(width>0){g.setColor(new Color(color,true));g.setStroke(new BasicStroke((float)width,BasicStroke.CAP_ROUND,BasicStroke.JOIN_ROUND));g.draw(p);}}
    private final java.util.Deque<AffineTransform> transforms=new java.util.ArrayDeque<>();
    private Png(Graphics2D g){this.g=g;g.setRenderingHint(RenderingHints.KEY_ANTIALIASING,RenderingHints.VALUE_ANTIALIAS_ON);}
    public static void export(Model m,int page,double x,double y,int w,int h,File out)throws IOException {
        BufferedImage image=new BufferedImage(w,h,BufferedImage.TYPE_INT_ARGB);Graphics2D g=image.createGraphics();g.setColor(Color.WHITE);g.fillRect(0,0,w,h);g.translate(-x,-y);Render.draw(m,page,Collections.emptySet(),new Png(g));g.dispose();ImageIO.write(image,"png",out);
    }
    public void line(double x,double y,double ex,double ey,int color,double width){g.setColor(new Color(color,true));g.setStroke(new BasicStroke((float)width,BasicStroke.CAP_ROUND,BasicStroke.JOIN_ROUND));g.draw(new Line2D.Double(x,y,ex,ey));}
    public void rect(double x,double y,double w,double h,int color,double width,Integer fill,boolean ellipse){Shape shape=ellipse?new Ellipse2D.Double(x,y,w,h):new Rectangle2D.Double(x,y,w,h);if(fill!=null){g.setColor(new Color(fill,true));g.fill(shape);}if(width>0){g.setColor(new Color(color,true));g.setStroke(new BasicStroke((float)width));g.draw(shape);}}
    public void text(double x,double y,String text,int color,double size){g.setColor(new Color(color,true));g.setFont(new Font(Font.SANS_SERIF,Font.PLAIN,1).deriveFont((float)size));g.drawString(text,(float)x,(float)y);}
    public void styledText(double x,double y,String text,int color,double size,String family,boolean bold,boolean italic,boolean underline,String alignment){g.setColor(new Color(color,true));int style=(bold?Font.BOLD:0)|(italic?Font.ITALIC:0);String name="serif".equals(family)?Font.SERIF:"monospace".equals(family)?Font.MONOSPACED:Font.SANS_SERIF;g.setFont(new Font(name,style,1).deriveFont((float)size));float drawX=(float)x-("center".equals(alignment)?g.getFontMetrics().stringWidth(text)/2f:"right".equals(alignment)?g.getFontMetrics().stringWidth(text):0);g.drawString(text,drawX,(float)y);if(underline){g.setStroke(new BasicStroke(1));float width=(float)g.getFontMetrics().getStringBounds(text,g).getWidth();g.draw(new Line2D.Double(drawX,y+2,drawX+width,y+2));}}
    public void image(double x,double y,double w,double h,String data){try{int comma=data.indexOf(',');BufferedImage img=ImageIO.read(new ByteArrayInputStream(java.util.Base64.getDecoder().decode(data.substring(comma+1))));if(img==null)throw new IllegalArgumentException("Could not decode image pixels");AffineTransform placement=AffineTransform.getTranslateInstance(x,y);placement.scale(w/img.getWidth(),h/img.getHeight());g.drawImage(img,placement,null);}catch(IllegalArgumentException e){throw e;}catch(Exception e){throw new IllegalArgumentException("Could not render image",e);}}
    public void svg(double x,double y,double w,double h,String source){
        synchronized(SVGS){
            com.github.weisj.jsvg.SVGDocument svg=SVGS.get(source);
            if(svg==null){svg=new com.github.weisj.jsvg.parser.SVGLoader().load(new ByteArrayInputStream(SvgAsset.viewport(source).getBytes(java.nio.charset.StandardCharsets.UTF_8)));Model.require(svg!=null,"Could not render SVG");SVGS.put(source,svg);}
            Graphics2D child=(Graphics2D)g.create();try{child.translate(x,y);child.clip(new Rectangle2D.Double(0,0,w,h));svg.render(null,child,new com.github.weisj.jsvg.attributes.ViewBox(0,0,(float)w,(float)h));}finally{child.dispose();}
        }
    }
    public void saveTransform(double cx,double cy,double rotation,boolean flipX,boolean flipY){transforms.push(g.getTransform());g.translate(cx,cy);g.rotate(Math.toRadians(rotation));g.scale(flipX?-1:1,flipY?-1:1);g.translate(-cx,-cy);}
    public void restoreTransform(){g.setTransform(transforms.pop());}
}
