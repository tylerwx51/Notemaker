package com.notemaker.core;

import java.util.*;

/** Item-local painted area. Implementations never know about layers or sessions. */
public interface Coverage {
    double[] bounds();
    boolean intersects(double[][] polygon);
    interface Factory { Coverage create(Map<String,Object> localContent); }

    /** Lazy alpha support, one document unit per pixel. The cache is bounded per item.
     * Empty/full tiles are sentinels; mixed 64x64 tiles have exactly 512 payload bytes.
     * No canvas-sized bitmap is allocated, including on infinite pages. */
    final class Tiles implements Coverage {
        public interface Painter { int[] paint(int x,int y,int size); }
        private static final long[] EMPTY=new long[0], FULL=new long[0];
        private final double[] box;
        private final Painter painter;
        private final Map<Long,long[]> cache=new LinkedHashMap<Long,long[]>(32,.75f,true){
            protected boolean removeEldestEntry(Map.Entry<Long,long[]> e){return size()>128;}
        };
        public Tiles(double[] box,Painter painter){this.box=box;this.painter=painter;}
        public double[] bounds(){return box.clone();}
        private synchronized long[] tile(int x,int y){
            long key=((long)x<<32)^(y&0xffffffffL);long[] bits=cache.get(key);if(bits!=null)return bits;
            int[] pixels=painter.paint(x*64,y*64,64);bits=new long[64];int count=0;
            for(int i=0;i<4096;i++)if((pixels[i]>>>24)!=0){bits[i>>>6]|=1L<<(i&63);count++;}
            if(count==0)bits=EMPTY;else if(count==4096)bits=FULL;cache.put(key,bits);return bits;
        }
        public boolean intersects(double[][] polygon){
            double[] q=Geometry.bounds(polygon);int l=(int)Math.floor(Math.max(box[0],q[0])),t=(int)Math.floor(Math.max(box[1],q[1]));
            int r=(int)Math.ceil(Math.min(box[2],q[2])),b=(int)Math.ceil(Math.min(box[3],q[3]));
            if(l>=r||t>=b)return false;
            // Reject pathological requests explicitly instead of allocating or silently missing hits.
            Model.require((long)(Math.floorDiv(r-1,64)-Math.floorDiv(l,64)+1)*(Math.floorDiv(b-1,64)-Math.floorDiv(t,64)+1)<=16384,"Coverage query too large; use a smaller region");
            for(int ty=Math.floorDiv(t,64);ty<=Math.floorDiv(b-1,64);ty++)for(int tx=Math.floorDiv(l,64);tx<=Math.floorDiv(r-1,64);tx++){
                if(!Geometry.intersects(polygon,Geometry.rect(tx*64,ty*64,64,64)))continue;
                long[] bits=tile(tx,ty);if(bits==EMPTY)continue;
                if(bits==FULL)return true;
                for(int y=Math.max(t,ty*64);y<Math.min(b,ty*64+64);y++)for(int x=Math.max(l,tx*64);x<Math.min(r,tx*64+64);x++){
                    int i=(y-ty*64)*64+x-tx*64;
                    if((bits[i>>>6]&(1L<<(i&63)))!=0&&Geometry.intersects(polygon,Geometry.rect(x,y,1,1)))return true;
                }
            }return false;
        }
    }
}
