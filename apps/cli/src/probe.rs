//! Shared support for the ports of the `tests/*.java` probes
//! (`src/bin/{latency_probe,cache_footprint_probe,coverage_parity}.rs`).

use notemaker_core::{json, object, Result, Value};
use std::io::Write;

/// `java.util.Random` (48-bit LCG), so seeded probes produce the same
/// sequences and digests as Java.
pub struct JavaRandom {
    seed: i64,
}

const MULTIPLIER: i64 = 0x5DEECE66D;
const MASK: i64 = (1 << 48) - 1;

impl JavaRandom {
    /// `new Random(seed)`.
    pub fn new(seed: i64) -> JavaRandom {
        JavaRandom {
            seed: (seed ^ MULTIPLIER) & MASK,
        }
    }
    /// `next(bits)`.
    pub fn next(&mut self, bits: u32) -> i32 {
        self.seed = (self.seed.wrapping_mul(MULTIPLIER).wrapping_add(0xB)) & MASK;
        ((self.seed as u64) >> (48 - bits)) as i64 as i32
    }
    /// `nextDouble()`.
    pub fn next_double(&mut self) -> f64 {
        let high = (self.next(26) as i64) << 27;
        let low = self.next(27) as i64;
        (high + low) as f64 * (1.0 / (1u64 << 53) as f64)
    }
}

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
pub fn memory(phase: &str) -> Result<()> {
    std::thread::sleep(std::time::Duration::from_millis(100));
    let mut out = object! {"phase" => phase, "heap_bytes" => allocated_bytes()};
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
pub fn ms_since(start: std::time::Instant) -> f64 {
    start.elapsed().as_nanos() as f64 / 1e6
}

/// `LatencyProbe.timed(phase, work)`: prints `{"phase", "ms"}` and flushes.
pub fn timed<T>(phase: &str, work: impl FnOnce() -> Result<T>) -> Result<T> {
    let start = std::time::Instant::now();
    let result = work()?;
    println(Value::Object(
        object! {"phase" => phase, "ms" => json::double(ms_since(start))},
    ));
    Ok(result)
}

/// `System.out.println(write(value))` for the probe binaries.
pub fn print(value: Value) {
    println(value)
}
