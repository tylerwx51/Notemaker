//! Port of `GifAsset.java`: bounds and frame delays for an embedded GIF.

use crate::error::{require, Error, Result};
use crate::jfmt;

pub const MAX_BYTES: usize = 2 * 1024 * 1024;

/// `GifAsset.fromBytes(bytes)`.
pub fn from_bytes(bytes: &[u8]) -> Result<String> {
    validate_bytes(Some(bytes))?;
    Ok(format!("data:image/gif;base64,{}", jfmt::base64_encode(bytes)))
}

/// `GifAsset.Info`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Info {
    pub width: i32,
    pub height: i32,
    pub delays: Vec<i32>,
    pub offsets: Vec<i32>,
    pub duration: i32,
}

impl Info {
    fn new(w: i32, h: i32, values: &[i32]) -> Info {
        let mut delays = vec![0; values.len()];
        let mut offsets = vec![0; values.len()];
        let mut total: i32 = 0;
        let mut raw: i32 = 0;
        for i in 0..delays.len() {
            offsets[i] = raw;
            raw = raw.wrapping_add(values[i]);
            delays[i] = 100.max(10000.min(values[i]));
            total = total.wrapping_add(delays[i]);
        }
        Info { width: w, height: h, delays, offsets, duration: total }
    }
    /// `timeForFrame(frame)`.
    pub fn time_for_frame(&self, frame: usize) -> i32 {
        self.offsets[frame]
    }
}

/// `GifAsset.validate(String uri)`. `None` mirrors a Java null.
pub fn validate(uri: Option<&str>) -> Result<Info> {
    let uri_ok = matches!(uri, Some(u) if u.starts_with("data:image/gif;base64,") && crate::json::jlen(u) as i64 <= MAX_BYTES as i64 * 4 / 3 + 100);
    require(uri_ok, "GIF must be embedded and at most 2 MiB")?;
    let u = uri.unwrap();
    let bytes = match jfmt::base64_decode(&u[22..]) {
        Ok(b) => b,
        Err(e) => return Err(Error::illegal_argument(format!("Invalid GIF: {}", e.message()))),
    };
    match validate_bytes(Some(&bytes)) {
        Ok(info) => Ok(info),
        Err(e) if e.kind.is_illegal_argument() => Err(Error::illegal_argument(format!("Invalid GIF: {}", e.message()))),
        Err(e) => Err(e),
    }
}

fn u16le(b: &[u8], p: usize) -> i32 {
    (b[p] as i32) | ((b[p + 1] as i32) << 8)
}

fn blocks(b: &[u8], mut p: usize) -> Result<usize> {
    loop {
        require(p < b.len(), "Truncated GIF block")?;
        let n = b[p] as usize;
        p += 1;
        if n == 0 {
            return Ok(p);
        }
        require(p + n <= b.len(), "Truncated GIF block")?;
        p += n;
    }
}

/// `GifAsset.validate(byte[])`.
pub fn validate_bytes(bytes: Option<&[u8]>) -> Result<Info> {
    require(matches!(bytes, Some(b) if b.len() >= 14 && b.len() <= MAX_BYTES), "GIF exceeds 2 MiB or has no header")?;
    let b = bytes.unwrap();
    let header = &b[0..6];
    require(header == b"GIF87a" || header == b"GIF89a", "Invalid GIF header")?;
    let w = u16le(b, 6);
    let h = u16le(b, 8);
    require(w > 0 && h > 0 && w <= 8192 && h <= 8192 && (w as i64) * (h as i64) <= 32 * 1024 * 1024, "Invalid GIF dimensions")?;
    let mut p: usize = 13;
    let packed = b[10] as usize;
    if (packed & 128) != 0 {
        p += 3 * (1 << ((packed & 7) + 1));
    }
    require(p <= b.len(), "Truncated GIF palette")?;
    let mut delays: Vec<i32> = Vec::new();
    let mut delay = 100;
    let mut ended = false;
    while p < b.len() {
        let tag = b[p];
        p += 1;
        if tag == 0x3b {
            ended = true;
            break;
        }
        if tag == 0x21 {
            require(p < b.len(), "Truncated GIF extension")?;
            let label = b[p];
            p += 1;
            if label == 0xf9 {
                require(p + 6 <= b.len() && b[p] == 4 && b[p + 5] == 0, "Invalid GIF frame control")?;
                let centiseconds = u16le(b, p + 2);
                delay = if centiseconds == 0 { 100 } else { centiseconds * 10 };
                p += 6;
            } else {
                p = blocks(b, p)?;
            }
            continue;
        }
        require(tag == 0x2c && p + 9 <= b.len(), "Invalid GIF frame")?;
        let fw = u16le(b, p + 4);
        let fh = u16le(b, p + 6);
        require(fw > 0 && fh > 0, "Invalid GIF frame size")?;
        let flags = b[p + 8] as usize;
        p += 9;
        if (flags & 128) != 0 {
            p += 3 * (1 << ((flags & 7) + 1));
        }
        require(p < b.len(), "Truncated GIF frame palette")?;
        p += 1;
        p = blocks(b, p)?;
        delays.push(delay);
        delay = 100;
        require(delays.len() <= 500, "GIF has too many frames")?;
    }
    require(ended && p == b.len() && !delays.is_empty(), "Incomplete GIF")?;
    Ok(Info::new(w, h, &delays))
}
