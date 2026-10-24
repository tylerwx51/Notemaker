package com.notemaker.core;

import java.util.*;
import static com.notemaker.core.Model.require;

/** Bounds and frame delays for an embedded GIF. Playback position is never stored here. */
public final class GifAsset {
    private GifAsset() {}
    public static final int MAX_BYTES=2*1024*1024;
    public static String fromBytes(byte[] bytes){validate(bytes);return "data:image/gif;base64,"+Base64.getEncoder().encodeToString(bytes);}
    public static Info validate(String uri){
        require(uri!=null&&uri.startsWith("data:image/gif;base64,")&&uri.length()<=MAX_BYTES*4L/3+100,"GIF must be embedded and at most 2 MiB");
        try{return validate(Base64.getDecoder().decode(uri.substring(22)));}catch(IllegalArgumentException e){throw new IllegalArgumentException("Invalid GIF: "+e.getMessage());}
    }
    public static final class Info {
        public final int width,height;public final int[] delays,offsets;public final int duration;
        Info(int w,int h,List<Integer> values){width=w;height=h;delays=new int[values.size()];offsets=new int[values.size()];int total=0,raw=0;for(int i=0;i<delays.length;i++){offsets[i]=raw;raw+=values.get(i);delays[i]=Math.max(100,Math.min(10000,values.get(i)));total+=delays[i];}duration=total;}
        public int timeForFrame(int frame){return offsets[frame];}
    }
    private static int u16(byte[] b,int p){return (b[p]&255)|((b[p+1]&255)<<8);}
    private static int blocks(byte[] b,int p){while(true){require(p<b.length,"Truncated GIF block");int n=b[p++]&255;if(n==0)return p;require(p+n<=b.length,"Truncated GIF block");p+=n;}}
    public static Info validate(byte[] b){
        require(b!=null&&b.length>=14&&b.length<=MAX_BYTES,"GIF exceeds 2 MiB or has no header");
        String header=new String(b,0,6,java.nio.charset.StandardCharsets.US_ASCII);require(header.equals("GIF87a")||header.equals("GIF89a"),"Invalid GIF header");
        int w=u16(b,6),h=u16(b,8);require(w>0&&h>0&&w<=8192&&h<=8192&&(long)w*h<=32L*1024*1024,"Invalid GIF dimensions");
        int p=13,packed=b[10]&255;if((packed&128)!=0)p+=3*(1<<((packed&7)+1));require(p<=b.length,"Truncated GIF palette");
        ArrayList<Integer> delays=new ArrayList<>();int delay=100;boolean ended=false;
        while(p<b.length){int tag=b[p++]&255;if(tag==0x3b){ended=true;break;}
            if(tag==0x21){require(p<b.length,"Truncated GIF extension");int label=b[p++]&255;if(label==0xf9){require(p+6<=b.length&&(b[p]&255)==4&&b[p+5]==0,"Invalid GIF frame control");int centiseconds=u16(b,p+2);delay=centiseconds==0?100:centiseconds*10;p+=6;}else p=blocks(b,p);continue;}
            require(tag==0x2c&&p+9<=b.length,"Invalid GIF frame");int fw=u16(b,p+4),fh=u16(b,p+6);require(fw>0&&fh>0,"Invalid GIF frame size");int flags=b[p+8]&255;p+=9;if((flags&128)!=0)p+=3*(1<<((flags&7)+1));require(p<b.length,"Truncated GIF frame palette");p++;p=blocks(b,p);delays.add(delay);delay=100;require(delays.size()<=500,"GIF has too many frames");
        }
        require(ended&&p==b.length&&!delays.isEmpty(),"Incomplete GIF");return new Info(w,h,delays);
    }
}
