//! Port of `EditorUi.java`: rail, header, popups and panels of the editor,
//! with its inner view classes (`Tool`, `Icon`, `LayerPreview`,
//! `StrokeSample`, `ConflictPreview`) and the stateful anonymous views
//! (`EditorUi$1` → [`ChoiceItem`], `EditorUi$2` → [`ColorSwatch`],
//! `EditorUi$6` → [`PopupShell`]). The stateless anonymous `SeekBar`
//! listeners (`$3`, `$4`, `$5`) are `jx::callback` closures in the panel
//! builders.
//!
//! All methods run on the UI thread. View structs are owned by their Java
//! view's native handle and name their view weakly (see `jx::native_view`).
//!
//! Ownership: every listener and callback stored in a Java object captures
//! the `EditorUi` weakly ([`EditorUi::act`], [`EditorUi::supplier`]) and any
//! view of its own tree as a [`WeakObj`], so no global reference cycle keeps
//! a view tree, popup or dialog alive.

use crate::android_canvas::{self, AndroidCanvas};
use crate::android_consts::{
    COLOR_BLACK, COLOR_TRANSPARENT, COLOR_WHITE, GRAVITY_CENTER, GRAVITY_CENTER_HORIZONTAL, GRAVITY_CENTER_VERTICAL, GRAVITY_LEFT,
    GRAVITY_RIGHT, GRAVITY_TOP, INPUTTYPE_TYPE_CLASS_NUMBER, INPUTTYPE_TYPE_CLASS_TEXT, INPUTTYPE_TYPE_TEXT_VARIATION_PASSWORD,
    INPUTTYPE_TYPE_TEXT_VARIATION_URI, LINEARLAYOUT_HORIZONTAL, LINEARLAYOUT_VERTICAL, MEASURE_SPEC_EXACTLY, MEASURE_SPEC_UNSPECIFIED,
    PAINT_ANTI_ALIAS_FLAG, TOAST_LENGTH_SHORT, VIEW_GONE, VIEW_VISIBLE,
};
use crate::document_browser::DocumentBrowser;
use crate::error::{AppError, AppResult};
use crate::field::Field;
use crate::ink_cache::InkCache;
use crate::jx::{self, JResult, JavaError, Local, Obj, Reply, WeakObj};
use crate::main_activity::{CanvasView, MainActivity};
use crate::org_json::{JSONArray, JV};
use crate::ui;
use indexmap::IndexMap;
use notemaker_cli::canvas::{Cap, Join, Style};
use notemaker_cli::Png;
use notemaker_core::json::{self, Map, Value};
use notemaker_core::model::Entity;
use notemaker_core::{jfmt, jmath, render, shape_catalog};
use std::sync::{Arc, Mutex, OnceLock, Weak};

/// `private static final int INK = Color.BLACK`.
pub const INK: i32 = crate::android_consts::COLOR_BLACK;
/// `private static final int MUTED = 0xff555555`.
pub const MUTED: i32 = 0xff555555u32 as i32;
/// `private static final int LINE = Color.BLACK`.
pub const LINE: i32 = crate::android_consts::COLOR_BLACK;
/// `private static final int PAPER = Color.WHITE`.
pub const PAPER: i32 = crate::android_consts::COLOR_WHITE;

/// `java.util.function.BooleanSupplier`.
pub type BooleanSupplier = Arc<dyn Fn() -> bool + Send + Sync>;
/// `Runnable` callbacks passed to the builders.
pub type Action = Arc<dyn Fn() -> AppResult<()> + Send + Sync>;

notemaker_core::string_newtype! {
    /// An editor tool kind (`pen`, `eraser`, `move`, `shapes`, `text`,
    /// `fill`, `select`, `interact`) or another rail/header entry with a
    /// popup (`document`, `layers`, `more`, ...).
    pub struct ToolKind;
}

notemaker_core::string_newtype! {
    /// A `panel.open` name: a [`ToolKind`] or one of the secondary panels
    /// `history`, `conflicts`, `network`, `preferences`, `presets`,
    /// `brushes`.
    pub struct PanelName;
}

notemaker_core::string_newtype! {
    /// The name of a pen preset (built-in such as `Black · 5`, or saved).
    pub struct PenPresetName;
}

/// Whether the rail shows a text label under each tool icon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolLabels {
    Shown,
    Hidden,
}

// ---------------------------------------------------------------------------
// Pure helpers (host-testable)

/// `EditorUi.actionIcon(label)`.
fn action_icon_for(label: &str) -> &'static str {
    let s = label.to_lowercase();
    if s == "agent" {
        return "agent";
    }
    if s.contains("undo") {
        return "undo";
    }
    if s.contains("redo") {
        return "redo";
    }
    if s.contains("layer") {
        return "layers";
    }
    if s.contains("picture") || s.contains("artwork") {
        return "insert";
    }
    if s.contains("fountain") {
        return "fountain";
    }
    if s.contains("pencil") {
        return "pencil";
    }
    if s.contains("marker") {
        return "marker";
    }
    if s.contains("charcoal") {
        return "charcoal";
    }
    if s.contains("oily") {
        return "oily";
    }
    if s.contains("neobrush") || s.contains("neo brush") {
        return "neoBrush";
    }
    if s.contains("calligraphy") {
        return if s.contains("asia") { "asiaCalligraphy" } else { "latinCalligraphy" };
    }
    if s.contains("square") {
        return "square";
    }
    if s.starts_with("pen") || s.contains(" pen") || s.contains("brush") || s.contains("preset") {
        return "pen";
    }
    if s.contains("text") {
        return "text";
    }
    if s.contains("erase") {
        return "eraser";
    }
    if s.contains("template") || s == "ruled" || s == "graph" || s == "dot grid" {
        return "template";
    }
    if s.contains("page") || s.contains("document") {
        return "document";
    }
    if s.contains("zoom") || s.contains("view") {
        return "move";
    }
    if s.contains("select") || s.contains("cop") || s.contains("paste") || s.contains("transform") {
        return "select";
    }
    if s.contains("done") || s.starts_with("ready") {
        return "check";
    }
    "more"
}

/// `brush.replaceAll("([a-z])([A-Z])", "$1 $2")`.
fn split_camel(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len() + 4);
    let mut i = 0;
    while i < chars.len() {
        if i + 1 < chars.len() && chars[i].is_ascii_lowercase() && chars[i + 1].is_ascii_uppercase() {
            out.push(chars[i]);
            out.push(' ');
            out.push(chars[i + 1]);
            i += 2;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// `Character.toUpperCase(s.charAt(0)) + s.substring(1)` for ASCII names.
fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(first) => first.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

/// `id.substring(0, Math.min(8, id.length()))`.
fn short_id(id: &str) -> String {
    let units: Vec<u16> = id.encode_utf16().collect();
    String::from_utf16_lossy(&units[..units.len().min(8)])
}

/// `EditorUi.materialDescription()` range for a brush.
fn material_range(brush: &str) -> &'static str {
    match brush {
        "pencil" => "Graphite grade · HB to 6B",
        "marker" => "Opacity · light to saturated",
        "brush" => "Bristles · sparse to dense",
        "oily" => "Paint load · lean to loaded",
        "neoBrush" => "Softness · focused to feathered",
        "charcoal" => "Charcoal · compressed to dusty",
        "fountain" => "Nib flex · fine to pressure-responsive",
        "latinCalligraphy" => "Nib angle · 25° to 65°",
        "asiaCalligraphy" => "Nib angle · 40° to 80°",
        "square" => "Chisel width · narrow to broad",
        _ => "Material response",
    }
}

/// `Math.min(float,float)` / `Math.max(float,float)` with Java NaN rules.
fn fmin(a: f32, b: f32) -> f32 {
    jmath::min(a as f64, b as f64) as f32
}
fn fmax(a: f32, b: f32) -> f32 {
    jmath::max(a as f64, b as f64) as f32
}

/// `((Number) value).floatValue()`.
fn float_value(value: Option<&Value>) -> AppResult<f32> {
    match value {
        Some(Value::Number(n)) if n.as_i64().is_some() => Ok(n.as_i64().unwrap() as f32),
        other => Ok(json::as_double(other)? as f32),
    }
}

fn npe() -> AppError {
    AppError::Core(notemaker_core::Error::null_pointer())
}

// ---------------------------------------------------------------------------
// Thin JNI helpers for the Android calls the Java code made inline.

fn new_paint() -> AppResult<Obj> {
    Ok(jnew!("android/graphics/Paint", "(I)V", PAINT_ANTI_ALIAS_FLAG)?)
}
fn set_color(p: &Obj, color: i32) -> AppResult<()> {
    jcall!(p, "android/graphics/Paint", "setColor", "(I)V", color)?;
    Ok(())
}
fn set_style(p: &Obj, style: Style) -> AppResult<()> {
    let s = android_canvas::style_obj(style)?;
    jcall!(p, "android/graphics/Paint", "setStyle", "(Landroid/graphics/Paint$Style;)V", &s)?;
    Ok(())
}
fn set_stroke_width(p: &Obj, width: f32) -> AppResult<()> {
    jcall!(p, "android/graphics/Paint", "setStrokeWidth", "(F)V", width)?;
    Ok(())
}
fn set_cap(p: &Obj, cap: Cap) -> AppResult<()> {
    let c = android_canvas::cap_obj(cap)?;
    jcall!(p, "android/graphics/Paint", "setStrokeCap", "(Landroid/graphics/Paint$Cap;)V", &c)?;
    Ok(())
}
fn set_join(p: &Obj, join: Join) -> AppResult<()> {
    let j = android_canvas::join_obj(join)?;
    jcall!(p, "android/graphics/Paint", "setStrokeJoin", "(Landroid/graphics/Paint$Join;)V", &j)?;
    Ok(())
}
fn canvas_save(c: &Obj) -> AppResult<()> {
    jcall!(c, "android/graphics/Canvas", "save", "()I")?;
    Ok(())
}
fn canvas_restore(c: &Obj) -> AppResult<()> {
    jcall!(c, "android/graphics/Canvas", "restore", "()V")?;
    Ok(())
}
fn translate(c: &Obj, dx: f32, dy: f32) -> AppResult<()> {
    jcall!(c, "android/graphics/Canvas", "translate", "(FF)V", dx, dy)?;
    Ok(())
}
fn scale(c: &Obj, sx: f32, sy: f32) -> AppResult<()> {
    jcall!(c, "android/graphics/Canvas", "scale", "(FF)V", sx, sy)?;
    Ok(())
}
fn clip_rect(c: &Obj, l: f32, t: f32, r: f32, b: f32) -> AppResult<()> {
    jcall!(c, "android/graphics/Canvas", "clipRect", "(FFFF)Z", l, t, r, b)?;
    Ok(())
}
fn draw_circle(c: &Obj, x: f32, y: f32, r: f32, p: &Obj) -> AppResult<()> {
    jcall!(c, "android/graphics/Canvas", "drawCircle", "(FFFLandroid/graphics/Paint;)V", x, y, r, p)?;
    Ok(())
}
fn draw_rect(c: &Obj, l: f32, t: f32, r: f32, b: f32, p: &Obj) -> AppResult<()> {
    jcall!(c, "android/graphics/Canvas", "drawRect", "(FFFFLandroid/graphics/Paint;)V", l, t, r, b, p)?;
    Ok(())
}
fn draw_oval(c: &Obj, l: f32, t: f32, r: f32, b: f32, p: &Obj) -> AppResult<()> {
    jcall!(c, "android/graphics/Canvas", "drawOval", "(FFFFLandroid/graphics/Paint;)V", l, t, r, b, p)?;
    Ok(())
}
fn draw_line(c: &Obj, x0: f32, y0: f32, x1: f32, y1: f32, p: &Obj) -> AppResult<()> {
    jcall!(c, "android/graphics/Canvas", "drawLine", "(FFFFLandroid/graphics/Paint;)V", x0, y0, x1, y1, p)?;
    Ok(())
}
#[allow(clippy::too_many_arguments)]
fn draw_round_rect(c: &Obj, l: f32, t: f32, r: f32, b: f32, rx: f32, ry: f32, p: &Obj) -> AppResult<()> {
    jcall!(c, "android/graphics/Canvas", "drawRoundRect", "(FFFFFFLandroid/graphics/Paint;)V", l, t, r, b, rx, ry, p)?;
    Ok(())
}
/// `canvas.drawArc(new RectF(l, t, r, b), start, sweep, useCenter, paint)`.
#[allow(clippy::too_many_arguments)]
fn draw_arc(c: &Obj, l: f32, t: f32, r: f32, b: f32, start: f32, sweep: f32, use_center: bool, p: &Obj) -> AppResult<()> {
    let oval = jnew!("android/graphics/RectF", "(FFFF)V", l, t, r, b)?;
    jcall!(c, "android/graphics/Canvas", "drawArc", "(Landroid/graphics/RectF;FFZLandroid/graphics/Paint;)V", &oval, start, sweep, use_center, p)?;
    Ok(())
}
fn draw_text(c: &Obj, text: &str, x: f32, y: f32, p: &Obj) -> AppResult<()> {
    jcall!(c, "android/graphics/Canvas", "drawText", "(Ljava/lang/String;FFLandroid/graphics/Paint;)V", text, x, y, p)?;
    Ok(())
}
fn draw_path(c: &Obj, path: &Obj, p: &Obj) -> AppResult<()> {
    jcall!(c, "android/graphics/Canvas", "drawPath", "(Landroid/graphics/Path;Landroid/graphics/Paint;)V", path, p)?;
    Ok(())
}
fn new_path() -> AppResult<Obj> {
    Ok(jnew!("android/graphics/Path", "()V")?)
}
fn move_to(path: &Obj, x: f32, y: f32) -> AppResult<()> {
    jcall!(path, "android/graphics/Path", "moveTo", "(FF)V", x, y)?;
    Ok(())
}
fn line_to(path: &Obj, x: f32, y: f32) -> AppResult<()> {
    jcall!(path, "android/graphics/Path", "lineTo", "(FF)V", x, y)?;
    Ok(())
}
fn quad_to(path: &Obj, x1: f32, y1: f32, x2: f32, y2: f32) -> AppResult<()> {
    jcall!(path, "android/graphics/Path", "quadTo", "(FFFF)V", x1, y1, x2, y2)?;
    Ok(())
}
#[allow(clippy::too_many_arguments)]
fn cubic_to(path: &Obj, x1: f32, y1: f32, x2: f32, y2: f32, x3: f32, y3: f32) -> AppResult<()> {
    jcall!(path, "android/graphics/Path", "cubicTo", "(FFFFFF)V", x1, y1, x2, y2, x3, y3)?;
    Ok(())
}
fn close_path(path: &Obj) -> AppResult<()> {
    jcall!(path, "android/graphics/Path", "close", "()V")?;
    Ok(())
}
fn super_dispatch_draw_linear(view: &Obj, canvas: &Obj) -> AppResult<()> {
    jcall!(view, "com/notemaker/NativeLinearLayout", "superDispatchDraw", "(Landroid/graphics/Canvas;)V", canvas)?;
    Ok(())
}
fn super_dispatch_draw_frame(view: &Obj, canvas: &Obj) -> AppResult<()> {
    jcall!(view, "com/notemaker/NativeFrameLayout", "superDispatchDraw", "(Landroid/graphics/Canvas;)V", canvas)?;
    Ok(())
}
fn super_on_draw(view: &Obj, canvas: &Obj) -> AppResult<()> {
    jcall!(view, "com/notemaker/NativeView", "superOnDraw", "(Landroid/graphics/Canvas;)V", canvas)?;
    Ok(())
}

fn linear_set_gravity(v: &Obj, gravity: crate::android_consts::Gravity) -> AppResult<()> {
    jcall!(v, "android/widget/LinearLayout", "setGravity", "(I)V", gravity)?;
    Ok(())
}
fn text_set_gravity(v: &Obj, gravity: crate::android_consts::Gravity) -> AppResult<()> {
    jcall!(v, "android/widget/TextView", "setGravity", "(I)V", gravity)?;
    Ok(())
}
fn set_single_line(v: &Obj, single: bool) -> AppResult<()> {
    jcall!(v, "android/widget/TextView", "setSingleLine", "(Z)V", single)?;
    Ok(())
}
fn set_ellipsize_end(v: &Obj) -> AppResult<()> {
    let end = jsget!("android/text/TextUtils$TruncateAt", "END", "Landroid/text/TextUtils$TruncateAt;")?.some();
    jcall!(v, "android/widget/TextView", "setEllipsize", "(Landroid/text/TextUtils$TruncateAt;)V", &end)?;
    Ok(())
}
fn set_text_color(v: &Obj, color: i32) -> AppResult<()> {
    jcall!(v, "android/widget/TextView", "setTextColor", "(I)V", color)?;
    Ok(())
}
fn set_tooltip(v: &Obj, text: &str) -> AppResult<()> {
    let s = jx::jstr(text);
    jcall!(v, "android/view/View", "setTooltipText", "(Ljava/lang/CharSequence;)V", &s)?;
    Ok(())
}
fn set_tag_int(v: &Obj, tag: i32) -> AppResult<()> {
    let boxed = jx::boxed_int(tag);
    jcall!(v, "android/view/View", "setTag", "(Ljava/lang/Object;)V", &boxed)?;
    Ok(())
}
fn set_enabled(v: &Obj, enabled: bool) -> AppResult<()> {
    jcall!(v, "android/view/View", "setEnabled", "(Z)V", enabled)?;
    Ok(())
}
fn is_enabled(v: &Obj) -> AppResult<bool> {
    Ok(jcall!(v, "android/view/View", "isEnabled", "()Z")?.bool())
}
fn set_alpha(v: &Obj, alpha: f32) -> AppResult<()> {
    jcall!(v, "android/view/View", "setAlpha", "(F)V", alpha)?;
    Ok(())
}
fn set_background(v: &Obj, drawable: &Obj) -> AppResult<()> {
    jcall!(v, "android/view/View", "setBackground", "(Landroid/graphics/drawable/Drawable;)V", drawable)?;
    Ok(())
}
fn color_state_list(color: i32) -> AppResult<Obj> {
    Ok(jstatic!("android/content/res/ColorStateList", "valueOf", "(I)Landroid/content/res/ColorStateList;", color)?.some())
}
fn scroll_view(context: &Obj) -> AppResult<Obj> {
    Ok(jnew!("android/widget/ScrollView", "(Landroid/content/Context;)V", context)?)
}
fn set_fill_viewport(v: &Obj, fill: bool) -> AppResult<()> {
    jcall!(v, "android/widget/ScrollView", "setFillViewport", "(Z)V", fill)?;
    Ok(())
}
fn set_vertical_scroll_bar_enabled(v: &Obj, enabled: bool) -> AppResult<()> {
    jcall!(v, "android/view/View", "setVerticalScrollBarEnabled", "(Z)V", enabled)?;
    Ok(())
}
fn child_count(v: &Obj) -> AppResult<i32> {
    Ok(jcall!(v, "android/view/ViewGroup", "getChildCount", "()I")?.int())
}
fn child_at(v: &Obj, index: i32) -> AppResult<Option<Obj>> {
    Ok(jcall!(v, "android/view/ViewGroup", "getChildAt", "(I)Landroid/view/View;", index)?.global())
}
fn check_box(context: &Obj, label: &str) -> AppResult<Obj> {
    let b = jnew!("android/widget/CheckBox", "(Landroid/content/Context;)V", context)?;
    ui::set_text(&b, &ui::UiText::from(label))?;
    Ok(b)
}
fn set_checked(b: &Obj, checked: bool) -> AppResult<()> {
    jcall!(b, "android/widget/CompoundButton", "setChecked", "(Z)V", checked)?;
    Ok(())
}
fn set_button_tint(b: &Obj, color: i32) -> AppResult<()> {
    let tint = color_state_list(color)?;
    jcall!(b, "android/widget/CompoundButton", "setButtonTintList", "(Landroid/content/res/ColorStateList;)V", &tint)?;
    Ok(())
}
/// `button.setOnCheckedChangeListener((button, checked) -> f(checked))`.
fn on_checked(b: &Obj, f: impl Fn(bool) -> AppResult<()> + Send + Sync + 'static) -> AppResult<()> {
    let l = jx::callback(move |method, args| {
        if method == "onCheckedChanged(CompoundButton,boolean)" {
            f(jx::unbox_bool(&args[1])).map_err(JavaError::from)?;
        }
        Ok(Reply::Void)
    });
    jcall!(b, "android/widget/CompoundButton", "setOnCheckedChangeListener", "(Landroid/widget/CompoundButton$OnCheckedChangeListener;)V", &l)?;
    Ok(())
}
fn seek_bar(context: &Obj) -> AppResult<Obj> {
    Ok(jnew!("android/widget/SeekBar", "(Landroid/content/Context;)V", context)?)
}
fn set_max(bar: &Obj, max: i32) -> AppResult<()> {
    jcall!(bar, "android/widget/ProgressBar", "setMax", "(I)V", max)?;
    Ok(())
}
fn set_progress(bar: &Obj, progress: i32) -> AppResult<()> {
    jcall!(bar, "android/widget/ProgressBar", "setProgress", "(I)V", progress)?;
    Ok(())
}
fn set_progress_tint(bar: &Obj, color: i32) -> AppResult<()> {
    let tint = color_state_list(color)?;
    jcall!(bar, "android/widget/ProgressBar", "setProgressTintList", "(Landroid/content/res/ColorStateList;)V", &tint)?;
    Ok(())
}
fn set_progress_background_tint(bar: &Obj, color: i32) -> AppResult<()> {
    let tint = color_state_list(color)?;
    jcall!(bar, "android/widget/ProgressBar", "setProgressBackgroundTintList", "(Landroid/content/res/ColorStateList;)V", &tint)?;
    Ok(())
}
fn set_thumb_tint(bar: &Obj, color: i32) -> AppResult<()> {
    let tint = color_state_list(color)?;
    jcall!(bar, "android/widget/AbsSeekBar", "setThumbTintList", "(Landroid/content/res/ColorStateList;)V", &tint)?;
    Ok(())
}
/// `bar.setOnSeekBarChangeListener(...)` whose `onProgressChanged(bar, value, user)` runs `f`.
fn on_progress(bar: &Obj, f: impl Fn(i32, bool) -> AppResult<()> + Send + Sync + 'static) -> AppResult<()> {
    let l = jx::callback(move |method, args| {
        if method == "onProgressChanged(SeekBar,int,boolean)" {
            f(jx::unbox_int(&args[1]), jx::unbox_bool(&args[2])).map_err(JavaError::from)?;
        }
        Ok(Reply::Void)
    });
    jcall!(bar, "android/widget/SeekBar", "setOnSeekBarChangeListener", "(Landroid/widget/SeekBar$OnSeekBarChangeListener;)V", &l)?;
    Ok(())
}
fn edit_text(context: &Obj) -> AppResult<Obj> {
    Ok(jnew!("android/widget/EditText", "(Landroid/content/Context;)V", context)?)
}
fn set_hint(v: &Obj, hint: &str) -> AppResult<()> {
    let s = jx::jstr(hint);
    jcall!(v, "android/widget/TextView", "setHint", "(Ljava/lang/CharSequence;)V", &s)?;
    Ok(())
}
fn set_input_type(v: &Obj, kind: i32) -> AppResult<()> {
    jcall!(v, "android/widget/TextView", "setInputType", "(I)V", kind)?;
    Ok(())
}
/// `view.setOnClickListener(v -> f(v))`, failures thrown into Java.
fn on_click(view: &Obj, f: impl Fn(&Local) -> AppResult<()> + Send + Sync + 'static) -> AppResult<()> {
    let l = jx::callback(move |_, args| f(&args[0]).map(|_| Reply::Void).map_err(JavaError::from));
    jcall!(view, "android/view/View", "setOnClickListener", "(Landroid/view/View$OnClickListener;)V", &l)?;
    Ok(())
}
/// A `DialogInterface.OnClickListener` running `f(which)`.
fn dialog_listener(f: impl Fn(i32) -> AppResult<()> + Send + Sync + 'static) -> Obj {
    jx::callback(move |_, args| {
        let which = args.get(1).map(|a| jx::unbox_int(a)).unwrap_or(0);
        f(which).map(|_| Reply::Void).map_err(JavaError::from)
    })
}
fn builder(context: &Obj) -> AppResult<Obj> {
    Ok(jnew!("android/app/AlertDialog$Builder", "(Landroid/content/Context;)V", context)?)
}
fn builder_title(b: &Obj, title: &str) -> AppResult<()> {
    let s = jx::jstr(title);
    jcall!(b, "android/app/AlertDialog$Builder", "setTitle", "(Ljava/lang/CharSequence;)Landroid/app/AlertDialog$Builder;", &s)?;
    Ok(())
}
fn builder_view(b: &Obj, view: &Obj) -> AppResult<()> {
    jcall!(b, "android/app/AlertDialog$Builder", "setView", "(Landroid/view/View;)Landroid/app/AlertDialog$Builder;", view)?;
    Ok(())
}
fn builder_message(b: &Obj, message: &str) -> AppResult<()> {
    let s = jx::jstr(message);
    jcall!(b, "android/app/AlertDialog$Builder", "setMessage", "(Ljava/lang/CharSequence;)Landroid/app/AlertDialog$Builder;", &s)?;
    Ok(())
}
fn builder_negative(b: &Obj, label: &str, listener: Option<&Obj>) -> AppResult<()> {
    let s = jx::jstr(label);
    jcall!(b, "android/app/AlertDialog$Builder", "setNegativeButton", "(Ljava/lang/CharSequence;Landroid/content/DialogInterface$OnClickListener;)Landroid/app/AlertDialog$Builder;", &s, listener)?;
    Ok(())
}
fn builder_positive(b: &Obj, label: &str, listener: &Obj) -> AppResult<()> {
    let s = jx::jstr(label);
    jcall!(b, "android/app/AlertDialog$Builder", "setPositiveButton", "(Ljava/lang/CharSequence;Landroid/content/DialogInterface$OnClickListener;)Landroid/app/AlertDialog$Builder;", &s, listener)?;
    Ok(())
}
fn builder_show(b: &Obj) -> AppResult<()> {
    jcall!(b, "android/app/AlertDialog$Builder", "show", "()Landroid/app/AlertDialog;")?;
    Ok(())
}
/// Java `object instanceof class` for framework classes.
fn is_instance(obj: &Obj, class: &str) -> bool {
    jx::env().is_instance_of(obj.as_obj(), class).unwrap_or(false)
}
/// Keeps `view` alive until the current native call returns (a JNI local
/// reference), for constructors that only return the view weakly.
fn pin_local(view: &Obj) {
    // `JObject` does not delete its reference on drop: the local stays in
    // the current JNI frame until the native callback returns.
    let _ = jx::env().new_local_ref(view.as_obj());
}
fn view_from(local: &Local) -> Option<Obj> {
    jx::global(local)
}

// ---------------------------------------------------------------------------

/// `final class EditorUi`.
pub struct EditorUi {
    pub app: Weak<MainActivity>,
    pub tools: Field<IndexMap<String, Arc<Tool>>>,
    pub previews: Field<Vec<Arc<LayerPreview>>>,
    pub root: Field<Option<Obj>>,
    pub layer_list: Field<Option<Obj>>,
    pub title: Field<Option<Obj>>,
    pub subtitle: Field<Option<Obj>>,
    pub connection: Field<Option<Obj>>,
    pub workspace: Field<Option<Obj>>,
    pub popup: Field<Option<Obj>>,
    pub popup_anchor: Field<Option<Arc<Tool>>>,
    pub undo_button: Field<Option<Arc<Icon>>>,
    pub redo_button: Field<Option<Arc<Icon>>>,
    pub sample: Field<Option<Arc<StrokeSample>>>,
    pub material_label: Field<Option<Obj>>,
    pub material_panel: Field<Option<Obj>>,
    pub show_tool_labels: Field<bool>,
    pub selected_tool: Field<String>,
    pub layer_state: Field<String>,
    pub thumbnail_time: Field<i64>,
    pub thumbnail_revision: Field<i32>,
}

impl EditorUi {
    /// `EditorUi(MainActivity app)`: reads `showToolLabels` from preferences.
    pub fn new(app: &Arc<MainActivity>) -> AppResult<Arc<EditorUi>> {
        let show = ui::pref_bool(&app.this, &ui::PrefKey::from("showToolLabels"), ui::PrefSwitch::from(true))?;
        Ok(Arc::new(EditorUi {
            app: Arc::downgrade(app),
            tools: Field::new(IndexMap::new()),
            previews: Field::new(Vec::new()),
            root: Field::new(None),
            layer_list: Field::new(None),
            title: Field::new(None),
            subtitle: Field::new(None),
            connection: Field::new(None),
            workspace: Field::new(None),
            popup: Field::new(None),
            popup_anchor: Field::new(None),
            undo_button: Field::new(None),
            redo_button: Field::new(None),
            sample: Field::new(None),
            material_label: Field::new(None),
            material_panel: Field::new(None),
            show_tool_labels: Field::new(show),
            selected_tool: Field::new("pen".into()),
            layer_state: Field::new(String::new()),
            thumbnail_time: Field::new(0),
            thumbnail_revision: Field::new(-1),
        }))
    }
    pub fn app(&self) -> Arc<MainActivity> {
        self.app.upgrade().expect("MainActivity dropped")
    }
    /// The activity as a `Context`.
    pub fn context(&self) -> Obj {
        self.app().this.clone()
    }

    /// A callback capturing this editor weakly (Java lambdas captured `this`).
    fn act(self: &Arc<Self>, f: impl Fn(&Arc<EditorUi>, &Arc<MainActivity>) -> AppResult<()> + Send + Sync + 'static) -> Action {
        let w = Arc::downgrade(self);
        Arc::new(move || match w.upgrade() {
            Some(ui) => {
                let app = ui.app();
                f(&ui, &app)
            }
            None => Ok(()),
        })
    }
    /// A `BooleanSupplier` capturing this editor weakly.
    fn supplier(self: &Arc<Self>, f: impl Fn(&Arc<MainActivity>) -> bool + Send + Sync + 'static) -> BooleanSupplier {
        let w = Arc::downgrade(self);
        Arc::new(move || match w.upgrade().and_then(|ui| ui.app.upgrade()) {
            Some(app) => f(&app),
            None => false,
        })
    }
    /// `tools.get(kind)`.
    fn tool_named(&self, kind: &str) -> Option<Arc<Tool>> {
        self.tools.with(|t| t.get(kind).cloned())
    }
    /// The `Tool` whose view is `anchor` (Java `anchor instanceof Tool`).
    fn tool_for(&self, anchor: &Obj) -> Option<Arc<Tool>> {
        let tools: Vec<Arc<Tool>> = self.tools.with(|t| t.values().cloned().collect());
        tools.into_iter().find(|t| t.view.get().and_then(WeakObj::upgrade).is_some_and(|v| v.same(anchor)))
    }
    /// `showPopup(body, tools.get(kind), false)`; Java threw an NPE for a missing tool.
    fn show_popup_at_tool(self: &Arc<Self>, body: &Obj, kind: &str) -> AppResult<()> {
        let tool = self.tool_named(kind).ok_or_else(npe)?;
        let view = tool.view();
        self.show_popup(body, &view, Some(tool), false)
    }
    fn root_obj(&self) -> AppResult<Obj> {
        self.root.get().ok_or_else(npe)
    }

    /// `int dp(float value)`.
    fn dp(&self, value: f32) -> AppResult<i32> {
        Ok(crate::ui::dp(&self.context(), crate::ui::Dp(value))?.0)
    }
    /// `View build()`: creates the editor tree and `app.canvas`.
    pub fn build(self: &Arc<Self>) -> AppResult<Obj> {
        let app = self.app();
        let ctx = self.context();
        let root = self.column()?;
        self.root.set(Some(root.clone()));
        ui::set_background_color(&root, notemaker_core::style::ArgbColor(COLOR_WHITE))?;
        let insets_listener = jx::callback(|_, args| {
            let view = &args[0];
            let insets = &args[1];
            let l = jcall!(insets, "android/view/WindowInsets", "getSystemWindowInsetLeft", "()I")?.int();
            let t = jcall!(insets, "android/view/WindowInsets", "getSystemWindowInsetTop", "()I")?.int();
            let r = jcall!(insets, "android/view/WindowInsets", "getSystemWindowInsetRight", "()I")?.int();
            let b = jcall!(insets, "android/view/WindowInsets", "getSystemWindowInsetBottom", "()I")?.int();
            jcall!(view, "android/view/View", "setPadding", "(IIII)V", l, t, r, b)?;
            Ok(Reply::Obj(jx::global(insets)))
        });
        jcall!(root, "android/view/View", "setOnApplyWindowInsetsListener", "(Landroid/view/View$OnApplyWindowInsetsListener;)V", &insets_listener)?;

        let header = self.row()?;
        linear_set_gravity(&header, GRAVITY_CENTER_VERTICAL)?;
        ui::set_padding(&header, ui::PxInsets { left: 0, top: 0, right: self.dp(8.0)?, bottom: 0 })?;
        self.top_tool(&header, "document", "Notes", "Document actions")?;
        let heading = self.column()?;
        ui::set_padding(&heading, ui::PxInsets { left: self.dp(12.0)?, top: 0, right: self.dp(8.0)?, bottom: 0 })?;
        let title = self.text("", 18, INK, false)?;
        set_single_line(&title, true)?;
        set_ellipsize_end(&title)?;
        self.title.set(Some(title.clone()));
        let subtitle = self.text("", 11, MUTED, false)?;
        self.subtitle.set(Some(subtitle.clone()));
        ui::add_view(&heading, &title)?;
        ui::add_view(&heading, &subtitle)?;
        ui::add_view_params(&header, &heading, &ui::linear_params_weight(ui::LayoutSize { width: 0, height: -2 }, ui::LayoutWeight(1.0))?)?;
        let connection = self.text("", 10, MUTED, false)?;
        set_single_line(&connection, true)?;
        set_ellipsize_end(&connection)?;
        self.connection.set(Some(connection.clone()));
        let w = Arc::downgrade(self);
        on_click(&connection, move |_| {
            if let Some(ui) = w.upgrade() {
                if !ui.app().conflicts.with(|c| c.is_empty()) {
                    ui.show_conflict_panel()?;
                }
            }
            Ok(())
        })?;
        ui::add_view(&heading, &connection)?;
        self.top_tool(&header, "pen", "Pen", "Pen tools")?;
        self.top_tool(&header, "eraser", "Erase", "Eraser tools")?;
        let top_rule = self.divider()?;
        let rule_params = ui::linear_params(ui::LayoutSize { width: self.dp(1.0)?, height: self.dp(32.0)? })?;
        ui::set_margins(&rule_params, ui::PxInsets { left: self.dp(6.0)?, top: 0, right: self.dp(6.0)?, bottom: 0 })?;
        ui::add_view_params(&header, &top_rule, &rule_params)?;
        let undo = self.icon(
            "undo",
            "Undo",
            self.act(|ui, app| {
                ui.dismiss_popup()?;
                app.history(notemaker_core::store::HistoryDirection::Undo)
            }),
        )?;
        let redo = self.icon(
            "redo",
            "Redo",
            self.act(|ui, app| {
                ui.dismiss_popup()?;
                app.history(notemaker_core::store::HistoryDirection::Redo)
            }),
        )?;
        self.undo_button.set(Some(undo.clone()));
        self.redo_button.set(Some(redo.clone()));
        ui::add_view_params(&header, &undo.view(), &ui::linear_params(ui::LayoutSize { width: self.dp(48.0)?, height: self.dp(56.0)? })?)?;
        ui::add_view_params(&header, &redo.view(), &ui::linear_params(ui::LayoutSize { width: self.dp(48.0)?, height: self.dp(56.0)? })?)?;
        ui::add_view_params(&root, &header, &ui::linear_params(ui::LayoutSize { width: -1, height: self.dp(72.0)? })?)?;
        ui::add_view_params(&root, &self.divider()?, &ui::linear_params(ui::LayoutSize { width: -1, height: self.dp(1.0)? })?)?;

        let body = self.row()?;
        ui::add_view_params(&root, &body, &ui::linear_params_weight(ui::LayoutSize { width: -1, height: 0 }, ui::LayoutWeight(1.0))?)?;
        let rail = self.column()?;
        linear_set_gravity(&rail, GRAVITY_CENTER_HORIZONTAL)?;
        ui::set_padding(&rail, ui::PxInsets { left: 0, top: self.dp(8.0)?, right: 0, bottom: self.dp(8.0)? })?;
        ui::set_background_color(&rail, notemaker_core::style::ArgbColor(PAPER))?;
        let rail_scroll = scroll_view(&ctx)?;
        set_fill_viewport(&rail_scroll, true)?;
        set_vertical_scroll_bar_enabled(&rail_scroll, false)?;
        ui::add_view(&rail_scroll, &rail)?;
        ui::add_view_params(&body, &rail_scroll, &ui::linear_params(ui::LayoutSize { width: self.dp(56.0)?, height: -1 })?)?;
        self.tool(&rail, "template", "Template", "Page templates")?;
        self.tool(&rail, "layers", "Layers", "Layer actions")?;
        self.tool(&rail, "shapes", "Shapes", "Shape tools")?;
        self.tool(&rail, "fill", "Fill", "Enclosed area fill")?;
        self.rail_rule(&rail)?;
        self.tool(&rail, "select", "Select", "Select and transform objects")?;
        self.tool(&rail, "interact", "Interact", "Press controls on the page")?;
        self.tool(&rail, "text", "Text", "Text placement")?;
        self.tool(&rail, "insert", "Insert", "Insert content")?;
        self.tool(&rail, "move", "Move", "Canvas navigation")?;
        self.rail_rule(&rail)?;
        self.tool(&rail, "page", "Pages", "Page actions")?;
        ui::add_view_params(&rail, &ui::view(&ctx)?, &ui::linear_params_weight(ui::LayoutSize { width: 1, height: 0 }, ui::LayoutWeight(1.0))?)?;
        self.tool(&rail, "agent", "Agent", "Open Agent")?;
        self.tool(&rail, "more", "More", "More tools")?;
        ui::add_view_params(&body, &self.divider()?, &ui::linear_params(ui::LayoutSize { width: self.dp(1.0)?, height: -1 })?)?;

        let workspace = ui::frame_layout(&ctx)?;
        self.workspace.set(Some(workspace.clone()));
        ui::add_view_params(&body, &workspace, &ui::linear_params_weight(ui::LayoutSize { width: 0, height: -1 }, ui::LayoutWeight(1.0))?)?;
        let canvas = CanvasView::new(&app, &ctx)?;
        app.canvas.set(Some(canvas.clone()));
        let canvas_view = canvas.view();
        ui::set_content_description(&canvas_view, &ui::UiText::from("Drawing canvas"))?;
        ui::add_view_params(&workspace, &canvas_view, &ui::frame_params(ui::LayoutSize { width: -1, height: -1 })?)?;
        Ok(root)
    }
    /// `private void topTool(LinearLayout header, String kind, String label, String description)`.
    fn top_tool(self: &Arc<Self>, header: &Obj, kind: &str, label: &str, description: &str) -> AppResult<()> {
        let (tool, view) = Tool::create(self, kind, label, description)?;
        tool.top.set(true);
        self.tools.with_mut(|t| t.insert(kind.to_string(), tool.clone()));
        ui::add_view_params(header, &view, &ui::linear_params(ui::LayoutSize { width: self.dp(54.0)?, height: -1 })?)?;
        let w = Arc::downgrade(self);
        let kind = ToolKind::from(kind);
        let anchor = tool.clone();
        on_click(&view, move |_| match w.upgrade() {
            Some(ui) => ui.open_tool(&kind, Some(&anchor.view())),
            None => Ok(()),
        })
    }
    /// `private void railRule(LinearLayout rail)`.
    pub fn rail_rule(&self, rail: &Obj) -> AppResult<()> {
        let params = ui::linear_params(ui::LayoutSize { width: self.dp(24.0)?, height: self.dp(1.0)? })?;
        ui::set_margins(&params, ui::PxInsets { left: 0, top: self.dp(7.0)?, right: 0, bottom: self.dp(7.0)? })?;
        ui::add_view_params(rail, &self.divider()?, &params)?;
        Ok(())
    }
    /// `private LinearLayout column()`.
    pub fn column(&self) -> AppResult<Obj> {
        Ok(crate::ui::column(&self.context())?)
    }
    /// `private LinearLayout row()`.
    pub fn row(&self) -> AppResult<Obj> {
        Ok(crate::ui::row(&self.context())?)
    }
    /// `private View divider()`.
    pub fn divider(&self) -> AppResult<Obj> {
        let v = crate::ui::view(&self.context())?;
        crate::ui::set_background_color(&v, notemaker_core::style::ArgbColor(LINE))?;
        Ok(v)
    }
    /// `private TextView text(String value, int size, int color, boolean bold)`.
    fn text(&self, value: &str, size: i32, color: i32, bold: bool) -> AppResult<Obj> {
        Ok(crate::ui::text(&self.context(), &crate::ui::UiText::from(value), crate::ui::Sp(size as f32), notemaker_core::style::ArgbColor(color), if bold { crate::ui::TextWeight::Bold } else { crate::ui::TextWeight::Normal })?)
    }
    /// `private GradientDrawable surface(int fill, int border, int radius, int stroke)`.
    fn surface(&self, fill: i32, border: i32, radius: i32, stroke: i32) -> AppResult<Obj> {
        Ok(crate::ui::surface(&self.context(), crate::ui::SurfaceStyle { fill: notemaker_core::style::ArgbColor(fill), border: notemaker_core::style::ArgbColor(border), radius: crate::ui::Dp(radius as f32), stroke: crate::ui::Dp(stroke as f32) })?)
    }
    /// `private TextView action(String label, Runnable callback)`.
    fn action(self: &Arc<Self>, label: &str, callback: Action) -> AppResult<Obj> {
        let button = self.text(label, 15, INK, false)?;
        text_set_gravity(&button, GRAVITY_CENTER_VERTICAL)?;
        ui::set_padding(&button, ui::PxInsets { left: self.dp(14.0)?, top: self.dp(12.0)?, right: self.dp(14.0)?, bottom: self.dp(12.0)? })?;
        jcall!(button, "android/widget/TextView", "setMinHeight", "(I)V", self.dp(48.0)?)?;
        ui::set_background_color(&button, notemaker_core::style::ArgbColor(PAPER))?;
        ui::set_content_description(&button, &ui::UiText::from(label))?;
        on_click(&button, move |_| callback())?;
        Ok(button)
    }
    /// `private void menuAction(LinearLayout parent, String label, boolean enabled, Runnable callback)`.
    fn menu_action(self: &Arc<Self>, parent: &Obj, label: &str, enabled: bool, callback: Action) -> AppResult<()> {
        ui::add_view_params(parent, &self.divider()?, &ui::linear_params(ui::LayoutSize { width: -1, height: self.dp(1.0)? })?)?;
        let button = self.row()?;
        linear_set_gravity(&button, GRAVITY_CENTER_VERTICAL)?;
        jcall!(button, "android/view/View", "setMinimumHeight", "(I)V", self.dp(52.0)?)?;
        ui::set_content_description(&button, &ui::UiText::from(label))?;
        set_enabled(&button, enabled)?;
        set_alpha(&button, if enabled { 1.0 } else { 0.4 })?;
        let icon_kind = self.action_icon(label);
        let (glyph, glyph_view) = Icon::create(self, &icon_kind)?;
        if matches!(
            icon_kind.as_str(),
            "pen" | "fountain" | "pencil" | "marker" | "brush" | "oily" | "neoBrush" | "charcoal" | "latinCalligraphy" | "asiaCalligraphy" | "square"
        ) {
            glyph.accent_color.set(self.app().color.get());
        }
        ui::add_view_params(&button, &glyph_view, &ui::linear_params(ui::LayoutSize { width: self.dp(38.0)?, height: self.dp(38.0)? })?)?;
        let name = self.text(label, 15, INK, false)?;
        ui::set_padding(&name, ui::PxInsets { left: self.dp(10.0)?, top: self.dp(12.0)?, right: self.dp(4.0)?, bottom: self.dp(12.0)? })?;
        ui::add_view_params(&button, &name, &ui::linear_params_weight(ui::LayoutSize { width: 0, height: -2 }, ui::LayoutWeight(1.0))?)?;
        let w = Arc::downgrade(self);
        on_click(&button, move |_| {
            if let Some(ui) = w.upgrade() {
                ui.dismiss_popup()?;
            }
            callback()
        })?;
        ui::add_view_params(parent, &button, &ui::linear_params(ui::LayoutSize { width: -1, height: -2 })?)?;
        Ok(())
    }
    /// `private String actionIcon(String label)`.
    fn action_icon(&self, label: &str) -> String {
        action_icon_for(label).to_string()
    }
    /// `private void tool(LinearLayout rail, String icon, String label, String description)`.
    fn tool(self: &Arc<Self>, rail: &Obj, icon: &str, label: &str, description: &str) -> AppResult<()> {
        let (tool, view) = Tool::create(self, icon, label, description)?;
        self.tools.with_mut(|t| t.insert(icon.to_string(), tool.clone()));
        let params = ui::linear_params(ui::LayoutSize { width: -1, height: self.dp(66.0)? })?;
        ui::add_view_params(rail, &view, &params)?;
        let w = Arc::downgrade(self);
        let kind = ToolKind::from(icon);
        let anchor = tool.clone();
        on_click(&view, move |_| match w.upgrade() {
            Some(ui) => ui.open_tool(&kind, Some(&anchor.view())),
            None => Ok(()),
        })
    }
    /// `private void setText(TextView view, String value)`: only when changed.
    fn set_text(&self, view: &Obj, value: &str) -> AppResult<()> {
        if String::from(ui::text_of(view)?) != value {
            ui::set_text(view, &ui::UiText::from(value))?;
        }
        Ok(())
    }
    /// `void refresh()`.
    pub fn refresh(self: &Arc<Self>) -> AppResult<()> {
        let app = self.app();
        let model = app.model();
        let title = self.title.get().ok_or_else(npe)?;
        let subtitle = self.subtitle.get().ok_or_else(npe)?;
        let connection = self.connection.get().ok_or_else(npe)?;
        self.set_text(&title, &format!("{}{}", app.document.get(), if model.deleted { " · Deleted" } else { "" }))?;
        let type_ = app.type_.get();
        let sub = if type_ == "pages" {
            format!("NOTEBOOK  /  PAGE {}", app.page.get() + 1)
        } else if type_ == "infinite" {
            "INFINITE CANVAS".to_string()
        } else {
            "SINGLE PAGE".to_string()
        };
        self.set_text(&subtitle, &sub)?;
        let status = if app.save_error.with(|e| e.is_some()) {
            "Save failed"
        } else if app.saving.get() > 0 {
            if app.replica().remote() {
                "Saving to proxy…"
            } else {
                "Saving on device…"
            }
        } else if app.connected.get() {
            if app.pending_count.get() == 0 {
                "●  Connected"
            } else {
                "●  Syncing changes"
            }
        } else if app.replica().remote() {
            "○  Proxy unavailable · last snapshot"
        } else {
            "○  Offline · saved on device"
        };
        self.set_text(&connection, status)?;
        let conflicts = app.conflicts.with(|c| c.len());
        if conflicts != 0 {
            let current = String::from(ui::text_of(&connection)?);
            self.set_text(&connection, &format!("{current}  ·  {conflicts} conflict(s)"))?;
        }
        let mode = if app.erase.get() {
            "eraser".to_string()
        } else if app.pan.get() {
            "move".to_string()
        } else if let Some(shape) = app.drawing_shape.get() {
            if shape == "text" { "text".to_string() } else { "shapes".to_string() }
        } else {
            self.selected_tool.get()
        };
        let color = app.color.get();
        let brush = app.brush.get();
        let tools = self.tools.get();
        for (key, tool) in tools.iter() {
            tool.active(*key == mode)?;
            let mut icon_changed = tool.icon.accent_color.get() != color;
            tool.icon.accent_color.set(color);
            if key == "pen" {
                icon_changed |= tool.icon.kind.get() != brush;
                tool.icon.kind.set(brush.clone());
                let description = format!("Pen tool, {brush} brush");
                let view = tool.view();
                let current = jcall!(view, "android/view/View", "getContentDescription", "()Ljava/lang/CharSequence;")?.global();
                let current = current.ok_or_else(npe)?;
                if jx::to_string(current.as_obj()).as_deref() != Some(description.as_str()) {
                    ui::set_content_description(&view, &ui::UiText::from(&description))?;
                }
            }
            if icon_changed {
                ui::invalidate(&tool.icon.view())?;
            }
        }
        if let Some(sample) = self.sample.get() {
            ui::invalidate(&sample.view())?;
        }
        if let Some(label) = self.material_label.get() {
            self.set_text(&label, &self.material_description())?;
        }
        if let Some(panel) = self.material_panel.get() {
            ui::set_visibility(&panel, if brush == "pen" { VIEW_GONE } else { VIEW_VISIBLE })?;
        }
        let ready = !model.deleted && !app.object_history_pending();
        let undo = self.undo_button.get().ok_or_else(npe)?.view();
        let redo = self.redo_button.get().ok_or_else(npe)?.view();
        set_enabled(&undo, ready && !app.undo.with(|u| u.is_empty()))?;
        set_alpha(&undo, if is_enabled(&undo)? { 1.0 } else { 0.3 })?;
        set_enabled(&redo, ready && !app.redo.with(|r| r.is_empty()))?;
        set_alpha(&redo, if is_enabled(&redo)? { 1.0 } else { 0.3 })?;

        let mut state = format!("{}/{}/{}", app.document.get(), app.page.get(), app.selected.get());
        let layers = app.layers.get();
        let model = app.model();
        for (key, layer) in layers.iter() {
            let hidden = app.hidden.with(|h| h.contains(key));
            let active = match model.layer_active.get(key) {
                Some(true) => "true",
                Some(false) => "false",
                None => "null",
            };
            state.push('|');
            state.push_str(key);
            state.push_str(&layer.to_string());
            state.push_str(if hidden { "true" } else { "false" });
            state.push_str(active);
        }
        if state != self.layer_state.get() {
            self.layer_state.set(state);
            self.rebuild_layers()?;
        }
        let revision = app.event_count.get().wrapping_add(app.pending_count.get());
        let now = jx::uptime_millis().0;
        let drawing = app.canvas().points.with(|p| p.is_some());
        if revision != self.thumbnail_revision.get() && (!drawing || now - self.thumbnail_time.get() > 400) {
            self.thumbnail_revision.set(revision);
            self.thumbnail_time.set(now);
            for preview in self.previews.get() {
                ui::invalidate(&preview.view())?;
            }
        }
        Ok(())
    }
    /// `private LinearLayout layersPanel()`.
    pub fn layers_panel(self: &Arc<Self>) -> AppResult<Obj> {
        let box_ = self.popup_body("Layers", Some("Topmost layer first"))?;
        set_tag_int(&box_, 440)?;
        let add = self.action(
            "＋  New layer",
            self.act(|ui, _| {
                ui.dismiss_popup()?;
                ui.new_layer()
            }),
        )?;
        ui::add_view_params(&box_, &add, &ui::linear_params(ui::LayoutSize { width: -1, height: self.dp(52.0)? })?)?;
        ui::add_view_params(&box_, &self.divider()?, &ui::linear_params(ui::LayoutSize { width: -1, height: self.dp(1.0)? })?)?;
        let layer_list = self.column()?;
        self.layer_list.set(Some(layer_list.clone()));
        let scroll = scroll_view(&self.context())?;
        set_fill_viewport(&scroll, false)?;
        ui::add_view(&scroll, &layer_list)?;
        ui::add_view_params(&box_, &scroll, &ui::linear_params(ui::LayoutSize { width: -1, height: self.dp(300.0)? })?)?;
        self.rebuild_layers()?;
        Ok(box_)
    }
    /// `private void rebuildLayers()`.
    pub fn rebuild_layers(self: &Arc<Self>) -> AppResult<()> {
        let Some(layer_list) = self.layer_list.get() else { return Ok(()) };
        let app = self.app();
        let _lock = app.lock.enter();
        jcall!(layer_list, "android/view/ViewGroup", "removeAllViews", "()V")?;
        self.previews.with_mut(|p| p.clear());
        let layers = app.layers.get();
        let model = app.model();
        let mut ids: Vec<String> = layers.keys().cloned().collect();
        ids.retain(|id| model.layer_active.get(id) != Some(&false));
        ids.reverse();
        let count = ids.len();
        for (index, id) in ids.iter().enumerate() {
            let layer = layers.get(id).ok_or_else(npe)?;
            let name = layer.opt_string("name");
            let selected = app.selected.get() == *id;
            let forced = layer.opt_boolean("force");
            let visible = forced || !app.hidden.with(|h| h.contains(id));
            let card = self.row()?;
            linear_set_gravity(&card, GRAVITY_CENTER_VERTICAL)?;
            ui::set_padding(&card, ui::PxInsets { left: self.dp(8.0)?, top: self.dp(10.0)?, right: self.dp(8.0)?, bottom: self.dp(10.0)? })?;
            set_background(&card, &self.surface(COLOR_WHITE, INK, 0, if selected { 2 } else { 0 })?)?;
            ui::set_content_description(&card, &ui::UiText::from(&format!("Select layer {name}")))?;
            {
                let w = Arc::downgrade(self);
                let id = id.clone();
                on_click(&card, move |_| {
                    if let Some(ui) = w.upgrade() {
                        let app = ui.app();
                        let _lock = app.lock.enter();
                        app.select_layer(&notemaker_core::ids::LayerId::from(&id))?;
                    }
                    Ok(())
                })?;
            }
            {
                let w = Arc::downgrade(self);
                let id = id.clone();
                let l = jx::callback(move |_, _| {
                    if let Some(ui) = w.upgrade() {
                        ui.layer_options_at_layers_tool(&id).map_err(JavaError::from)?;
                    }
                    Ok(Reply::Bool(true))
                });
                jcall!(card, "android/view/View", "setOnLongClickListener", "(Landroid/view/View$OnLongClickListener;)V", &l)?;
            }
            let eye_kind = if forced {
                "lock"
            } else if visible {
                "eye"
            } else {
                "hidden"
            };
            let eye_description = format!("{}{}", if forced { "Forced visible layer " } else if visible { "Hide layer " } else { "Show layer " }, name);
            let eye_id = id.clone();
            let eye = self.icon(
                eye_kind,
                &eye_description,
                self.act(move |ui, app| {
                    let _lock = app.lock.enter();
                    let forced = app.layers.with(|l| l.get(&eye_id).map(|l| l.opt_boolean("force"))).ok_or_else(npe)?;
                    if forced {
                        ui.layer_options_at_layers_tool(&eye_id)?;
                    } else {
                        app.hidden.with_mut(|h| {
                            if h.contains(&eye_id) {
                                h.remove(&eye_id);
                            } else {
                                h.insert(eye_id.clone());
                            }
                        });
                        app.save();
                        app.update()?;
                    }
                    Ok(())
                }),
            )?;
            ui::add_view_params(&card, &eye.view(), &ui::linear_params(ui::LayoutSize { width: self.dp(48.0)?, height: self.dp(60.0)? })?)?;
            let (preview, preview_view) = LayerPreview::create(self, id, (count - index) as i32, visible)?;
            self.previews.with_mut(|p| p.push(preview));
            ui::add_view_params(&card, &preview_view, &ui::linear_params(ui::LayoutSize { width: self.dp(48.0)?, height: self.dp(66.0)? })?)?;
            let details = self.column()?;
            ui::set_padding(&details, ui::PxInsets { left: self.dp(14.0)?, top: 0, right: self.dp(4.0)?, bottom: 0 })?;
            let name_view = self.text(&name, 16, INK, selected)?;
            jcall!(name_view, "android/widget/TextView", "setMaxLines", "(I)V", 2)?;
            set_ellipsize_end(&name_view)?;
            ui::add_view(&details, &name_view)?;
            let state = self.text(
                if forced {
                    "For everyone"
                } else if visible {
                    "Visible"
                } else {
                    "Hidden"
                },
                12,
                MUTED,
                false,
            )?;
            ui::set_padding(&state, ui::PxInsets { left: 0, top: self.dp(6.0)?, right: 0, bottom: 0 })?;
            ui::add_view(&details, &state)?;
            ui::add_view_params(&card, &details, &ui::linear_params_weight(ui::LayoutSize { width: 0, height: -2 }, ui::LayoutWeight(1.0))?)?;
            let more_id = id.clone();
            let more = self.icon("more", &format!("Options for layer {name}"), self.act(move |ui, _| ui.layer_options_at_layers_tool(&more_id)))?;
            ui::add_view_params(&card, &more.view(), &ui::linear_params(ui::LayoutSize { width: self.dp(48.0)?, height: self.dp(60.0)? })?)?;
            ui::add_view_params(&layer_list, &card, &ui::linear_params(ui::LayoutSize { width: -1, height: -2 })?)?;
            ui::add_view_params(&layer_list, &self.divider()?, &ui::linear_params(ui::LayoutSize { width: -1, height: self.dp(1.0)? })?)?;
        }
        Ok(())
    }
    /// `layerOptions(id, tools.get("layers"))`.
    fn layer_options_at_layers_tool(self: &Arc<Self>, id: &str) -> AppResult<()> {
        let anchor = self.tool_named("layers").ok_or_else(npe)?;
        self.layer_options(id, &anchor.view())
    }
    /// `private void newLayer()`.
    pub fn new_layer(self: &Arc<Self>) -> AppResult<()> {
        let ctx = self.context();
        let input = edit_text(&ctx)?;
        set_single_line(&input, true)?;
        set_hint(&input, "Layer name")?;
        let box_ = self.column()?;
        ui::set_padding(&box_, ui::PxInsets { left: self.dp(24.0)?, top: self.dp(8.0)?, right: self.dp(24.0)?, bottom: self.dp(8.0)? })?;
        ui::add_view(&box_, &input)?;
        let b = builder(&ctx)?;
        builder_title(&b, "New layer")?;
        builder_view(&b, &box_)?;
        builder_negative(&b, "Cancel", None)?;
        let w = Arc::downgrade(self);
        let input = WeakObj::new(&input);
        let create = dialog_listener(move |_| {
            let Some(ui) = w.upgrade() else { return Ok(()) };
            let app = ui.app();
            let mut name = jfmt::java_trim(&String::from(ui::text_of(&input.get())?)).to_string();
            if name.is_empty() {
                name = format!("Layer {}", app.layers.with(|l| l.len()) + 1);
            }
            let _lock = app.lock.enter();
            app.deselect()?;
            app.reset_drawing_gesture()?;
            app.selected.set(String::from(MainActivity::id()));
            let selected = app.selected.get();
            app.enqueue(&MainActivity::obj(&[("kind", JV::from("layer")), ("layer", JV::from(selected)), ("name", JV::from(name)), ("force", JV::from(false))]))?;
            app.save();
            app.update()?;
            Ok(())
        });
        builder_positive(&b, "Create", &create)?;
        builder_show(&b)
    }
    /// `private void layerOptions(String id, View anchor)`.
    fn layer_options(self: &Arc<Self>, id: &str, anchor: &Obj) -> AppResult<()> {
        let app = self.app();
        let Some(layer) = app.layers.with(|l| l.get(id).cloned()) else { return Ok(()) };
        let box_ = self.popup_body(&layer.opt_string("name"), Some("Layer actions"))?;
        let forced = layer.opt_boolean("force");
        let i = id.to_string();
        self.menu_action(
            &box_,
            if forced { "Stop forcing visibility" } else { "Force visible for everyone" },
            true,
            self.act(move |_, app| {
                let _lock = app.lock.enter();
                let current = app.layers.with(|l| l.get(&i).cloned()).ok_or_else(npe)?;
                app.enqueue(&MainActivity::obj(&[
                    ("kind", JV::from("layer")),
                    ("layer", JV::from(i.as_str())),
                    ("name", JV::from(current.opt_string("name"))),
                    ("force", JV::from(!current.opt_boolean("force"))),
                ]))?;
                app.update()
            }),
        )?;
        let i = id.to_string();
        self.menu_action(
            &box_,
            "Draw on this layer",
            true,
            self.act(move |_, app| {
                let _lock = app.lock.enter();
                app.hidden.with_mut(|h| h.remove(&i));
                app.select_layer(&notemaker_core::ids::LayerId::from(&i))
            }),
        )?;
        let i = id.to_string();
        self.menu_action(&box_, "Rename layer…", true, self.act(move |_, app| app.rename_layer(&notemaker_core::ids::LayerId::from(&i))))?;
        let i = id.to_string();
        self.menu_action(
            &box_,
            "Archive layer",
            id != "base",
            self.act(move |_, app| {
                let b = builder(&app.this)?;
                builder_title(&b, "Archive layer?")?;
                builder_message(&b, "This hides the layer and its contents. Undo restores it.")?;
                builder_negative(&b, "Cancel", None)?;
                let weak = Arc::downgrade(app);
                let id = i.clone();
                let archive = dialog_listener(move |_| match weak.upgrade() {
                    Some(app) => app.layer_command(crate::main_activity::LayerCommand::Hide, &notemaker_core::ids::LayerId::from(&id)),
                    None => Ok(()),
                });
                builder_positive(&b, "Archive", &archive)?;
                builder_show(&b)
            }),
        )?;
        let i = id.to_string();
        self.menu_action(&box_, "Duplicate layer", true, self.act(move |_, app| app.layer_command(crate::main_activity::LayerCommand::Duplicate, &notemaker_core::ids::LayerId::from(&i))))?;
        let order: Vec<String> = app.layers.with(|l| l.keys().cloned().collect());
        let position: i32 = order.iter().position(|k| k == id).map(|p| p as i32).unwrap_or(-1);
        // The list displays the highest order first.
        let i = id.to_string();
        self.menu_action(
            &box_,
            "Move layer up",
            position >= 0 && ((position + 1) as usize) < order.len(),
            self.act(move |_, app| app.layer_command(crate::main_activity::LayerCommand::Reorder(notemaker_core::ids::LayerPosition(position + 1)), &notemaker_core::ids::LayerId::from(&i))),
        )?;
        let i = id.to_string();
        self.menu_action(&box_, "Move layer down", position > 0, self.act(move |_, app| app.layer_command(crate::main_activity::LayerCommand::Reorder(notemaker_core::ids::LayerPosition(position - 1)), &notemaker_core::ids::LayerId::from(&i))))?;
        let i = id.to_string();
        let merge_order = order.clone();
        self.menu_action(&box_, "Merge into another layer…", order.len() > 1, self.act(move |ui, _| ui.choose_merge_target(&i, merge_order.clone())))?;
        let anchor_tool = self.tool_for(anchor);
        self.show_popup(&box_, anchor, anchor_tool, false)
    }
    /// `private void chooseMergeTarget(String source, ArrayList<String> ids)`.
    fn choose_merge_target(self: &Arc<Self>, source: &str, ids: Vec<String>) -> AppResult<()> {
        let app = self.app();
        let mut targets: Vec<String> = Vec::new();
        let mut labels: Vec<String> = Vec::new();
        for id in &ids {
            if id != source {
                targets.push(id.clone());
                let label = app.layers.with(|l| l.get(id).map(|layer| layer.opt_string_or("name", id)));
                labels.push(label.unwrap_or_else(|| id.clone()));
            }
        }
        let b = builder(&self.context())?;
        builder_title(&b, "Merge layer into")?;
        let items = jx::new_string_array(&labels.iter().map(String::as_str).collect::<Vec<_>>())?;
        let weak = Arc::downgrade(&app);
        let source = source.to_string();
        let listener = dialog_listener(move |index| match weak.upgrade() {
            Some(app) => {
                let target = targets.get(index as usize).ok_or_else(|| AppError::java("java.lang.IndexOutOfBoundsException", format!("Index {index} out of bounds")))?;
                app.layer_command(crate::main_activity::LayerCommand::Merge(notemaker_core::ids::LayerId::from(target)), &notemaker_core::ids::LayerId::from(&source))
            }
            None => Ok(()),
        });
        jcall!(b, "android/app/AlertDialog$Builder", "setItems", "([Ljava/lang/CharSequence;Landroid/content/DialogInterface$OnClickListener;)Landroid/app/AlertDialog$Builder;", &items, &listener)?;
        builder_negative(&b, "Cancel", None)?;
        builder_show(&b)
    }
    /// `private LinearLayout popupBody(String heading, String description)`.
    fn popup_body(self: &Arc<Self>, heading: &str, description: Option<&str>) -> AppResult<Obj> {
        let box_ = self.column()?;
        ui::set_padding(&box_, ui::PxInsets { left: self.dp(18.0)?, top: self.dp(16.0)?, right: self.dp(18.0)?, bottom: self.dp(12.0)? })?;
        set_tag_int(&box_, 320)?;
        ui::set_background_color(&box_, notemaker_core::style::ArgbColor(COLOR_WHITE))?;
        let label = self.text(heading, 18, INK, false)?;
        ui::set_padding(&label, ui::PxInsets { left: 0, top: 0, right: 0, bottom: self.dp(12.0)? })?;
        ui::add_view(&box_, &label)?;
        let description = description.ok_or_else(npe)?;
        if !description.is_empty() {
            let help = self.text(description, 12, MUTED, false)?;
            ui::set_padding(&help, ui::PxInsets { left: 0, top: self.dp(6.0)?, right: 0, bottom: self.dp(10.0)? })?;
            ui::add_view(&box_, &help)?;
        }
        Ok(box_)
    }
    /// `void openTool(String kind, View anchor)`.
    pub fn open_tool(self: &Arc<Self>, kind: &ToolKind, anchor: Option<&Obj>) -> AppResult<()> {
        let kind: &str = kind;
        let app = self.app();
        if kind != "select" && (app.selecting.get() || app.selection_session.with(|s| s.is_some()) || !app.selected_entities.with(|s| s.is_empty())) {
            app.deselect()?;
        }
        app.fill_tap_mode.set(false);
        const TOOLS: [&str; 8] = ["pen", "eraser", "move", "shapes", "text", "fill", "select", "interact"];
        if TOOLS.contains(&kind) {
            self.selected_tool.set(kind.to_string());
        }
        if TOOLS.contains(&kind) {
            app.selecting.set(kind == "select");
        }
        let canvas = || app.canvas.get();
        let box_ = match kind {
            "agent" => {
                self.dismiss_popup()?;
                return app.agent.get().ok_or_else(npe)?.show();
            }
            "interact" => {
                app.pan.set(false);
                app.erase.set(false);
                app.drawing_shape.set(None);
                app.pending_text.set(None);
                app.reset_drawing_gesture()?;
                self.dismiss_popup()?;
                return app.update();
            }
            "layers" => self.layers_panel()?,
            "template" => self.template_panel()?,
            "insert" => {
                let box_ = self.popup_body("Insert", Some(""))?;
                self.menu_action(&box_, "Insert picture…", true, self.act(|_, app| app.choose_picture()))?;
                self.menu_action(&box_, "Insert GIF animation…", true, self.act(|_, app| app.choose_gif()))?;
                self.menu_action(&box_, "GIF player…", true, self.act(|_, app| app.animation_player().show()))?;
                self.menu_action(&box_, "Insert SVG artwork…", true, self.act(|_, app| app.choose_svg_picture()))?;
                self.menu_action(&box_, "Enter text…", true, self.act(|_, app| app.text_placement_dialog()))?;
                box_
            }
            "more" => {
                let box_ = self.popup_body("More", Some(""))?;
                self.menu_action(&box_, "Toolbar preferences…", true, self.act(|ui, _| ui.show_popup_at_tool(&ui.preferences_panel()?, "more")))?;
                self.menu_action(&box_, "Network settings…", true, self.act(|ui, _| ui.show_popup_at_tool(&ui.network_settings_panel()?, "more")))?;
                self.menu_action(&box_, "Pen presets…", true, self.act(|ui, _| ui.show_popup_at_tool(&ui.preset_panel()?, "pen")))?;
                self.menu_action(
                    &box_,
                    "Undo and redo",
                    true,
                    self.act(|ui, _| {
                        let more = ui.tool_named("more").map(|t| t.view());
                        ui.open_tool(&ToolKind::from("history"), more.as_ref())
                    }),
                )?;
                self.menu_action(&box_, "Agent", true, self.act(|_, app| app.agent.get().ok_or_else(npe)?.show()))?;
                if !app.conflicts.with(|c| c.is_empty()) {
                    self.menu_action(&box_, "Sync conflicts", true, self.act(|ui, _| ui.show_conflict_panel()))?;
                }
                box_
            }
            "pen" => {
                app.erase.set(false);
                app.pan.set(false);
                app.drawing_shape.set(None);
                app.pending_text.set(None);
                if let Some(c) = canvas() {
                    c.shape_dragging.set(false);
                }
                app.update()?;
                self.pen_panel()?
            }
            "eraser" => {
                app.erase.set(true);
                app.pan.set(false);
                app.drawing_shape.set(None);
                app.pending_text.set(None);
                if let Some(c) = canvas() {
                    c.shape_dragging.set(false);
                }
                app.update()?;
                let box_ = self.eraser_panel()?;
                self.menu_action(&box_, "Undo last action", !app.undo.with(|u| u.is_empty()), self.act(|_, app| app.history(notemaker_core::store::HistoryDirection::Undo)))?;
                self.menu_action(&box_, "Ready to erase", true, self.act(|_, _| Ok(())))?;
                box_
            }
            "move" => {
                app.pan.set(true);
                app.erase.set(false);
                app.drawing_shape.set(None);
                app.pending_text.set(None);
                if let Some(c) = canvas() {
                    c.shape_dragging.set(false);
                }
                app.update()?;
                let box_ = self.popup_body("Move canvas", Some("Drag to move. Pinch to zoom."))?;
                self.menu_action(
                    &box_,
                    "Zoom out",
                    true,
                    self.act(|_, app| {
                        let c = app.canvas();
                        c.zoom.set(fmax(0.2, c.zoom.get() / 1.25));
                        c.invalidate()
                    }),
                )?;
                self.menu_action(
                    &box_,
                    "Zoom in",
                    true,
                    self.act(|_, app| {
                        let c = app.canvas();
                        c.zoom.set(fmin(5.0, c.zoom.get() * 1.25));
                        c.invalidate()
                    }),
                )?;
                self.menu_action(&box_, "Reset view", true, self.act(|_, app| Self::reset_view(app)))?;
                self.menu_action(
                    &box_,
                    "Return to pen",
                    true,
                    self.act(|ui, app| {
                        ui.selected_tool.set("pen".into());
                        app.erase.set(false);
                        app.pan.set(false);
                        app.update()
                    }),
                )?;
                box_
            }
            "shapes" => {
                app.pan.set(false);
                app.erase.set(false);
                app.pending_text.set(None);
                app.update()?;
                self.shape_panel()?
            }
            "text" => {
                app.pan.set(false);
                app.erase.set(false);
                app.drawing_shape.set(None);
                app.pending_text.set(None);
                if let Some(c) = canvas() {
                    c.shape_dragging.set(false);
                }
                app.update()?;
                let box_ = self.popup_body("Text", Some("Enter text and font size, then tap the canvas to place it."))?;
                self.menu_action(&box_, "Enter text…", true, self.act(|_, app| app.text_placement_dialog()))?;
                let model = app.model();
                for entity in app.selected_entities.get() {
                    if entity.get("kind").and_then(Value::as_str) == Some("object") {
                        let object_id = json::java_string(entity.get("id"));
                        let selected = model.objects.get(&object_id);
                        if let Some(selected) = selected {
                            if selected.get("shape").and_then(Value::as_str) == Some("text") {
                                self.menu_action(&box_, "Edit selected text…", true, self.act(move |_, app| app.edit_object(&notemaker_core::ids::EntityId::from(&object_id))))?;
                                break;
                            }
                        }
                    }
                }
                let placed = model.objects.values().any(|o| {
                    o.get("shape").and_then(Value::as_str) == Some("text")
                        && !matches!(json::key_of(o.get("object")).and_then(|k| model.object_active.get(k)), Some(false))
                });
                self.menu_action(&box_, "Edit placed text…", placed, self.act(|_, app| app.object_dialog()))?;
                box_
            }
            "fill" => {
                app.pan.set(false);
                app.erase.set(false);
                app.drawing_shape.set(None);
                app.pending_text.set(None);
                app.fill_tap_mode.set(false);
                app.update()?;
                self.fill_panel()?
            }
            "select" => {
                app.pan.set(false);
                app.erase.set(false);
                app.drawing_shape.set(None);
                app.pending_text.set(None);
                app.selecting.set(true);
                app.update()?;
                self.selection_panel()?
            }
            "conflicts" => self.conflicts_panel()?,
            "history" => {
                let box_ = self.popup_body("History", Some("Your drawing and erasing actions"))?;
                let history_ready = !app.object_history_pending();
                self.menu_action(&box_, "Undo", history_ready && !app.undo.with(|u| u.is_empty()), self.act(|_, app| app.history(notemaker_core::store::HistoryDirection::Undo)))?;
                self.menu_action(&box_, "Redo", history_ready && !app.redo.with(|r| r.is_empty()), self.act(|_, app| app.history(notemaker_core::store::HistoryDirection::Redo)))?;
                box_
            }
            "page" => {
                let type_ = app.type_.get();
                let page = app.page.get();
                let heading = if type_ == "pages" {
                    format!("Page {}", page + 1)
                } else if type_ == "infinite" {
                    "Infinite canvas".to_string()
                } else {
                    "Single page".to_string()
                };
                let description = if type_ == "pages" { "Navigate your notebook" } else { "This document uses one continuous drawing surface." };
                let box_ = self.popup_body(&heading, Some(description))?;
                if type_ == "pages" {
                    self.menu_action(&box_, "Previous page", page > 0, self.act(|ui, app| ui.change_page(app.page.get() - 1)))?;
                    self.menu_action(&box_, "Next page", page < 10000, self.act(|ui, app| ui.change_page(app.page.get() + 1)))?;
                    self.menu_action(&box_, "Go to page…", true, self.act(|_, app| app.page_dialog()))?;
                }
                self.menu_action(&box_, "Reset view", true, self.act(|_, app| Self::reset_view(app)))?;
                box_
            }
            _ => {
                let box_ = self.popup_body("Documents", Some("Open a note or start a new one."))?;
                self.menu_action(&box_, "Browse files…", true, self.act(|ui, _| ui.show_files()))?;
                self.menu_action(&box_, "Open or create document…", true, self.act(|_, app| app.document_dialog()))?;
                self.menu_action(&box_, "Edit diagram objects…", !app.model().objects.is_empty(), self.act(|_, app| app.object_dialog()))?;
                box_
            }
        };
        let anchor = anchor.ok_or_else(npe)?;
        let anchor_tool = self.tool_for(anchor);
        self.show_popup(&box_, anchor, anchor_tool, false)
    }
    /// `app.canvas.zoom = 1; app.canvas.offsetX = app.canvas.offsetY = 0; app.canvas.invalidate();`
    fn reset_view(app: &Arc<MainActivity>) -> AppResult<()> {
        let c = app.canvas();
        c.zoom.set(1.0);
        c.offset_y.set(0.0);
        c.offset_x.set(0.0);
        c.invalidate()
    }
    /// `private void changePage(int page)`.
    fn change_page(self: &Arc<Self>, page: i32) -> AppResult<()> {
        let app = self.app();
        app.deselect()?;
        let _lock = app.lock.enter();
        app.reset_drawing_gesture()?;
        app.page.set(page);
        app.save();
        app.update()
    }
    /// `private LinearLayout shapePanel()`.
    pub fn shape_panel(self: &Arc<Self>) -> AppResult<Obj> {
        let app = self.app();
        let ctx = self.context();
        let box_ = self.popup_body("Shapes", Some("Choose a shape, then drag on the canvas."))?;
        set_tag_int(&box_, 480)?;
        let (_, sample_view) = StrokeSample::create(self)?;
        ui::add_view_params(&box_, &sample_view, &ui::linear_params(ui::LayoutSize { width: -1, height: self.dp(64.0)? })?)?;
        self.section(&box_, "Shapes")?;
        let grid = self.column()?;
        let excluded = ["circle", "rectangle", "rhombus", "arrowLine", "waveLine", "regularHexagon", "text", "svg", "image"];
        let ids: Vec<notemaker_core::ids::ShapeId> = shape_catalog::ids().filter(|id| !excluded.contains(&&**id)).collect();
        // Six columns on tablets, four on narrow displays, always at least 48dp targets.
        let resources = jcall!(ctx, "android/content/Context", "getResources", "()Landroid/content/res/Resources;")?.some();
        let configuration = jcall!(resources, "android/content/res/Resources", "getConfiguration", "()Landroid/content/res/Configuration;")?.some();
        let columns = if jget!(configuration, "android/content/res/Configuration", "screenWidthDp", "I")?.int() < 500 { 4 } else { 6 };
        let mut row: Option<Obj> = None;
        for (i, id) in ids.iter().enumerate() {
            if i % columns == 0 {
                let r = self.row()?;
                ui::add_view_params(&grid, &r, &ui::linear_params(ui::LayoutSize { width: -1, height: self.dp(58.0)? })?)?;
                row = Some(r);
            }
            let label = shape_catalog::name(id)?;
            let shape = id.to_string();
            let tool_shape = id.to_string();
            let choice = self.choice(
                id,
                &format!("Create {label}"),
                self.supplier(move |app| app.drawing_shape.with(|s| s.as_deref() == Some(shape.as_str()))),
                self.act(move |ui, app| {
                    ui.dismiss_popup()?;
                    ui.selected_tool.set("shapes".into());
                    app.set_drawing_tool(Some(&notemaker_core::ids::ShapeId::from(&tool_shape)))
                }),
                false,
            )?;
            ui::add_view_params(row.as_ref().ok_or_else(npe)?, &choice, &ui::linear_params_weight(ui::LayoutSize { width: 0, height: -1 }, ui::LayoutWeight(1.0))?)?;
        }
        let last = row.ok_or_else(npe)?;
        while child_count(&last)? < columns as i32 {
            ui::add_view_params(&last, &ui::view(&ctx)?, &ui::linear_params_weight(ui::LayoutSize { width: 0, height: -1 }, ui::LayoutWeight(1.0))?)?;
        }
        ui::add_view(&box_, &grid)?;
        self.section(&box_, "Colors")?;
        ui::add_view(&box_, &self.color_choices(true)?)?;
        self.section(&box_, "Line width")?;
        ui::add_view(&box_, &self.width_control()?)?;
        let fill = check_box(&ctx, "Shape fill")?;
        set_text_color(&fill, INK)?;
        set_button_tint(&fill, INK)?;
        set_checked(&fill, app.object_fill_color.get() != COLOR_TRANSPARENT)?;
        let w = Arc::downgrade(self);
        on_checked(&fill, move |checked| {
            if let Some(ui) = w.upgrade() {
                let app = ui.app();
                app.object_fill_color.set(if checked { app.color.get() } else { COLOR_TRANSPARENT });
            }
            Ok(())
        })?;
        ui::add_view(&box_, &fill)?;
        Ok(box_)
    }
    /// `private void section(LinearLayout box, String title)`.
    fn section(&self, box_: &Obj, title: &str) -> AppResult<()> {
        let rule = ui::linear_params(ui::LayoutSize { width: -1, height: self.dp(1.0)? })?;
        ui::set_margins(&rule, ui::PxInsets { left: 0, top: self.dp(12.0)?, right: 0, bottom: self.dp(12.0)? })?;
        ui::add_view_params(box_, &self.divider()?, &rule)?;
        let label = self.text(title, 15, INK, false)?;
        ui::set_padding(&label, ui::PxInsets { left: 0, top: 0, right: 0, bottom: self.dp(8.0)? })?;
        ui::add_view(box_, &label)?;
        Ok(())
    }
    /// `private View choice(String kind, String description, BooleanSupplier selected, Runnable callback, boolean label)`.
    fn choice(self: &Arc<Self>, kind: &str, description: &str, selected: BooleanSupplier, callback: Action, label: bool) -> AppResult<Obj> {
        let (_, item) = ChoiceItem::create(self, selected)?;
        jcall!(item, "android/widget/LinearLayout", "setOrientation", "(I)V", LINEARLAYOUT_VERTICAL)?;
        linear_set_gravity(&item, GRAVITY_CENTER)?;
        ui::set_padding(&item, ui::PxInsets { left: self.dp(2.0)?, top: self.dp(4.0)?, right: self.dp(2.0)?, bottom: self.dp(12.0)? })?;
        ui::set_content_description(&item, &ui::UiText::from(description))?;
        set_tooltip(&item, description)?;
        let (choice_icon, icon_view) = Icon::create(self, kind)?;
        choice_icon.accent_color.set(self.app().color.get());
        ui::add_view_params(&item, &icon_view, &ui::linear_params(ui::LayoutSize { width: self.dp(40.0)?, height: self.dp(36.0)? })?)?;
        if label {
            let name = self.text(description, 11, INK, false)?;
            text_set_gravity(&name, GRAVITY_CENTER)?;
            set_single_line(&name, true)?;
            set_ellipsize_end(&name)?;
            ui::add_view_params(&item, &name, &ui::linear_params(ui::LayoutSize { width: -1, height: -2 })?)?;
        }
        on_click(&item, move |v| {
            callback()?;
            let parent = jcall!(v, "android/view/View", "getParent", "()Landroid/view/ViewParent;")?.global();
            if let Some(group) = parent.filter(|p| is_instance(p, "android/view/ViewGroup")) {
                for i in 0..child_count(&group)? {
                    if let Some(child) = child_at(&group, i)? {
                        ui::invalidate(&child)?;
                    }
                }
            }
            Ok(())
        })?;
        Ok(item)
    }
    /// `private LinearLayout colorChoices(boolean shape)`.
    fn color_choices(self: &Arc<Self>, shape: bool) -> AppResult<Obj> {
        let grid = self.column()?;
        let colors: [i32; 10] = [
            COLOR_BLACK,
            0xff555555u32 as i32,
            0xffaaaaaau32 as i32,
            COLOR_WHITE,
            0xffbd2424u32 as i32,
            0xff245dbdu32 as i32,
            0xff247d38u32 as i32,
            0xff7824aau32 as i32,
            0xffd06a00u32 as i32,
            0xffffe59au32 as i32,
        ];
        let names = ["Black", "Dark gray", "Gray", "White", "Red", "Blue", "Green", "Purple", "Orange", "Yellow"];
        let grid_weak = WeakObj::new(&grid);
        let mut row: Option<Obj> = None;
        for i in 0..colors.len() {
            if i % 5 == 0 {
                let r = self.row()?;
                ui::add_view_params(&grid, &r, &ui::linear_params(ui::LayoutSize { width: -1, height: self.dp(48.0)? })?)?;
                row = Some(r);
            }
            let color = colors[i];
            let (_, swatch) = ColorSwatch::create(self, color)?;
            ui::set_content_description(&swatch, &ui::UiText::from(&format!("{} ink", names[i])))?;
            set_tooltip(&swatch, &format!("{} ink", names[i]))?;
            let w = Arc::downgrade(self);
            let grid_weak = grid_weak.clone();
            on_click(&swatch, move |_| {
                let Some(ui) = w.upgrade() else { return Ok(()) };
                let app = ui.app();
                app.color.set(color);
                if let Some(sample) = ui.sample.get() {
                    ui::invalidate(&sample.view())?;
                }
                if ui.selected_tool.get() == "fill" {
                    app.object_fill_color.set(color);
                }
                if shape && app.object_fill_color.get() != COLOR_TRANSPARENT {
                    app.object_fill_color.set(color);
                }
                app.update()?;
                let grid = grid_weak.upgrade().ok_or_else(npe)?;
                for r in 0..child_count(&grid)? {
                    let row = child_at(&grid, r)?.ok_or_else(npe)?;
                    for c in 0..child_count(&row)? {
                        if let Some(child) = child_at(&row, c)? {
                            ui::invalidate(&child)?;
                        }
                    }
                }
                Ok(())
            })?;
            ui::add_view_params(row.as_ref().ok_or_else(npe)?, &swatch, &ui::linear_params_weight(ui::LayoutSize { width: 0, height: -1 }, ui::LayoutWeight(1.0))?)?;
        }
        Ok(grid)
    }
    /// `private View widthControl()` (listener `EditorUi$3`).
    pub fn width_control(self: &Arc<Self>) -> AppResult<Obj> {
        let app = self.app();
        let row = self.row()?;
        linear_set_gravity(&row, GRAVITY_CENTER_VERTICAL)?;
        let value = self.text(&(app.width.get() as i32).to_string(), 15, INK, false)?;
        text_set_gravity(&value, GRAVITY_RIGHT)?;
        let width = seek_bar(&self.context())?;
        ui::set_content_description(&width, &ui::UiText::from("Pen width"))?;
        set_max(&width, 29)?;
        set_progress(&width, 0.max(app.width.get() as i32 - 1))?;
        set_progress_tint(&width, INK)?;
        set_progress_background_tint(&width, 0xffbbbbbbu32 as i32)?;
        set_thumb_tint(&width, INK)?;
        let w = Arc::downgrade(self);
        let value_weak = WeakObj::new(&value);
        on_progress(&width, move |v, user| {
            if user {
                let Some(ui) = w.upgrade() else { return Ok(()) };
                ui.app().width.set((v + 1) as f32);
                ui::set_text(&value_weak.get(), &ui::UiText::from(&(v + 1).to_string()))?;
                if let Some(sample) = ui.sample.get() {
                    ui::invalidate(&sample.view())?;
                }
            }
            Ok(())
        })?;
        ui::add_view_params(&row, &width, &ui::linear_params_weight(ui::LayoutSize { width: 0, height: self.dp(48.0)? }, ui::LayoutWeight(1.0))?)?;
        ui::add_view_params(&row, &value, &ui::linear_params(ui::LayoutSize { width: self.dp(40.0)?, height: -2 })?)?;
        Ok(row)
    }
    /// `private String materialDescription()`.
    fn material_description(&self) -> String {
        let app = self.app();
        format!("{} · {}%", material_range(&app.brush.get()), app.material.get())
    }
    /// `private View materialControl()` (listener `EditorUi$4`).
    pub fn material_control(self: &Arc<Self>) -> AppResult<Obj> {
        let app = self.app();
        let group = self.column()?;
        let label = self.text(&self.material_description(), 13, INK, false)?;
        self.material_label.set(Some(label.clone()));
        ui::add_view(&group, &label)?;
        let seek = seek_bar(&self.context())?;
        ui::set_content_description(&seek, &ui::UiText::from("Brush material"))?;
        set_max(&seek, 100)?;
        set_progress(&seek, app.material.get())?;
        set_progress_tint(&seek, INK)?;
        set_progress_background_tint(&seek, 0xffbbbbbbu32 as i32)?;
        set_thumb_tint(&seek, INK)?;
        let w = Arc::downgrade(self);
        on_progress(&seek, move |value, user| {
            if user {
                let Some(ui) = w.upgrade() else { return Ok(()) };
                let app = ui.app();
                app.material.set(value);
                ui::set_text(&ui.material_label.get().ok_or_else(npe)?, &ui::UiText::from(&ui.material_description()))?;
                let editor = ui::edit(&app.this)?;
                ui::put_int(&editor, &ui::PrefKey::from("penMaterial"), ui::PrefNumber(value))?;
                ui::apply(&editor)?;
                if let Some(sample) = ui.sample.get() {
                    ui::invalidate(&sample.view())?;
                }
            }
            Ok(())
        })?;
        ui::add_view_params(&group, &seek, &ui::linear_params(ui::LayoutSize { width: -1, height: self.dp(42.0)? })?)?;
        Ok(group)
    }
    /// `private LinearLayout penPanel()`.
    pub fn pen_panel(self: &Arc<Self>) -> AppResult<Obj> {
        let box_ = self.popup_body("Pen", Some(""))?;
        set_tag_int(&box_, 480)?;
        let brushes = self.row()?;
        for brush in ["pen", "fountain", "pencil", "brush", "marker"] {
            let name = capitalize(brush);
            let choice = self.choice(
                brush,
                &name,
                self.supplier(move |app| app.brush.with(|b| b == brush)),
                self.act(move |_, app| {
                    app.brush.set(brush.to_string());
                    app.update()
                }),
                true,
            )?;
            ui::add_view_params(&brushes, &choice, &ui::linear_params_weight(ui::LayoutSize { width: 0, height: self.dp(70.0)? }, ui::LayoutWeight(1.0))?)?;
        }
        ui::add_view(&box_, &brushes)?;
        let (_, sample_view) = StrokeSample::create(self)?;
        ui::add_view_params(&box_, &sample_view, &ui::linear_params(ui::LayoutSize { width: -1, height: self.dp(64.0)? })?)?;
        self.section(&box_, "Line width")?;
        ui::add_view(&box_, &self.width_control()?)?;
        let material_panel = self.material_control()?;
        self.material_panel.set(Some(material_panel.clone()));
        ui::add_view(&box_, &material_panel)?;
        self.section(&box_, "Colors")?;
        ui::add_view(&box_, &self.color_choices(false)?)?;
        self.menu_action(&box_, "More brush types…", true, self.act(|ui, _| ui.show_popup_at_tool(&ui.brush_panel()?, "pen")))?;
        self.menu_action(&box_, "Presets…", true, self.act(|ui, _| ui.show_popup_at_tool(&ui.preset_panel()?, "pen")))?;
        self.menu_action(&box_, "Done", true, self.act(|_, _| Ok(())))?;
        Ok(box_)
    }
    /// `private LinearLayout presetPanel()`.
    pub fn preset_panel(self: &Arc<Self>) -> AppResult<Obj> {
        let box_ = self.popup_body("Pen presets", Some("Saved presets keep color and width together."))?;
        let defaults = ["Black · 2", "Black · 5", "Black · 9", "Red · 5", "Blue · 5"];
        let colors = [COLOR_BLACK, COLOR_BLACK, COLOR_BLACK, 0xffbd2424u32 as i32, 0xff245dbdu32 as i32];
        let widths = [2, 5, 9, 5, 5];
        for i in 0..defaults.len() {
            let (color, width) = (colors[i], widths[i]);
            self.menu_action(&box_, defaults[i], true, self.act(move |ui, _| ui.apply_pen_preset_values(color, width, "pen", 50)))?;
        }
        let saved = ui::pref_string(&self.context(), &ui::PrefKey::from("penPresets"), Some(&ui::PrefText::from("")))?.map(String::from).ok_or_else(npe)?;
        if !saved.is_empty() {
            let presets = || -> AppResult<()> {
                let list = JSONArray::parse(&saved)?;
                for i in 0..list.length() {
                    let Some(preset) = list.opt_json_object(i) else { continue };
                    let color = preset.opt_int_or("color", COLOR_BLACK);
                    let width = preset.opt_int_or("width", 5);
                    let material = preset.opt_int_or("material", 50);
                    let brush = preset.opt_string_or("brush", "pen");
                    self.menu_action(
                        &box_,
                        &format!("{} · {}", preset.opt_string_or("name", "Custom"), width),
                        true,
                        self.act(move |ui, _| ui.apply_pen_preset_values(color, width, &brush, material)),
                    )?;
                }
                Ok(())
            };
            // catch (Exception ignored)
            let _ = presets();
        }
        self.menu_action(&box_, "Save current as preset…", true, self.act(|ui, _| ui.save_pen_preset()))?;
        Ok(box_)
    }
    /// `private LinearLayout brushPanel()`.
    pub fn brush_panel(self: &Arc<Self>) -> AppResult<Obj> {
        let box_ = self.popup_body("Brush types", Some("Choose a monochrome brush profile."))?;
        for brush in ["oily", "fountain", "charcoal", "neoBrush", "latinCalligraphy", "asiaCalligraphy", "square"] {
            let label = split_camel(brush);
            self.menu_action(
                &box_,
                &label,
                true,
                self.act(move |ui, app| {
                    app.brush.set(brush.to_string());
                    app.update()?;
                    ui.dismiss_popup()
                }),
            )?;
        }
        Ok(box_)
    }
    /// `private LinearLayout preferencesPanel()`.
    pub fn preferences_panel(self: &Arc<Self>) -> AppResult<Obj> {
        let box_ = self.popup_body("Toolbar preferences", Some("Keep controls readable on the monochrome display."))?;
        let labels = check_box(&self.context(), "Show tool labels")?;
        set_text_color(&labels, INK)?;
        set_button_tint(&labels, INK)?;
        set_checked(&labels, self.show_tool_labels.get())?;
        ui::add_view(&box_, &labels)?;
        let w = Arc::downgrade(self);
        on_checked(&labels, move |checked| match w.upgrade() {
            Some(ui) => ui.show_tool_labels(if checked { ToolLabels::Shown } else { ToolLabels::Hidden }),
            None => Ok(()),
        })?;
        self.menu_action(&box_, "Save current pen as preset…", true, self.act(|ui, _| ui.save_pen_preset()))?;
        Ok(box_)
    }
    /// `private LinearLayout networkSettingsPanel()`.
    pub fn network_settings_panel(self: &Arc<Self>) -> AppResult<Obj> {
        let app = self.app();
        let ctx = self.context();
        let box_ = self.popup_body("Network settings", Some("Choose where notes sync and where the editing service runs."))?;
        set_tag_int(&box_, 480)?;
        let host = app.host.get();
        let server = edit_text(&ctx)?;
        jcall!(server, "android/widget/TextView", "setSingleLine", "()V")?;
        ui::set_text(&server, &ui::UiText::from(&MainActivity::server_address_for_settings(Some(&host))))?;
        set_hint(&server, "192.168.50.180")?;
        set_input_type(&server, INPUTTYPE_TYPE_CLASS_TEXT | INPUTTYPE_TYPE_TEXT_VARIATION_URI)?;
        ui::add_view(&box_, &self.text("Notes server address", 12, INK, true)?)?;
        ui::add_view(&box_, &server)?;
        let server_port = edit_text(&ctx)?;
        jcall!(server_port, "android/widget/TextView", "setSingleLine", "()V")?;
        ui::set_text(&server_port, &ui::UiText::from(&MainActivity::server_port_for_settings(Some(&host))))?;
        set_hint(&server_port, "8765")?;
        set_input_type(&server_port, INPUTTYPE_TYPE_CLASS_NUMBER)?;
        ui::add_view(&box_, &self.text("Server port", 12, INK, true)?)?;
        ui::add_view(&box_, &server_port)?;
        let server_help = self.text(
            "Leave the address blank for local-only notes. A blank port uses 8765. For the Android emulator, use 10.0.2.2. On your LAN, enter the computer running NoteMaker, for example 192.168.50.180.",
            11,
            MUTED,
            false,
        )?;
        ui::set_padding(&server_help, ui::PxInsets { left: 0, top: self.dp(4.0)?, right: 0, bottom: self.dp(8.0)? })?;
        ui::add_view(&box_, &server_help)?;
        let proxy = edit_text(&ctx)?;
        jcall!(proxy, "android/widget/TextView", "setSingleLine", "()V")?;
        ui::set_text(&proxy, &ui::UiText::from(&app.proxy_url.get()))?;
        set_hint(&proxy, "Optional editing proxy URL")?;
        set_input_type(&proxy, INPUTTYPE_TYPE_CLASS_TEXT | INPUTTYPE_TYPE_TEXT_VARIATION_URI)?;
        ui::add_view(&box_, &self.text("Editing proxy URL", 12, INK, true)?)?;
        ui::add_view(&box_, &proxy)?;
        let token = edit_text(&ctx)?;
        jcall!(token, "android/widget/TextView", "setSingleLine", "()V")?;
        ui::set_text(&token, &ui::UiText::from(&app.proxy_token.get()))?;
        set_hint(&token, "Proxy bearer token")?;
        set_input_type(&token, INPUTTYPE_TYPE_CLASS_TEXT | INPUTTYPE_TYPE_TEXT_VARIATION_PASSWORD)?;
        ui::add_view(&box_, &self.text("Editing proxy token", 12, INK, true)?)?;
        ui::add_view(&box_, &token)?;
        let proxy_help = self.text(
            "Optional. Leave both proxy fields blank to use this device’s local editing service. A proxy URL requires a token.",
            11,
            MUTED,
            false,
        )?;
        ui::set_padding(&proxy_help, ui::PxInsets { left: 0, top: self.dp(4.0)?, right: 0, bottom: self.dp(8.0)? })?;
        ui::add_view(&box_, &proxy_help)?;
        let fields = [WeakObj::new(&server), WeakObj::new(&server_port), WeakObj::new(&proxy), WeakObj::new(&token)];
        self.menu_action(
            &box_,
            "Save network settings",
            true,
            self.act(move |_, app| {
                let server = String::from(ui::text_of(&fields[0].get())?);
                let port = String::from(ui::text_of(&fields[1].get())?);
                let proxy = String::from(ui::text_of(&fields[2].get())?);
                let token = String::from(ui::text_of(&fields[3].get())?);
                app.save_network_settings(&server, &port, &proxy, &token)
            }),
        )?;
        self.menu_action(&box_, "Done", true, self.act(|ui, _| ui.dismiss_popup()))?;
        Ok(box_)
    }
    /// `private LinearLayout fillPanel()`.
    pub fn fill_panel(self: &Arc<Self>) -> AppResult<Obj> {
        let app = self.app();
        let box_ = self.popup_body("Enclosed fill", Some("Choose a fill color, then fill a closed object or ink path."))?;
        set_tag_int(&box_, 400)?;
        if app.object_fill_color.get() == COLOR_TRANSPARENT {
            app.object_fill_color.set(app.color.get());
        }
        ui::add_view(&box_, &self.color_choices(true)?)?;
        self.menu_action(
            &box_,
            "Tap enclosed area to fill",
            true,
            self.act(|ui, app| {
                app.fill_tap_mode.set(true);
                app.selecting.set(false);
                let fill = app.object_fill_color.get();
                app.object_fill_color.set(if fill == COLOR_TRANSPARENT { app.color.get() } else { fill });
                ui.dismiss_popup()?;
                app.update()?;
                ui::toast(&app.this, &ui::UiText::from("Tap inside a closed area"), TOAST_LENGTH_SHORT)?;
                Ok(())
            }),
        )?;
        let has_fill = app.object_fill_color.get() != COLOR_TRANSPARENT;
        self.menu_action(
            &box_,
            "Apply to a shape…",
            has_fill && !app.model().objects.is_empty(),
            self.act(|ui, app| {
                ui.dismiss_popup()?;
                let targets = ui.popup_body("Fill a shape", Some("Choose a native object."))?;
                let model = app.model();
                for (id, object) in model.objects.iter() {
                    if model.object_active.get(id) == Some(&false) {
                        continue;
                    }
                    let label = format!("{} · {}", shape_catalog::name(&notemaker_core::ids::ShapeId::from(json::java_string(object.get("shape"))))?, short_id(id));
                    let id = id.clone();
                    ui.menu_action(&targets, &label, true, ui.act(move |_, app| app.fill_entity(&notemaker_core::ids::EntityKind::object(), &notemaker_core::ids::EntityId::from(&id), notemaker_core::style::ArgbColor(app.object_fill_color.get()))))?;
                }
                ui.show_popup_at_tool(&targets, "fill")
            }),
        )?;
        self.menu_action(
            &box_,
            "Apply to a closed ink path…",
            has_fill && !app.strokes.with(|s| s.is_empty()),
            self.act(|ui, app| {
                ui.dismiss_popup()?;
                let targets = ui.popup_body("Fill a path", Some("Open paths are rejected by the shared validator."))?;
                let strokes = app.strokes.get();
                let selected = app.selected.get();
                let page = app.page.get();
                for (id, stroke) in strokes.iter() {
                    if app.active.with(|a| a.get(id) == Some(&false)) || selected != stroke.opt_string("layer") || stroke.opt_int("page") != page {
                        continue;
                    }
                    let label = format!("Ink path · {}", short_id(id));
                    let id = id.clone();
                    ui.menu_action(&targets, &label, true, ui.act(move |_, app| app.fill_entity(&notemaker_core::ids::EntityKind::stroke(), &notemaker_core::ids::EntityId::from(&id), notemaker_core::style::ArgbColor(app.object_fill_color.get()))))?;
                }
                ui.show_popup_at_tool(&targets, "fill")
            }),
        )?;
        Ok(box_)
    }
    /// `private LinearLayout eraserPanel()` (listener `EditorUi$5`).
    pub fn eraser_panel(self: &Arc<Self>) -> AppResult<Obj> {
        let app = self.app();
        let ctx = self.context();
        let box_ = self.popup_body("Eraser", Some("Erase items on the selected layer. Each gesture saves as one action."))?;
        set_tag_int(&box_, 400)?;
        let modes = self.row()?;
        for mode in ["stroke", "partial", "area"] {
            let label = if mode == "stroke" { "Whole item".to_string() } else { format!("{} erase", capitalize(mode)) };
            let choice = self.choice(
                "eraser",
                &label,
                self.supplier(move |app| app.eraser_mode.with(|m| m == mode)),
                self.act(move |_, app| {
                    app.eraser_mode.set(mode.to_string());
                    app.update()
                }),
                true,
            )?;
            ui::add_view_params(&modes, &choice, &ui::linear_params_weight(ui::LayoutSize { width: 0, height: self.dp(72.0)? }, ui::LayoutWeight(1.0))?)?;
        }
        ui::add_view(&box_, &modes)?;
        self.section(&box_, "Options")?;
        let scribble = check_box(&ctx, "Scribble to erase")?;
        set_checked(&scribble, app.scribble_to_erase.get())?;
        set_text_color(&scribble, INK)?;
        set_button_tint(&scribble, INK)?;
        let w = Arc::downgrade(self);
        on_checked(&scribble, move |checked| {
            if let Some(ui) = w.upgrade() {
                let app = ui.app();
                app.scribble_to_erase.set(checked);
                let editor = ui::edit(&app.this)?;
                ui::put_bool(&editor, &ui::PrefKey::from("scribbleToErase"), ui::PrefSwitch::from(checked))?;
                ui::apply(&editor)?;
            }
            Ok(())
        })?;
        ui::add_view(&box_, &scribble)?;
        let radius_label = self.text(&format!("Eraser size  {}", app.eraser_radius.get() as i32), 12, INK, true)?;
        ui::set_padding(&radius_label, ui::PxInsets { left: 0, top: self.dp(6.0)?, right: 0, bottom: 0 })?;
        ui::add_view(&box_, &radius_label)?;
        let radius = seek_bar(&ctx)?;
        set_max(&radius, 79)?;
        set_progress(&radius, 0.max(app.eraser_radius.get() as i32 - 1))?;
        set_progress_tint(&radius, INK)?;
        set_thumb_tint(&radius, INK)?;
        let w = Arc::downgrade(self);
        let label_weak = WeakObj::new(&radius_label);
        on_progress(&radius, move |v, _| {
            if let Some(ui) = w.upgrade() {
                ui.app().eraser_radius.set((v + 1) as f32);
                ui::set_text(&label_weak.get(), &ui::UiText::from(&format!("Eraser size  {}", v + 1)))?;
            }
            Ok(())
        })?;
        ui::add_view_params(&box_, &radius, &ui::linear_params(ui::LayoutSize { width: -1, height: self.dp(42.0)? })?)?;
        self.menu_action(
            &box_,
            "Erase page",
            true,
            self.act(|_, app| {
                let selected = app.selected.get();
                app.apply_erase(&crate::main_activity::EraserMode::from("page"), &notemaker_core::ids::LayerId::from(&selected), notemaker_core::ids::PageNumber(app.page.get()), Some(&JSONArray::new()), notemaker_core::geometry::Radius(app.eraser_radius.get() as f64))
            }),
        )?;
        self.menu_action(&box_, "Erase objects…", !app.model().objects.is_empty(), self.act(|_, app| app.object_dialog()))?;
        Ok(box_)
    }
    /// `private LinearLayout selectionPanel()`.
    pub fn selection_panel(self: &Arc<Self>) -> AppResult<Obj> {
        let app = self.app();
        let box_ = self.popup_body("Selection", Some("Selected objects are locked while you transform them."))?;
        let state = self.text(
            &format!("{} selected · {} skipped as locked", app.selected_entities.with(|s| s.len()), app.selection_skipped.get()),
            12,
            MUTED,
            false,
        )?;
        ui::set_padding(&state, ui::PxInsets { left: 0, top: self.dp(4.0)?, right: 0, bottom: self.dp(8.0)? })?;
        ui::add_view(&box_, &state)?;
        let lasso = check_box(&self.context(), "Freeform lasso (off: rectangle)")?;
        set_text_color(&lasso, INK)?;
        set_button_tint(&lasso, INK)?;
        set_checked(&lasso, app.freeform_lasso.get())?;
        ui::add_view(&box_, &lasso)?;
        let w = Arc::downgrade(self);
        on_checked(&lasso, move |checked| {
            if let Some(ui) = w.upgrade() {
                let app = ui.app();
                app.freeform_lasso.set(checked);
                let editor = ui::edit(&app.this)?;
                ui::put_bool(&editor, &ui::PrefKey::from("freeformLasso"), ui::PrefSwitch::from(checked))?;
                ui::apply(&editor)?;
                app.update()?;
            }
            Ok(())
        })?;
        let any = !app.selected_entities.with(|s| s.is_empty());
        let pending = app.pending_transform.with(|p| p.is_some());
        self.menu_action(&box_, "Choose objects and strokes…", true, self.act(|ui, _| ui.choose_entities()))?;
        self.menu_action(&box_, "Select all unlocked on this page", true, self.act(|_, app| app.select_all_visible()))?;
        self.menu_action(&box_, "Tap an item or draw a selection boundary", true, self.act(|_, _| Ok(())))?;
        self.menu_action(&box_, "Preview scale up", any, self.act(|_, app| app.preview_selection(notemaker_core::collaboration::SelectionTransform { scale: 1.1, dx: 0.0, dy: 0.0 })))?;
        self.menu_action(&box_, "Preview scale down", any, self.act(|_, app| app.preview_selection(notemaker_core::collaboration::SelectionTransform { scale: 0.9, dx: 0.0, dy: 0.0 })))?;
        self.menu_action(
            &box_,
            "Preview rotate clockwise",
            any,
            self.act(|_, app| app.preview_transform(&notemaker_core::object! {"scale" => 1, "dx" => 0, "dy" => 0, "rotation" => 15})),
        )?;
        self.menu_action(
            &box_,
            "Preview flip horizontal",
            any,
            self.act(|_, app| app.preview_transform(&notemaker_core::object! {"scale" => 1, "dx" => 0, "dy" => 0, "flipX" => true})),
        )?;
        self.menu_action(
            &box_,
            "Preview flip vertical",
            any,
            self.act(|_, app| app.preview_transform(&notemaker_core::object! {"scale" => 1, "dx" => 0, "dy" => 0, "flipY" => true})),
        )?;
        self.menu_action(&box_, "Preview move right", any, self.act(|_, app| app.preview_selection(notemaker_core::collaboration::SelectionTransform { scale: 1.0, dx: 24.0, dy: 0.0 })))?;
        self.menu_action(&box_, "Preview move down", any, self.act(|_, app| app.preview_selection(notemaker_core::collaboration::SelectionTransform { scale: 1.0, dx: 0.0, dy: 24.0 })))?;
        self.menu_action(&box_, "Commit transform", pending, self.act(|_, app| app.commit_transform_preview()))?;
        self.menu_action(&box_, "Cancel preview", pending, self.act(|_, app| app.cancel_transform_preview()))?;
        self.menu_action(&box_, "Duplicate selection", any, self.act(|_, app| app.duplicate_selection()))?;
        self.menu_action(&box_, "Delete selection", any, self.act(|_, app| app.delete_selection()))?;
        self.menu_action(&box_, "Cut selection", any, self.act(|_, app| app.cut_selection()))?;
        self.menu_action(&box_, "Copy selection", any, self.act(|_, app| app.copy_selection()))?;
        let empty = Value::Array(Vec::new());
        let clipboard = app.selection_clipboard.with(|c| json::get_or_default(c, "entities", &empty).clone());
        let can_paste = !json::list(&clipboard)?.is_empty();
        self.menu_action(&box_, "Paste copied selection", can_paste, self.act(|_, app| app.paste_selection()))?;
        self.menu_action(&box_, "Deselect and release locks", app.selection_session.with(|s| s.is_some()), self.act(|_, app| app.deselect()))?;
        Ok(box_)
    }
    /// `private LinearLayout conflictsPanel()`.
    pub fn conflicts_panel(self: &Arc<Self>) -> AppResult<Obj> {
        let app = self.app();
        let box_ = self.popup_body("Sync conflicts", Some("Both offline versions are preserved until you choose."))?;
        for raw in app.conflicts.get() {
            let conflict = json::map(&raw)?.clone();
            let id = json::java_string(conflict.get("id"));
            let kind = match conflict.get("kind") {
                Some(v) => json::java_string(Some(v)),
                None => "Edit".to_string(),
            };
            let entity = match conflict.get("entity") {
                Some(v) => json::java_string(Some(v)),
                None => id.clone(),
            };
            let title = format!("{kind} · {entity}");
            self.menu_action(&box_, &title, true, self.act(move |ui, _| ui.show_conflict(&id, &conflict)))?;
        }
        Ok(box_)
    }
    /// `private void showConflict(String id, Map<String,Object> conflict)`.
    fn show_conflict(self: &Arc<Self>, id: &str, conflict: &Map) -> AppResult<()> {
        self.dismiss_popup()?;
        let box_ = self.popup_body("Compare versions", Some("Choose either version or keep both. Actor details appear when recorded."))?;
        let columns = jnew!("android/widget/LinearLayout", "(Landroid/content/Context;)V", &self.context())?;
        jcall!(columns, "android/widget/LinearLayout", "setOrientation", "(I)V", LINEARLAYOUT_HORIZONTAL)?;
        let actor = |key: &str| match conflict.get(key) {
            Some(v) => json::java_string(Some(v)),
            None => "actor not recorded".to_string(),
        };
        let (actor_a, actor_b) = (actor("localActor"), actor("remoteActor"));
        let local = conflict.get("local").cloned().unwrap_or(Value::Null);
        let remote = conflict.get("remote").cloned().unwrap_or(Value::Null);
        ui::add_view_params(&columns, &self.conflict_version("Version A", &actor_a, &local)?, &ui::linear_params_weight(ui::LayoutSize { width: 0, height: self.dp(190.0)? }, ui::LayoutWeight(1.0))?)?;
        ui::add_view_params(&columns, &self.conflict_version("Version B", &actor_b, &remote)?, &ui::linear_params_weight(ui::LayoutSize { width: 0, height: self.dp(190.0)? }, ui::LayoutWeight(1.0))?)?;
        ui::add_view(&box_, &columns)?;
        let details = self.text(
            &format!("Version A details\n{}\nVersion B details\n{}", self.version_summary(&local), self.version_summary(&remote)),
            11,
            MUTED,
            false,
        )?;
        jcall!(details, "android/widget/TextView", "setTextIsSelectable", "(Z)V", true)?;
        ui::add_view(&box_, &details)?;
        for (label, choice) in [("Use Version A", notemaker_core::ids::ConflictChoice::Local), ("Use Version B", notemaker_core::ids::ConflictChoice::Remote), ("Keep both versions", notemaker_core::ids::ConflictChoice::Both)] {
            let id = notemaker_core::ids::ConflictId::from(id);
            self.menu_action(&box_, label, true, self.act(move |_, app| app.resolve_conflict(&id, choice)))?;
        }
        let connection = self.connection.get().ok_or_else(npe)?;
        self.show_popup(&box_, &connection, None, false)
    }
    /// `private LinearLayout conflictVersion(String title, String actor, Object raw)`.
    fn conflict_version(self: &Arc<Self>, title: &str, actor: &str, raw: &Value) -> AppResult<Obj> {
        let column = jnew!("android/widget/LinearLayout", "(Landroid/content/Context;)V", &self.context())?;
        jcall!(column, "android/widget/LinearLayout", "setOrientation", "(I)V", LINEARLAYOUT_VERTICAL)?;
        ui::set_padding(&column, ui::PxInsets { left: self.dp(4.0)?, top: self.dp(4.0)?, right: self.dp(4.0)?, bottom: self.dp(4.0)? })?;
        let heading = self.text(&format!("{title} · {actor}"), 12, INK, true)?;
        ui::add_view(&column, &heading)?;
        let (_, preview) = ConflictPreview::create(self, raw.clone())?;
        ui::add_view_params(&column, &preview, &ui::linear_params(ui::LayoutSize { width: -1, height: self.dp(120.0)? })?)?;
        if let Value::Object(value) = raw {
            let shape = match value.get("shape") {
                Some(v) => json::java_string(Some(v)),
                None => json::java_string(value.get("kind")),
            };
            let text = if value.contains_key("text") { format!(" · {}", json::java_string(value.get("text"))) } else { String::new() };
            ui::add_view(&column, &self.text(&format!("{shape}{text}"), 11, MUTED, false)?)?;
        } else {
            ui::add_view(&column, &self.text("Deleted or unavailable", 11, MUTED, false)?)?;
        }
        Ok(column)
    }
    /// `private String pretty(Object value)`.
    fn pretty(&self, value: &Value) -> String {
        if value.is_null() {
            return "(empty)".into();
        }
        match crate::org_json::JSONObject::parse(&json::write(value)) {
            Ok(o) => o.to_string_indent(2),
            Err(_) => json::java_string(Some(value)),
        }
    }
    /// `private String versionSummary(Object raw)`.
    fn version_summary(&self, raw: &Value) -> String {
        let Value::Object(version) = raw else { return self.pretty(raw) };
        let mut out = String::new();
        for key in [
            "kind", "shape", "text", "fontFamily", "alignment", "bold", "italic", "underline", "x", "y", "w", "h", "fontSize", "color", "fill", "brush",
            "width", "active",
        ] {
            if version.contains_key(key) {
                out.push_str(key);
                out.push_str(": ");
                out.push_str(&json::java_string(version.get(key)));
                out.push('\n');
            }
        }
        if let Some(Value::Array(points)) = version.get("points") {
            out.push_str(&format!("points: {}\n", points.len()));
        }
        jfmt::java_trim(&out).to_string()
    }
    /// `private void chooseEntities()`.
    pub fn choose_entities(self: &Arc<Self>) -> AppResult<()> {
        let app = self.app();
        let mut entities: Vec<Map> = Vec::new();
        let mut labels: Vec<String> = Vec::new();
        {
            let _lock = app.lock.enter();
            let selected = app.selected.get();
            if app.canvas.get().is_some_and(|c| c.visible(&notemaker_core::ids::LayerId::from(&selected))) {
                let editor = app.layer_editor()?;
                for item in editor.items() {
                    let owner = self.lease_actor(&item.kind, &item.id);
                    let label = if item.kind == "stroke" { "Ink stroke" } else { &*shape_catalog::name(&notemaker_core::ids::ShapeId::from(json::java_string(item.content.get("shape"))))? };
                    entities.push(item.ref_());
                    labels.push(format!(
                        "{} · {}{}",
                        label,
                        short_id(&item.id),
                        match owner {
                            None => String::new(),
                            Some(owner) => format!(" · Locked by {owner}"),
                        }
                    ));
                }
            }
        }
        if labels.is_empty() {
            ui::toast(&app.this, &ui::UiText::from("There are no items on this layer and page"), TOAST_LENGTH_SHORT)?;
            return Ok(());
        }
        let selected_entities = app.selected_entities.get();
        let mut checked = vec![false; labels.len()];
        for (i, entity) in entities.iter().enumerate() {
            for selected in &selected_entities {
                if entity == selected {
                    checked[i] = true;
                }
            }
        }
        let java_checked = jx::new_boolean_array(&checked)?;
        let checked = Arc::new(Mutex::new(checked));
        let b = builder(&self.context())?;
        builder_title(&b, "Select entities")?;
        let items = jx::new_string_array(&labels.iter().map(String::as_str).collect::<Vec<_>>())?;
        let toggles = checked.clone();
        let multi = jx::callback(move |_, args| {
            let index = jx::unbox_int(&args[1]) as usize;
            let value = jx::unbox_bool(&args[2]);
            if let Some(slot) = toggles.lock().unwrap().get_mut(index) {
                *slot = value;
            }
            Ok(Reply::Void)
        });
        jcall!(
            b,
            "android/app/AlertDialog$Builder",
            "setMultiChoiceItems",
            "([Ljava/lang/CharSequence;[ZLandroid/content/DialogInterface$OnMultiChoiceClickListener;)Landroid/app/AlertDialog$Builder;",
            &items,
            &java_checked,
            &multi
        )?;
        builder_negative(&b, "Cancel", None)?;
        let weak = Arc::downgrade(&app);
        let select = dialog_listener(move |_| {
            let Some(app) = weak.upgrade() else { return Ok(()) };
            let flags = checked.lock().unwrap().clone();
            let chosen: Vec<Map> = flags.iter().zip(entities.iter()).filter(|(c, _)| **c).map(|(_, e)| e.clone()).collect();
            if app.selection_session.with(|s| s.is_some()) {
                app.deselect()?;
                app.selecting.set(true);
            }
            if chosen.is_empty() {
                app.selecting.set(false);
            } else {
                app.start_selection(chosen)?;
            }
            Ok(())
        });
        builder_positive(&b, "Select", &select)?;
        builder_show(&b)
    }
    /// `private String leaseActor(String kind, String id)` (`null` → `None`).
    fn lease_actor(&self, kind: &str, id: &str) -> Option<String> {
        let app = self.app();
        let key = format!("{}:{}:{}", app.page.get(), kind, id);
        app.leases.with(|leases| match leases.get(&key) {
            Some(Value::Object(lease)) => Some(match lease.get("actor") {
                Some(v) => json::java_string(Some(v)),
                None => "another session".to_string(),
            }),
            _ => None,
        })
    }
    /// `private LinearLayout templatePanel()`.
    pub fn template_panel(self: &Arc<Self>) -> AppResult<Obj> {
        let box_ = self.popup_body("Page templates", Some("Adds a page-sized background guide behind your ink."))?;
        for (label, template) in [("Ruled", "ruled"), ("Graph", "grid"), ("Dot grid", "dots"), ("Planner · checklist", "planner")] {
            self.menu_action(&box_, label, true, self.act(move |_, app| app.insert_template(&crate::main_activity::TemplateKind::from(template))))?;
        }
        Ok(box_)
    }
    /// `private void applyPenPreset(int color, int width, String brush, int material)`.
    fn apply_pen_preset_values(self: &Arc<Self>, color: i32, width: i32, brush: &str, material: i32) -> AppResult<()> {
        let app = self.app();
        if app.selecting.get() || app.selection_session.with(|s| s.is_some()) || !app.selected_entities.with(|s| s.is_empty()) {
            app.deselect()?;
        }
        self.selected_tool.set("pen".into());
        app.erase.set(false);
        app.pan.set(false);
        app.fill_tap_mode.set(false);
        app.reset_drawing_gesture()?;
        app.color.set(color);
        app.width.set(1.max(30.min(width)) as f32);
        app.brush.set(brush.to_string());
        app.material.set(0.max(100.min(material)));
        let editor = ui::edit(&app.this)?;
        ui::put_int(&editor, &ui::PrefKey::from("penMaterial"), ui::PrefNumber(app.material.get()))?;
        ui::apply(&editor)?;
        app.update()?;
        self.dismiss_popup()
    }
    /// `void savePenPreset(String label)`.
    pub fn save_pen_preset_named(self: &Arc<Self>, label: &PenPresetName) -> AppResult<()> {
        let label: &str = label;
        let label = jfmt::java_trim(label);
        if label.is_empty() {
            return Err(AppError::illegal_argument("Preset name is required"));
        }
        let app = self.app();
        let saved = ui::pref_string(&app.this, &ui::PrefKey::from("penPresets"), Some(&ui::PrefText::from("")))?.map(String::from);
        let list = saved.as_deref().ok_or(()).and_then(|s| JSONArray::parse(s).map_err(|_| ())).unwrap_or_else(|_| JSONArray::new());
        list.put(MainActivity::obj(&[
            ("name", JV::from(label)),
            ("color", JV::from(app.color.get())),
            ("width", JV::from(app.width.get() as i32)),
            ("brush", JV::from(app.brush.get())),
            ("material", JV::from(app.material.get())),
        ]))?;
        let editor = ui::edit(&app.this)?;
        ui::put_string(&editor, &ui::PrefKey::from("penPresets"), &ui::PrefText::from(&list.to_string()))?;
        ui::apply(&editor)?;
        Ok(())
    }
    /// `void applyPenPreset(String label)`.
    pub fn apply_pen_preset(self: &Arc<Self>, label: &PenPresetName) -> AppResult<()> {
        let label: &str = label;
        let defaults = ["Black · 2", "Black · 5", "Black · 9", "Red · 5", "Blue · 5"];
        let colors = [COLOR_BLACK, COLOR_BLACK, COLOR_BLACK, 0xffbd2424u32 as i32, 0xff245dbdu32 as i32];
        let widths = [2, 5, 9, 5, 5];
        for i in 0..defaults.len() {
            if defaults[i] == label {
                return self.apply_pen_preset_values(colors[i], widths[i], "pen", 50);
            }
        }
        let saved = || -> AppResult<bool> {
            let stored = ui::pref_string(&self.context(), &ui::PrefKey::from("penPresets"), Some(&ui::PrefText::from("")))?.map(String::from).ok_or_else(npe)?;
            let list = JSONArray::parse(&stored)?;
            for i in 0..list.length() {
                if let Some(preset) = list.opt_json_object(i) {
                    if label == preset.opt_string("name") {
                        self.apply_pen_preset_values(
                            preset.opt_int_or("color", COLOR_BLACK),
                            preset.opt_int_or("width", 5),
                            &preset.opt_string_or("brush", "pen"),
                            preset.opt_int_or("material", 50),
                        )?;
                        return Ok(true);
                    }
                }
            }
            Ok(false)
        };
        // catch (Exception ignored)
        if let Ok(true) = saved() {
            return Ok(());
        }
        Err(AppError::illegal_argument(format!("Unknown pen preset: {label}")))
    }
    /// `private void savePenPreset()` (dialog asking for a name).
    pub fn save_pen_preset(self: &Arc<Self>) -> AppResult<()> {
        let ctx = self.context();
        let name = edit_text(&ctx)?;
        set_single_line(&name, true)?;
        set_hint(&name, "Preset name")?;
        let form = self.column()?;
        ui::set_padding(&form, ui::PxInsets { left: self.dp(24.0)?, top: self.dp(8.0)?, right: self.dp(24.0)?, bottom: self.dp(8.0)? })?;
        ui::add_view(&form, &name)?;
        let b = builder(&ctx)?;
        builder_title(&b, "Save pen preset")?;
        builder_view(&b, &form)?;
        builder_negative(&b, "Cancel", None)?;
        let w = Arc::downgrade(self);
        let name = WeakObj::new(&name);
        let save = dialog_listener(move |_| {
            let Some(ui) = w.upgrade() else { return Ok(()) };
            let mut label = jfmt::java_trim(&String::from(ui::text_of(&name.get())?)).to_string();
            if label.is_empty() {
                let stored = ui::pref_string(&ui.context(), &ui::PrefKey::from("penPresets"), Some(&ui::PrefText::from("")))?.map(String::from).ok_or_else(npe)?;
                label = format!("Preset {}", json::jlen(&stored) + 1);
            }
            ui.save_pen_preset_named(&PenPresetName::from(label))?;
            ui.dismiss_popup()
        });
        builder_positive(&b, "Save", &save)?;
        builder_show(&b)
    }
    /// `void dismissPopup()`.
    pub fn dismiss_popup(&self) -> AppResult<()> {
        if let Some(popup) = self.popup.get() {
            jcall!(popup, "android/widget/PopupWindow", "dismiss", "()V")?;
            self.popup.set(None);
        }
        Ok(())
    }
    /// `void openPanel(String kind)`.
    pub fn open_panel(self: &Arc<Self>, kind: &PanelName) -> AppResult<()> {
        let kind: &str = kind;
        match kind {
            "history" => {
                let more = self.tool_named("more").map(|t| t.view());
                self.open_tool(&ToolKind::from("history"), more.as_ref())
            }
            "conflicts" => self.show_conflict_panel(),
            "network" => self.show_popup_at_tool(&self.network_settings_panel()?, "more"),
            "preferences" => self.show_popup_at_tool(&self.preferences_panel()?, "more"),
            "presets" => self.show_popup_at_tool(&self.preset_panel()?, "pen"),
            "brushes" => self.show_popup_at_tool(&self.brush_panel()?, "pen"),
            _ => {
                let anchor = self.tool_named(kind).ok_or_else(|| AppError::illegal_argument(format!("Unknown tool or panel: {kind}")))?;
                self.open_tool(&ToolKind::from(kind), Some(&anchor.view()))
            }
        }
    }
    /// `String selectedTool()`.
    pub fn selected_tool(&self) -> ToolKind {
        ToolKind::from(self.selected_tool.get())
    }
    /// `boolean panelShowing()`.
    pub fn panel_showing(&self) -> AppResult<bool> {
        match self.popup.get() {
            Some(popup) => Ok(jcall!(popup, "android/widget/PopupWindow", "isShowing", "()Z")?.bool()),
            None => Ok(false),
        }
    }
    /// `void showFiles()`: `new DocumentBrowser(app).show()`.
    pub fn show_files(self: &Arc<Self>) -> AppResult<()> {
        self.dismiss_popup()?;
        DocumentBrowser::new(&self.app())?.show()
    }
    /// `void showToolLabels(boolean show)`.
    pub fn show_tool_labels(self: &Arc<Self>, labels: ToolLabels) -> AppResult<()> {
        let show = labels == ToolLabels::Shown;
        self.show_tool_labels.set(show);
        let editor = ui::edit(&self.context())?;
        ui::put_bool(&editor, &ui::PrefKey::from("showToolLabels"), ui::PrefSwitch::from(show))?;
        ui::apply(&editor)?;
        for item in self.tools.get().values() {
            ui::set_visibility(&item.label.get(), if show { VIEW_VISIBLE } else { VIEW_GONE })?;
        }
        Ok(())
    }
    /// `void showConflictPanel()`.
    pub fn show_conflict_panel(self: &Arc<Self>) -> AppResult<()> {
        self.dismiss_popup()?;
        if let Some(connection) = self.connection.get() {
            if !self.app().conflicts.with(|c| c.is_empty()) {
                let panel = self.conflicts_panel()?;
                self.show_popup(&panel, &connection, None, false)?;
            }
        }
        Ok(())
    }
    /// `void showSelectionPanel()`.
    pub fn show_selection_panel(self: &Arc<Self>) -> AppResult<()> {
        self.dismiss_popup()?;
        if self.tool_named("select").is_some() {
            let panel = self.selection_panel()?;
            self.show_popup_at_tool(&panel, "select")?;
        }
        Ok(())
    }
    /// `private void showPopup(LinearLayout body, View anchor, boolean fromRight)`.
    /// `anchor_tool` is the anchor's Rust `Tool` when the Java anchor was a `Tool`.
    /// `fromRight` was unused in Java too.
    #[allow(clippy::only_used_in_recursion)]
    fn show_popup(self: &Arc<Self>, body: &Obj, anchor: &Obj, anchor_tool: Option<Arc<Tool>>, from_right: bool) -> AppResult<()> {
        let anchor_tool = anchor_tool.or_else(|| self.tool_for(anchor));
        let root = self.root_obj()?;
        if ui::width(&root)?.0 <= 0 || !jcall!(root, "android/view/View", "isAttachedToWindow", "()Z")?.bool() {
            let ui = self.clone();
            let (body, anchor) = (body.clone(), anchor.clone());
            let retry = jx::try_once_runnable(move || ui.show_popup(&body, &anchor, anchor_tool, from_right).map_err(JavaError::from));
            ui::view_post(&root, &retry)?;
            return Ok(());
        }
        self.dismiss_popup()?;
        let ctx = self.context();
        let location = |view: &Obj| -> AppResult<[i32; 2]> {
            let array = jx::new_int_array(&[0, 0])?;
            jcall!(view, "android/view/View", "getLocationOnScreen", "([I)V", &array)?;
            let values = jx::int_array(array.as_obj());
            Ok([values[0], values[1]])
        };
        let root_position = location(&root)?;
        let position = location(anchor)?;
        let top = anchor_tool.as_ref().is_some_and(|t| t.top.get());
        let padding = |name: &str| -> AppResult<i32> {
            Ok(match name {
                "left" => jcall!(root, "android/view/View", "getPaddingLeft", "()I")?.int(),
                "right" => jcall!(root, "android/view/View", "getPaddingRight", "()I")?.int(),
                "top" => jcall!(root, "android/view/View", "getPaddingTop", "()I")?.int(),
                _ => jcall!(root, "android/view/View", "getPaddingBottom", "()I")?.int(),
            })
        };
        let root_width = ui::width(&root)?.0;
        let root_height = ui::height(&root)?.0;
        let anchor_width = ui::width(anchor)?.0;
        let anchor_height = ui::height(anchor)?.0;
        let left = root_position[0] + padding("left")? + self.dp(8.0)?;
        let right = root_position[0] + root_width - padding("right")? - self.dp(8.0)?;
        let mut top_limit = root_position[1] + padding("top")? + self.dp(8.0)?;
        let bottom = root_position[1] + root_height - padding("bottom")? - self.dp(8.0)?;
        let mut panel_left = if top { left } else { left.max(position[0] + anchor_width + self.dp(8.0)?) };
        let tag = jcall!(body, "android/view/View", "getTag", "()Ljava/lang/Object;")?.global();
        let requested = match tag {
            Some(t) if is_instance(&t, "java/lang/Integer") => jx::unbox_int(t.as_obj()),
            _ => 320,
        };
        let mut popup_width = self.dp(requested as f32)?.min(right - panel_left);
        if popup_width < self.dp(220.0)? {
            panel_left = left;
            popup_width = self.dp(requested as f32)?.min(right - left);
        }
        if top {
            top_limit = (bottom - self.dp(48.0)?).min(position[1] + anchor_height + self.dp(8.0)?);
        }
        let width_spec = jstatic!("android/view/View$MeasureSpec", "makeMeasureSpec", "(II)I", popup_width, MEASURE_SPEC_EXACTLY)?.int();
        let height_spec = jstatic!("android/view/View$MeasureSpec", "makeMeasureSpec", "(II)I", 0, MEASURE_SPEC_UNSPECIFIED)?.int();
        jcall!(body, "android/view/View", "measure", "(II)V", width_spec, height_spec)?;
        let natural_height = jcall!(body, "android/view/View", "getMeasuredHeight", "()I")?.int();
        let max_height = self.dp(48.0)?.max(bottom - top_limit);
        let popup_height = natural_height.min(max_height);
        let mut content = body.clone();
        if natural_height > max_height {
            let scroll = scroll_view(&ctx)?;
            set_fill_viewport(&scroll, false)?;
            set_vertical_scroll_bar_enabled(&scroll, true)?;
            ui::add_view_params(&scroll, body, &ui::frame_params(ui::LayoutSize { width: -1, height: -2 })?)?;
            content = scroll;
        }
        let (_, shell) = PopupShell::create(self)?;
        ui::set_padding(&shell, ui::PxInsets { left: self.dp(1.0)?, top: self.dp(1.0)?, right: self.dp(1.0)?, bottom: self.dp(1.0)? })?;
        set_background(&shell, &self.surface(COLOR_WHITE, INK, 8, 0)?)?;
        jcall!(shell, "android/view/View", "setClipToOutline", "(Z)V", true)?;
        ui::add_view_params(&shell, &content, &ui::frame_params(ui::LayoutSize { width: -1, height: -1 })?)?;
        let popup = jnew!("android/widget/PopupWindow", "(Landroid/view/View;IIZ)V", &shell, popup_width, popup_height, true)?;
        self.popup.set(Some(popup.clone()));
        let transparent = jnew!("android/graphics/drawable/ColorDrawable", "(I)V", COLOR_TRANSPARENT)?;
        jcall!(popup, "android/widget/PopupWindow", "setBackgroundDrawable", "(Landroid/graphics/drawable/Drawable;)V", &transparent)?;
        jcall!(popup, "android/widget/PopupWindow", "setOutsideTouchable", "(Z)V", true)?;
        jcall!(popup, "android/widget/PopupWindow", "setElevation", "(F)V", 0.0f32)?;
        jcall!(popup, "android/widget/PopupWindow", "setAnimationStyle", "(I)V", 0)?;
        let x = if top { left.max((position[0] + anchor_width - popup_width).min(right - popup_width)) } else { panel_left };
        let y = if top { top_limit } else { top_limit.max(position[1].min(bottom - popup_height)) };
        self.popup_anchor.set(anchor_tool.clone());
        if let Some(tool) = &anchor_tool {
            tool.open.set(true);
            ui::invalidate(&tool.view())?;
        }
        let displayed_layers = self.layer_list.get().map(|l| WeakObj::new(&l));
        let displayed_sample = self.sample.get().map(|s| Arc::downgrade(&s));
        let w = Arc::downgrade(self);
        let on_dismiss = jx::callback(move |method, _| {
            if method != "onDismiss()" {
                return Ok(Reply::Void);
            }
            let Some(ui) = w.upgrade() else { return Ok(Reply::Void) };
            if let Some(tool) = ui.popup_anchor.get() {
                tool.open.set(false);
                ui::invalidate(&tool.view())?;
                ui.popup_anchor.set(None);
            }
            let same_layers = match (ui.layer_list.get(), &displayed_layers) {
                (None, None) => true,
                (Some(current), Some(shown)) => shown.upgrade().is_some_and(|s| s.same(&current)),
                _ => false,
            };
            if same_layers {
                ui.layer_list.set(None);
                ui.previews.with_mut(|p| p.clear());
            }
            let same_sample = match (ui.sample.get(), &displayed_sample) {
                (None, None) => true,
                (Some(current), Some(shown)) => std::ptr::eq(Arc::as_ptr(&current), shown.as_ptr()),
                _ => false,
            };
            if same_sample {
                ui.sample.set(None);
            }
            Ok(Reply::Void)
        });
        jcall!(popup, "android/widget/PopupWindow", "setOnDismissListener", "(Landroid/widget/PopupWindow$OnDismissListener;)V", &on_dismiss)?;
        jcall!(popup, "android/widget/PopupWindow", "showAtLocation", "(Landroid/view/View;III)V", &root, GRAVITY_TOP | GRAVITY_LEFT, x, y)?;
        Ok(())
    }
    /// `private Icon icon(String kind, String description, Runnable callback)`.
    fn icon(self: &Arc<Self>, kind: &str, description: &str, callback: Action) -> AppResult<Arc<Icon>> {
        let (icon, view) = Icon::create(self, kind)?;
        ui::set_content_description(&view, &ui::UiText::from(description))?;
        on_click(&view, move |_| callback())?;
        pin_local(&view);
        Ok(icon)
    }
}

/// Routes a `NativeView`-family callback to `f(canvas)` for `method`.
fn draw_dispatch(method: &str, args: &[Local], expected: &str, f: impl FnOnce(&Obj) -> AppResult<()>) -> JResult<Reply> {
    if method == expected {
        if let Some(canvas) = args.get(1).and_then(view_from) {
            f(&canvas)?;
        }
    }
    Ok(Reply::Void)
}

/// `private final class Tool extends LinearLayout` (`NativeLinearLayout`, mask DISPATCH_DRAW).
pub struct Tool {
    pub owner: Weak<EditorUi>,
    pub view: OnceLock<WeakObj>,
    pub icon: Arc<Icon>,
    /// `final TextView label` (same view tree: weak).
    pub label: WeakObj,
    pub selected: Field<bool>,
    pub open: Field<bool>,
    pub top: Field<bool>,
    pub marker: Obj,
}
impl Tool {
    /// The constructor, also returning a strong reference to the new view.
    fn create(owner: &Arc<EditorUi>, kind: &str, name: &str, description: &str) -> AppResult<(Arc<Tool>, Obj)> {
        let marker = new_paint()?;
        let (icon, icon_view) = Icon::create(owner, kind)?;
        let label = owner.text(name, 11, INK, false)?;
        let tool = Arc::new(Tool {
            owner: Arc::downgrade(owner),
            view: OnceLock::new(),
            icon,
            label: WeakObj::new(&label),
            selected: Field::new(false),
            open: Field::new(false),
            top: Field::new(false),
            marker,
        });
        let handle = tool.clone();
        let view = jx::native_linear_layout(&owner.context(), jx::mask::DISPATCH_DRAW, move |m, a| handle.dispatch(m, a))?;
        let _ = tool.view.set(WeakObj::new(&view));
        jcall!(view, "android/widget/LinearLayout", "setOrientation", "(I)V", LINEARLAYOUT_VERTICAL)?;
        linear_set_gravity(&view, GRAVITY_CENTER)?;
        ui::set_content_description(&view, &ui::UiText::from(description))?;
        text_set_gravity(&label, GRAVITY_CENTER)?;
        set_single_line(&label, true)?;
        set_ellipsize_end(&label)?;
        ui::set_visibility(&label, if owner.show_tool_labels.get() { VIEW_VISIBLE } else { VIEW_GONE })?;
        ui::add_view_params(&view, &icon_view, &ui::linear_params(ui::LayoutSize { width: owner.dp(40.0)?, height: owner.dp(40.0)? })?)?;
        ui::add_view_params(&view, &label, &ui::linear_params(ui::LayoutSize { width: -1, height: -2 })?)?;
        ui::set_padding(&view, ui::PxInsets { left: owner.dp(2.0)?, top: 0, right: owner.dp(2.0)?, bottom: owner.dp(6.0)? })?;
        ui::set_background_color(&view, notemaker_core::style::ArgbColor(PAPER))?;
        Ok((tool, view))
    }
    pub fn view(&self) -> Obj {
        self.view.get().expect("Tool not constructed").get()
    }
    /// Native dispatch: `dispatchDraw(Canvas)`.
    fn dispatch(self: &Arc<Self>, method: &str, args: &[Local]) -> JResult<Reply> {
        draw_dispatch(method, args, "dispatchDraw(Canvas)", |c| self.dispatch_draw(c))
    }
    /// `protected void dispatchDraw(Canvas canvas)` (after `super.dispatchDraw`).
    pub fn dispatch_draw(&self, canvas: &Obj) -> AppResult<()> {
        let view = self.view();
        super_dispatch_draw_linear(&view, canvas)?;
        let Some(owner) = self.owner.upgrade() else { return Ok(()) };
        set_color(&self.marker, INK)?;
        let (w, h) = (ui::width(&view)?.0, ui::height(&view)?.0);
        if self.selected.get() {
            draw_circle(canvas, w as f32 / 2.0, (h - owner.dp(4.0)?) as f32, owner.dp(2.5)? as f32, &self.marker)?;
        }
        if self.open.get() {
            let triangle = new_path()?;
            let (wf, hf) = (w as f32, h as f32);
            if self.top.get() {
                move_to(&triangle, wf / 2.0 - owner.dp(5.0)? as f32, hf)?;
                line_to(&triangle, wf / 2.0, (h - owner.dp(6.0)?) as f32)?;
                line_to(&triangle, wf / 2.0 + owner.dp(5.0)? as f32, hf)?;
            } else {
                move_to(&triangle, wf, hf / 2.0 - owner.dp(5.0)? as f32)?;
                line_to(&triangle, (w - owner.dp(6.0)?) as f32, hf / 2.0)?;
                line_to(&triangle, wf, hf / 2.0 + owner.dp(5.0)? as f32)?;
            }
            close_path(&triangle)?;
            draw_path(canvas, &triangle, &self.marker)?;
        }
        Ok(())
    }
    /// `void active(boolean value)`.
    fn active(&self, value: bool) -> AppResult<()> {
        if self.selected.get() == value {
            return Ok(());
        }
        self.selected.set(value);
        let view = self.view();
        jcall!(view, "android/view/View", "setSelected", "(Z)V", value)?;
        ui::invalidate(&view)?;
        Ok(())
    }
}

/// `private final class LayerPreview extends View` (`NativeView`, mask DRAW).
pub struct LayerPreview {
    pub owner: Weak<EditorUi>,
    pub view: OnceLock<WeakObj>,
    pub layer: String,
    pub number: i32,
    pub visible: bool,
    pub paint: Obj,
    pub raster: Field<Option<InkCache>>,
    pub raster_key: Field<String>,
}
impl LayerPreview {
    fn create(owner: &Arc<EditorUi>, layer: &str, number: i32, visible: bool) -> AppResult<(Arc<LayerPreview>, Obj)> {
        let preview = Arc::new(LayerPreview {
            owner: Arc::downgrade(owner),
            view: OnceLock::new(),
            layer: layer.to_string(),
            number,
            visible,
            paint: new_paint()?,
            raster: Field::new(None),
            raster_key: Field::new(String::new()),
        });
        let handle = preview.clone();
        let view = jx::native_view(&owner.context(), jx::mask::DRAW, move |m, a| handle.dispatch(m, a))?;
        let _ = preview.view.set(WeakObj::new(&view));
        Ok((preview, view))
    }
    pub fn view(&self) -> Obj {
        self.view.get().expect("LayerPreview not constructed").get()
    }
    /// Native dispatch: `onDraw(Canvas)`.
    fn dispatch(self: &Arc<Self>, method: &str, args: &[Local]) -> JResult<Reply> {
        draw_dispatch(method, args, "onDraw(Canvas)", |c| self.on_draw(c))
    }
    /// `protected void onDraw(Canvas c)`.
    pub fn on_draw(&self, c: &Obj) -> AppResult<()> {
        let Some(owner) = self.owner.upgrade() else { return Ok(()) };
        let app = owner.app();
        let view = self.view();
        let (wi, hi) = (ui::width(&view)?.0, ui::height(&view)?.0);
        let (w, h) = (wi as f32, hi as f32);
        let paint = &self.paint;
        set_style(paint, Style::Fill)?;
        set_color(paint, if self.visible { COLOR_WHITE } else { 0xffecefe9u32 as i32 })?;
        draw_rect(c, 1.0, 1.0, w - 1.0, h - 1.0, paint)?;
        let scale_factor = fmin((w - owner.dp(6.0)? as f32) / 1000.0, (h - owner.dp(20.0)? as f32) / 1400.0);
        let key = format!("{}/{}/{}/{}", app.document.get(), app.page.get(), wi, hi);
        if key != self.raster_key.get() {
            if let Some(old) = self.raster.take() {
                jcall!(old.bitmap, "android/graphics/Bitmap", "recycle", "()V")?;
            }
            self.raster.set(Some(InkCache::new(crate::ink_cache::BitmapSize { width: wi, height: hi })?));
            self.raster_key.set(key);
        }
        let canvas = app.canvas();
        let current = canvas.current.get();
        let page = app.page.get();
        let min_width = owner.dp(1.4)? as f32 / scale_factor;
        let mut settled: IndexMap<String, Entity> = IndexMap::new();
        let model = app.model();
        for (key, stroke) in model.strokes.iter() {
            if current.as_deref() != Some(key.as_str())
                && stroke.get("layer").and_then(Value::as_str) == Some(self.layer.as_str())
                && json::as_int(stroke.get("page"))? == page
                && !app.active.with(|a| a.get(key) == Some(&false))
            {
                let mut preview: Map = (**stroke).clone();
                let width = jmath::max(json::as_double(stroke.get("width"))?, min_width as f64);
                preview.insert("width".into(), json::double(width));
                settled.insert(key.clone(), Arc::new(preview));
            }
        }
        let dp16 = owner.dp(16.0)? as f32;
        let mut raster = self.raster.take().ok_or_else(npe)?;
        let drawn = raster.draw(
            c,
            crate::ink_cache::VisibleInk(settled),
            &mut |image: &Obj| {
                clip_rect(image, 1.0, dp16, w - 1.0, h - 1.0)?;
                translate(image, (w - 1000.0 * scale_factor) / 2.0, dp16)?;
                scale(image, scale_factor, scale_factor)
            },
            &mut |image: &Obj, stroke: &Entity| {
                render::draw_stroke(stroke, &mut Png::new(&mut AndroidCanvas::new(image.clone())))?;
                Ok(())
            },
        );
        self.raster.set(Some(raster));
        drawn?;
        canvas_save(c)?;
        clip_rect(c, 1.0, dp16, w - 1.0, h - 1.0)?;
        translate(c, (w - 1000.0 * scale_factor) / 2.0, dp16)?;
        scale(c, scale_factor, scale_factor)?;
        if self.layer == app.selected.get() && canvas.points.with(|p| p.is_some()) {
            canvas.draw_stroke(c, &canvas.stroke())?;
        }
        {
            let _lock = app.lock.enter();
            let model = app.model();
            let page = app.page.get();
            for (key, o) in model.objects.iter() {
                if o.get("layer").and_then(Value::as_str) == Some(self.layer.as_str())
                    && json::as_int(o.get("page"))? == page
                    && model.object_active.get(key) != Some(&false)
                {
                    render::draw_object(&model, o, &mut Png::new(&mut AndroidCanvas::new(c.clone())))?;
                }
            }
        }
        canvas_restore(c)?;
        set_style(paint, Style::Stroke)?;
        set_stroke_width(paint, owner.dp(1.0)? as f32)?;
        set_color(paint, LINE)?;
        draw_rect(c, 1.0, 1.0, w - 1.0, h - 1.0, paint)?;
        if !self.visible {
            set_color(paint, MUTED)?;
            draw_line(c, owner.dp(4.0)? as f32, h - owner.dp(4.0)? as f32, w - owner.dp(4.0)? as f32, owner.dp(4.0)? as f32, paint)?;
        }
        set_style(paint, Style::Fill)?;
        set_color(paint, PAPER)?;
        draw_rect(c, 1.0, 1.0, owner.dp(17.0)? as f32, owner.dp(17.0)? as f32, paint)?;
        set_color(paint, MUTED)?;
        jcall!(paint, "android/graphics/Paint", "setTextSize", "(F)V", owner.dp(10.0)? as f32)?;
        draw_text(c, &self.number.to_string(), owner.dp(5.0)? as f32, owner.dp(12.0)? as f32, paint)?;
        Ok(())
    }
}

/// `private final class StrokeSample extends View` (`NativeView`, mask DRAW).
pub struct StrokeSample {
    pub owner: Weak<EditorUi>,
    pub view: OnceLock<WeakObj>,
    pub paint: Obj,
}
impl StrokeSample {
    /// `StrokeSample()`: also sets `EditorUi.sample = this`.
    pub fn new(owner: &Arc<EditorUi>) -> AppResult<Arc<StrokeSample>> {
        let (sample, view) = StrokeSample::create(owner)?;
        pin_local(&view);
        Ok(sample)
    }
    fn create(owner: &Arc<EditorUi>) -> AppResult<(Arc<StrokeSample>, Obj)> {
        let sample = Arc::new(StrokeSample { owner: Arc::downgrade(owner), view: OnceLock::new(), paint: new_paint()? });
        let handle = sample.clone();
        let view = jx::native_view(&owner.context(), jx::mask::DRAW, move |m, a| handle.dispatch(m, a))?;
        let _ = sample.view.set(WeakObj::new(&view));
        owner.sample.set(Some(sample.clone()));
        ui::set_content_description(&view, &ui::UiText::from("Stroke preview"))?;
        Ok((sample, view))
    }
    pub fn view(&self) -> Obj {
        self.view.get().expect("StrokeSample not constructed").get()
    }
    /// Native dispatch: `onDraw(Canvas)`.
    fn dispatch(self: &Arc<Self>, method: &str, args: &[Local]) -> JResult<Reply> {
        draw_dispatch(method, args, "onDraw(Canvas)", |c| self.on_draw(c))
    }
    /// `protected void onDraw(Canvas c)`.
    pub fn on_draw(&self, c: &Obj) -> AppResult<()> {
        use std::f64::consts::PI;
        let Some(owner) = self.owner.upgrade() else { return Ok(()) };
        let app = owner.app();
        let view = self.view();
        let w = ui::width(&view)?.0 as f32;
        let h = ui::height(&view)?.0 as f32;
        let left = owner.dp(14.0)? as f32;
        let right = w - owner.dp(14.0)? as f32;
        let mid = h * 0.52f32;
        let amplitude = fmin(owner.dp(13.0)? as f32, h * 0.27f32);
        let mut points: Vec<Value> = Vec::with_capacity(161);
        for i in 0..=160 {
            let t = i as f64 / 160.0;
            let x = left as f64 + (right - left) as f64 * t;
            let y = mid as f64 + (t * PI * 3.0).sin() * amplitude as f64 * (0.82 + 0.18 * (t * PI * 2.0).cos());
            let pressure = 0.35 + 0.55 * (0.5 + 0.5 * (t * PI * 2.0 - 0.7).sin());
            points.push(Value::Array(vec![json::double(x), json::double(y), json::double(pressure)]));
        }
        let mut stroke = Map::new();
        stroke.insert("kind".into(), Value::from("stroke"));
        stroke.insert("brush".into(), Value::from(app.brush.get()));
        stroke.insert("color".into(), Value::from(app.color.get()));
        stroke.insert("material".into(), Value::from(app.material.get()));
        stroke.insert("width".into(), Value::from(owner.dp(fmax(1.0, fmin(8.0, app.width.get())))?));
        stroke.insert("points".into(), Value::Array(points));
        canvas_save(c)?;
        clip_rect(c, left - owner.dp(2.0)? as f32, owner.dp(3.0)? as f32, right + owner.dp(2.0)? as f32, h - owner.dp(3.0)? as f32)?;
        render::draw_stroke(&stroke, &mut Png::new(&mut AndroidCanvas::new(c.clone())))?;
        canvas_restore(c)?;
        Ok(())
    }
}

/// `private final class ConflictPreview extends View` (`NativeView`, mask DRAW).
pub struct ConflictPreview {
    pub owner: Weak<EditorUi>,
    pub view: OnceLock<WeakObj>,
    /// `final Object raw` (a version map, or null/other).
    pub raw: Value,
    pub paint: Obj,
}
impl ConflictPreview {
    fn create(owner: &Arc<EditorUi>, raw: Value) -> AppResult<(Arc<ConflictPreview>, Obj)> {
        let preview = Arc::new(ConflictPreview { owner: Arc::downgrade(owner), view: OnceLock::new(), raw, paint: new_paint()? });
        let handle = preview.clone();
        let view = jx::native_view(&owner.context(), jx::mask::DRAW, move |m, a| handle.dispatch(m, a))?;
        let _ = preview.view.set(WeakObj::new(&view));
        ui::set_background_color(&view, notemaker_core::style::ArgbColor(0xffffffffu32 as i32))?;
        ui::set_content_description(&view, &ui::UiText::from("Conflict version preview"))?;
        Ok((preview, view))
    }
    pub fn view(&self) -> Obj {
        self.view.get().expect("ConflictPreview not constructed").get()
    }
    /// Native dispatch: `onDraw(Canvas)`.
    fn dispatch(self: &Arc<Self>, method: &str, args: &[Local]) -> JResult<Reply> {
        draw_dispatch(method, args, "onDraw(Canvas)", |c| self.on_draw(c))
    }
    /// `protected void onDraw(Canvas canvas)`.
    pub fn on_draw(&self, canvas: &Obj) -> AppResult<()> {
        let view = self.view();
        super_on_draw(&view, canvas)?;
        let Some(owner) = self.owner.upgrade() else { return Ok(()) };
        let paint = &self.paint;
        let Value::Object(value) = &self.raw else {
            draw_text(canvas, "No version", owner.dp(8.0)? as f32, owner.dp(24.0)? as f32, paint)?;
            return Ok(());
        };
        let body = || -> AppResult<()> {
            let (mut left, mut top, mut right, mut bottom) = (0.0f64, 0.0f64, 100.0f64, 100.0f64);
            if value.contains_key("points") {
                let points = json::list(value.get("points").unwrap_or(&Value::Null))?;
                if points.is_empty() {
                    return Ok(());
                }
                left = f64::INFINITY;
                top = f64::INFINITY;
                right = f64::NEG_INFINITY;
                bottom = f64::NEG_INFINITY;
                for item in points {
                    let point = json::list(item)?;
                    left = jmath::min(left, json::as_double(point.first())?);
                    right = jmath::max(right, json::as_double(point.first())?);
                    top = jmath::min(top, json::as_double(point.get(1))?);
                    bottom = jmath::max(bottom, json::as_double(point.get(1))?);
                }
            } else if value.contains_key("x") {
                left = json::as_double(value.get("x"))?;
                top = json::as_double(value.get("y"))?;
                right = left + json::as_double(value.get("w"))?;
                bottom = top + json::as_double(value.get("h"))?;
            }
            let gw = ui::width(&view)?.0;
            let gh = ui::height(&view)?.0;
            let dp16 = owner.dp(16.0)?;
            let width = jmath::max(24.0, right - left);
            let height = jmath::max(24.0, bottom - top);
            let mut s = jmath::min((gw - dp16) as f64 / width, (gh - dp16) as f64 / height);
            s = jmath::max(0.01, jmath::min(4.0, s));
            canvas_save(canvas)?;
            translate(canvas, ((gw as f64 - width * s) / 2.0 - left * s) as f32, ((gh as f64 - height * s) / 2.0 - top * s) as f32)?;
            scale(canvas, s as f32, s as f32)?;
            if value.contains_key("points") {
                let black = Value::from(COLOR_BLACK);
                let color = json::as_int(Some(json::get_or_default(value, "color", &black)))?;
                set_color(paint, color)?;
                set_style(paint, Style::Stroke)?;
                let four = Value::from(4);
                set_stroke_width(paint, json::as_double(Some(json::get_or_default(value, "width", &four)))? as f32)?;
                set_cap(paint, Cap::Round)?;
                set_join(paint, Join::Round)?;
                let points = json::list(value.get("points").unwrap_or(&Value::Null))?;
                let path = new_path()?;
                for (i, item) in points.iter().enumerate() {
                    let point = json::list(item)?;
                    let (x, y) = (float_value(point.first())?, float_value(point.get(1))?);
                    if i == 0 {
                        move_to(&path, x, y)?;
                    } else {
                        line_to(&path, x, y)?;
                    }
                }
                draw_path(canvas, &path, paint)?;
                set_style(paint, Style::Fill)?;
            } else {
                render::draw_object(&owner.app().model(), value, &mut Png::new(&mut AndroidCanvas::new(canvas.clone())))?;
            }
            canvas_restore(canvas)
        };
        // catch (Exception ignored)
        let _ = body();
        Ok(())
    }
}

/// `private final class Icon extends View` (`NativeView`, mask DRAW):
/// small vector icons stay crisp and monochrome on E Ink.
pub struct Icon {
    pub owner: Weak<EditorUi>,
    pub view: OnceLock<WeakObj>,
    pub kind: Field<String>,
    pub color: Field<i32>,
    pub accent_color: Field<i32>,
    pub paint: Obj,
}
impl Icon {
    fn create(owner: &Arc<EditorUi>, kind: &str) -> AppResult<(Arc<Icon>, Obj)> {
        let icon = Arc::new(Icon {
            owner: Arc::downgrade(owner),
            view: OnceLock::new(),
            kind: Field::new(kind.to_string()),
            color: Field::new(INK),
            accent_color: Field::new(INK),
            paint: new_paint()?,
        });
        let handle = icon.clone();
        let view = jx::native_view(&owner.context(), jx::mask::DRAW, move |m, a| handle.dispatch(m, a))?;
        let _ = icon.view.set(WeakObj::new(&view));
        Ok((icon, view))
    }
    pub fn view(&self) -> Obj {
        self.view.get().expect("Icon not constructed").get()
    }
    /// Native dispatch: `onDraw(Canvas)`.
    fn dispatch(self: &Arc<Self>, method: &str, args: &[Local]) -> JResult<Reply> {
        draw_dispatch(method, args, "onDraw(Canvas)", |c| self.on_draw(c))
    }
    /// `protected void onDraw(Canvas canvas)`.
    pub fn on_draw(&self, canvas: &Obj) -> AppResult<()> {
        let c = canvas;
        let p = &self.paint;
        let view = self.view();
        let (gw, gh) = (ui::width(&view)?.0, ui::height(&view)?.0);
        canvas_save(c)?;
        let size = gw.min(gh) as f32 * 0.62f32;
        translate(c, (gw as f32 - size) / 2.0, (gh as f32 - size) / 2.0)?;
        scale(c, size / 24.0, size / 24.0)?;
        set_color(p, INK)?;
        set_style(p, Style::Stroke)?;
        set_stroke_width(p, 1.7)?;
        set_cap(p, Cap::Round)?;
        set_join(p, Join::Round)?;
        let kind = self.kind.get();
        macro_rules! path {
            ($($v:expr),+ $(,)?) => { self.path(c, &[$($v as f32),+])? };
        }
        match kind.as_str() {
            "agent" => {
                draw_round_rect(c, 4.0, 7.0, 20.0, 21.0, 3.0, 3.0, p)?;
                path!(12, 7, 12, 4);
                draw_circle(c, 12.0, 2.5, 1.5, p)?;
                path!(2, 11, 2, 17);
                path!(22, 11, 22, 17);
                path!(9, 17, 15, 17);
                set_style(p, Style::Fill)?;
                draw_circle(c, 8.5, 12.0, 1.5, p)?;
                draw_circle(c, 15.5, 12.0, 1.5, p)?;
            }
            "undo" => {
                path!(8, 5, 3, 10, 8, 15);
                path!(3, 10, 14, 10);
                draw_arc(c, 9.0, 10.0, 21.0, 22.0, 270.0, 150.0, false, p)?;
            }
            "redo" => {
                path!(16, 5, 21, 10, 16, 15);
                path!(21, 10, 10, 10);
                draw_arc(c, 3.0, 10.0, 15.0, 22.0, 270.0, -150.0, false, p)?;
            }
            "check" => path!(4, 12, 10, 18, 21, 5),
            "layers" => {
                path!(2, 10, 12, 3, 22, 10, 12, 17, 2, 10);
                path!(3, 15, 12, 22, 21, 15);
            }
            "template" => {
                draw_rect(c, 3.0, 3.0, 21.0, 21.0, p)?;
                let mut y = 8;
                while y < 22 {
                    path!(6, y, 18, y);
                    y += 5;
                }
            }
            "insert" => {
                draw_rect(c, 3.0, 3.0, 21.0, 21.0, p)?;
                draw_circle(c, 8.0, 8.0, 2.0, p)?;
                path!(4, 19, 11, 12, 15, 16, 19, 11, 21, 13);
            }
            "fountain" => {
                path!(12, 2, 5, 15, 8, 20, 16, 20, 19, 15, 12, 2);
                path!(12, 3, 12, 12);
                draw_circle(c, 12.0, 14.0, 2.0, p)?;
            }
            "pencil" => {
                path!(5, 21, 5, 10, 12, 2, 19, 10, 19, 21, 5, 21);
                path!(5, 10, 9, 13, 12, 10, 15, 13, 19, 10);
                path!(12, 11, 12, 21);
            }
            "marker" => {
                path!(4, 21, 4, 12, 20, 12, 20, 21, 4, 21);
                path!(7, 12, 7, 6, 17, 3, 17, 12);
            }
            "brush" => {
                let brush = new_path()?;
                move_to(&brush, 9.0, 16.0)?;
                cubic_to(&brush, 2.0, 11.0, 13.0, 8.0, 11.0, 2.0)?;
                cubic_to(&brush, 22.0, 10.0, 17.0, 16.0, 15.0, 16.0)?;
                draw_path(c, &brush, p)?;
                draw_rect(c, 6.0, 17.0, 18.0, 22.0, p)?;
            }
            "pen" => {
                path!(4, 20, 6, 13, 16, 3, 21, 8, 11, 18, 4, 20);
                path!(14, 5, 19, 10);
            }
            "eraser" => {
                path!(3, 14, 13, 4, 21, 12, 13, 20, 9, 20, 3, 14);
                path!(7, 10, 15, 18);
                path!(13, 20, 22, 20);
            }
            "move" => {
                path!(12, 2, 12, 22);
                path!(2, 12, 22, 12);
                path!(8, 6, 12, 2, 16, 6);
                path!(8, 18, 12, 22, 16, 18);
                path!(6, 8, 2, 12, 6, 16);
                path!(18, 8, 22, 12, 18, 16);
            }
            "history" => {
                draw_arc(c, 4.0, 4.0, 21.0, 21.0, 210.0, 290.0, false, p)?;
                path!(3, 3, 3, 9, 9, 9);
                path!(12, 7, 12, 13, 16, 15);
            }
            "document" | "page" => {
                path!(5, 2, 15, 2, 20, 7, 20, 22, 5, 22, 5, 2);
                path!(15, 2, 15, 7, 20, 7);
                path!(9, 12, 16, 12);
                path!(9, 16, 16, 16);
            }
            "text" => {
                path!(5, 5, 19, 5);
                path!(12, 5, 12, 20);
                path!(8, 20, 16, 20);
            }
            "select" => {
                let intervals = jx::new_float_array(&[3.0, 2.0])?;
                let dash = jnew!("android/graphics/DashPathEffect", "([FF)V", &intervals, 0.0f32)?;
                jcall!(p, "android/graphics/Paint", "setPathEffect", "(Landroid/graphics/PathEffect;)Landroid/graphics/PathEffect;", &dash)?;
                draw_rect(c, 4.0, 4.0, 20.0, 20.0, p)?;
                jcall!(p, "android/graphics/Paint", "setPathEffect", "(Landroid/graphics/PathEffect;)Landroid/graphics/PathEffect;", jx::NULL)?;
                path!(12, 4, 12, 20);
                path!(4, 12, 20, 12);
            }
            "interact" => {
                draw_circle(c, 15.0, 5.0, 2.0, p)?;
                path!(15, 7, 15, 15, 19, 11, 21, 13, 21, 18, 17, 21, 10, 21, 5, 16, 5, 13, 8, 13, 11, 16, 11, 6);
            }
            "fill" => {
                let fill = new_path()?;
                move_to(&fill, 12.0, 3.0)?;
                cubic_to(&fill, 9.0, 8.0, 4.0, 12.0, 4.0, 16.0)?;
                cubic_to(&fill, 4.0, 22.0, 20.0, 22.0, 20.0, 16.0)?;
                cubic_to(&fill, 20.0, 12.0, 15.0, 8.0, 12.0, 3.0)?;
                draw_path(c, &fill, p)?;
            }
            "shapes" => {
                draw_rect(c, 3.0, 4.0, 13.0, 14.0, p)?;
                draw_oval(c, 11.0, 10.0, 21.0, 20.0, p)?;
                path!(4, 21, 10, 13, 16, 21, 4, 21);
            }
            "rect" => draw_rect(c, 4.0, 5.0, 20.0, 19.0, p)?,
            "ellipse" => draw_oval(c, 4.0, 5.0, 20.0, 19.0, p)?,
            "circle" => draw_circle(c, 12.0, 12.0, 8.0, p)?,
            "rectangle" => draw_rect(c, 4.0, 5.0, 20.0, 19.0, p)?,
            "line" => path!(4, 19, 20, 5),
            "arrow" => {
                path!(4, 19, 19, 5, 19, 12);
                path!(19, 5, 12, 5);
            }
            "triangle" => path!(12, 4, 21, 20, 3, 20, 12, 4),
            "triangle45" => {
                path!(4, 20, 20, 20, 20, 4, 4, 20);
                path!(15, 20, 20, 15);
            }
            "triangle60" => path!(12, 3, 21, 20, 3, 20, 12, 3),
            "triangle90" => {
                path!(4, 3, 4, 20, 21, 20, 4, 3);
                path!(4, 16, 8, 16, 8, 20);
            }
            "diamond" => path!(12, 3, 21, 12, 12, 21, 3, 12, 12, 3),
            "rhombus" => path!(12, 3, 21, 12, 12, 21, 3, 12, 12, 3),
            "pentagon" => self.poly(c, 5, 12.0)?,
            "hexagon" => self.poly(c, 6, 12.0)?,
            "regularHexagon" => self.poly(c, 6, 12.0)?,
            "regularPolygon" => self.poly(c, 8, 11.0)?,
            "star" => self.poly(c, 10, 12.0)?,
            "cube" => {
                path!(4, 8, 12, 3, 20, 8, 12, 13, 4, 8);
                path!(4, 8, 4, 18, 12, 22, 12, 13);
                path!(20, 8, 20, 18, 12, 22);
            }
            "pyramid" => {
                path!(12, 3, 3, 20, 21, 20, 12, 3);
                path!(12, 3, 12, 20);
                path!(3, 20, 15, 12);
                path!(21, 20, 9, 12);
            }
            "cylinder" => {
                draw_oval(c, 5.0, 3.0, 19.0, 9.0, p)?;
                path!(5, 6, 5, 18);
                path!(19, 6, 19, 18);
                draw_arc(c, 5.0, 15.0, 19.0, 21.0, 0.0, 180.0, false, p)?;
            }
            "cone" => {
                path!(12, 3, 4, 19);
                path!(12, 3, 20, 19);
                draw_oval(c, 4.0, 15.0, 20.0, 21.0, p)?;
            }
            "oily" | "neoBrush" | "charcoal" | "latinCalligraphy" | "asiaCalligraphy" | "square" => {
                path!(4, 20, 6, 13, 16, 3, 21, 8, 11, 18, 4, 20);
                path!(14, 5, 19, 10);
            }
            "sphere" => {
                draw_oval(c, 4.0, 4.0, 20.0, 20.0, p)?;
                path!(4, 13, 9, 10, 15, 10, 20, 13);
            }
            "arrowLine" => {
                path!(4, 19, 19, 5, 19, 12);
                path!(19, 5, 12, 5);
            }
            "trapezoid" => path!(7, 4, 17, 4, 21, 20, 3, 20, 7, 4),
            "wave" | "waveLine" => {
                let wave = new_path()?;
                move_to(&wave, 3.0, 15.0)?;
                cubic_to(&wave, 7.0, 2.0, 11.0, 2.0, 13.0, 13.0)?;
                cubic_to(&wave, 15.0, 24.0, 19.0, 21.0, 21.0, 8.0)?;
                draw_path(c, &wave, p)?;
            }
            "curve" => {
                let curve = new_path()?;
                move_to(&curve, 4.0, 20.0)?;
                cubic_to(&curve, 3.0, 3.0, 20.0, 3.0, 20.0, 20.0)?;
                draw_path(c, &curve, p)?;
            }
            "polyline" => path!(3, 18, 8, 6, 14, 17, 20, 5),
            "eye" | "hidden" => {
                let eye = new_path()?;
                move_to(&eye, 2.0, 12.0)?;
                quad_to(&eye, 12.0, 0.0, 22.0, 12.0)?;
                quad_to(&eye, 12.0, 24.0, 2.0, 12.0)?;
                draw_path(c, &eye, p)?;
                draw_circle(c, 12.0, 12.0, 3.0, p)?;
                if kind == "hidden" {
                    path!(3, 3, 21, 21);
                }
            }
            "lock" => {
                draw_round_rect(c, 5.0, 10.0, 19.0, 21.0, 2.0, 2.0, p)?;
                draw_arc(c, 8.0, 2.0, 16.0, 17.0, 180.0, 180.0, false, p)?;
                path!(12, 14, 12, 17);
            }
            "collapse" => {
                path!(8, 5, 15, 12, 8, 19);
                path!(20, 5, 20, 19);
            }
            "expand" => {
                path!(16, 5, 9, 12, 16, 19);
                path!(4, 5, 4, 19);
            }
            "plus" => {
                path!(12, 4, 12, 20);
                path!(4, 12, 20, 12);
            }
            "more" => {
                set_style(p, Style::Fill)?;
                let mut x = 5;
                while x <= 19 {
                    draw_circle(c, x as f32, 12.0, 1.7, p)?;
                    x += 7;
                }
            }
            _ => self.poly(c, 6, 11.0)?,
        }
        if matches!(
            kind.as_str(),
            "pen" | "oily" | "neoBrush" | "charcoal" | "latinCalligraphy" | "asiaCalligraphy" | "square" | "pencil" | "marker" | "brush" | "fountain"
        ) {
            set_color(p, self.accent_color.get())?;
            set_style(p, Style::Stroke)?;
            set_stroke_width(p, 2.5)?;
            match kind.as_str() {
                "pen" | "oily" | "neoBrush" | "charcoal" | "latinCalligraphy" | "asiaCalligraphy" | "square" => path!(16, 3, 21, 8, 19, 10),
                "pencil" => path!(12, 2, 15, 5, 12, 8, 9, 5, 12, 2),
                "marker" => path!(7, 6, 17, 3, 17, 8, 7, 10),
                "brush" => draw_circle(c, 12.0, 7.0, 2.0, p)?,
                "fountain" => draw_circle(c, 12.0, 14.0, 2.0, p)?,
                _ => {}
            }
        }
        canvas_restore(c)
    }
    /// `void path(Canvas canvas, float... coordinates)`.
    fn path(&self, canvas: &Obj, coordinates: &[f32]) -> AppResult<()> {
        let path = new_path()?;
        move_to(&path, coordinates[0], coordinates[1])?;
        let mut i = 2;
        while i < coordinates.len() {
            line_to(&path, coordinates[i], coordinates[i + 1])?;
            i += 2;
        }
        draw_path(canvas, &path, &self.paint)
    }
    /// `void poly(Canvas canvas, int count, float radius)`.
    fn poly(&self, canvas: &Obj, count: i32, radius: f32) -> AppResult<()> {
        use std::f64::consts::PI;
        let star = self.kind.with(|k| k == "star");
        let path = new_path()?;
        for i in 0..count {
            let r = if star && i % 2 == 1 { radius * 0.45f32 } else { radius };
            let a = -PI / 2.0 + i as f64 * PI * 2.0 / count as f64;
            let x = 12.0f32 + a.cos() as f32 * r;
            let y = 12.0f32 + a.sin() as f32 * r;
            if i == 0 {
                move_to(&path, x, y)?;
            } else {
                line_to(&path, x, y)?;
            }
        }
        close_path(&path)?;
        draw_path(canvas, &path, &self.paint)
    }
}

/// `EditorUi$1`: the `LinearLayout` built by `choice(...)` that draws a
/// selection dot in `dispatchDraw` (`NativeLinearLayout`, mask DISPATCH_DRAW).
pub struct ChoiceItem {
    pub owner: Weak<EditorUi>,
    pub view: OnceLock<WeakObj>,
    /// Captured `BooleanSupplier selected`.
    pub selected: BooleanSupplier,
    /// `final Paint marker`.
    pub marker: Obj,
}
impl ChoiceItem {
    pub fn new(owner: &Arc<EditorUi>, selected: BooleanSupplier) -> AppResult<Arc<ChoiceItem>> {
        let (item, view) = ChoiceItem::create(owner, selected)?;
        pin_local(&view);
        Ok(item)
    }
    fn create(owner: &Arc<EditorUi>, selected: BooleanSupplier) -> AppResult<(Arc<ChoiceItem>, Obj)> {
        let item = Arc::new(ChoiceItem { owner: Arc::downgrade(owner), view: OnceLock::new(), selected, marker: new_paint()? });
        let handle = item.clone();
        let view = jx::native_linear_layout(&owner.context(), jx::mask::DISPATCH_DRAW, move |m, a| handle.dispatch(m, a))?;
        let _ = item.view.set(WeakObj::new(&view));
        Ok((item, view))
    }
    pub fn view(&self) -> Obj {
        self.view.get().expect("ChoiceItem not constructed").get()
    }
    fn dispatch(self: &Arc<Self>, method: &str, args: &[Local]) -> JResult<Reply> {
        draw_dispatch(method, args, "dispatchDraw(Canvas)", |c| self.dispatch_draw(c))
    }
    /// `protected void dispatchDraw(Canvas canvas)`.
    pub fn dispatch_draw(&self, canvas: &Obj) -> AppResult<()> {
        let view = self.view();
        super_dispatch_draw_linear(&view, canvas)?;
        if (self.selected)() {
            let Some(owner) = self.owner.upgrade() else { return Ok(()) };
            set_color(&self.marker, INK)?;
            draw_circle(canvas, ui::width(&view)?.0 as f32 / 2.0, (ui::height(&view)?.0 - owner.dp(5.0)?) as f32, owner.dp(3.0)? as f32, &self.marker)?;
        }
        Ok(())
    }
}

/// `EditorUi$2`: a color swatch `View` from `colorChoices(...)` (`NativeView`, mask DRAW).
pub struct ColorSwatch {
    pub owner: Weak<EditorUi>,
    pub view: OnceLock<WeakObj>,
    /// Captured `final int color`.
    pub color: i32,
    /// `final Paint p`.
    pub p: Obj,
}
impl ColorSwatch {
    fn create(owner: &Arc<EditorUi>, color: i32) -> AppResult<(Arc<ColorSwatch>, Obj)> {
        let swatch = Arc::new(ColorSwatch { owner: Arc::downgrade(owner), view: OnceLock::new(), color, p: new_paint()? });
        let handle = swatch.clone();
        let view = jx::native_view(&owner.context(), jx::mask::DRAW, move |m, a| handle.dispatch(m, a))?;
        let _ = swatch.view.set(WeakObj::new(&view));
        Ok((swatch, view))
    }
    pub fn view(&self) -> Obj {
        self.view.get().expect("ColorSwatch not constructed").get()
    }
    fn dispatch(self: &Arc<Self>, method: &str, args: &[Local]) -> JResult<Reply> {
        draw_dispatch(method, args, "onDraw(Canvas)", |c| self.on_draw(c))
    }
    /// `protected void onDraw(Canvas c)`.
    pub fn on_draw(&self, c: &Obj) -> AppResult<()> {
        let Some(owner) = self.owner.upgrade() else { return Ok(()) };
        let p = &self.p;
        let x = ui::width(&self.view())?.0 as f32 / 2.0;
        let y = owner.dp(19.0)? as f32;
        set_style(p, Style::Fill)?;
        set_color(p, self.color)?;
        draw_circle(c, x, y, owner.dp(13.0)? as f32, p)?;
        set_style(p, Style::Stroke)?;
        set_color(p, INK)?;
        set_stroke_width(p, owner.dp(1.0)? as f32)?;
        draw_circle(c, x, y, owner.dp(13.0)? as f32, p)?;
        if owner.app().color.get() == self.color {
            set_style(p, Style::Fill)?;
            draw_circle(c, x, owner.dp(40.0)? as f32, owner.dp(3.0)? as f32, p)?;
        }
        Ok(())
    }
}

/// `EditorUi$6`: the popup's bordered `FrameLayout` shell from `showPopup`
/// (`NativeFrameLayout`, mask DISPATCH_DRAW).
pub struct PopupShell {
    pub owner: Weak<EditorUi>,
    pub view: OnceLock<WeakObj>,
    /// `final Paint border`.
    pub border: Obj,
}
impl PopupShell {
    pub fn new(owner: &Arc<EditorUi>) -> AppResult<Arc<PopupShell>> {
        let (shell, view) = PopupShell::create(owner)?;
        pin_local(&view);
        Ok(shell)
    }
    fn create(owner: &Arc<EditorUi>) -> AppResult<(Arc<PopupShell>, Obj)> {
        let shell = Arc::new(PopupShell { owner: Arc::downgrade(owner), view: OnceLock::new(), border: new_paint()? });
        let handle = shell.clone();
        let view = jx::native_frame_layout(&owner.context(), jx::mask::DISPATCH_DRAW, move |m, a| handle.dispatch(m, a))?;
        let _ = shell.view.set(WeakObj::new(&view));
        Ok((shell, view))
    }
    pub fn view(&self) -> Obj {
        self.view.get().expect("PopupShell not constructed").get()
    }
    fn dispatch(self: &Arc<Self>, method: &str, args: &[Local]) -> JResult<Reply> {
        draw_dispatch(method, args, "dispatchDraw(Canvas)", |c| self.dispatch_draw(c))
    }
    /// `protected void dispatchDraw(Canvas canvas)`.
    pub fn dispatch_draw(&self, canvas: &Obj) -> AppResult<()> {
        let view = self.view();
        super_dispatch_draw_frame(&view, canvas)?;
        let Some(owner) = self.owner.upgrade() else { return Ok(()) };
        let border = &self.border;
        set_color(border, INK)?;
        set_style(border, Style::Stroke)?;
        set_stroke_width(border, owner.dp(1.0)? as f32)?;
        let inset = owner.dp(1.0)? as f32 / 2.0;
        let (w, h) = (ui::width(&view)?.0 as f32, ui::height(&view)?.0 as f32);
        let radius = owner.dp(8.0)? as f32;
        draw_round_rect(canvas, inset, inset, w - inset, h - inset, radius, radius, border)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_icons_match_java() {
        assert_eq!(action_icon_for("Agent"), "agent");
        assert_eq!(action_icon_for("Undo last action"), "undo");
        assert_eq!(action_icon_for("Redo"), "redo");
        assert_eq!(action_icon_for("Rename layer…"), "layers");
        assert_eq!(action_icon_for("Insert picture…"), "insert");
        assert_eq!(action_icon_for("Insert SVG artwork…"), "insert");
        assert_eq!(action_icon_for("Insert GIF animation…"), "more");
        assert_eq!(action_icon_for("neo Brush"), "neoBrush");
        assert_eq!(action_icon_for("asia Calligraphy"), "asiaCalligraphy");
        assert_eq!(action_icon_for("latin Calligraphy"), "latinCalligraphy");
        assert_eq!(action_icon_for("square"), "square");
        assert_eq!(action_icon_for("Pen presets…"), "pen");
        assert_eq!(action_icon_for("Save current as preset…"), "pen");
        assert_eq!(action_icon_for("Black · 2"), "more");
        assert_eq!(action_icon_for("Enter text…"), "text");
        assert_eq!(action_icon_for("Erase page"), "eraser");
        assert_eq!(action_icon_for("Erase objects…"), "eraser");
        assert_eq!(action_icon_for("Ruled"), "template");
        assert_eq!(action_icon_for("Dot grid"), "template");
        assert_eq!(action_icon_for("Graph"), "template");
        assert_eq!(action_icon_for("Reset view"), "move");
        assert_eq!(action_icon_for("Copy selection"), "select");
        assert_eq!(action_icon_for("Done"), "check");
        assert_eq!(action_icon_for("Ready to erase"), "eraser");
        assert_eq!(action_icon_for("Browse files…"), "more");
    }

    #[test]
    fn camel_case_split_matches_regex() {
        assert_eq!(split_camel("neoBrush"), "neo Brush");
        assert_eq!(split_camel("latinCalligraphy"), "latin Calligraphy");
        assert_eq!(split_camel("oily"), "oily");
        assert_eq!(split_camel("aBC"), "a BC");
        assert_eq!(split_camel("aBcD"), "a Bc D");
    }

    #[test]
    fn helpers() {
        assert_eq!(capitalize("partial"), "Partial");
        assert_eq!(short_id("0123456789abcdef"), "01234567");
        assert_eq!(short_id("abc"), "abc");
        assert_eq!(material_range("pencil"), "Graphite grade · HB to 6B");
        assert_eq!(material_range("pen"), "Material response");
        assert_eq!(fmax(0.2, 0.1), 0.2);
        assert!(fmin(f32::NAN, 1.0).is_nan());
    }
}
