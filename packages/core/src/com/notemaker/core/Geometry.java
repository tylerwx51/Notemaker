package com.notemaker.core;

import java.util.*;
import static com.notemaker.core.Json.*;

/** Small platform-independent query geometry; polygons use the even/odd fill rule. */
public final class Geometry {
    private Geometry(){}
    public static double[][] rect(double x,double y,double w,double h){return new double[][]{{x,y},{x+w,y},{x+w,y+h},{x,y+h}};}
    public static double[] bounds(double[][] p){double[] b={Double.POSITIVE_INFINITY,Double.POSITIVE_INFINITY,Double.NEGATIVE_INFINITY,Double.NEGATIVE_INFINITY};for(double[] v:p){b[0]=Math.min(b[0],v[0]);b[1]=Math.min(b[1],v[1]);b[2]=Math.max(b[2],v[0]);b[3]=Math.max(b[3],v[1]);}return b;}
    public static boolean contains(double[][] p,double x,double y){boolean in=false;for(int i=0,j=p.length-1;i<p.length;j=i++){double[] a=p[i],b=p[j];if((a[1]>y)!=(b[1]>y)&&x<(b[0]-a[0])*(y-a[1])/(b[1]-a[1])+a[0])in=!in;}return in;}
    private static double cross(double[] a,double[] b,double[] c){return (b[0]-a[0])*(c[1]-a[1])-(b[1]-a[1])*(c[0]-a[0]);}
    public static boolean intersects(double[][] a,double[][] b){
        double[] x=bounds(a),y=bounds(b);if(x[2]<y[0]||y[2]<x[0]||x[3]<y[1]||y[3]<x[1])return false;
        for(double[] p:a)if(contains(b,p[0],p[1]))return true;for(double[] p:b)if(contains(a,p[0],p[1]))return true;
        for(int i=0;i<a.length;i++)for(int j=0;j<b.length;j++){double[] p=a[i],q=a[(i+1)%a.length],r=b[j],s=b[(j+1)%b.length];if(Math.max(p[0],q[0])<Math.min(r[0],s[0])||Math.max(r[0],s[0])<Math.min(p[0],q[0])||Math.max(p[1],q[1])<Math.min(r[1],s[1])||Math.max(r[1],s[1])<Math.min(p[1],q[1]))continue;if(cross(p,q,r)*cross(p,q,s)<=0&&cross(r,s,p)*cross(r,s,q)<=0)return true;}return false;
    }
    public static double[][] polygon(Object raw){List<Object> list=list(raw);Model.require(list.size()>=3&&list.size()<=4096,"Region needs 3–4096 vertices");double[][] p=new double[list.size()][2];for(int i=0;i<p.length;i++){List<Object> v=list(list.get(i));Model.require(v.size()==2,"Region vertex needs x/y");for(int k=0;k<2;k++){Model.require(v.get(k) instanceof Number,"Invalid region vertex");p[i][k]=((Number)v.get(k)).doubleValue();Model.require(Double.isFinite(p[i][k])&&Math.abs(p[i][k])<=1e9,"Invalid region vertex");}}return p;}
    public static List<Object> json(double[][] p){List<Object> out=new ArrayList<>();for(double[] v:p)out.add(Arrays.asList(v[0],v[1]));return out;}
    /** Round swept segment, at most 0.1 document unit chord error for ordinary erasers. */
    public static double[][] capsule(double[] a,double[] b,double radius){int n=Math.min(512,Math.max(16,(int)Math.ceil(Math.PI/Math.acos(Math.max(-1,1-.1/radius)))));double angle=Math.atan2(b[1]-a[1],b[0]-a[0]);double[][] p=new double[2*(n+1)][2];for(int i=0;i<=n;i++){double t=angle-Math.PI/2+Math.PI*i/n;p[i]=new double[]{b[0]+radius*Math.cos(t),b[1]+radius*Math.sin(t)};t+=Math.PI;p[n+1+i]=new double[]{a[0]+radius*Math.cos(t),a[1]+radius*Math.sin(t)};}return p;}
}
