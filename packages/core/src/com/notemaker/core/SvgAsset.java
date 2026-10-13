package com.notemaker.core;

import java.io.*;
import java.nio.charset.StandardCharsets;
import java.util.*;
import javax.xml.parsers.*;
import javax.xml.transform.*;
import javax.xml.transform.dom.DOMSource;
import javax.xml.transform.stream.StreamResult;
import org.w3c.dom.*;
import org.xml.sax.*;
import static com.notemaker.core.Model.require;

/** Portable, self-contained static SVG profile shared by both CLI runtimes. */
public final class SvgAsset {
    private SvgAsset() {}
    public static final int MAX_BYTES=262144;
    private static final Set<String> TAGS=words("svg g defs title desc path rect circle ellipse line polyline polygon text tspan linearGradient radialGradient stop clipPath marker style");
    private static final Set<String> ATTRS=words("id class style version x y x1 y1 x2 y2 dx dy width height viewBox preserveAspectRatio transform d points rx ry r cx cy fx fy fr fill fill-opacity fill-rule stroke stroke-width stroke-opacity stroke-linecap stroke-linejoin stroke-miterlimit stroke-dasharray stroke-dashoffset opacity color font-family font-size font-style font-weight text-anchor dominant-baseline alignment-baseline baseline-shift textLength lengthAdjust display visibility clip-path clip-rule gradientUnits gradientTransform spreadMethod offset stop-color stop-opacity href marker-start marker-mid marker-end markerWidth markerHeight markerUnits refX refY orient type space");
    private static Set<String> words(String s){return new HashSet<>(Arrays.asList(s.split(" ")));}
    private static final class Asset {double[] size;String normalized;Asset(double[] size,String normalized){this.size=size;this.normalized=normalized;}}
    private static final Map<String,Asset> CACHE=new LinkedHashMap<String,Asset>(16,.75f,true){protected boolean removeEldestEntry(Map.Entry<String,Asset> e){return size()>16;}};

    /** Returns intrinsic viewBox dimensions. No external resources or XML entities. */
    public static synchronized double[] validate(String source){
        Asset cached=CACHE.get(source);if(cached!=null)return cached.size.clone();
        require(source.getBytes(StandardCharsets.UTF_8).length<=MAX_BYTES,"SVG exceeds 256 KiB");
        require(!source.toUpperCase(Locale.ROOT).contains("<!DOCTYPE")&&!source.toUpperCase(Locale.ROOT).contains("<!ENTITY"),"SVG DTDs and entities are unsupported");
        try{
            DocumentBuilderFactory f=DocumentBuilderFactory.newInstance();f.setNamespaceAware(true);f.setExpandEntityReferences(false);
            DocumentBuilder b=f.newDocumentBuilder();
            b.setEntityResolver((publicId,systemId)->{throw new SAXException("External SVG resource");});
            b.setErrorHandler(new org.xml.sax.helpers.DefaultHandler(){public void error(SAXParseException e)throws SAXException{throw e;}public void fatalError(SAXParseException e)throws SAXException{throw e;}});
            Document document=b.parse(new InputSource(new StringReader(source)));
            Element root=document.getDocumentElement();require("svg".equals(root.getLocalName()),"Expected SVG root");
            visit(document,0,new int[]{0});
            double[] size;
            if(root.hasAttribute("viewBox")){
                String[] values=root.getAttribute("viewBox").trim().split("[\\s,]+");require(values.length==4,"SVG viewBox needs four numbers");
                double[] box=new double[4];for(int i=0;i<4;i++){box[i]=Double.parseDouble(values[i]);require(Double.isFinite(box[i])&&Math.abs(box[i])<=1000000,"Invalid SVG viewBox");}
                size=new double[]{box[2],box[3]};
            }else size=new double[]{length(root.getAttribute("width")),length(root.getAttribute("height"))};
            require(size[0]>0&&size[1]>0&&size[0]<=1000000&&size[1]<=1000000,"SVG needs positive viewBox or pixel width/height");
            if(!root.hasAttribute("viewBox"))root.setAttribute("viewBox","0 0 "+size[0]+" "+size[1]);
            root.setAttribute("width","100%");root.setAttribute("height","100%");
            Transformer transformer=TransformerFactory.newInstance().newTransformer();transformer.setOutputProperty(OutputKeys.OMIT_XML_DECLARATION,"yes");
            StringWriter normalized=new StringWriter();transformer.transform(new DOMSource(root),new StreamResult(normalized));
            CACHE.put(source,new Asset(size,normalized.toString()));return size.clone();
        }catch(Exception e){throw new IllegalArgumentException("Invalid SVG: "+e.getMessage(),e);}
    }
    private static double length(String s){return Double.parseDouble(s.trim().replaceFirst("px$",""));}
    private static void visit(Node node,int depth,int[] count){
        require(depth<=64&&++count[0]<=10000,"SVG is too complex");
        require(node.getNodeType()!=Node.PROCESSING_INSTRUCTION_NODE&&node.getNodeType()!=Node.ENTITY_REFERENCE_NODE,"SVG processing instructions/entities are unsupported");
        if(node instanceof Element){
            Element e=(Element)node;String ns=e.getNamespaceURI(),tag=e.getLocalName();
            require("http://www.w3.org/2000/svg".equals(ns)&&TAGS.contains(tag),"Unsupported SVG element: "+tag);
            NamedNodeMap attrs=e.getAttributes();
            for(int i=0;i<attrs.getLength();i++){
                Node a=attrs.item(i);String name=a.getLocalName(),ans=a.getNamespaceURI(),value=a.getNodeValue();
                if("http://www.w3.org/2000/xmlns/".equals(ans))continue;
                require(ans==null||"http://www.w3.org/1999/xlink".equals(ans)&&name.equals("href")||"http://www.w3.org/XML/1998/namespace".equals(ans)&&name.equals("space"),"Unsupported SVG attribute namespace");
                require(ATTRS.contains(name),"Unsupported SVG attribute: "+name);
                if(name.equals("href"))require(value.matches("#[A-Za-z_][A-Za-z0-9_.:-]*"),"SVG references must be local fragments");
                if(name.equals("style")||name.equals("fill")||name.equals("stroke")||name.equals("clip-path")||name.startsWith("marker-"))css(value);
            }
            if(tag.equals("style"))css(e.getTextContent());
        }
        NodeList children=node.getChildNodes();for(int i=0;i<children.getLength();i++)visit(children.item(i),depth+1,count);
    }
    private static void css(String value){
        String s=value.replaceAll("(?s)/\\*.*?\\*/","").toLowerCase(Locale.ROOT);
        require(!s.contains("\\")&&!s.contains("@")&&!s.contains("/*"),"SVG CSS escapes/imports are unsupported");
        s=s.replaceAll("url\\(\\s*['\"]?#[a-z_][a-z0-9_.:-]*['\"]?\\s*\\)","");
        require(!s.contains("url")&&!s.contains("expression("),"SVG resources must be local fragments");
    }
    /** A viewport wrapper gives all renderers the same meet/center placement. */
    public static synchronized String viewport(String source){
        validate(source);return CACHE.get(source).normalized;
    }
}
