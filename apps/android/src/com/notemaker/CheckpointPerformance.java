package com.notemaker;

import android.os.Debug;
import android.os.Handler;
import android.os.Looper;
import android.os.Process;
import android.os.SystemClock;
import java.io.File;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Paths;
import java.util.*;
import static com.notemaker.core.Json.*;

/** Test-only physical-device cost of a selected-proxy journal checkpoint. */
final class CheckpointPerformance {
    private static Map<String,Long> io() throws Exception {
        Map<String,Long> out=new LinkedHashMap<>();
        for(String line:Files.readAllLines(Paths.get("/proc/self/io"),StandardCharsets.UTF_8)){
            String[] parts=line.split(":\\s*",2);if(parts.length==2)out.put(parts[0],Long.parseLong(parts[1]));
        }
        return out;
    }
    private static Map<String,Object> measure(MainActivity app) throws Exception {
        Map<String,Long> before=io();long cpu=Process.getElapsedCpuTime(),pss=Debug.getPss();
        long wall=SystemClock.elapsedRealtimeNanos();
        Object result=app.replica.command(Arrays.asList("checkpoint"),null);
        double wallMs=(SystemClock.elapsedRealtimeNanos()-wall)/1e6;
        long cpuMs=Process.getElapsedCpuTime()-cpu,afterPss=Debug.getPss();
        Map<String,Long> after=io();Map<String,Object> delta=object();
        for(String key:after.keySet())if(before.containsKey(key))delta.put(key,after.get(key)-before.get(key));
        return object("result",result,"wallMs",wallMs,"processCpuMs",cpuMs,
                "pssBeforeKiB",pss,"pssAfterKiB",afterPss,"ioDelta",delta);
    }
    static Map<String,Object> run(MainActivity app) throws Exception {
        File wal=new File(app.replica.local.file+".journal");
        long original=wal.length();if(original<1000000)throw new IllegalStateException("Checkpoint fixture needs a large WAL");
        Map<String,Object> first=measure(app),noop=measure(app);
        if(wal.length()!=0)throw new IllegalStateException("Checkpoint did not clear the WAL");
        return object("originalJournalBytes",original,"checkpoint",first,"noop",noop);
    }
}
