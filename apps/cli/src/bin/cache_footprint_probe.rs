//! Port of `tests/CacheFootprintProbe.java`: retained coverage cache memory;
//! no source document is mutated.

use notemaker_cli::item_coverage;
use notemaker_cli::probe::{self, ProbePhase};
use notemaker_core::{json, object, Map, Result, Value};

fn stroke(i: i64, count: i64) -> Map {
    let mut points = Vec::new();
    for p in 0..count {
        points.push(Value::Array(vec![
            json::double(p as f64 * 2.0),
            json::double((p as f64 * 0.7).sin() * 10.0),
            json::double(0.5 + (p % 5) as f64 * 0.1),
        ]));
    }
    object! {"kind" => "stroke", "stroke" => format!("memory-{}", i), "layer" => "base", "page" => 0, "width" => json::double(2.0 + i as f64 * 0.0001), "color" => -16777216, "points" => points}
}

fn run() -> Result<()> {
    item_coverage::create((&stroke(-1, 20)).into())?;
    item_coverage::cache_clear();
    probe::memory(&ProbePhase::from("cache_empty"))?;
    let start = probe::ProbeStart::now();
    let mut from = 0;
    for count in [100i64, 1000, 10000] {
        for i in from..count {
            item_coverage::create((&stroke(i, 20)).into())?;
        }
        probe::print(Value::Object(
            object! {"items" => count, "points_per_item" => 20, "cache_entries" => item_coverage::cache_len().0, "build_ms" => json::double(probe::ms_since(start).0)},
        ));
        probe::memory(&ProbePhase::from(format!("cache_{}", count)))?;
        from = count;
    }
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("Exception in thread \"main\" {}", e);
        std::process::exit(1);
    }
    std::process::exit(0);
}
