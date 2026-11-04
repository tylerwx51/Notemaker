//! Port of `ShapeCatalog.java`: stable shape identifiers and normalized geometry.

use crate::error::{Error, Result};
use crate::geometry::{DocPoint, Polygon};
use crate::ids::ShapeId;
use crate::jmath;
use crate::json::Value;
use crate::object;

const IDS: [&str; 32] = [
    "rect", "ellipse", "text", "line", "triangle", "diamond", "pentagon", "hexagon", "star", "arrow", "svg",
    "trapezoid", "triangle45", "triangle60", "triangle90", "wave", "curve", "polyline", "regularPolygon",
    "cube", "cylinder", "cone", "sphere", "pyramid", "image", "gif", "circle", "rectangle", "rhombus", "arrowLine", "waveLine", "regularHexagon",
];

/// A shape's display name, such as `"Rectangle"`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShapeName(&'static str);

impl std::ops::Deref for ShapeName {
    type Target = str;
    fn deref(&self) -> &str {
        self.0
    }
}

impl std::fmt::Display for ShapeName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

/// `ShapeCatalog.ids()`.
pub fn ids() -> impl Iterator<Item = ShapeId> {
    IDS.iter().map(|id| ShapeId::from(*id))
}

/// `ShapeCatalog.known(id)`.
pub fn known(id: &ShapeId) -> bool {
    IDS.contains(&id.as_ref())
}

/// `ShapeCatalog.json()`: `[{id,name},...]`.
pub fn json() -> Value {
    Value::Array(IDS.iter().map(|id| Value::Object(object! {"id" => *id, "name" => &*name(&ShapeId::from(*id)).unwrap()})).collect())
}

/// `ShapeCatalog.name(id)`.
pub fn name(id: &ShapeId) -> Result<ShapeName> {
    Ok(ShapeName(match id.as_ref() {
        "rect" => "Rectangle",
        "ellipse" => "Ellipse",
        "text" => "Text",
        "line" => "Line",
        "triangle" => "Triangle",
        "diamond" => "Diamond",
        "pentagon" => "Pentagon",
        "hexagon" => "Hexagon",
        "star" => "Star",
        "arrow" => "Arrow",
        "svg" => "SVG",
        "trapezoid" => "Trapezoid",
        "triangle45" => "45° Triangle",
        "triangle60" => "60° Triangle",
        "triangle90" => "Right Triangle",
        "wave" => "Wave",
        "curve" => "Curve",
        "polyline" => "Polyline",
        "regularPolygon" => "Regular Polygon",
        "cube" => "Cube",
        "cylinder" => "Cylinder",
        "cone" => "Cone",
        "sphere" => "Sphere",
        "pyramid" => "Pyramid",
        "image" => "Picture",
        "gif" => "GIF animation",
        "circle" => "Circle",
        "rectangle" => "Rectangle",
        "rhombus" => "Rhombus",
        "arrowLine" => "Arrow Line",
        "waveLine" => "Wave Line",
        "regularHexagon" => "Regular Hexagon",
        _ => return Err(Error::illegal_argument(format!("Unknown shape: {}", id))),
    }))
}

/// `ShapeCatalog.polygon(id)`.
pub fn polygon(id: &ShapeId) -> bool {
    matches!(
        id.as_ref(),
        "triangle" | "diamond" | "rhombus" | "pentagon" | "hexagon" | "star" | "trapezoid" | "triangle45" | "triangle60"
            | "triangle90" | "regularPolygon" | "regularHexagon" | "polyline"
    )
}

/// `ShapeCatalog.vertices(id)`: unit bounding box, clockwise from the top.
pub fn vertices(id: &ShapeId) -> Result<Polygon> {
    use std::f64::consts::PI;
    Ok(match id.as_ref() {
        "triangle" | "triangle60" => Polygon::from(vec![DocPoint { x: 0.5, y: 0.0 }, DocPoint { x: 1.0, y: 1.0 }, DocPoint { x: 0.0, y: 1.0 }]),
        "triangle45" => Polygon::from(vec![DocPoint { x: 0.0, y: 1.0 }, DocPoint { x: 1.0, y: 0.0 }, DocPoint { x: 1.0, y: 1.0 }]),
        "triangle90" => Polygon::from(vec![DocPoint { x: 0.0, y: 0.0 }, DocPoint { x: 0.0, y: 1.0 }, DocPoint { x: 1.0, y: 1.0 }]),
        "diamond" | "rhombus" => Polygon::from(vec![DocPoint { x: 0.5, y: 0.0 }, DocPoint { x: 1.0, y: 0.5 }, DocPoint { x: 0.5, y: 1.0 }, DocPoint { x: 0.0, y: 0.5 }]),
        "trapezoid" => Polygon::from(vec![DocPoint { x: 0.2, y: 0.0 }, DocPoint { x: 0.8, y: 0.0 }, DocPoint { x: 1.0, y: 1.0 }, DocPoint { x: 0.0, y: 1.0 }]),
        "pentagon" => regular(5, PI * 1.5),
        "hexagon" | "regularHexagon" => regular(6, PI * 1.5),
        "regularPolygon" => regular(8, PI * 1.5),
        "polyline" => Polygon::from(vec![DocPoint { x: 0.0, y: 1.0 }, DocPoint { x: 0.3, y: 0.25 }, DocPoint { x: 0.65, y: 0.7 }, DocPoint { x: 1.0, y: 0.0 }]),
        "star" => {
            let mut p = Polygon::from(vec![DocPoint::default(); 10]);
            for (i, v) in p.iter_mut().enumerate() {
                let a = -PI / 2.0 + i as f64 * PI / 5.0;
                let r = if i % 2 == 0 { 0.5 } else { 0.22 };
                v.x = 0.5 + jmath::cos(a) * r;
                v.y = 0.5 + jmath::sin(a) * r;
            }
            p
        }
        _ => return Err(Error::illegal_argument(format!("Shape has no polygon geometry: {}", id))),
    })
}

fn regular(n: usize, start: f64) -> Polygon {
    let mut p = Polygon::from(vec![DocPoint::default(); n]);
    for (i, v) in p.iter_mut().enumerate() {
        let a = start + i as f64 * 2.0 * std::f64::consts::PI / n as f64;
        v.x = 0.5 + 0.5 * jmath::cos(a);
        v.y = 0.5 + 0.5 * jmath::sin(a);
    }
    p
}

/// `ShapeCatalog.drawingSvg(id)`.
pub fn drawing_svg(id: &ShapeId) -> Result<&'static str> {
    Ok(match id.as_ref() {
        "cube" => "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 100 100\"><path d=\"M50 4 92 27 92 73 50 96 8 73 8 27Z M50 4V50 M8 27 50 50 92 27 M50 50V96\" fill=\"none\"/></svg>",
        "pyramid" => "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 100 100\"><path d=\"M50 6 8 84 92 84Z M50 6 50 84 M8 84 50 65 92 84\" fill=\"none\"/></svg>",
        "cone" => "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 100 100\"><path d=\"M50 5 8 82 M50 5 92 82 M8 82 C15 98 85 98 92 82 M8 82 C28 68 72 68 92 82\" fill=\"none\"/></svg>",
        "cylinder" => "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 100 100\"><path d=\"M8 20 C8 2 92 2 92 20 V80 C92 98 8 98 8 80 Z M8 20 C8 38 92 38 92 20 M8 80 C8 62 92 62 92 80\" fill=\"none\"/></svg>",
        "sphere" => "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 100 100\"><circle cx=\"50\" cy=\"50\" r=\"46\" fill=\"none\"/><path d=\"M5 50 C20 28 80 28 95 50 C80 72 20 72 5 50 M50 4 C22 25 22 75 50 96 M50 4 C78 25 78 75 50 96\" fill=\"none\"/></svg>",
        "wave" | "waveLine" => "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 100 100\"><path d=\"M0 55 C16 5 34 5 50 55 S84 105 100 55\" fill=\"none\"/></svg>",
        "curve" => "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 100 100\"><path d=\"M2 90 C15 5 62 5 98 10\" fill=\"none\"/></svg>",
        _ => return Err(Error::illegal_argument(format!("Shape has no SVG geometry: {}", id))),
    })
}
