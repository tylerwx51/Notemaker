//! Port of `tests/CoverageParity.java`: deterministic query digest over
//! brushes and shapes. Prints `{"queries","hits","sha256"}`.

use notemaker_cli::item_coverage::FACTORY;
use notemaker_cli::probe;
use notemaker_core::draw_item::DrawItem;
use notemaker_core::geometry::{DocPoint, DocRect, Polygon, Radius, Segment};
use notemaker_core::ids::LayerId;
use notemaker_core::model::Model;
use notemaker_core::{actions, json, object, Result, Value};
use sha2::{Digest, Sha256};

/// `java.util.Random` (48-bit LCG), so seeded probes produce the same
/// sequences and digests as Java.
struct JavaRandom {
    seed: i64,
}

const MULTIPLIER: i64 = 0x5DEECE66D;
const MASK: i64 = (1 << 48) - 1;

impl JavaRandom {
    /// `new Random(seed)`.
    fn new(seed: i64) -> JavaRandom {
        JavaRandom {
            seed: (seed ^ MULTIPLIER) & MASK,
        }
    }
    /// `next(bits)`.
    fn next(&mut self, bits: u32) -> i32 {
        self.seed = (self.seed.wrapping_mul(MULTIPLIER).wrapping_add(0xB)) & MASK;
        ((self.seed as u64) >> (48 - bits)) as i64 as i32
    }
    /// `nextDouble()`.
    fn next_double(&mut self) -> f64 {
        let high = (self.next(26) as i64) << 27;
        let low = self.next(27) as i64;
        (high + low) as f64 * (1.0 / (1u64 << 53) as f64)
    }
}


fn run() -> Result<()> {
    let mut random = JavaRandom::new(72931);
    let mut hash = Sha256::new();
    let (mut queries, mut hits) = (0i64, 0i64);
    for brush in [
        "pen",
        "pencil",
        "marker",
        "brush",
        "oily",
        "fountain",
        "charcoal",
        "neoBrush",
        "latinCalligraphy",
        "asiaCalligraphy",
        "square",
    ] {
        let mut points = Vec::new();
        for i in 0..90 {
            let i = i as f64;
            points.push(Value::Array(vec![
                json::double(60.0 + 40.0 * (i * 0.3).sin()),
                json::double(60.0 + 40.0 * (i * 0.7).cos()),
                json::double(0.1 + random.next_double() * 0.9),
            ]));
        }
        let mut stroke = object! {"kind" => "stroke", "stroke" => brush, "layer" => "base", "page" => 0, "color" => -16777216, "width" => 7, "brush" => brush, "points" => points};
        for masked in [false, true] {
            stroke = stroke.clone(); // Published model snapshots are immutable.
            if masked {
                stroke.insert(
                    "erasures".into(),
                    Value::Array(vec![
                        Value::from(&DocRect { x: -45.0, y: -25.0, width: 30.0, height: 110.0 }.polygon()),
                        Value::from(&Polygon::capsule(
                            Segment { start: DocPoint { x: -40.0, y: -50.0 }, end: DocPoint { x: 30.0, y: 0.0 } },
                            Radius(9.0),
                        )),
                    ]),
                );
            }
            let mut model = Model::new();
            model.apply((&stroke).into())?;
            let item = DrawItem::from_map(&model, stroke.clone().into())?;
            for _ in 0..300 {
                let x = random.next_double() * 130.0;
                let y = random.next_double() * 130.0;
                let w = 0.4 + random.next_double() * 8.0;
                let h = 0.4 + random.next_double() * 8.0;
                let hit = item.intersects(&DocRect { x, y, width: w, height: h }.polygon(), &FACTORY)?;
                hash.update([hit as u8]);
                queries += 1;
                if hit {
                    hits += 1;
                }
            }
        }
    }
    for shape in [
        "rect", "ellipse", "triangle", "line", "arrow", "text", "svg",
    ] {
        let mut input = object! {"object" => shape, "shape" => shape, "x" => 20, "y" => 30, "w" => 60, "h" => 40, "width" => 5, "color" => -16777216, "fill" => -65536, "text" => if shape == "text" { "O gj" } else { "" }, "fontSize" => 30};
        if shape == "svg" {
            input.insert("svg".into(), Value::from("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"60\" height=\"40\"><circle cx=\"30\" cy=\"20\" r=\"18\" fill=\"none\" stroke=\"black\" stroke-width=\"4\"/></svg>"));
        }
        let mut o = actions::drawing((&input).into(), &LayerId::base())?;
        o.insert("rotation".into(), Value::from(33));
        o.insert("flipX".into(), Value::from(true));
        o.insert(
            "erasures".into(),
            Value::Array(vec![Value::from(&DocRect { x: 0.3, y: -1.0, width: 0.2, height: 3.0 }.polygon())]),
        );
        let mut model = Model::new();
        model.apply(&o)?;
        let item = DrawItem::from_map(&model, notemaker_core::wire::EntityContent(o.0.clone()))?;
        for _ in 0..300 {
            let x = random.next_double() * 130.0;
            let y = random.next_double() * 130.0;
            let hit = item.intersects(&DocRect { x, y, width: 2.0, height: 2.0 }.polygon(), &FACTORY)?;
            hash.update([hit as u8]);
            queries += 1;
            if hit {
                hits += 1;
            }
        }
    }
    let digest: String = hash
        .finalize()
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect();
    probe::print(Value::Object(
        object! {"queries" => queries, "hits" => hits, "sha256" => digest},
    ));
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("Exception in thread \"main\" {}", e);
        std::process::exit(1);
    }
    std::process::exit(0);
}
