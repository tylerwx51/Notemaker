//! Port of `CheckpointPerformance.java`: test-only physical-device cost of a
//! selected-proxy journal checkpoint.

use crate::error::{AppError, AppResult};
use crate::main_activity::MainActivity;
use indexmap::IndexMap;
use notemaker_core::json::{self, Map, Value};
use notemaker_core::object;
use std::sync::Arc;

/// `Long.parseLong(s)`: `NumberFormatException` with the Java message.
fn parse_long(s: &str) -> AppResult<i64> {
    let digits = s.strip_prefix(['+', '-']).unwrap_or(s);
    if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
        if let Ok(v) = s.parse::<i64>() {
            return Ok(v);
        }
    }
    Err(AppError::java("java.lang.NumberFormatException", format!("For input string: \"{s}\"")))
}

/// The `/proc/self/io` lines split like `line.split(":\\s*",2)`.
fn parse_io(text: &str) -> AppResult<IndexMap<String, i64>> {
    let mut out = IndexMap::new();
    // Files.readAllLines: \n, \r or \r\n terminators.
    for line in text.split_terminator('\n').flat_map(|l| l.strip_suffix('\r').unwrap_or(l).split('\r')) {
        if let Some((key, rest)) = line.split_once(':') {
            let value = rest.trim_start_matches([' ', '\t', '\n', '\u{b}', '\u{c}', '\r']);
            out.insert(key.to_string(), parse_long(value)?);
        }
    }
    Ok(out)
}

/// `private static Map<String,Long> io() throws Exception` (`/proc/self/io`).
pub fn io() -> AppResult<IndexMap<String, i64>> {
    let text = std::fs::read_to_string("/proc/self/io")?;
    parse_io(&text)
}
/// `private static Map<String,Object> measure(MainActivity app) throws Exception`.
pub fn measure(app: &Arc<MainActivity>) -> AppResult<Map> {
    let before = io()?;
    let cpu = jstatic!("android/os/Process", "getElapsedCpuTime", "()J")?.long();
    let pss = jstatic!("android/os/Debug", "getPss", "()J")?.long();
    let wall = jstatic!("android/os/SystemClock", "elapsedRealtimeNanos", "()J")?.long();
    let result = app.replica().command(&["checkpoint"], None)?;
    let wall_ms = (jstatic!("android/os/SystemClock", "elapsedRealtimeNanos", "()J")?.long() - wall) as f64 / 1e6;
    let cpu_ms = jstatic!("android/os/Process", "getElapsedCpuTime", "()J")?.long() - cpu;
    let after_pss = jstatic!("android/os/Debug", "getPss", "()J")?.long();
    let after = io()?;
    let mut delta = Map::new();
    for (key, value) in &after {
        if let Some(b) = before.get(key) {
            delta.insert(key.clone(), Value::from(value - b));
        }
    }
    Ok(object! {
        "result" => result,
        "wallMs" => json::double(wall_ms),
        "processCpuMs" => cpu_ms,
        "pssBeforeKiB" => pss,
        "pssAfterKiB" => after_pss,
        "ioDelta" => delta,
    })
}
/// `static Map<String,Object> run(MainActivity app) throws Exception`.
pub fn run(app: &Arc<MainActivity>) -> AppResult<Map> {
    let wal = std::path::PathBuf::from(format!("{}.journal", app.replica().local.file.display()));
    // File.length(): 0 when the file does not exist.
    let length = || std::fs::metadata(&wal).map(|m| m.len() as i64).unwrap_or(0);
    let original = length();
    if original < 1000000 {
        return Err(AppError::illegal_state("Checkpoint fixture needs a large WAL"));
    }
    let first = measure(app)?;
    let noop = measure(app)?;
    if length() != 0 {
        return Err(AppError::illegal_state("Checkpoint did not clear the WAL"));
    }
    Ok(object! {"originalJournalBytes" => original, "checkpoint" => first, "noop" => noop})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proc_io_lines() {
        let io = parse_io("rchar: 10\nwchar:\t20\ncancelled_write_bytes: 0\n").unwrap();
        assert_eq!(io.iter().map(|(k, v)| (k.as_str(), *v)).collect::<Vec<_>>(), vec![("rchar", 10), ("wchar", 20), ("cancelled_write_bytes", 0)]);
        assert!(parse_io("rchar: x\n").is_err());
        assert!(parse_io("no separator\n").unwrap().is_empty());
    }
}
