package com.notemaker.core;

import java.util.Base64;
import java.util.zip.CRC32;
import static com.notemaker.core.Model.require;

/** Portable raster header validation, without desktop-only image libraries. */
public final class ImageAsset {
    private ImageAsset() {}
    public static final int MAX_BYTES=2*1024*1024, MAX_DIMENSION=8192;
    public static String fromBytes(byte[] data){
        String mime=png(data)?"image/png":"image/jpeg";
        String uri="data:"+mime+";base64,"+Base64.getEncoder().encodeToString(data);
        validate(uri);return uri;
    }
    private static boolean png(byte[] b){return b.length>=24&&(b[0]&255)==137&&b[1]==80&&b[2]==78&&b[3]==71&&b[4]==13&&b[5]==10&&b[6]==26&&b[7]==10;}
    private static int u16(byte[] b,int p){return ((b[p]&255)<<8)|(b[p+1]&255);}
    private static long u32(byte[] b,int p){return ((long)u16(b,p)<<16)|u16(b,p+2);}
    public static double[] validate(String uri){
        require(uri!=null&&uri.length()<=MAX_BYTES*4L/3+100,"Image exceeds 2 MiB");
        boolean isPng=uri.startsWith("data:image/png;base64,"),isJpeg=uri.startsWith("data:image/jpeg;base64,");
        require(isPng||isJpeg,"Image must be an embedded PNG or JPEG");
        byte[] b;try{b=Base64.getDecoder().decode(uri.substring(uri.indexOf(',')+1));}catch(IllegalArgumentException e){throw new IllegalArgumentException("Invalid image base64");}
        require(b.length>0&&b.length<=MAX_BYTES,"Image exceeds 2 MiB");
        long w=0,h=0;
        if(isPng){
            require(png(b)&&u32(b,8)==13&&b[12]==73&&b[13]==72&&b[14]==68&&b[15]==82,"Invalid PNG header");w=u32(b,16);h=u32(b,20);
            boolean pixels=false,end=false;int offset=8;
            while(offset+12<=b.length){long size=u32(b,offset);require(size<=b.length-offset-12,"Truncated PNG chunk");int length=(int)size;CRC32 crc=new CRC32();crc.update(b,offset+4,length+4);require(crc.getValue()==u32(b,offset+8+length),"Invalid PNG checksum");
                String type=new String(b,offset+4,4,java.nio.charset.StandardCharsets.US_ASCII);pixels|=type.equals("IDAT");offset+=length+12;if(type.equals("IEND")){require(length==0,"Invalid PNG end");end=true;break;}}
            require(pixels&&end&&offset==b.length,"Incomplete PNG image");
        }
        else{
            require(b.length>=4&&(b[0]&255)==255&&(b[1]&255)==216,"Invalid JPEG header");
            require((b[b.length-2]&255)==255&&(b[b.length-1]&255)==217,"Truncated JPEG image");
            for(int p=2;p+3<b.length;){
                require((b[p]&255)==255,"Invalid JPEG marker");while(p<b.length&&(b[p]&255)==255)p++;
                require(p<b.length,"Truncated JPEG");int marker=b[p++]&255;
                if(marker==217||marker==218)break;if(marker==1||marker>=208&&marker<=215)continue;
                require(p+1<b.length,"Truncated JPEG");int length=u16(b,p);require(length>=2&&p+length<=b.length,"Truncated JPEG segment");
                if(marker>=192&&marker<=207&&marker!=196&&marker!=200&&marker!=204){require(length>=8,"Invalid JPEG frame");h=u16(b,p+3);w=u16(b,p+5);break;}p+=length;
            }
        }
        require(w>0&&h>0&&w<=MAX_DIMENSION&&h<=MAX_DIMENSION,"Invalid image dimensions (maximum 8192)");
        require(w*h<=32L*1024*1024,"Image exceeds 32 megapixels");return new double[]{w,h};
    }
}
