package com.notemaker;

import android.graphics.*;
import com.notemaker.cli.Png;
import com.notemaker.core.*;
import java.util.*;
import static com.notemaker.core.Json.*;

/** Crossing ink must be independent of whether the pen was lifted between passes. */
final class StrokeCrossings {
    private static Bitmap render(double[][][] paths,String brush,int color){
        Bitmap image=Bitmap.createBitmap(200,200,Bitmap.Config.ARGB_8888);
        Canvas canvas=new Canvas(image);canvas.drawColor(Color.WHITE);
        for(double[][] path:paths){List<Object> points=new ArrayList<>();for(double[] p:path)points.add(Arrays.asList(p[0],p[1],1));
            Render.drawStroke(object("width",12,"brush",brush,"material",50,"color",color,"points",points),new Png(canvas));}
        return image;
    }
    private static void seams(){
        double[][] circle=new double[181][2];for(int i=0;i<circle.length;i++){double angle=Math.toRadians(i*2);circle[i]=new double[]{100+60*Math.cos(angle),100+60*Math.sin(angle)};}
        for(String brush:new String[]{"pen","marker"}){
            int color=brush.equals("pen")?0x80000000:0xff000000;
            Bitmap curve=render(new double[][][]{circle},brush,color),straight=render(new double[][][]{{{30,100},{170,100}}},brush,color);
            try{int ink=straight.getPixel(100,100);for(int join:new int[]{90,180,270})for(int degrees=join-15;degrees<=join+15;degrees++){
                double angle=Math.toRadians(degrees);int x=(int)Math.round(100+60*Math.cos(angle)),y=(int)Math.round(100+60*Math.sin(angle));
                for(int dy=-1;dy<=1;dy++)for(int dx=-1;dx<=1;dx++)Model.require(curve.getPixel(x+dx,y+dy)==ink,brush+" ribbon seam at "+degrees+" degrees");
            }}finally{curve.recycle();straight.recycle();}
        }
    }
    static void run(){
        seams();
        double[][][] loops={{{30,30},{170,170},{170,30},{30,170}},{{30,100},{170,100},{170,30},{100,30},{100,170}},{{30,100},{170,100},{30,100}}};
        double[][][][] lines={{{{30,30},{170,170}},{{170,30},{30,170}}},{{{30,100},{170,100}},{{100,30},{100,170}}},{{{30,100},{170,100}},{{170,100},{30,100}}}};
        String[] brushes={"pen","pen","marker"};int[] colors={0xff000000,0x80000000,0xff000000};
        for(int style=0;style<brushes.length;style++)for(int fixture=0;fixture<loops.length;fixture++)for(boolean reverse:new boolean[]{false,true}){
            double[][] path=loops[fixture].clone();if(reverse)Collections.reverse(Arrays.asList(path));
            Bitmap actual=render(new double[][][]{path},brushes[style],colors[style]),expected=render(lines[fixture],brushes[style],colors[style]);
            try{int max=0;long sum=0;int interiorChanges=0;for(int y=70;y<130;y++)for(int x=70;x<130;x++){
                    int difference=Math.abs(Color.red(actual.getPixel(x,y))-Color.red(expected.getPixel(x,y)));max=Math.max(max,difference);sum+=difference;
                    boolean flat=true;int pixel=expected.getPixel(x,y);for(int dy=-1;dy<=1;dy++)for(int dx=-1;dx<=1;dx++)if(expected.getPixel(x+dx,y+dy)!=pixel)flat=false;
                    if(flat&&actual.getPixel(x,y)!=pixel)interiorChanges++;
                }
                String label=brushes[style]+" color="+colors[style]+" fixture="+fixture+" reversed="+reverse;
                // A union and two source-over passes differ at antialiased crossing
                // corners; fully covered ink and paper must match exactly.
                Model.require(interiorChanges==0,"Crossing interior changed: "+label+" pixels="+interiorChanges);
                Model.require(max<=64&&sum/(60.0*60)<.34,"Self crossing changed coverage: "+label+" max="+max+" mean="+(sum/(60.0*60)));
                Model.require(actual.getPixel(100,100)==expected.getPixel(100,100),"Crossing ink density changed: "+label);
            }finally{actual.recycle();expected.recycle();}
        }
    }
}
