//! Host benchmark for the editor's per-stroke save path (ADR 0027).
//!
//! Mirrors `MainActivity.enqueueFinalStroke` → `persist`: one in-process
//! `append settings -` with the growing editor undo list, then the app's
//! `AppReplica.read` (`LocalProxy::snapshot_state`) twice (after the write and
//! on the change notification) and the model handoff to `refreshShared`.
//! Every 100th stroke has about 500 points, like `performance.rs`.
//!
//! With `objects`, each write is instead an in-process `object put snapshot -`
//! commit, as `MainActivity.persistObject` sends it (the actor's undo
//! history grows by one entry per write).
//!
//! Usage: cargo run --release -p notemaker_cli --example append_bench -- [COUNT] [objects]
//! Prints per-100-write mean/max milliseconds for the write and the read.

use notemaker_cli::local_proxy::LocalProxy;
use notemaker_core::ids::DocumentName;
use notemaker_core::store;
use notemaker_core::{json, object, Map, Value};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

fn stroke(number: usize) -> Map {
    let count = if number % 100 == 99 { 6 * 83 + 2 } else { 6 * 8 + 2 };
    let mut points = Vec::with_capacity(count);
    for i in 0..count {
        let x = 20.0 + (number % 40) as f64 * 24.0 + (i as f64 * 0.37).sin() * 9.0;
        let y = 20.0 + (number / 40) as f64 * 50.0 + i as f64 * 0.5;
        points.push(Value::Array(vec![json::double(x), json::double(y), json::double(0.5 + (i % 5) as f64 * 0.1)]));
    }
    object! {"kind" => "stroke", "stroke" => format!("bench-{number}"), "layer" => "base", "page" => 0, "color" => -16777216i64, "width" => 4, "brush" => "pen", "points" => Value::Array(points)}
}

fn main() -> notemaker_core::Result<()> {
    let strokes: usize = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(1000);
    let objects = std::env::args().nth(2).as_deref() == Some("objects");
    let dir = std::env::temp_dir().join(format!("notemaker-append-bench-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(notemaker_core::Error::from)?;
    let proxy = LocalProxy::new(&notemaker_core::paths::ReplicaDir::from(dir.as_path()), notemaker_core::net::Port::EPHEMERAL)?;
    let doc = "bench";
    let body = |args: &[&str], input: Value| {
        object! {"document" => doc, "actor" => "human", "args" => Value::Array(args.iter().map(|a| Value::from(*a)).collect()), "input" => input}
    };
    proxy.execute((&body(&["append", "-"], Value::Object(object! {"kind" => "document", "type" => "fixed"}))).into())?;
    let mut undo: Vec<Value> = Vec::new();
    let (mut write_sum, mut write_max, mut read_sum, mut read_max) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    // Per-phase totals from `timing::mark` (journal phases), printed at the end
    // for the last 100 strokes.
    let phases: Rc<RefCell<(Instant, Vec<(String, f64)>)>> = Rc::new(RefCell::new((Instant::now(), Vec::new())));
    let sink = phases.clone();
    notemaker_core::timing::set_observer(Some(Box::new(move |phase: &str| {
        let mut p = sink.borrow_mut();
        let now = Instant::now();
        let ms = (now - p.0).as_secs_f64() * 1e3;
        p.0 = now;
        match p.1.iter_mut().find(|(k, _)| k == phase) {
            Some(entry) => entry.1 += ms,
            None => p.1.push((phase.to_string(), ms)),
        }
    })));
    let total = Instant::now();
    println!("{:>7} {:>10} {:>10} {:>10} {:>10}", "strokes", "write_avg", "write_max", "read_avg", "read_max");
    for number in 0..strokes {
        let op = stroke(number);
        undo.push(Value::String(format!("bench-{number}")));
        let settings = object! {"hidden" => Value::Array(Vec::new()), "undo" => Value::Array(undo.clone()), "redo" => Value::Array(Vec::new()), "selected" => "base", "page" => 0, "clipboard" => Value::Object(object! {"entities" => Value::Array(Vec::new()), "copiedAt" => 0})};
        if number + 100 == strokes {
            phases.borrow_mut().1.clear();
        }
        let start = Instant::now();
        phases.borrow_mut().0 = start;
        if objects {
            let object = object! {"object" => format!("box-{number}"), "shape" => "rect", "layer" => "base", "page" => 0, "x" => (number % 40) as i64 * 24, "y" => (number / 40) as i64 * 50, "w" => 20, "h" => 40, "color" => -16777216i64, "width" => 2};
            // As `MainActivity.persistObject`: the reply's snapshot carries the model.
            let notemaker_cli::local_proxy::ExecutedCommand { snapshot, .. } = proxy.execute_with_snapshot((&body(&["object", "put", "snapshot", "-"], Value::Object(object))).into())?;
            std::hint::black_box(store::model(&snapshot.expect("snapshot"))?.objects.len());
        } else {
            proxy.execute((&body(&["append", "settings", "-"], Value::Object(object! {"operation" => op, "settings" => settings}))).into())?;
        }
        let wrote = start.elapsed().as_secs_f64() * 1e3;
        {
            let mut p = phases.borrow_mut();
            let ms = p.0.elapsed().as_secs_f64() * 1e3;
            match p.1.iter_mut().find(|(k, _)| k == "execute_return") {
                Some(entry) => entry.1 += ms,
                None => p.1.push(("execute_return".to_string(), ms)),
            }
        }
        let observer = notemaker_core::timing::set_observer(None);
        let start = Instant::now();
        for _ in 0..2 {
            let state = proxy.snapshot_state(&DocumentName::from(doc))?;
            let model = store::model(&state)?;
            std::hint::black_box(model.strokes.len());
        }
        let read = start.elapsed().as_secs_f64() * 1e3;
        notemaker_core::timing::set_observer(observer);
        write_sum += wrote;
        read_sum += read;
        write_max = write_max.max(wrote);
        read_max = read_max.max(read);
        if number % 100 == 99 {
            println!("{:>7} {:>10.3} {:>10.3} {:>10.3} {:>10.3}", number + 1, write_sum / 100.0, write_max, read_sum / 100.0, read_max);
            (write_sum, write_max, read_sum, read_max) = (0.0, 0.0, 0.0, 0.0);
        }
    }
    println!("total_s {:.3}", total.elapsed().as_secs_f64());
    {
        let state = proxy.snapshot_state(&DocumentName::from(doc))?;
        let model = store::model(&state)?;
        let start = Instant::now();
        for _ in 0..100 {
            std::hint::black_box(model.clone());
        }
        println!("model_clone_ms {:.3}", start.elapsed().as_secs_f64() * 10.0);
    }
    notemaker_core::timing::set_observer(None);
    for (phase, ms) in &phases.borrow().1 {
        println!("phase {:<28} {:>8.3} ms/stroke (time since previous mark, last 100 strokes)", phase, ms / 100.0);
    }
    let state = proxy.snapshot_state(&DocumentName::from(doc))?;
    let model = store::model(&state)?;
    assert_eq!(if objects { model.objects.len() } else { model.strokes.len() }, strokes);
    drop(model);
    drop(state);
    let _ = std::fs::remove_dir_all(&dir);
    std::process::exit(0);
}
