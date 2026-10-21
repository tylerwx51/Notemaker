package com.notemaker.cli;

import java.util.*;
import static com.notemaker.core.Json.*;
import static com.notemaker.core.Model.require;

/** In-memory UI rendezvous for a proxy that is the reachable root of a tree. */
final class UiHub {
    private static final long LEASE_MS=12000, RESULT_MS=60000;
    private final Map<String,Map<String,Object>> sessions=new LinkedHashMap<>();
    private final Map<String,Pending> pending=new LinkedHashMap<>();
    private final Map<String,Long> completed=new HashMap<>();
    private static final class Pending {
        final String session,proxy,verb,action;
        final Map<String,Object> input;
        Map<String,Object> result;
        Pending(String session,String proxy,String verb,String action,Map<String,Object> input){this.session=session;this.proxy=proxy;this.verb=verb;this.action=action;this.input=input;}
    }
    private static String id(Object value,String name){require(value instanceof String&&!((String)value).isEmpty()&&((String)value).length()<=200,"Invalid "+name);return (String)value;}
    private void expire(long now){
        for(String session:new ArrayList<>(sessions.keySet()))if(((Number)sessions.get(session).get("expires")).longValue()<=now){sessions.remove(session);for(Pending request:pending.values())if(request.session.equals(session)&&request.result==null)request.result=object("error","App session disconnected");notifyAll();}
        for(String key:new ArrayList<>(completed.keySet()))if(completed.get(key)<=now)completed.remove(key);
    }
    synchronized Map<String,Object> list(){expire(System.currentTimeMillis());List<Object> values=new ArrayList<>(sessions.values());values.sort((a,b)->String.valueOf(map(a).get("session")).compareTo(String.valueOf(map(b).get("session"))));return object("sessions",values,"scope","root");}
    synchronized Map<String,Object> heartbeat(Map<String,Object> body){
        String proxy=id(body.get("proxyId"),"proxy ID");List<Object> reported=com.notemaker.core.Json.list(body.get("sessions")),results=com.notemaker.core.Json.list(body.getOrDefault("results",Collections.emptyList()));
        require(reported.size()<=32&&results.size()<=64,"Invalid UI heartbeat");long now=System.currentTimeMillis();expire(now);Set<String> current=new HashSet<>();
        for(Object value:reported){Map<String,Object> state=map(value);String session=id(state.get("session"),"session ID");Map<String,Object> old=sessions.get(session);require(old==null||proxy.equals(old.get("proxyId")),"Session ID belongs to another proxy");current.add(session);Map<String,Object> record=map(copy(state));record.put("proxyId",proxy);record.put("lastSeen",now);record.put("expires",now+LEASE_MS);sessions.put(session,record);}
        for(String session:new ArrayList<>(sessions.keySet()))if(proxy.equals(sessions.get(session).get("proxyId"))&&!current.contains(session)){sessions.remove(session);for(Pending request:pending.values())if(request.session.equals(session)&&request.result==null)request.result=object("error","App session disconnected");notifyAll();}
        List<Object> acknowledged=new ArrayList<>(),commands=new ArrayList<>();
        for(Object value:results){Map<String,Object> result=map(value);String key=id(result.get("requestId"),"request ID");Pending request=pending.get(key);if(request!=null&&request.proxy.equals(proxy)){request.result=result.containsKey("error")?object("error",result.get("error")):object("value",result.get("value"));completed.put(key,now+RESULT_MS);acknowledged.add(key);notifyAll();}else if(completed.containsKey(key))acknowledged.add(key);}
        for(Map.Entry<String,Pending> entry:pending.entrySet()){Pending request=entry.getValue();if(request.proxy.equals(proxy)&&request.result==null)commands.add(object("requestId",entry.getKey(),"session",request.session,"verb",request.verb,"action",request.action,"input",request.input));}
        return object("commands",commands,"acknowledged",acknowledged,"serverTime",now);
    }
    synchronized Map<String,Object> command(Map<String,Object> body) throws InterruptedException {
        String session=id(body.get("session"),"session ID"),verb=id(body.get("verb"),"UI verb");require(Arrays.asList("state","actions","action").contains(verb),"Invalid UI verb");Object raw=body.getOrDefault("action","");require(raw instanceof String&&((String)raw).length()<=200,"Invalid UI action");String action=(String)raw;require(!verb.equals("action")||!action.isEmpty(),"UI action name is required");Map<String,Object> input=map(body.getOrDefault("input",object()));
        expire(System.currentTimeMillis());Map<String,Object> owner=sessions.get(session);require(owner!=null,"App session is not connected: "+session);String key=UUID.randomUUID().toString();Pending request=new Pending(session,String.valueOf(owner.get("proxyId")),verb,action,input);pending.put(key,request);long until=System.nanoTime()+32_000_000_000L;
        try{while(request.result==null){long remaining=(until-System.nanoTime())/1_000_000;if(remaining<=0)throw new IllegalArgumentException("UI command timed out; inspect session state before retrying");wait(Math.min(remaining,1000));expire(System.currentTimeMillis());}if(request.result.containsKey("error"))throw new IllegalArgumentException(String.valueOf(request.result.get("error")));return object("result",request.result.get("value"));}
        finally{pending.remove(key);completed.put(key,System.currentTimeMillis()+RESULT_MS);}
    }
}
