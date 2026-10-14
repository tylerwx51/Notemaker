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
        if (value==null) return "null";
        if (value instanceof String) {
            StringBuilder s=new StringBuilder("\"");
            for(char c:((String)value).toCharArray()) {
                if(c=='"'||c=='\\') s.append('\\').append(c);
                else if(c<32) s.append(String.format(Locale.ROOT,"\\u%04x",(int)c));
                else s.append(c);
            }
            return s.append('"').toString();
        }
        if(value instanceof Boolean) return value.toString();
        if(value instanceof Number) {
            if(!Double.isFinite(((Number)value).doubleValue())) throw new IllegalArgumentException("Nonfinite JSON number");
            return value.toString();
        }
        if(value instanceof Map) {
            StringJoiner s=new StringJoiner(",","{","}");
            for(Map.Entry<String,Object> e:map(value).entrySet()) s.add(write(e.getKey())+":"+write(e.getValue()));
            return s.toString();
        }
        if(value instanceof List) {
            StringJoiner s=new StringJoiner(",","[","]");
            for(Object v:list(value)) s.add(write(v));
            return s.toString();
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
        void ws(){while(i<s.length()&&" \t\r\n".indexOf(s.charAt(i))>=0)i++;}
        boolean take(char c){ws();if(i<s.length()&&s.charAt(i)==c){i++;return true;}return false;}
        Object value(int depth){
            if(depth>64)throw error(); ws();if(i==s.length())throw error();char c=s.charAt(i);
            if(c=='"')return str();
            if(take('{')){Map<String,Object> m=object();if(take('}'))return m;do{ws();String k=str();if(!take(':')||m.containsKey(k))throw error();m.put(k,value(depth+1));}while(take(','));if(!take('}'))throw error();return m;}
            if(take('[')){List<Object>a=new ArrayList<>();if(take(']'))return a;do{a.add(value(depth+1));}while(take(','));if(!take(']'))throw error();return a;}
            for(String token:new String[]{"true","false","null"})if(s.startsWith(token,i)){i+=token.length();return token.equals("null")?null:Boolean.valueOf(token);}
            int start=i;while(i<s.length()&&"0123456789.eE+-".indexOf(s.charAt(i))>=0)i++;
            String n=s.substring(start,i);if(!validNumber(n))throw error();
            try{if(!n.contains(".")&&!n.contains("e")&&!n.contains("E"))return Long.valueOf(n);double d=Double.parseDouble(n);if(!Double.isFinite(d))throw error();return d;}catch(NumberFormatException e){throw error();}
        }
        boolean validNumber(String n){
            int p=0,len=n.length();if(p<len&&n.charAt(p)=='-')p++;
            if(p==len)return false;
            if(n.charAt(p)=='0')p++;else{int first=p;while(p<len&&n.charAt(p)>='0'&&n.charAt(p)<='9')p++;if(p==first)return false;}
            if(p<len&&n.charAt(p)=='.'){int first=++p;while(p<len&&n.charAt(p)>='0'&&n.charAt(p)<='9')p++;if(p==first)return false;}
            if(p<len&&(n.charAt(p)=='e'||n.charAt(p)=='E')){p++;if(p<len&&(n.charAt(p)=='+'||n.charAt(p)=='-'))p++;int first=p;while(p<len&&n.charAt(p)>='0'&&n.charAt(p)<='9')p++;if(p==first)return false;}
            return p==len;
        }
        String str(){
            if(i>=s.length()||s.charAt(i++)!='"')throw error();StringBuilder b=new StringBuilder();
            while(i<s.length()){char c=s.charAt(i++);if(c=='"')return b.toString();if(c<32)throw error();
                if(c=='\\'){if(i==s.length())throw error();c=s.charAt(i++);switch(c){
                    case '"':case '\\':case '/':break;case 'b':c='\b';break;case 'f':c='\f';break;case 'n':c='\n';break;case 'r':c='\r';break;case 't':c='\t';break;
                    case 'u':if(i+4>s.length())throw error();try{c=(char)Integer.parseInt(s.substring(i,i+4),16);}catch(NumberFormatException e){throw error();}i+=4;break;default:throw error();}}
                b.append(c);
            }throw error();
        }
    }
}
