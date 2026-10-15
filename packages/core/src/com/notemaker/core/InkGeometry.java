package com.notemaker.core;

import java.util.Arrays;

/** Packed paint primitives: six doubles per capsule/dot, without one native path per point. */
public final class InkGeometry {
    private double[] values=new double[96];
    private int length;
    private final double[] box={Double.POSITIVE_INFINITY,Double.POSITIVE_INFINITY,Double.NEGATIVE_INFINITY,Double.NEGATIVE_INFINITY};
    public int size(){return length/6;}
    public double value(int part,int field){return values[part*6+field];}
    public void line(double x,double y,double ex,double ey,double width){add(0,x,y,ex,ey,width);}
    public void dot(double x,double y,double w,double h,boolean ellipse){add(ellipse?1:2,x,y,w,h,0);}
    private void add(double type,double a,double b,double c,double d,double width){
        if(length+6>values.length)values=Arrays.copyOf(values,values.length*2);
        int part=size();values[length++]=type;values[length++]=a;values[length++]=b;values[length++]=c;values[length++]=d;values[length++]=width;
        box[0]=Math.min(box[0],left(part));box[1]=Math.min(box[1],top(part));box[2]=Math.max(box[2],right(part));box[3]=Math.max(box[3],bottom(part));
    }
    public void finish(){values=Arrays.copyOf(values,length);}
    public double[] bounds(){return length==0?new double[4]:box.clone();}
    private double left(int i){return value(i,0)==0?Math.min(value(i,1),value(i,3))-value(i,5)/2:value(i,1);}
    private double top(int i){return value(i,0)==0?Math.min(value(i,2),value(i,4))-value(i,5)/2:value(i,2);}
    private double right(int i){return value(i,0)==0?Math.max(value(i,1),value(i,3))+value(i,5)/2:value(i,1)+value(i,3);}
    private double bottom(int i){return value(i,0)==0?Math.max(value(i,2),value(i,4))+value(i,5)/2:value(i,2)+value(i,4);}
    public boolean overlaps(int i,double l,double t,double r,double b){return left(i)<=r&&right(i)>=l&&top(i)<=b&&bottom(i)>=t;}
}
