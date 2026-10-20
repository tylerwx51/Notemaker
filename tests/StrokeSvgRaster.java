import java.awt.*;
import java.awt.image.BufferedImage;
import java.io.*;
import javax.imageio.ImageIO;

/** Rasterize an exported SVG as a document, outside the imported-asset profile. */
public final class StrokeSvgRaster {
    public static void main(String[] args)throws Exception{
        com.github.weisj.jsvg.SVGDocument svg=new com.github.weisj.jsvg.parser.SVGLoader().load(new File(args[0]).toURI().toURL());
        if(svg==null)throw new IllegalArgumentException("Invalid exported SVG");
        BufferedImage image=new BufferedImage(200,200,BufferedImage.TYPE_INT_ARGB);Graphics2D g=image.createGraphics();
        try{g.setRenderingHint(RenderingHints.KEY_ANTIALIASING,RenderingHints.VALUE_ANTIALIAS_ON);svg.render(null,g,new com.github.weisj.jsvg.attributes.ViewBox(0,0,200,200));}finally{g.dispose();}
        ImageIO.write(image,"png",new File(args[1]));
    }
}
