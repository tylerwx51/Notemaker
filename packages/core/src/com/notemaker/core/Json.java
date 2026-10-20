package com.notemaker.core;

import java.util.*;

/** Small strict JSON codec shared by the JVM and Android, without runtime dependencies. */
public final class Json {
    private Json() {}
    public static Map<String,Object> object(Object... pairs) {
        Map<String,Object> value = new LinkedHashMap<>();
        for (int i=0;i<pairs.length;i+=2) value.put((String)pairs[i], pairs[i+1]);
        return value;
    }
    @SuppressWarnings("unchecked") public static Map<String,Object> map(Object value) {
        if (!(value instanceof Map)) throw new IllegalArgumentException("Expected JSON object");
        return (Map<String,Object>)value;
    }
    @SuppressWarnings("unchecked") public static List<Object> list(Object value) {
        if (!(value instanceof List)) throw new IllegalArgumentException("Expected JSON array");
        return (List<Object>)value;
    }
    public static String string(Map<String,Object> m,String key) {
        Object v=m.get(key);
        if (!(v instanceof String)) throw new IllegalArgumentException("Expected string: "+key);
        return (String)v;
    }
    public static double number(Map<String,Object> m,String key) {
        Object v=m.get(key);
        if (!(v instanceof Number) || !Double.isFinite(((Number)v).doubleValue())) throw new IllegalArgumentException("Expected finite number: "+key);
        return ((Number)v).doubleValue();
    }
    public static Object copy(Object v) {
        if(v instanceof Map){Map<String,Object> out=object();for(Map.Entry<String,Object> e:map(v).entrySet())out.put(e.getKey(),copy(e.getValue()));return out;}
        if(v instanceof List){List<Object> out=new ArrayList<>();for(Object item:list(v))out.add(copy(item));return out;}
        return v;
    }
    /** JSON numbers have value semantics across Java and wire representations. */
    public static boolean equivalent(Object a,Object b) {
        if(a instanceof Number&&b instanceof Number)return new java.math.BigDecimal(a.toString()).compareTo(new java.math.BigDecimal(b.toString()))==0;
        if(a instanceof Map&&b instanceof Map){Map<String,Object> x=map(a),y=map(b);if(!x.keySet().equals(y.keySet()))return false;for(String k:x.keySet())if(!equivalent(x.get(k),y.get(k)))return false;return true;}
        if(a instanceof List&&b instanceof List){List<Object> x=list(a),y=list(b);if(x.size()!=y.size())return false;for(int i=0;i<x.size();i++)if(!equivalent(x.get(i),y.get(i)))return false;return true;}
        return Objects.equals(a,b);
    }
    public static String write(Object value) {
        StringBuilder out=new StringBuilder();append(out,value);return out.toString();
    }
    private static void append(StringBuilder out,Object value) {
        if (value==null){out.append("null");return;}
        if (value instanceof String) {
            out.append('"');String text=(String)value;
            for(int i=0;i<text.length();i++) {char c=text.charAt(i);
                if(c=='"'||c=='\\')out.append('\\').append(c);
                else if(c<32)out.append("\\u00").append("0123456789abcdef".charAt(c>>>4)).append("0123456789abcdef".charAt(c&15));
                else out.append(c);
            }
            out.append('"');return;
        }
        if(value instanceof Boolean){out.append(value);return;}
        if(value instanceof Number) {
            if(!Double.isFinite(((Number)value).doubleValue())) throw new IllegalArgumentException("Nonfinite JSON number");
            out.append(value);return;
        }
        if(value instanceof Map) {
            out.append('{');boolean comma=false;
            for(Map.Entry<String,Object> e:map(value).entrySet()){if(comma)out.append(',');append(out,e.getKey());out.append(':');append(out,e.getValue());comma=true;}
            out.append('}');return;
        }
        if(value instanceof List) {
            out.append('[');boolean comma=false;for(Object v:list(value)){if(comma)out.append(',');append(out,v);comma=true;}
            out.append(']');return;
        }
        throw new IllegalArgumentException("Unsupported JSON value");
    }
    public static Object parse(String text) {
        Parser p=new Parser(text); Object v=p.value(0); p.ws();
        if(p.i!=text.length()) throw p.error();
        return v;
    }
    private static final class Parser {
        final String s; int i;
        Parser(String s){this.s=s;}
        IllegalArgumentException error(){return new IllegalArgumentException("Invalid JSON at offset "+i);}
        void ws(){while(i<s.length()){char c=s.charAt(i);if(c!=' '&&c!='\t'&&c!='\r'&&c!='\n')break;i++;}}
        boolean take(char c){ws();if(i<s.length()&&s.charAt(i)==c){i++;return true;}return false;}
        Object value(int depth){
            if(depth>64)throw error(); ws();if(i==s.length())throw error();char c=s.charAt(i);
            if(c=='"')return str();
            if(take('{')){Map<String,Object> m=object();if(take('}'))return m;do{ws();String k=str();if(!take(':')||m.containsKey(k))throw error();m.put(k,value(depth+1));}while(take(','));if(!take('}'))throw error();return m;}
            if(take('[')){List<Object>a=new ArrayList<>();if(take(']'))return a;do{a.add(value(depth+1));}while(take(','));if(!take(']'))throw error();return a;}
            if(c=='t'&&s.startsWith("true",i)){i+=4;return Boolean.TRUE;}
            if(c=='f'&&s.startsWith("false",i)){i+=5;return Boolean.FALSE;}
            if(c=='n'&&s.startsWith("null",i)){i+=4;return null;}
            return number();
        }
        private boolean digit(char c){return c>='0'&&c<='9';}
        Object number(){
            int start=i,len=s.length();if(i<len&&s.charAt(i)=='-')i++;
            if(i==len)throw error();
            if(s.charAt(i)=='0')i++;
            else {int first=i;while(i<len&&digit(s.charAt(i)))i++;if(i==first)throw error();}
            boolean decimal=false;
            if(i<len&&s.charAt(i)=='.'){
                decimal=true;int first=++i;while(i<len&&digit(s.charAt(i)))i++;if(i==first)throw error();
            }
            if(i<len&&(s.charAt(i)=='e'||s.charAt(i)=='E')){
                decimal=true;i++;if(i<len&&(s.charAt(i)=='+'||s.charAt(i)=='-'))i++;
                int first=i;while(i<len&&digit(s.charAt(i)))i++;if(i==first)throw error();
            }
            try {
                String n=s.substring(start,i);
                if(!decimal)return Long.valueOf(n);
                double d=Double.parseDouble(n);if(!Double.isFinite(d))throw error();return d;
            }catch(NumberFormatException e){throw error();}
        }
        String str(){
            if(i>=s.length()||s.charAt(i++)!='"')throw error();int start=i;StringBuilder b=null;
            while(i<s.length()){char c=s.charAt(i++);
                if(c=='"')return b==null?s.substring(start,i-1):b.append(s,start,i-1).toString();
                if(c<32)throw error();
                if(c=='\\'){if(b==null)b=new StringBuilder();b.append(s,start,i-1);if(i==s.length())throw error();c=s.charAt(i++);switch(c){
                    case '"':case '\\':case '/':break;case 'b':c='\b';break;case 'f':c='\f';break;case 'n':c='\n';break;case 'r':c='\r';break;case 't':c='\t';break;
                    case 'u':if(i+4>s.length())throw error();try{c=(char)Integer.parseInt(s.substring(i,i+4),16);}catch(NumberFormatException e){throw error();}i+=4;break;default:throw error();}}
                if(b!=null){b.append(c);start=i;}
            }throw error();
        }
    }
}
