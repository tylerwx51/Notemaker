package com.notemaker.core;

/** Optional per-thread phase observer for local latency diagnostics. */
public final class Timing {
    private Timing(){}
    public static final ThreadLocal<java.util.function.Consumer<String>> observer=new ThreadLocal<>();
    public static void mark(String phase){java.util.function.Consumer<String> sink=observer.get();if(sink!=null)sink.accept(phase);}
}
