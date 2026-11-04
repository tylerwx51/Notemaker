//! Port of `RegionFill.java`: bounded flood selection of an enclosed region
//! formed by native ink and shapes.

use crate::actions;
use crate::draw_item::DrawItem;
use crate::error::{require, Error, Result};
use crate::geometry::{DocPoint, Erasures, Polygon};
use crate::wire::{ActionInput, OperationList};
use crate::ids::{LayerId, PageNumber, ShapeId};
use crate::jfmt;
use crate::jmath;
use crate::json::{self, Value};
use crate::layer_editor::LayerEditor;
use crate::model::{self, Model};
use crate::object;
use crate::shape_catalog;
use std::f64::consts::PI;
use std::fmt::Write as _;

struct FillRun {
    start: i32,
    end: i32,
    top: i32,
}

fn append_fill_run(path: &mut String, run: &FillRun, bottom: i32) {
    let length = run.end - run.start;
    path.push('M');
    jfmt::append_double(path, run.start as f64 - 0.5);
    path.push(' ');
    jfmt::append_double(path, run.top as f64 - 0.5);
    let _ = write!(path, "h{}v{}h-{}Z", length, bottom - run.top, length);
}

fn add(lines: &mut Vec<[f64; 5]>, bounds: &mut [f64; 4], x: f64, y: f64, xx: f64, yy: f64, width: f64) {
    lines.push([x, y, xx, yy, width]);
    bounds[0] = jmath::min(bounds[0], jmath::min(x, xx));
    bounds[1] = jmath::min(bounds[1], jmath::min(y, yy));
    bounds[2] = jmath::max(bounds[2], jmath::max(x, xx));
    bounds[3] = jmath::max(bounds[3], jmath::max(y, yy));
}

/// `RegionFill.create(model, input)`: an SVG fill object for the region around (x, y).
pub fn create(model: &Model, input: &ActionInput) -> Result<OperationList> {
    let x = json::number(input, "x")?;
    let y = json::number(input, "y")?;
    model::input_color(input)?;
    let requested_page = match input.get("page") {
        None => 0.0,
        v => json::as_double(v)?,
    };
    require(requested_page == jmath::rint(requested_page) && (0.0..=10000.0).contains(&requested_page), "Invalid fill page")?;
    let page = jmath::d2i(requested_page);
    let layer = json::string_or(input, "layer", "base")?.ok_or_else(Error::null_pointer)?.to_string();
    require(x.is_finite() && y.is_finite() && x.abs() <= 1000000.0 && y.abs() <= 1000000.0, "Invalid fill point")?;
    let mut lines: Vec<[f64; 5]> = Vec::new();
    let mut bounds = [x, y, x, y];
    let editor = LayerEditor::new(model, &LayerId::from(&layer), PageNumber(page))?;
    let items: Vec<&DrawItem> = editor.items().collect();
    let mut owners: Vec<usize> = Vec::new();
    let mut masks: Vec<Erasures> = vec![Erasures::default(); items.len()];
    for (index, item) in items.iter().enumerate() {
        if item.kind != "stroke" {
            continue;
        }
        let s = &item.content;
        let points = json::list_at(s, "points")?;
        for i in 1..points.len() {
            let a = json::list(&points[i - 1])?;
            let b = json::list(&points[i])?;
            add(
                &mut lines,
                &mut bounds,
                json::as_double(a.first())?,
                json::as_double(a.get(1))?,
                json::as_double(b.first())?,
                json::as_double(b.get(1))?,
                json::number(s, "width")?,
            );
        }
        masks[index] = DrawItem::erasures(&*item.local_content()?)?;
        owners.resize(lines.len(), index);
    }
    for (index, item) in items.iter().enumerate() {
        if item.kind != "object" {
            continue;
        }
        let o = item.geometry();
        let shape = json::string(o, "shape")?;
        let ox = json::number(o, "x")?;
        let oy = json::number(o, "y")?;
        let w = json::number(o, "w")?;
        let h = json::number(o, "h")?;
        let width = json::number(o, "width")?;
        let vertices: Polygon = if shape_catalog::polygon(&ShapeId::from(shape)) {
            shape_catalog::vertices(&ShapeId::from(shape))?
        } else if matches!(shape, "rect" | "rectangle") {
            Polygon::from(vec![DocPoint { x: 0.0, y: 0.0 }, DocPoint { x: 1.0, y: 0.0 }, DocPoint { x: 1.0, y: 1.0 }, DocPoint { x: 0.0, y: 1.0 }])
        } else if matches!(shape, "ellipse" | "circle") {
            (0..128)
                .map(|i| {
                    let a = i as f64 * PI / 64.0;
                    DocPoint { x: 0.5 + 0.5 * jmath::cos(a), y: 0.5 + 0.5 * jmath::sin(a) }
                })
                .collect()
        } else if matches!(shape, "line" | "arrow" | "arrowLine") {
            Polygon::from(vec![DocPoint { x: 0.0, y: 0.0 }, DocPoint { x: 1.0, y: 1.0 }])
        } else {
            continue;
        };
        let rotation = match o.get("rotation") {
            None => 0.0,
            v => json::as_double(v)?,
        } * PI
            / 180.0;
        let cos = jmath::cos(rotation);
        let sin = jmath::sin(rotation);
        let fx = if json::is_true(o.get("flipX")) { -1.0 } else { 1.0 };
        let fy = if json::is_true(o.get("flipY")) { -1.0 } else { 1.0 };
        let transformed: Polygon = vertices
            .iter()
            .map(|v| {
                let vx = (v.x - 0.5) * w * fx;
                let vy = (v.y - 0.5) * h * fy;
                DocPoint { x: ox + w / 2.0 + vx * cos - vy * sin, y: oy + h / 2.0 + vx * sin + vy * cos }
            })
            .collect();
        let closed = !matches!(shape, "line" | "arrow" | "arrowLine" | "polyline");
        for i in 1..vertices.len() + if closed { 1 } else { 0 } {
            let a = transformed[i - 1];
            let b = transformed[i % vertices.len()];
            add(&mut lines, &mut bounds, a.x, a.y, b.x, b.y, width);
        }
        masks[index] = DrawItem::erasures(&*item.local_content()?)?;
        owners.resize(lines.len(), index);
    }
    require(!lines.is_empty(), "No enclosing ink on this layer")?;
    let pad = 20.0;
    let scale = jmath::max(1.0, jmath::max(bounds[2] - bounds[0] + 2.0 * pad, bounds[3] - bounds[1] + 2.0 * pad) / 1000.0);
    let left = bounds[0] - pad;
    let top = bounds[1] - pad;
    let width = jmath::d2i(((bounds[2] - bounds[0] + 2.0 * pad) / scale).ceil()) + 1;
    let height = jmath::d2i(((bounds[3] - bounds[1] + 2.0 * pad) / scale).ceil()) + 1;
    let mut mask = vec![0u8; (width as usize) * (height as usize)];
    let mut work: i64 = 0;
    for (line_index, l) in lines.iter().enumerate() {
        let ax = (l[0] - left) / scale;
        let ay = (l[1] - top) / scale;
        let bx = (l[2] - left) / scale;
        let by = (l[3] - top) / scale;
        let r = jmath::max(0.7, l[4] / 2.0 / scale);
        let x0 = 0.max(jmath::d2i((jmath::min(ax, bx) - r).floor()));
        let x1 = (width - 1).min(jmath::d2i((jmath::max(ax, bx) + r).ceil()));
        let y0 = 0.max(jmath::d2i((jmath::min(ay, by) - r).floor()));
        let y1 = (height - 1).min(jmath::d2i((jmath::max(ay, by) + r).ceil()));
        let dx = bx - ax;
        let dy = by - ay;
        let q = dx * dx + dy * dy;
        work += ((x1 - x0 + 1) as i64) * ((y1 - y0 + 1) as i64);
        require(work <= 20000000, "Fill boundary is too complex; use a smaller layer or closed outline")?;
        let owner = owners[line_index];
        let holes = &masks[owner];
        for yy in y0..=y1 {
            for xx in x0..=x1 {
                let t = if q == 0.0 { 0.0 } else { jmath::max(0.0, jmath::min(1.0, ((xx as f64 - ax) * dx + (yy as f64 - ay) * dy) / q)) };
                if jmath::hypot(xx as f64 - ax - t * dx, yy as f64 - ay - t * dy) <= r {
                    let mut erased = false;
                    if !holes.is_empty() {
                        let local = items[owner].local(DocPoint { x: left + xx as f64 * scale, y: top + yy as f64 * scale })?;
                        for hole in holes {
                            if hole.contains(local) {
                                erased = true;
                                break;
                            }
                        }
                    }
                    if !erased {
                        mask[(yy * width + xx) as usize] = 1;
                    }
                }
            }
        }
    }
    let sx = jmath::round((x - left) / scale) as i32;
    let sy = jmath::round((y - top) / scale) as i32;
    let seed = (sy * width + sx) as usize;
    require(mask[seed] == 0, "Tap inside the outline, away from its edge")?;
    let mut queue = vec![0i32; mask.len()];
    let (mut head, mut tail) = (0usize, 0usize);
    queue[tail] = seed as i32;
    tail += 1;
    mask[seed] = 2;
    while head < tail {
        let p = queue[head];
        head += 1;
        let xx = p % width;
        let yy = p / width;
        require(xx > 0 && xx < width - 1 && yy > 0 && yy < height - 1, "Area is open; fill requires a closed boundary")?;
        for next in [p - 1, p + 1, p - width, p + width] {
            if mask[next as usize] == 0 {
                mask[next as usize] = 2;
                queue[tail] = next;
                tail += 1;
            }
        }
    }
    let mut path = String::new();
    let mut prior_runs: Vec<FillRun> = Vec::new();
    let mut next_runs: Vec<FillRun> = Vec::new();
    for yy in 1..height - 1 {
        next_runs.clear();
        let mut prior_iter = std::mem::take(&mut prior_runs).into_iter().peekable();
        let mut xx = 1;
        while xx < width - 1 {
            if mask[(yy * width + xx) as usize] == 2 {
                let start = xx;
                while xx + 1 < width - 1 && mask[(yy * width + xx + 1) as usize] == 2 {
                    xx += 1;
                }
                let end = xx + 1;
                while let Some(prev) = prior_iter.peek() {
                    if prev.start < start || (prev.start == start && prev.end < end) {
                        append_fill_run(&mut path, prev, yy);
                        prior_iter.next();
                    } else {
                        break;
                    }
                }
                let run = match prior_iter.peek() {
                    Some(prev) if prev.start == start && prev.end == end => prior_iter.next().unwrap(),
                    _ => FillRun { start, end, top: yy },
                };
                next_runs.push(run);
            }
            xx += 1;
        }
        for prev in prior_iter {
            append_fill_run(&mut path, &prev, yy);
        }
        std::mem::swap(&mut prior_runs, &mut next_runs);
    }
    for run in &prior_runs {
        append_fill_run(&mut path, run, height - 1);
    }
    let color = json::as_int(input.get("color"))?;
    let svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\"><path d=\"{}\" fill=\"{}\" fill-opacity=\"{}\"/></svg>",
        path,
        crate::style::ArgbColor(color).to_hex(),
        jfmt::double(((color as u32) >> 24) as f64 / 255.0),
        w = width,
        h = height
    );
    let drawing = actions::drawing(
        &ActionInput(object! {
            "shape" => "svg",
            "svg" => svg,
            "x" => json::double(left),
            "y" => json::double(top),
            "w" => json::double(width as f64 * scale),
            "h" => json::double(height as f64 * scale),
            "page" => page,
        }),
        &LayerId::from(&layer),
    )?;
    Ok(OperationList(vec![Value::from(drawing)]))
}
