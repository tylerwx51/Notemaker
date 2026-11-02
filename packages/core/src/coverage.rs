//! Port of `Coverage.java`: item-local painted area.

use crate::error::{require, Result};
use crate::geometry;
use crate::jmath::{self, floor_div};
use crate::json::Map;
use indexmap::IndexMap;
use std::sync::{Arc, Mutex};

/// `Coverage` interface. Implementations never know about layers or sessions.
pub trait Coverage: Send + Sync {
    fn bounds(&self) -> [f64; 4];
    fn intersects(&self, polygon: &[[f64; 2]]) -> Result<bool>;
}

/// `Coverage.Factory`. `DrawItem` caches coverage per factory identity
/// (the address of the `&dyn CoverageFactory`), like Java's `!=` check.
pub trait CoverageFactory {
    fn create(&self, local_content: &Map) -> Result<Box<dyn Coverage>>;
}

/// `Coverage.Tiles.Painter`: ARGB pixels of a `size`×`size` tile at (x,y).
pub trait Painter: Send + Sync {
    /// Errors propagate out of `Tiles::intersects`, as the Java painter's
    /// unchecked exceptions did.
    fn paint(&self, x: i32, y: i32, size: i32) -> Result<Vec<i32>>;
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
    box_: [f64; 4],
    painter: Box<dyn Painter>,
    cache: Mutex<IndexMap<i64, Tile>>,
}

impl Tiles {
    pub fn new(box_: [f64; 4], painter: Box<dyn Painter>) -> Tiles {
        Tiles { box_, painter, cache: Mutex::new(IndexMap::new()) }
    }
    fn tile(&self, x: i32, y: i32) -> Result<Tile> {
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        let key = ((x as i64) << 32) ^ ((y as i64) & 0xffff_ffff);
        if let Some(bits) = cache.shift_remove(&key) {
            cache.insert(key, bits.clone());
            return Ok(bits);
        }
        let pixels = self.painter.paint(x * 64, y * 64, 64)?;
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
    fn bounds(&self) -> [f64; 4] {
        self.box_
    }
    fn intersects(&self, polygon: &[[f64; 2]]) -> Result<bool> {
        let q = geometry::bounds(polygon);
        let b0 = &self.box_;
        let l = jmath::d2i(jmath::max(b0[0], q[0]).floor());
        let t = jmath::d2i(jmath::max(b0[1], q[1]).floor());
        let r = jmath::d2i(jmath::min(b0[2], q[2]).ceil());
        let b = jmath::d2i(jmath::min(b0[3], q[3]).ceil());
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
                if geometry::intersects(polygon, &geometry::rect((tx * 64) as f64, (ty * 64) as f64, 64.0, 64.0)) {
                    match self.tile(tx, ty)? {
                        Tile::Empty => {}
                        Tile::Full => return Ok(true),
                        Tile::Mixed(bits) => {
                            for y in t.max(ty * 64)..b.min(ty * 64 + 64) {
                                for x in l.max(tx * 64)..r.min(tx * 64 + 64) {
                                    let i = ((y - ty * 64) * 64 + x - tx * 64) as usize;
                                    if (bits[i >> 6] & (1u64 << (i & 63))) != 0
                                        && geometry::intersects(polygon, &geometry::rect(x as f64, y as f64, 1.0, 1.0))
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
