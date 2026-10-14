package com.notemaker.core;

import java.util.*;

/** Stable NoteMaker shape identifiers and normalized geometry. */
public final class ShapeCatalog {
    private static final List<String> IDS=Collections.unmodifiableList(Arrays.asList(
        "rect","ellipse","text","line","triangle","diamond","pentagon","hexagon","star","arrow","svg",
        "trapezoid","triangle45","triangle60","triangle90","wave","curve","polyline","regularPolygon",
        "cube","cylinder","cone","sphere","pyramid","image","circle","rectangle","rhombus","arrowLine","waveLine","regularHexagon"));
    private ShapeCatalog() {}
    public static List<String> ids(){return IDS;}
    public static boolean known(String id){return IDS.contains(id);}
    public static List<Object> json(){List<Object> result=new ArrayList<>();for(String id:IDS)result.add(Json.object("id",id,"name",name(id)));return result;}
    public static String name(String id){
        switch(id){case "rect":return "Rectangle";case "ellipse":return "Ellipse";case "text":return "Text";case "line":return "Line";
            case "triangle":return "Triangle";case "diamond":return "Diamond";case "pentagon":return "Pentagon";case "hexagon":return "Hexagon";
            case "star":return "Star";case "arrow":return "Arrow";case "svg":return "SVG";case "trapezoid":return "Trapezoid";
            case "triangle45":return "45° Triangle";case "triangle60":return "60° Triangle";case "triangle90":return "Right Triangle";
            case "wave":return "Wave";case "curve":return "Curve";case "polyline":return "Polyline";case "regularPolygon":return "Regular Polygon";
            case "cube":return "Cube";case "cylinder":return "Cylinder";case "cone":return "Cone";case "sphere":return "Sphere";case "pyramid":return "Pyramid";case "image":return "Picture";
            case "circle":return "Circle";case "rectangle":return "Rectangle";case "rhombus":return "Rhombus";case "arrowLine":return "Arrow Line";case "waveLine":return "Wave Line";case "regularHexagon":return "Regular Hexagon";
            default:throw new IllegalArgumentException("Unknown shape: "+id);}
    }
    public static boolean polygon(String id){return Arrays.asList("triangle","diamond","rhombus","pentagon","hexagon","star","trapezoid","triangle45","triangle60","triangle90","regularPolygon","regularHexagon","polyline").contains(id);}
    /** Vertices normalized to a unit bounding box, clockwise from the top. */
    public static double[][] vertices(String id){
        switch(id){
            case "triangle":return new double[][]{{.5,0},{1,1},{0,1}};
            case "triangle45":return new double[][]{{0,1},{1,0},{1,1}};
            case "triangle60":return new double[][]{{.5,0},{1,1},{0,1}};
            case "triangle90":return new double[][]{{0,0},{0,1},{1,1}};
            case "diamond":return new double[][]{{.5,0},{1,.5},{.5,1},{0,.5}};
            case "rhombus":return new double[][]{{.5,0},{1,.5},{.5,1},{0,.5}};
            case "trapezoid":return new double[][]{{.2,0},{.8,0},{1,1},{0,1}};
            case "pentagon":return regular(5,Math.PI*1.5);
            case "hexagon":return regular(6,Math.PI*1.5);
            case "regularHexagon":return regular(6,Math.PI*1.5);
            case "regularPolygon":return regular(8,Math.PI*1.5);
            case "polyline":return new double[][]{{0,1},{.3,.25},{.65,.7},{1,0}};
            case "star":{double[][] p=new double[10][2];for(int i=0;i<10;i++){double a=-Math.PI/2+i*Math.PI/5,r=i%2==0?.5:.22;p[i][0]=.5+Math.cos(a)*r;p[i][1]=.5+Math.sin(a)*r;}return p;}
            default:throw new IllegalArgumentException("Shape has no polygon geometry: "+id);
        }
    }
    private static double[][] regular(int n,double start){double[][] p=new double[n][2];for(int i=0;i<n;i++){double a=start+i*2*Math.PI/n;p[i][0]=.5+.5*Math.cos(a);p[i][1]=.5+.5*Math.sin(a);}return p;}
    /** Small SVG scene for geometric families that need more than one path. */
    public static String drawingSvg(String id){
        switch(id){
            case "cube":return "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 100 100\"><path d=\"M50 4 92 27 92 73 50 96 8 73 8 27Z M50 4V50 M8 27 50 50 92 27 M50 50V96\" fill=\"none\"/></svg>";
            case "pyramid":return "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 100 100\"><path d=\"M50 6 8 84 92 84Z M50 6 50 84 M8 84 50 65 92 84\" fill=\"none\"/></svg>";
            case "cone":return "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 100 100\"><path d=\"M50 5 8 82 M50 5 92 82 M8 82 C15 98 85 98 92 82 M8 82 C28 68 72 68 92 82\" fill=\"none\"/></svg>";
            case "cylinder":return "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 100 100\"><path d=\"M8 20 C8 2 92 2 92 20 V80 C92 98 8 98 8 80 Z M8 20 C8 38 92 38 92 20 M8 80 C8 62 92 62 92 80\" fill=\"none\"/></svg>";
            case "sphere":return "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 100 100\"><circle cx=\"50\" cy=\"50\" r=\"46\" fill=\"none\"/><path d=\"M5 50 C20 28 80 28 95 50 C80 72 20 72 5 50 M50 4 C22 25 22 75 50 96 M50 4 C78 25 78 75 50 96\" fill=\"none\"/></svg>";
            case "wave":case "waveLine":return "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 100 100\"><path d=\"M0 55 C16 5 34 5 50 55 S84 105 100 55\" fill=\"none\"/></svg>";
            case "curve":return "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 100 100\"><path d=\"M2 90 C15 5 62 5 98 10\" fill=\"none\"/></svg>";
            default:throw new IllegalArgumentException("Shape has no SVG geometry: "+id);
        }
    }
}
