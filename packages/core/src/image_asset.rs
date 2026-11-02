//! Port of `ImageAsset.java`: portable PNG/JPEG header validation.

use crate::error::{require, Error, Result};
use crate::jfmt;

pub const MAX_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_DIMENSION: i64 = 8192;

/// `ImageAsset.fromBytes(data)`.
pub fn from_bytes(data: &[u8]) -> Result<String> {
    let mime = if png(data) { "image/png" } else { "image/jpeg" };
    let uri = format!("data:{};base64,{}", mime, jfmt::base64_encode(data));
    validate(Some(&uri))?;
    Ok(uri)
}

fn png(b: &[u8]) -> bool {
    b.len() >= 24 && b[0] == 137 && b[1] == 80 && b[2] == 78 && b[3] == 71 && b[4] == 13 && b[5] == 10 && b[6] == 26 && b[7] == 10
}

/// Bounds-checked byte read; Java threw `ArrayIndexOutOfBoundsException`.
fn at(b: &[u8], p: usize) -> Result<i64> {
    b.get(p).map(|v| *v as i64).ok_or_else(|| {
        Error::new(crate::error::ErrorKind::IndexOutOfBounds, format!("Index {} out of bounds for length {}", p, b.len()))
    })
}
fn u16be(b: &[u8], p: usize) -> Result<i64> {
    Ok((at(b, p)? << 8) | at(b, p + 1)?)
}
fn u32be(b: &[u8], p: usize) -> Result<i64> {
    Ok((u16be(b, p)? << 16) | u16be(b, p + 2)?)
}

/// `ImageAsset.validate(uri)`: `{w, h}`. `None` mirrors a Java null.
pub fn validate(uri: Option<&str>) -> Result<[f64; 2]> {
    require(matches!(uri, Some(u) if crate::json::jlen(u) as i64 <= MAX_BYTES as i64 * 4 / 3 + 100), "Image exceeds 2 MiB")?;
    let uri = uri.unwrap();
    let is_png = uri.starts_with("data:image/png;base64,");
    let is_jpeg = uri.starts_with("data:image/jpeg;base64,");
    require(is_png || is_jpeg, "Image must be an embedded PNG or JPEG")?;
    let b = match jfmt::base64_decode(&uri[uri.find(',').unwrap() + 1..]) {
        Ok(b) => b,
        Err(_) => return Err(Error::illegal_argument("Invalid image base64")),
    };
    require(!b.is_empty() && b.len() <= MAX_BYTES, "Image exceeds 2 MiB")?;
    let mut w: i64 = 0;
    let mut h: i64 = 0;
    if is_png {
        require(png(&b) && u32be(&b, 8)? == 13 && b[12] == 73 && b[13] == 72 && b[14] == 68 && b[15] == 82, "Invalid PNG header")?;
        w = u32be(&b, 16)?;
        h = u32be(&b, 20)?;
        let mut pixels = false;
        let mut end = false;
        let mut offset: usize = 8;
        while offset + 12 <= b.len() {
            let size = u32be(&b, offset)?;
            require(size <= (b.len() - offset - 12) as i64, "Truncated PNG chunk")?;
            let length = size as usize;
            let crc = crc32fast::hash(&b[offset + 4..offset + 8 + length]) as i64;
            require(crc == u32be(&b, offset + 8 + length)?, "Invalid PNG checksum")?;
            let kind = &b[offset + 4..offset + 8];
            pixels |= kind == b"IDAT";
            offset += length + 12;
            if kind == b"IEND" {
                require(length == 0, "Invalid PNG end")?;
                end = true;
                break;
            }
        }
        require(pixels && end && offset == b.len(), "Incomplete PNG image")?;
    } else {
        require(b.len() >= 4 && b[0] == 255 && b[1] == 216, "Invalid JPEG header")?;
        require(b[b.len() - 2] == 255 && b[b.len() - 1] == 217, "Truncated JPEG image")?;
        let mut p: usize = 2;
        while p + 3 < b.len() {
            require(b[p] == 255, "Invalid JPEG marker")?;
            while p < b.len() && b[p] == 255 {
                p += 1;
            }
            require(p < b.len(), "Truncated JPEG")?;
            let marker = b[p];
            p += 1;
            if marker == 217 || marker == 218 {
                break;
            }
            if marker == 1 || (208..=215).contains(&marker) {
                continue;
            }
            require(p + 1 < b.len(), "Truncated JPEG")?;
            let length = u16be(&b, p)? as usize;
            require(length >= 2 && p + length <= b.len(), "Truncated JPEG segment")?;
            if (192..=207).contains(&marker) && marker != 196 && marker != 200 && marker != 204 {
                require(length >= 8, "Invalid JPEG frame")?;
                h = u16be(&b, p + 3)?;
                w = u16be(&b, p + 5)?;
                break;
            }
            p += length;
        }
    }
    require(w > 0 && h > 0 && w <= MAX_DIMENSION && h <= MAX_DIMENSION, "Invalid image dimensions (maximum 8192)")?;
    require(w * h <= 32 * 1024 * 1024, "Image exceeds 32 megapixels")?;
    Ok([w as f64, h as f64])
}
