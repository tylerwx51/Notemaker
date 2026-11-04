//! Paint and text style values used by the rendering surfaces.

/// A packed `0xAARRGGBB` colour (Java `int` colour).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ArgbColor(pub i32);

impl ArgbColor {
    pub const BLACK: ArgbColor = ArgbColor(0xff000000_u32 as i32);
    pub const WHITE: ArgbColor = ArgbColor(0xffffffff_u32 as i32);
    pub const TRANSPARENT: ArgbColor = ArgbColor(0);

    /// The four 8-bit channels.
    pub const fn channels(self) -> ArgbChannels {
        let c = self.0;
        ArgbChannels { alpha: (c >> 24) & 0xff, red: (c >> 16) & 0xff, green: (c >> 8) & 0xff, blue: c & 0xff }
    }

    /// `String.format(Locale.ROOT, "#%06x", c & 0xffffff)`.
    pub fn to_hex(self) -> CssColor {
        CssColor::from(format!("#{:06x}", self.0 & 0xffffff))
    }
}

/// The channels of an [`ArgbColor`], each `0..=255`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ArgbChannels {
    pub alpha: i32,
    pub red: i32,
    pub green: i32,
    pub blue: i32,
}

crate::string_newtype! {
    /// A CSS/SVG colour such as `"#1a2b3c"`.
    pub struct CssColor;
}

/// Row-major ARGB pixels (Java `int[]` from `Bitmap.getPixels`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ArgbPixels(pub Vec<i32>);

impl std::ops::Deref for ArgbPixels {
    type Target = [i32];
    fn deref(&self) -> &[i32] {
        &self.0
    }
}

/// A stroke or outline width in document units.
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
pub struct StrokeWidth(pub f64);

/// A font size in document units.
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
pub struct FontSize(pub f64);

crate::string_newtype! {
    /// A font family name such as `"sans-serif"`.
    pub struct FontFamily;
}

crate::string_newtype! {
    /// A text alignment name: `"left"`, `"center"` or `"right"`.
    pub struct TextAlignment;
}

crate::string_newtype! {
    /// Text drawn by a text object.
    pub struct TextContent;
}

/// Outline colour and width plus an optional fill, for closed shapes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShapePaint {
    pub color: ArgbColor,
    pub width: StrokeWidth,
    pub fill: Option<ArgbColor>,
}

/// Bold, italic and underline flags of a text object.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextStyle {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}

/// Mirroring of an object about its centre.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Flip {
    pub horizontal: bool,
    pub vertical: bool,
}

/// Whether a drawn path is closed back to its first point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathEnd {
    Open,
    Closed,
}

/// The outline drawn for a rectangle-shaped object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RectShape {
    Rectangle,
    Ellipse,
}
