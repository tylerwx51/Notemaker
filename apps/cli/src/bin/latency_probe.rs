//! Port of `tests/LatencyProbe.java`: run only against a disposable replica
//! copy; emits phase timings as JSON lines. Args: DIR DOCUMENT

use notemaker_cli::item_coverage::{self, FACTORY};
use notemaker_cli::local_proxy::LocalProxy;
use notemaker_cli::png::{Png, RenderOutputPath};
use notemaker_cli::probe::{self, memory, ms_since, timed, ProbePhase, ProbeStart};
use notemaker_cli::raster::PixelSize;
use notemaker_core::coverage::{Coverage, CoverageFactory};
use notemaker_core::drawing_actions;
use notemaker_core::geometry::{DocPoint, Polygon, Radius, Segment};
use notemaker_core::ids::{DocumentName, LayerId, PageNumber};
use notemaker_core::layer_editor::LayerEditor;
use notemaker_core::paths::ReplicaDir;
use notemaker_core::store::{self, Store};
use notemaker_core::{json, object, Error, Map, Result, Value};
use std::cell::Cell;
use std::path::PathBuf;
use std::time::Instant;

/// `measured` factory: `ItemCoverage.create` with accumulated cost.
struct Measured {
    nanos: Cell<u128>,
    calls: Cell<i64>,
}

impl CoverageFactory for Measured {
    fn create(&self, o: &notemaker_core::wire::EntityContent) -> Result<Box<dyn Coverage>> {
        let start = Instant::now();
        let result = FACTORY.create(o);
        self.nanos
            .set(self.nanos.get() + start.elapsed().as_nanos());
        self.calls.set(self.calls.get() + 1);
        result
    }
}

fn args_value(args: &[&str]) -> Value {
    Value::Array(args.iter().map(|a| Value::from(*a)).collect())
}

fn command(doc: &str, args: &[&str], input: Option<&Map>) -> Map {
    let mut body =
        object! {"document" => doc, "actor" => "latency-probe", "args" => args_value(args)};
    if let Some(input) = input {
        body.insert("input".into(), Value::Object(input.clone()));
    }
    body
}

fn point_list(item: &Map) -> Result<Vec<Value>> {
    Ok(json::list(item.get("points").unwrap_or(&Value::Null))?.clone())
}

fn run(args: &[String]) -> Result<()> {
    let dir = PathBuf::from(&args[0]);
    if !dir.join("LATENCY-SCRATCH").is_file() {
        return Err(Error::illegal_argument(
            "Requires marked disposable replica copy",
        ));
    }
    let doc = args[1].clone();
    let store = Store::writer(&ReplicaDir::from(dir.as_path()), &DocumentName::from(&doc))?;
    let state = timed(&ProbePhase::from("cold_read"), || store.read())?;
    let model = store::model(&state)?;
    let layer = match state.get("selected") {
        None => "base".to_string(),
        Some(v) => v
            .as_str()
            .ok_or_else(|| Error::class_cast("selected is not a String"))?
            .to_string(),
    };
    let page = json::as_int(Some(json::get_or_default(&state, "page", &Value::from(0))))?;
    let editor = LayerEditor::new(&model, &LayerId::from(&layer), PageNumber(page))?;
    let mut points = 0usize;
    for item in editor.items() {
        if item.kind == "stroke" {
            points += point_list(&item.content)?.len();
        }
    }
    probe::print(Value::Object(
        object! {"items" => editor.items().count(), "points" => points, "layer" => layer.clone(), "page" => page},
    ));
    memory(&ProbePhase::from("memory_before_coverage"))?;
    for _ in 0..3 {
        timed(&ProbePhase::from("warm_read"), || store.read())?;
    }
    // Use a real ink point, retaining the note's item sizes and brush geometry.
    let (mut x, mut y) = (200.0, 200.0);
    for item in editor.items() {
        if item.kind == "stroke" {
            let p = point_list(&item.content)?;
            let first = json::list(&p[0])?;
            x = json::as_double(first.first())?;
            y = json::as_double(first.get(1))?;
            break;
        }
    }
    let region = Polygon::capsule(Segment { start: DocPoint { x, y }, end: DocPoint { x, y } }, Radius(12.0));
    let input = object! {"mode" => "partial", "layer" => layer.clone(), "page" => page, "radius" => 12, "points" => vec![Value::Array(vec![json::double(x), json::double(y)])]};
    let mut swipe = Vec::new();
    for i in 0..120 {
        swipe.push(Value::Array(vec![
            json::double(x + 600.0 * i as f64 / 119.0),
            json::double(y + 400.0 * i as f64 / 119.0),
        ]));
    }
    let swipe_input = object! {"mode" => "partial", "layer" => layer.clone(), "page" => page, "radius" => 12, "points" => swipe};
    let mut costs: Vec<Map> = Vec::new();
    let measured = Measured {
        nanos: Cell::new(0),
        calls: Cell::new(0),
    };
    timed(&ProbePhase::from("all_item_hit_tests"), || {
        for item in editor.items() {
            let start = ProbeStart::now();
            let hit = item.intersects(&region, &measured)?;
            let n = if item.kind == "stroke" {
                point_list(&item.content)?.len()
            } else {
                0
            };
            costs.push(object! {"id" => item.id.clone(), "kind" => item.kind.clone(), "points" => n, "ms" => json::double(ms_since(start).0), "hit" => hit});
        }
        Ok(())
    })?;
    probe::print(Value::Object(
        object! {"coverage_factory_ms" => json::double(measured.nanos.get() as f64 / 1e6), "coverage_factory_calls" => measured.calls.get()},
    ));
    memory(&ProbePhase::from("memory_after_coverage"))?;
    costs.sort_by(|a, b| {
        json::number(b, "ms")
            .unwrap_or(0.0)
            .total_cmp(&json::number(a, "ms").unwrap_or(0.0))
    });
    let slowest: Vec<Value> = costs.iter().take(10).cloned().map(Value::Object).collect();
    probe::print(Value::Object(object! {"slowest" => slowest}));
    for _ in 0..3 {
        timed(&ProbePhase::from("erase_geometry"), || {
            drawing_actions::erase(&model, (&input).into(), &FACTORY)
        })?;
    }
    // Diagnostic only: conservative ink-point bounds before exact coverage.
    // Production still uses the original query; this isolates avoidable work.
    item_coverage::cache_clear();
    timed(&ProbePhase::from("experimental_ink_prefilter_cold"), || {
        let (mut candidates, mut hits) = (0, 0);
        let q = region.bounds();
        for item in LayerEditor::new(&model, &LayerId::from(&layer), PageNumber(page))?.items() {
            if item.kind == "stroke" {
                let (mut l, mut t, mut r, mut b) = (
                    f64::INFINITY,
                    f64::INFINITY,
                    f64::NEG_INFINITY,
                    f64::NEG_INFINITY,
                );
                for raw in point_list(&item.content)? {
                    let p = json::list(&raw)?;
                    let (px, py) = (json::as_double(p.first())?, json::as_double(p.get(1))?);
                    l = l.min(px);
                    t = t.min(py);
                    r = r.max(px);
                    b = b.max(py);
                }
                let pad = json::number(&item.content, "width")? * 2.0;
                if r + pad < q.left || l - pad > q.right || b + pad < q.top || t - pad > q.bottom {
                    continue;
                }
            }
            candidates += 1;
            if item.intersects(&region, &FACTORY)? {
                hits += 1;
            }
        }
        let expected = costs
            .iter()
            .filter(|c| c.get("hit") == Some(&Value::Bool(true)))
            .count();
        if hits != expected {
            return Err(Error::illegal_state("Prefilter changed hit count"));
        }
        probe::print(Value::Object(
            object! {"prefilter_candidates" => candidates, "prefilter_hits" => hits},
        ));
        Ok(())
    })?;
    timed(&ProbePhase::from("render_png"), || {
        Png::export(
            &model,
            PageNumber(page),
            DocPoint { x: 0.0, y: 0.0 },
            PixelSize { width: 1000, height: 1400 },
            &RenderOutputPath::new(dir.join("render.png")),
        )
    })?;
    let proxy = LocalProxy::new(&ReplicaDir::from(dir.as_path()), notemaker_core::net::Port::EPHEMERAL)?;
    let outcome = (|| -> Result<()> {
        for _ in 0..3 {
            timed(&ProbePhase::from("proxy_erase"), || {
                proxy.execute((&command(&doc, &["erase", "-"], Some(&input))).into())
            })?;
            let snap = timed(&ProbePhase::from("proxy_snapshot"), || {
                proxy.execute((&command(&doc, &["snapshot"], None)).into())
            })?;
            timed(&ProbePhase::from("snapshot_to_model"), || {
                store::model_of_map((json::map(&snap)?).into())
            })?;
            timed(&ProbePhase::from("proxy_undo"), || {
                proxy.execute((&command(&doc, &["undo"], None)).into())
            })?;
        }
        timed(&ProbePhase::from("swipe_geometry"), || {
            drawing_actions::erase(&model, (&swipe_input).into(), &FACTORY)
        })?;
        timed(&ProbePhase::from("proxy_swipe"), || {
            proxy.execute((&command(&doc, &["erase", "-"], Some(&swipe_input))).into())
        })?;
        timed(&ProbePhase::from("post_swipe_snapshot"), || {
            proxy.execute((&command(&doc, &["snapshot"], None)).into())
        })?;
        match timed(&ProbePhase::from("post_swipe_tap_geometry"), || {
            drawing_actions::erase(&store.model()?, (&input).into(), &FACTORY)
        }) {
            Ok(_) => {}
            Err(expected) if expected.kind.is_illegal_argument() => probe::print(Value::Object(
                object! {"post_swipe_tap" => expected.message_or_null()},
            )),
            Err(e) => return Err(e),
        }
        proxy.execute((&command(&doc, &["undo"], None)).into())?;
        let (sender, receiver) = std::sync::mpsc::channel::<(usize, Instant)>();
        let worker_proxy = proxy.clone();
        let worker_doc = doc.clone();
        let worker_input = input.clone();
        let writer = std::thread::Builder::new()
            .name("pool-1-thread-1".into())
            .spawn(move || {
                for (index, submitted) in receiver {
                    let start = Instant::now();
                    let mut error = Value::Null;
                    if let Err(e) = worker_proxy.execute((&command(&worker_doc, &["erase", "-"], Some(&worker_input))).into()).and_then(|_| worker_proxy.execute((&command(&worker_doc, &["snapshot"], None)).into())) {
                        error = Value::from(e.message_or_null());
                    }
                    probe::print(Value::Object(object! {
                        "phase" => "queued_erase", "index" => index, "queue_ms" => json::double((start - submitted).as_nanos() as f64 / 1e6),
                        "work_ms" => json::double(ms_since(ProbeStart::from(start)).0), "total_ms" => json::double(ms_since(ProbeStart::from(submitted)).0), "error" => error
                    }));
                }
            })
            .map_err(|e| Error::io(e.to_string()))?;
        for i in 0..5 {
            let _ = sender.send((i, Instant::now()));
        }
        drop(sender);
        let _ = writer.join();
        proxy.execute((&command(&doc, &["undo"], None)).into())?;
        let mut sample = 0;
        for item in editor.items() {
            if item.kind != "stroke" {
                continue;
            }
            let pts = point_list(&item.content)?;
            let point = json::list(&pts[pts.len() / 2])?.clone();
            let request = object! {"mode" => "partial", "layer" => layer.clone(), "page" => page, "radius" => 12, "points" => vec![Value::Array(vec![point[0].clone(), point[1].clone()])]};
            let start = ProbeStart::now();
            let mut changed = true;
            if let Err(e) = proxy.execute((&command(&doc, &["erase", "-"], Some(&request))).into()) {
                if !(e.kind.is_illegal_argument()
                    && e.message_or_null() == "No matching editable content")
                {
                    return Err(e);
                }
                changed = false;
            }
            let command_ms = ms_since(start);
            proxy.execute((&command(&doc, &["snapshot"], None)).into())?;
            probe::print(Value::Object(
                object! {"phase" => "varied_erase", "sample" => sample, "points" => pts.len(), "command_ms" => json::double(command_ms.0), "through_snapshot_ms" => json::double(ms_since(start).0), "changed" => changed},
            ));
            sample += 1;
            if changed {
                proxy.execute((&command(&doc, &["undo"], None)).into())?;
            }
        }
        Ok(())
    })();
    let closed = proxy.close();
    outcome?;
    closed
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(e) = run(&args) {
        eprintln!("Exception in thread \"main\" {}", e);
        std::process::exit(1);
    }
    std::process::exit(0);
}
