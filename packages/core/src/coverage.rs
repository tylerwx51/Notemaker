//! Port of `Coverage.java`: item-local painted area.

use crate::error::{require, Result};
use crate::geometry::{Bounds, DocRect, Polygon};
use crate::jmath::{self, floor_div};
use crate::json::Map;
use crate::style::ArgbPixels;
use indexmap::IndexMap;
use std::sync::{Arc, Mutex};

/// `Coverage` interface. Implementations never know about layers or sessions.
pub trait Coverage: Send + Sync {
    fn bounds(&self) -> Bounds;
    fn intersects(&self, polygon: &Polygon) -> Result<bool>;
}

/// `Coverage.Factory`. `DrawItem` caches coverage per factory identity
/// (the address of the `&dyn CoverageFactory`), like Java's `!=` check.
pub trait CoverageFactory {
    fn create(&self, local_content: &Map) -> Result<Box<dyn Coverage>>;
}

/// A square tile of coverage pixels: `size`×`size` document units with its
/// top-left corner at (`x`, `y`), one pixel per unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CoverageTile {
    pub x: i32,
    pub y: i32,
    pub size: i32,
}

/// `Coverage.Tiles.Painter`: ARGB pixels of one [`CoverageTile`].
pub trait Painter: Send + Sync {
    /// Errors propagate out of `Tiles::intersects`, as the Java painter's
    /// unchecked exceptions did.
    fn paint(&self, tile: CoverageTile) -> Result<ArgbPixels>;
}

#[derive(Clone)]
enum Tile {
    Empty,
    Full,
    Mixed(Arc<[u64; 64]>),
}

/// `Coverage.Tiles`: lazy alpha support, one document unit per pixel,
/// at most 128 cached 64×64 tiles (access order, like `LinkedHashMap(…, true)`).
pub struct Tiles {
    box_: Bounds,
    painter: Box<dyn Painter>,
    cache: Mutex<IndexMap<i64, Tile>>,
}

impl Tiles {
    pub fn new(box_: Bounds, painter: Box<dyn Painter>) -> Tiles {
        Tiles { box_, painter, cache: Mutex::new(IndexMap::new()) }
    }
    fn tile(&self, x: i32, y: i32) -> Result<Tile> {
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        let key = ((x as i64) << 32) ^ ((y as i64) & 0xffff_ffff);
        if let Some(bits) = cache.shift_remove(&key) {
            cache.insert(key, bits.clone());
            return Ok(bits);
        }
        let pixels = self.painter.paint(CoverageTile { x: x * 64, y: y * 64, size: 64 })?;
        let mut bits = [0u64; 64];
        let mut count = 0;
        for i in 0..4096usize {
            if ((pixels[i] as u32) >> 24) != 0 {
                bits[i >> 6] |= 1u64 << (i & 63);
                count += 1;
            }
        }
        let tile = if count == 0 {
            Tile::Empty
        } else if count == 4096 {
            Tile::Full
        } else {
            Tile::Mixed(Arc::new(bits))
        };
        cache.insert(key, tile.clone());
        if cache.len() > 128 {
            cache.shift_remove_index(0);
        }
        Ok(tile)
    }
}

impl Coverage for Tiles {
    fn bounds(&self) -> Bounds {
        self.box_
    }
    fn intersects(&self, polygon: &Polygon) -> Result<bool> {
        let q = polygon.bounds();
        let b0 = &self.box_;
        let l = jmath::d2i(jmath::max(b0.left, q.left).floor());
        let t = jmath::d2i(jmath::max(b0.top, q.top).floor());
        let r = jmath::d2i(jmath::min(b0.right, q.right).ceil());
        let b = jmath::d2i(jmath::min(b0.bottom, q.bottom).ceil());
        if l >= r || t >= b {
            return Ok(false);
        }
        require(
            ((floor_div(r - 1, 64) - floor_div(l, 64) + 1) as i64).wrapping_mul((floor_div(b - 1, 64) - floor_div(t, 64) + 1) as i64) <= 16384,
            "Coverage query too large; use a smaller region",
        )?;
        let mut ty = floor_div(t, 64);
        while ty <= floor_div(b - 1, 64) {
            let mut tx = floor_div(l, 64);
            while tx <= floor_div(r - 1, 64) {
                if polygon.intersects(&DocRect { x: (tx * 64) as f64, y: (ty * 64) as f64, width: 64.0, height: 64.0 }.polygon()) {
                    match self.tile(tx, ty)? {
                        Tile::Empty => {}
                        Tile::Full => return Ok(true),
                        Tile::Mixed(bits) => {
                            for y in t.max(ty * 64)..b.min(ty * 64 + 64) {
                                for x in l.max(tx * 64)..r.min(tx * 64 + 64) {
                                    let i = ((y - ty * 64) * 64 + x - tx * 64) as usize;
                                    if (bits[i >> 6] & (1u64 << (i & 63))) != 0
                                        && polygon.intersects(&DocRect { x: x as f64, y: y as f64, width: 1.0, height: 1.0 }.polygon())
                                    {
                                        return Ok(true);
                                    }
                                }
                            }
                        }
                    }
                }
                tx += 1;
            }
            ty += 1;
        }
        Ok(false)
    }
}
