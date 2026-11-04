//! Widget and context helpers shared by the UI modules.
//!
//! These mirror helper methods that already exist in the Java sources
//! (`EditorUi.dp/column/row/divider/space/text/surface`, `AgentUi.dp/column/
//! text/button`) plus one-line framework idioms the Java repeated inline
//! (`getSharedPreferences("settings",0)`, `Toast.makeText(...).show()`,
//! `new Handler(Looper.getMainLooper())`, `getFilesDir()`). They add no
//! behavior; the per-class Java helpers stay as methods of their Rust structs
//! and delegate here.

use crate::android_consts::{Gravity, Orientation, ToastDuration, Visibility, TYPEFACE_NORMAL};
use crate::jx::{self, DelayMillis, JResult, Local, Obj, Reply};
use notemaker_core::style::ArgbColor;
use std::path::PathBuf;

pub use crate::java_port::math::round_f32;

// ---------------------------------------------------------------------------
// Values

/// A length in density-independent pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
pub struct Dp(pub f32);

/// A length in device pixels (`int` view geometry).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Px(pub i32);

/// A text size in scaled pixels (`TextView.setTextSize(float)`).
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
pub struct Sp(pub f32);

/// `ViewGroup.LayoutParams` width and height: device pixels or
/// `LAYOUT_PARAMS_MATCH_PARENT` (`-1`) / `LAYOUT_PARAMS_WRAP_CONTENT` (`-2`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayoutSize {
    pub width: i32,
    pub height: i32,
}

/// `LinearLayout.LayoutParams.weight`.
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
pub struct LayoutWeight(pub f32);

/// Margins or padding in device pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PxInsets {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

notemaker_core::string_newtype! {
    /// Text shown to the user (labels, messages, content descriptions,
    /// `TextView` contents).
    pub struct UiText;
}

/// The weight of [`text`] labels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextWeight {
    Normal,
    /// `sans-serif-medium`.
    Bold,
}

/// A rounded [`surface`] background.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfaceStyle {
    pub fill: ArgbColor,
    pub border: ArgbColor,
    pub radius: Dp,
    /// Border width; no border when not positive.
    pub stroke: Dp,
}

notemaker_core::string_newtype! {
    /// A `SharedPreferences` key in the `settings` file.
    pub struct PrefKey;
}

notemaker_core::string_newtype! {
    /// The stored value of a string preference.
    pub struct PrefText;
}

/// The stored value of a boolean preference.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrefSwitch {
    On,
    Off,
}
impl From<bool> for PrefSwitch {
    fn from(on: bool) -> PrefSwitch {
        if on { PrefSwitch::On } else { PrefSwitch::Off }
    }
}
impl From<PrefSwitch> for bool {
    fn from(switch: PrefSwitch) -> bool {
        switch == PrefSwitch::On
    }
}

/// The stored value of an integer preference.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrefNumber(pub i32);

/// `context.getResources().getDisplayMetrics().density`.
fn density(context: &Obj) -> JResult<f32> {
    let resources = jcall!(context, "android/content/Context", "getResources", "()Landroid/content/res/Resources;")?.some();
    let metrics = jcall!(resources, "android/content/res/Resources", "getDisplayMetrics", "()Landroid/util/DisplayMetrics;")?.some();
    Ok(jget!(metrics, "android/util/DisplayMetrics", "density", "F")?.float())
}
/// `EditorUi.dp(float)` / `AgentUi.dp(int)`: `Math.round(value * density)`.
pub fn dp(context: &Obj, value: Dp) -> JResult<Px> {
    Ok(Px(round_f32(value.0 * density(context)?)))
}

// ---------------------------------------------------------------------------
// Views

/// `new View(context)`.
pub fn view(context: &Obj) -> JResult<Obj> {
    jnew!("android/view/View", "(Landroid/content/Context;)V", context)
}
/// `new LinearLayout(context); setOrientation(orientation)`.
pub fn linear_layout(context: &Obj, orientation: Orientation) -> JResult<Obj> {
    let v = jnew!("android/widget/LinearLayout", "(Landroid/content/Context;)V", context)?;
    jcall!(v, "android/widget/LinearLayout", "setOrientation", "(I)V", orientation)?;
    Ok(v)
}
/// `EditorUi.column()` / `AgentUi.column()`: vertical `LinearLayout`.
pub fn column(context: &Obj) -> JResult<Obj> {
    linear_layout(context, Orientation::Vertical)
}
/// `EditorUi.row()`: horizontal `LinearLayout`.
pub fn row(context: &Obj) -> JResult<Obj> {
    linear_layout(context, Orientation::Horizontal)
}
/// `new FrameLayout(context)`.
pub fn frame_layout(context: &Obj) -> JResult<Obj> {
    jnew!("android/widget/FrameLayout", "(Landroid/content/Context;)V", context)
}
/// `new TextView(context)`.
pub fn text_view(context: &Obj) -> JResult<Obj> {
    jnew!("android/widget/TextView", "(Landroid/content/Context;)V", context)
}
/// `new LinearLayout.LayoutParams(width, height)`.
pub fn linear_params(size: LayoutSize) -> JResult<Obj> {
    let LayoutSize { width, height } = size;
    jnew!("android/widget/LinearLayout$LayoutParams", "(II)V", width, height)
}
/// `new LinearLayout.LayoutParams(width, height, weight)`.
pub fn linear_params_weight(size: LayoutSize, weight: LayoutWeight) -> JResult<Obj> {
    let (LayoutSize { width, height }, weight) = (size, weight.0);
    jnew!("android/widget/LinearLayout$LayoutParams", "(IIF)V", width, height, weight)
}
/// `new FrameLayout.LayoutParams(width, height)`.
pub fn frame_params(size: LayoutSize) -> JResult<Obj> {
    let LayoutSize { width, height } = size;
    jnew!("android/widget/FrameLayout$LayoutParams", "(II)V", width, height)
}
/// `new FrameLayout.LayoutParams(width, height, gravity)`.
pub fn frame_params_gravity(size: LayoutSize, gravity: Gravity) -> JResult<Obj> {
    let LayoutSize { width, height } = size;
    jnew!("android/widget/FrameLayout$LayoutParams", "(III)V", width, height, gravity)
}
/// `params.setMargins(l, t, r, b)` on any `MarginLayoutParams`.
pub fn set_margins(params: &Obj, margins: PxInsets) -> JResult<()> {
    let PxInsets { left, top, right, bottom } = margins;
    jcall!(params, "android/view/ViewGroup$MarginLayoutParams", "setMargins", "(IIII)V", left, top, right, bottom)?;
    Ok(())
}
/// `parent.addView(child)`.
pub fn add_view(parent: &Obj, child: &Obj) -> JResult<()> {
    jcall!(parent, "android/view/ViewGroup", "addView", "(Landroid/view/View;)V", child)?;
    Ok(())
}
/// `parent.addView(child, params)`.
pub fn add_view_params(parent: &Obj, child: &Obj, params: &Obj) -> JResult<()> {
    jcall!(parent, "android/view/ViewGroup", "addView", "(Landroid/view/View;Landroid/view/ViewGroup$LayoutParams;)V", child, params)?;
    Ok(())
}
/// `view.setPadding(l, t, r, b)`.
pub fn set_padding(view: &Obj, padding: PxInsets) -> JResult<()> {
    let PxInsets { left, top, right, bottom } = padding;
    jcall!(view, "android/view/View", "setPadding", "(IIII)V", left, top, right, bottom)?;
    Ok(())
}
/// `view.setBackgroundColor(color)`.
pub fn set_background_color(view: &Obj, color: ArgbColor) -> JResult<()> {
    jcall!(view, "android/view/View", "setBackgroundColor", "(I)V", color)?;
    Ok(())
}
/// `view.setVisibility(visibility)`.
pub fn set_visibility(view: &Obj, visibility: Visibility) -> JResult<()> {
    jcall!(view, "android/view/View", "setVisibility", "(I)V", visibility)?;
    Ok(())
}
/// `view.setContentDescription(text)`.
pub fn set_content_description(view: &Obj, text: &UiText) -> JResult<()> {
    let s = jx::jstr(text);
    jcall!(view, "android/view/View", "setContentDescription", "(Ljava/lang/CharSequence;)V", &s)?;
    Ok(())
}
/// `view.invalidate()`.
pub fn invalidate(view: &Obj) -> JResult<()> {
    jcall!(view, "android/view/View", "invalidate", "()V")?;
    Ok(())
}
/// `view.getWidth()`.
pub fn width(view: &Obj) -> JResult<Px> {
    Ok(Px(jcall!(view, "android/view/View", "getWidth", "()I")?.int()))
}
/// `view.getHeight()`.
pub fn height(view: &Obj) -> JResult<Px> {
    Ok(Px(jcall!(view, "android/view/View", "getHeight", "()I")?.int()))
}
/// `view.setOnClickListener(v -> f(v))`. The listener belongs to the view;
/// capture owners weakly (see `jx::native_view`).
pub fn on_click(view: &Obj, f: impl Fn(&Local) -> JResult<()> + Send + Sync + 'static) -> JResult<()> {
    let listener = jx::callback(move |_, args| f(&args[0]).map(|_| Reply::Void));
    jcall!(view, "android/view/View", "setOnClickListener", "(Landroid/view/View$OnClickListener;)V", &listener)?;
    Ok(())
}

/// `textView.setText(value)` (the `CharSequence` overload).
pub fn set_text(view: &Obj, value: &UiText) -> JResult<()> {
    let s = jx::jstr(value);
    jcall!(view, "android/widget/TextView", "setText", "(Ljava/lang/CharSequence;)V", &s)?;
    Ok(())
}
/// `textView.getText().toString()`.
pub fn text_of(view: &Obj) -> JResult<UiText> {
    let t = jcall!(view, "android/widget/TextView", "getText", "()Ljava/lang/CharSequence;")?.global();
    Ok(UiText::from(t.and_then(|t| jx::to_string(t.as_obj())).unwrap_or_default()))
}
/// `EditorUi.text(value, size, color, bold)`.
pub fn text(context: &Obj, value: &UiText, size: Sp, color: ArgbColor, weight: TextWeight) -> JResult<Obj> {
    let view = text_view(context)?;
    set_text(&view, value)?;
    jcall!(view, "android/widget/TextView", "setTextSize", "(F)V", size.0)?;
    jcall!(view, "android/widget/TextView", "setTextColor", "(I)V", color)?;
    if weight == TextWeight::Bold {
        let face = jstatic!("android/graphics/Typeface", "create", "(Ljava/lang/String;I)Landroid/graphics/Typeface;", "sans-serif-medium", TYPEFACE_NORMAL)?.global();
        jcall!(view, "android/widget/TextView", "setTypeface", "(Landroid/graphics/Typeface;)V", &face)?;
    }
    Ok(view)
}
/// `EditorUi.surface(fill, border, radius, stroke)`: a rounded
/// `GradientDrawable` (radius and stroke in dp).
pub fn surface(context: &Obj, style: SurfaceStyle) -> JResult<Obj> {
    let SurfaceStyle { fill, border, radius, stroke } = style;
    let shape = jnew!("android/graphics/drawable/GradientDrawable", "()V")?;
    jcall!(shape, "android/graphics/drawable/GradientDrawable", "setColor", "(I)V", fill)?;
    jcall!(shape, "android/graphics/drawable/GradientDrawable", "setCornerRadius", "(F)V", dp(context, radius)?.0 as f32)?;
    if stroke.0 > 0.0 {
        jcall!(shape, "android/graphics/drawable/GradientDrawable", "setStroke", "(II)V", dp(context, stroke)?.0, border)?;
    }
    Ok(shape)
}

// ---------------------------------------------------------------------------
// Context

/// `Toast.makeText(context, text, duration).show()`.
pub fn toast(context: &Obj, text: &UiText, duration: ToastDuration) -> JResult<()> {
    let s = jx::jstr(text);
    let t = jstatic!("android/widget/Toast", "makeText", "(Landroid/content/Context;Ljava/lang/CharSequence;I)Landroid/widget/Toast;", context, &s, duration)?.some();
    jcall!(t, "android/widget/Toast", "show", "()V")?;
    Ok(())
}
/// `context.getSharedPreferences("settings", 0)`.
pub fn prefs(context: &Obj) -> JResult<Obj> {
    Ok(jcall!(context, "android/content/Context", "getSharedPreferences", "(Ljava/lang/String;I)Landroid/content/SharedPreferences;", "settings", 0)?.some())
}
/// `prefs.getString(key, fallback)` (`None` when stored/fallback null).
pub fn pref_string(context: &Obj, key: &PrefKey, fallback: Option<&PrefText>) -> JResult<Option<PrefText>> {
    let (key, fallback): (&str, Option<&str>) = (key, fallback.map(|f| &**f));
    let p = prefs(context)?;
    Ok(jcall!(p, "android/content/SharedPreferences", "getString", "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;", key, fallback)?.string().map(PrefText::from))
}
/// `prefs.getBoolean(key, fallback)`.
pub fn pref_bool(context: &Obj, key: &PrefKey, fallback: PrefSwitch) -> JResult<bool> {
    let (key, fallback): (&str, bool) = (key, fallback.into());
    let p = prefs(context)?;
    Ok(jcall!(p, "android/content/SharedPreferences", "getBoolean", "(Ljava/lang/String;Z)Z", key, fallback)?.bool())
}
/// `prefs.getInt(key, fallback)`.
pub fn pref_int(context: &Obj, key: &PrefKey, fallback: PrefNumber) -> JResult<PrefNumber> {
    let (key, fallback): (&str, i32) = (key, fallback.0);
    let p = prefs(context)?;
    Ok(PrefNumber(jcall!(p, "android/content/SharedPreferences", "getInt", "(Ljava/lang/String;I)I", key, fallback)?.int()))
}
/// `prefs.contains(key)`.
pub fn pref_contains(context: &Obj, key: &PrefKey) -> JResult<bool> {
    let key: &str = key;
    let p = prefs(context)?;
    Ok(jcall!(p, "android/content/SharedPreferences", "contains", "(Ljava/lang/String;)Z", key)?.bool())
}
/// `prefs.edit()`; finish with [`apply`].
pub fn edit(context: &Obj) -> JResult<Obj> {
    let p = prefs(context)?;
    Ok(jcall!(p, "android/content/SharedPreferences", "edit", "()Landroid/content/SharedPreferences$Editor;")?.some())
}
pub fn put_string(editor: &Obj, key: &PrefKey, value: &PrefText) -> JResult<()> {
    let (key, value): (&str, &str) = (key, value);
    jcall!(editor, "android/content/SharedPreferences$Editor", "putString", "(Ljava/lang/String;Ljava/lang/String;)Landroid/content/SharedPreferences$Editor;", key, value)?;
    Ok(())
}
pub fn put_bool(editor: &Obj, key: &PrefKey, value: PrefSwitch) -> JResult<()> {
    let (key, value): (&str, bool) = (key, value.into());
    jcall!(editor, "android/content/SharedPreferences$Editor", "putBoolean", "(Ljava/lang/String;Z)Landroid/content/SharedPreferences$Editor;", key, value)?;
    Ok(())
}
pub fn put_int(editor: &Obj, key: &PrefKey, value: PrefNumber) -> JResult<()> {
    let (key, value): (&str, i32) = (key, value.0);
    jcall!(editor, "android/content/SharedPreferences$Editor", "putInt", "(Ljava/lang/String;I)Landroid/content/SharedPreferences$Editor;", key, value)?;
    Ok(())
}
pub fn remove_pref(editor: &Obj, key: &PrefKey) -> JResult<()> {
    let key: &str = key;
    jcall!(editor, "android/content/SharedPreferences$Editor", "remove", "(Ljava/lang/String;)Landroid/content/SharedPreferences$Editor;", key)?;
    Ok(())
}
/// `editor.apply()`.
pub fn apply(editor: &Obj) -> JResult<()> {
    jcall!(editor, "android/content/SharedPreferences$Editor", "apply", "()V")?;
    Ok(())
}

fn file_path(file: Option<Obj>) -> JResult<PathBuf> {
    let file = file.ok_or_else(|| jx::JavaError::new("java.lang.NullPointerException", "null directory"))?;
    let s = jcall!(file, "java/io/File", "getAbsolutePath", "()Ljava/lang/String;")?.string().unwrap_or_default();
    Ok(PathBuf::from(s))
}
/// `context.getFilesDir()`.
pub fn files_dir(context: &Obj) -> JResult<PathBuf> {
    file_path(jcall!(context, "android/content/Context", "getFilesDir", "()Ljava/io/File;")?.global())
}
/// `context.getCacheDir()`.
pub fn cache_dir(context: &Obj) -> JResult<PathBuf> {
    file_path(jcall!(context, "android/content/Context", "getCacheDir", "()Ljava/io/File;")?.global())
}

// ---------------------------------------------------------------------------
// Threads

/// `new Handler(Looper.getMainLooper())`.
pub fn main_handler() -> JResult<Obj> {
    let looper = jstatic!("android/os/Looper", "getMainLooper", "()Landroid/os/Looper;")?.some();
    jnew!("android/os/Handler", "(Landroid/os/Looper;)V", &looper)
}
/// `Looper.myLooper() == Looper.getMainLooper()`.
pub fn on_main_thread() -> JResult<bool> {
    let main = jstatic!("android/os/Looper", "getMainLooper", "()Landroid/os/Looper;")?.global();
    let mine = jstatic!("android/os/Looper", "myLooper", "()Landroid/os/Looper;")?.global();
    Ok(match (main, mine) {
        (Some(a), Some(b)) => a.same(&b),
        _ => false,
    })
}
/// `handler.post(runnable)`.
pub fn post(handler: &Obj, runnable: &Obj) -> JResult<bool> {
    Ok(jcall!(handler, "android/os/Handler", "post", "(Ljava/lang/Runnable;)Z", runnable)?.bool())
}
/// `handler.postDelayed(runnable, delay)`.
pub fn post_delayed(handler: &Obj, runnable: &Obj, delay: DelayMillis) -> JResult<bool> {
    Ok(jcall!(handler, "android/os/Handler", "postDelayed", "(Ljava/lang/Runnable;J)Z", runnable, delay.0)?.bool())
}
/// `handler.removeCallbacks(runnable)`.
pub fn remove_callbacks(handler: &Obj, runnable: &Obj) -> JResult<()> {
    jcall!(handler, "android/os/Handler", "removeCallbacks", "(Ljava/lang/Runnable;)V", runnable)?;
    Ok(())
}
/// `view.post(runnable)`.
pub fn view_post(view: &Obj, runnable: &Obj) -> JResult<bool> {
    Ok(jcall!(view, "android/view/View", "post", "(Ljava/lang/Runnable;)Z", runnable)?.bool())
}
