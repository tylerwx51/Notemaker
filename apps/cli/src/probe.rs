//! Shared support for the ports of the `tests/*.java` probes
//! (`src/bin/{latency_probe,cache_footprint_probe,coverage_parity}.rs`).

use notemaker_core::{json, object, Result, Value};
use std::io::Write;

notemaker_core::string_newtype! {
    /// The name of a measured probe phase, such as `"cold_read"`.
    pub struct ProbePhase;
}

/// Wall time in milliseconds, as Java's `(System.nanoTime()-start)/1e6`.
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
pub struct ElapsedMillis(pub f64);

/// Allocator bytes in use (`mallinfo().uordblks`).
fn allocated_bytes() -> i64 {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    {
        // SAFETY: mallinfo2 only reads allocator statistics.
        unsafe { libc::mallinfo2().uordblks as i64 }
    }
    #[cfg(target_os = "android")]
    {
        // SAFETY: mallinfo only reads allocator statistics.
        unsafe { libc::mallinfo().uordblks as i64 }
    }
    #[cfg(not(any(all(target_os = "linux", target_env = "gnu"), target_os = "android")))]
    {
        0
    }
}

fn println(value: Value) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{}", json::write(&value));
    let _ = out.flush();
}

/// `LatencyProbe.memory(phase)`: prints `{"phase", "heap_bytes", ["native_bytes",] "rss_kib"}`.
/// There is no GC heap: `heap_bytes` is the allocator's in-use bytes
/// (`mallinfo`), and `native_bytes` (Android only, as
/// `Debug.getNativeHeapAllocatedSize`) is the same figure.
pub fn memory(phase: &ProbePhase) -> Result<()> {
    std::thread::sleep(std::time::Duration::from_millis(100));
    let mut out = object! {"phase" => phase.clone(), "heap_bytes" => allocated_bytes()};
    if cfg!(target_os = "android") {
        out.insert("native_bytes".into(), Value::from(allocated_bytes()));
    }
    if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
        for line in status.lines() {
            if let Some(rest) = line.strip_prefix("VmRSS:") {
                if let Some(kib) = rest
                    .split_whitespace()
                    .next()
                    .and_then(|v| v.parse::<i64>().ok())
                {
                    out.insert("rss_kib".into(), Value::from(kib));
                }
            }
        }
    }
    println(Value::Object(out));
    Ok(())
}

/// Milliseconds since `start` as Java's `(System.nanoTime()-start)/1e6`.
pub fn ms_since(start: std::time::Instant) -> ElapsedMillis {
    ElapsedMillis(start.elapsed().as_nanos() as f64 / 1e6)
}

/// `LatencyProbe.timed(phase, work)`: prints `{"phase", "ms"}` and flushes.
pub fn timed<T>(phase: &ProbePhase, work: impl FnOnce() -> Result<T>) -> Result<T> {
    let start = std::time::Instant::now();
    let result = work()?;
    println(Value::Object(
        object! {"phase" => phase.clone(), "ms" => json::double(ms_since(start).0)},
    ));
    Ok(result)
}

notemaker_core::json_value_newtype! {
    /// One JSON line a probe binary prints.
    pub struct ProbeRecord;
}

/// `System.out.println(write(value))` for the probe binaries.
pub fn print(value: impl Into<ProbeRecord>) {
    println(value.into().0)
}
