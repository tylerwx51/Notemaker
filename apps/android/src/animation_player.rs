//! Port of `AnimationPlayer.java`: session-local GIF playback. Playback
//! belongs to one Activity session; document snapshots contain only GIF bytes.
//! All methods run on the UI thread (`handler` is the main looper).
//!
//! `clips` was a `HashMap`; it is an insertion-ordered map here and `state()`
//! lists clips in `java.util.HashMap` iteration order.

use crate::android_canvas::{rect_to_java, RectFExt};
use crate::android_consts::{BASE64_DEFAULT, COLOR_BLACK, COLOR_WHITE, MOTIONEVENT_ACTION_CANCEL, MOTIONEVENT_ACTION_DOWN, MOTIONEVENT_ACTION_MOVE, MOTIONEVENT_ACTION_UP, PAINT_ANTI_ALIAS_FLAG, TOAST_LENGTH_SHORT};
use crate::error::{AppError, AppResult};
use crate::field::Field;
use crate::ink_cache::{fmax, fmin};
use crate::jx::{self, Obj, Reply};
use crate::main_activity::MainActivity;
use crate::ui;
use indexmap::IndexMap;
use notemaker_cli::canvas::{Style, RectF};
use notemaker_core::draw_item::DrawItem;
use notemaker_core::gif_asset::{self, Info};
use notemaker_core::json::{self, Map, Value};
use notemaker_core::{jmath, object};
use std::sync::{Arc, Weak};

/// `private static final float BAR_HEIGHT = 76`.
pub const BAR_HEIGHT: f32 = 76.0;
/// `private static final float ROW_HEIGHT = 39`.
pub const ROW_HEIGHT: f32 = 39.0;

/// `((Number) o.get(key)).floatValue()` for object geometry.
fn fv(o: &Map, key: &str) -> AppResult<f32> {
    crate::ink_cache::float_value(o.get(key))
}
/// `((Number) o.getOrDefault("rotation", 0))`.
fn rotation(o: &Map) -> AppResult<f64> {
    match o.get("rotation") {
        None => Ok(0.0),
        v => Ok(json::as_double(v)?),
    }
}

/// `private static final class Clip`.
pub struct Clip {
    pub source: String,
    pub info: Info,
    /// `final android.graphics.Movie movie`.
    pub movie: Obj,
    pub frame: Field<i32>,
    pub playing: Field<bool>,
    pub next_at: Field<i64>,
}
impl Clip {
    /// `Clip(String data)`: decodes the data URI; Java threw
    /// `IllegalArgumentException` for undecodable GIFs.
    pub fn new(data: &str) -> AppResult<Clip> {
        let units: Vec<u16> = data.encode_utf16().collect();
        if units.len() < 22 {
            return Err(AppError::java("java.lang.StringIndexOutOfBoundsException", format!("begin 22, end {}, length {}", units.len(), units.len())));
        }
        let encoded = String::from_utf16_lossy(&units[22..]);
        let array = jstatic!("android/util/Base64", "decode", "(Ljava/lang/String;I)[B", encoded, BASE64_DEFAULT)?.some();
        let bytes = jx::byte_array(array.as_obj());
        let info = gif_asset::validate_bytes(Some(&bytes))?;
        let movie = jstatic!("android/graphics/Movie", "decodeByteArray", "([BII)Landroid/graphics/Movie;", &array, 0, bytes.len() as i32)?.global();
        let Some(movie) = movie else {
            return Err(AppError::illegal_argument("Could not decode GIF"));
        };
        Ok(Clip { source: data.to_string(), info, movie, frame: Field::new(0), playing: Field::new(false), next_at: Field::new(0) })
    }
    fn frames(&self) -> i32 {
        self.info.delays.len() as i32
    }
}

/// `private static final class Hit`.
pub struct Hit {
    pub id: String,
    pub control: String,
    pub bar: RectF,
}
impl Hit {
    pub fn new(id: String, control: String, bar: RectF) -> Hit {
        Hit { id, control, bar }
    }
}

/// `final class AnimationPlayer`.
pub struct AnimationPlayer {
    pub app: Weak<MainActivity>,
    /// `new Handler(Looper.getMainLooper())`.
    pub handler: Obj,
    pub clips: Field<IndexMap<String, Arc<Clip>>>,
    pub selected: Field<Option<String>>,
    pub dialog: Field<Option<Obj>>,
    pub label: Field<Option<Obj>>,
    pub timeline: Field<Option<Obj>>,
    pub toggle: Field<Option<Obj>>,
    /// `final Paint controlPaint = new Paint(Paint.ANTI_ALIAS_FLAG)`.
    pub control_paint: Obj,
    pub pressed_id: Field<Option<String>>,
    pub pressed_control: Field<Option<String>>,
    /// `final Runnable tick` (`AnimationPlayer$1`); a Java `Runnable` for
    /// `postDelayed`/`removeCallbacks`, capturing the player weakly and
    /// calling [`AnimationPlayer::tick`].
    pub tick: Obj,
}

impl AnimationPlayer {
    /// `AnimationPlayer(MainActivity app)`.
    pub fn new(app: &Arc<MainActivity>) -> AppResult<Arc<AnimationPlayer>> {
        let handler = ui::main_handler()?;
        let control_paint = jnew!("android/graphics/Paint", "(I)V", PAINT_ANTI_ALIAS_FLAG)?;
        Ok(Arc::new_cyclic(|me: &Weak<AnimationPlayer>| {
            let me = me.clone();
            let tick = jx::try_runnable(move || match me.upgrade() {
                Some(player) => player.tick().map_err(Into::into),
                None => Ok(()),
            });
            AnimationPlayer {
                app: Arc::downgrade(app),
                handler,
                clips: Field::new(IndexMap::new()),
                selected: Field::new(None),
                dialog: Field::new(None),
                label: Field::new(None),
                timeline: Field::new(None),
                toggle: Field::new(None),
                control_paint,
                pressed_id: Field::new(None),
                pressed_control: Field::new(None),
                tick,
            }
        }))
    }
    pub fn app(&self) -> Arc<MainActivity> {
        self.app.upgrade().expect("MainActivity dropped")
    }
    fn clip_list(&self) -> Vec<(String, Arc<Clip>)> {
        self.clips.with(|c| c.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
    }
    fn object(&self, id: Option<&str>) -> Option<Arc<Map>> {
        id.and_then(|id| self.app().model().objects.get(id).cloned())
    }
    /// Body of `AnimationPlayer$1.run()`.
    pub fn tick(self: &Arc<Self>) -> AppResult<()> {
        let now = jx::uptime_millis();
        let mut more = false;
        for (id, clip) in self.clip_list() {
            if !clip.playing.get() {
                continue;
            }
            let object = self.object(Some(&id));
            if !self.available(object.as_deref()) || object.as_ref().and_then(|o| o.get("gif")).and_then(Value::as_str) != Some(clip.source.as_str()) {
                clip.playing.set(false);
                continue;
            }
            if now >= clip.next_at.get() {
                let frame = (clip.frame.get() + 1) % clip.frames();
                clip.frame.set(frame);
                clip.next_at.set(now + clip.info.delays[frame as usize] as i64);
                self.invalidate(object.as_deref())?;
            }
            more = true;
        }
        self.refresh()?;
        if more {
            ui::post_delayed(&self.handler, &self.tick, 50)?;
        }
        Ok(())
    }
    /// `private boolean available(Map<String,Object> object)`.
    pub fn available(&self, object: Option<&Map>) -> bool {
        let Some(o) = object else { return false };
        if o.get("shape").and_then(Value::as_str) != Some("gif") {
            return false;
        }
        let app = self.app();
        if json::as_int(o.get("page")).ok() != Some(app.page.get()) {
            return false;
        }
        let model = app.model();
        let object_active = json::key_of(o.get("object")).and_then(|k| model.object_active.get(k)).copied();
        let layer_active = json::key_of(o.get("layer")).and_then(|k| model.layer_active.get(k)).copied();
        object_active != Some(false) && layer_active != Some(false)
    }
    /// `private Clip clip(String id)`.
    pub fn clip(&self, id: &str) -> AppResult<Arc<Clip>> {
        let o = self.object(Some(id));
        if !self.available(o.as_deref()) {
            return Err(AppError::illegal_argument("GIF is unavailable on this page"));
        }
        let o = o.unwrap();
        let result = self.clips.with(|c| c.get(id).cloned());
        let source = json::as_string_opt(o.get("gif"))?;
        match result {
            Some(r) if Some(r.source.as_str()) == source => Ok(r),
            previous => {
                if let Some(previous) = previous {
                    previous.playing.set(false);
                }
                let Some(source) = source else {
                    return Err(AppError::java("java.lang.NullPointerException", "GIF source is null"));
                };
                let result = Arc::new(Clip::new(source)?);
                self.clips.with_mut(|c| c.insert(id.to_string(), result.clone()));
                self.schedule()?;
                Ok(result)
            }
        }
    }
    /// `private void invalidate(Map<String,Object> o)`.
    pub fn invalidate(&self, o: Option<&Map>) -> AppResult<()> {
        let app = self.app();
        let (Some(canvas), Some(o)) = (app.canvas.get(), o) else { return Ok(()) };
        let x = fv(o, "x")?;
        let y = fv(o, "y")?;
        let w = fv(o, "w")?;
        let h = fv(o, "h")?;
        let scale = canvas.scale() * canvas.zoom.get();
        if scale <= 0.0 {
            return canvas.invalidate();
        }
        let radius = jmath::hypot(w as f64, h as f64) as f32 / 2.0 + 4.0;
        let cx = x + w / 2.0;
        let cy = y + h / 2.0;
        let bar = self.controls(o);
        let left = fmin(cx - radius, bar.left) - 4.0;
        let top = fmin(cy - radius, bar.top) - 4.0;
        let right = fmax(cx + radius, bar.right) + 4.0;
        let bottom = fmax(cy + radius, bar.bottom) + 4.0;
        let ox = canvas.offset_x.get();
        let oy = canvas.offset_y.get();
        canvas.invalidate_rect(
            (ox + left * scale) as i32,
            (oy + top * scale) as i32,
            ((ox + right * scale) as f64).ceil() as i32,
            ((oy + bottom * scale) as f64).ceil() as i32,
        )
    }
    /// `boolean loaded(String id)`.
    pub fn loaded(&self, id: &str) -> bool {
        let clip = self.clips.with(|c| c.get(id).cloned());
        let object = self.object(Some(id));
        match (clip, object) {
            (Some(clip), Some(object)) => object.get("gif").and_then(Value::as_str) == Some(clip.source.as_str()),
            _ => false,
        }
    }
    /// `void prepareVisible()`.
    pub fn prepare_visible(&self) -> AppResult<()> {
        let app = self.app();
        let canvas = app.canvas();
        let objects: Vec<Arc<Map>> = app.model().objects.values().cloned().collect();
        for o in objects {
            if self.available(Some(&o)) && canvas.visible(&json::java_string(o.get("layer"))) {
                self.clip(&json::java_string(o.get("object")))?;
            }
        }
        Ok(())
    }
    /// `void select(String id)`.
    pub fn select(self: &Arc<Self>, id: &str) -> AppResult<()> {
        self.clip(id)?;
        if self.selected.get().as_deref() != Some(id) {
            self.selected.set(Some(id.to_string()));
            self.invalidate(self.object(Some(id)).as_deref())?;
        }
        self.refresh()
    }
    /// `private void schedule()`.
    pub fn schedule(&self) -> AppResult<()> {
        ui::remove_callbacks(&self.handler, &self.tick)?;
        let playing = self.clips.with(|c| c.values().any(|c| c.playing.get()));
        if playing {
            ui::post_delayed(&self.handler, &self.tick, 50)?;
        }
        Ok(())
    }
    /// `void play(boolean value)`.
    pub fn play(self: &Arc<Self>, value: bool) -> AppResult<()> {
        let c = self.clip(&self.require_selected()?)?;
        c.playing.set(value);
        c.next_at.set(jx::uptime_millis() + c.info.delays[c.frame.get() as usize] as i64);
        self.schedule()?;
        self.refresh()
    }
    /// `void step(int delta)`.
    pub fn step(self: &Arc<Self>, delta: i32) -> AppResult<()> {
        let c = self.clip(&self.require_selected()?)?;
        c.playing.set(false);
        c.frame.set((c.frame.get() + delta).rem_euclid(c.frames()));
        self.schedule()?;
        self.invalidate(self.object(self.selected.get().as_deref()).as_deref())?;
        self.refresh()
    }
    /// `void seek(int frame)`.
    pub fn seek(self: &Arc<Self>, frame: i32) -> AppResult<()> {
        let c = self.clip(&self.require_selected()?)?;
        if frame < 0 || frame >= c.frames() {
            return Err(AppError::illegal_argument("Frame is out of range"));
        }
        let changed = c.frame.get() != frame;
        c.frame.set(frame);
        c.next_at.set(jx::uptime_millis() + c.info.delays[frame as usize] as i64);
        if changed {
            self.invalidate(self.object(self.selected.get().as_deref()).as_deref())?;
        }
        self.refresh()
    }
    /// `private String requireSelected()`.
    pub fn require_selected(&self) -> AppResult<String> {
        self.selected.get().ok_or_else(|| AppError::illegal_argument("Select a GIF first"))
    }
    /// `Map<String,Object> state()`.
    pub fn state(&self) -> AppResult<Map> {
        let clips = self.clips.with(|c| c.clone());
        let mut entries: Vec<Value> = Vec::new();
        for id in json::java_hash_set_order(clips.keys().cloned()) {
            let c = &clips[&id];
            entries.push(Value::Object(object! {
                "id" => id.as_str(),
                "frame" => c.frame.get() as i64,
                "frames" => c.frames() as i64,
                "playing" => c.playing.get(),
                "durationMs" => c.info.duration as i64,
            }));
        }
        let selected = match self.selected.get() {
            Some(s) => Value::from(s),
            None => Value::Null,
        };
        Ok(object! { "selected" => selected, "clips" => entries })
    }
    /// `void draw(Canvas canvas, String layer)`.
    pub fn draw(&self, canvas: &Obj, layer: &str) -> AppResult<()> {
        for (id, c) in self.clip_list() {
            let o = self.object(Some(&id));
            if !self.available(o.as_deref()) {
                continue;
            }
            let o = o.unwrap();
            if o.get("gif").and_then(Value::as_str) != Some(c.source.as_str()) || o.get("layer").and_then(Value::as_str) != Some(layer) {
                continue;
            }
            let x = fv(&o, "x")?;
            let y = fv(&o, "y")?;
            let w = fv(&o, "w")?;
            let h = fv(&o, "h")?;
            jcall!(canvas, "android/graphics/Canvas", "save", "()I")?;
            let cx = x + w / 2.0;
            let cy = y + h / 2.0;
            jcall!(canvas, "android/graphics/Canvas", "translate", "(FF)V", cx, cy)?;
            jcall!(canvas, "android/graphics/Canvas", "rotate", "(F)V", match o.get("rotation") { None => 0.0, v => crate::ink_cache::float_value(v)? })?;
            let sx = if json::is_true(o.get("flipX")) { -1.0f32 } else { 1.0 };
            let sy = if json::is_true(o.get("flipY")) { -1.0f32 } else { 1.0 };
            jcall!(canvas, "android/graphics/Canvas", "scale", "(FF)V", sx, sy)?;
            jcall!(canvas, "android/graphics/Canvas", "translate", "(FF)V", -cx, -cy)?;
            for polygon in DrawItem::erasures(&o)? {
                let path = jnew!("android/graphics/Path", "()V")?;
                for (i, p) in polygon.iter().enumerate() {
                    if i == 0 {
                        jcall!(path, "android/graphics/Path", "moveTo", "(FF)V", p[0] as f32, p[1] as f32)?;
                    } else {
                        jcall!(path, "android/graphics/Path", "lineTo", "(FF)V", p[0] as f32, p[1] as f32)?;
                    }
                }
                jcall!(path, "android/graphics/Path", "close", "()V")?;
                jcall!(canvas, "android/graphics/Canvas", "clipOutPath", "(Landroid/graphics/Path;)Z", &path)?;
            }
            jcall!(canvas, "android/graphics/Canvas", "clipRect", "(FFFF)Z", x, y, x + w, y + h)?;
            jcall!(canvas, "android/graphics/Canvas", "translate", "(FF)V", x, y)?;
            jcall!(canvas, "android/graphics/Canvas", "scale", "(FF)V", w / c.info.width as f32, h / c.info.height as f32)?;
            jcall!(c.movie, "android/graphics/Movie", "setTime", "(I)Z", c.info.time_for_frame(c.frame.get() as usize))?;
            jcall!(c.movie, "android/graphics/Movie", "draw", "(Landroid/graphics/Canvas;FF)V", canvas, 0.0f32, 0.0f32)?;
            jcall!(canvas, "android/graphics/Canvas", "restore", "()V")?;
        }
        Ok(())
    }
    /// `private RectF controls(Map<String,Object> o)`.
    pub fn controls(&self, o: &Map) -> RectF {
        let x = fv(o, "x").unwrap_or(0.0);
        let y = fv(o, "y").unwrap_or(0.0);
        let w = fv(o, "w").unwrap_or(0.0);
        let h = fv(o, "h").unwrap_or(0.0);
        let angle = jmath::to_radians(rotation(o).unwrap_or(0.0));
        let visual_w = ((w as f64 * jmath::cos(angle)).abs() + (h as f64 * jmath::sin(angle)).abs()) as f32;
        let visual_h = ((w as f64 * jmath::sin(angle)).abs() + (h as f64 * jmath::cos(angle)).abs()) as f32;
        let width = fmax(220.0, fmin(360.0, fmax(w, visual_w)));
        let mut left = x + w / 2.0 - width / 2.0;
        let mut top = y + h / 2.0 + visual_h / 2.0 + 8.0;
        if self.app().type_.get() != "infinite" {
            left = fmax(0.0, fmin(1000.0 - width, left));
            if top + BAR_HEIGHT > 1400.0 {
                top = fmax(0.0, y + h / 2.0 - visual_h / 2.0 - BAR_HEIGHT - 8.0);
            }
        }
        RectF::new(left, top, left + width, top + BAR_HEIGHT)
    }
    /// `private static String controlAt(RectF bar, float x, float y)` (`null` → `None`).
    pub fn control_at(bar: &RectF, x: f32, y: f32) -> Option<String> {
        if x < bar.left || x > bar.right || y < bar.top || y > bar.bottom {
            return None;
        }
        if y > bar.top + ROW_HEIGHT {
            return Some("timeline".into());
        }
        let part = (x - bar.left) / bar.width();
        if part < 0.24 {
            Some("previous".into())
        } else if part < 0.58 {
            Some("toggle".into())
        } else if part < 0.82 {
            Some("next".into())
        } else {
            None
        }
    }
    /// `private Hit hit(float x, float y)` (`null` → `None`).
    pub fn hit(&self, x: f32, y: f32) -> Option<Hit> {
        let app = self.app();
        let canvas = app.canvas();
        let objects: Vec<Arc<Map>> = app.model().objects.values().cloned().collect();
        for o in objects.iter().rev() {
            if !self.available(Some(o)) || !canvas.visible(&json::java_string(o.get("layer"))) {
                continue;
            }
            let bar = self.controls(o);
            if let Some(control) = AnimationPlayer::control_at(&bar, x, y) {
                return Some(Hit::new(json::java_string(o.get("object")), control, bar));
            }
        }
        None
    }
    /// `private void seekAt(String id, RectF bar, float x)`.
    pub fn seek_at(self: &Arc<Self>, id: &str, bar: &RectF, x: f32) -> AppResult<()> {
        self.select(id)?;
        let c = self.clip(id)?;
        let fraction = fmax(0.0, fmin(1.0, (x - bar.left - 12.0) / (bar.width() - 24.0)));
        self.seek(ui::round_f32(fraction * (c.frames() - 1) as f32))
    }
    /// `boolean touch(int action, float x, float y)`.
    pub fn touch(self: &Arc<Self>, action: i32, x: f32, y: f32) -> AppResult<bool> {
        if action == MOTIONEVENT_ACTION_DOWN {
            let hit = self.hit(x, y);
            self.pressed_id.set(hit.as_ref().map(|h| h.id.clone()));
            self.pressed_control.set(hit.as_ref().map(|h| h.control.clone()));
            if let Some(hit) = hit {
                if hit.control == "timeline" {
                    self.seek_at(&hit.id, &hit.bar, x)?;
                }
            }
            return Ok(true);
        }
        if action == MOTIONEVENT_ACTION_MOVE {
            if let Some(id) = self.pressed_id.get() {
                if self.pressed_control.get().as_deref() == Some("timeline") {
                    let o = self.object(Some(&id));
                    if self.available(o.as_deref()) {
                        let bar = self.controls(&o.unwrap());
                        self.seek_at(&id, &bar, x)?;
                    }
                }
            }
            return Ok(true);
        }
        if action == MOTIONEVENT_ACTION_UP {
            let id = self.pressed_id.get();
            let control = self.pressed_control.get();
            self.pressed_id.set(None);
            self.pressed_control.set(None);
            let Some(id) = id else { return Ok(true) };
            let o = self.object(Some(&id));
            if !self.available(o.as_deref()) {
                return Ok(true);
            }
            if control.as_deref() == Some("timeline") {
                let bar = self.controls(&o.unwrap());
                self.seek_at(&id, &bar, x)?;
                return Ok(true);
            }
            let hit = self.hit(x, y);
            let control = control.unwrap_or_default();
            match &hit {
                Some(hit) if hit.id == id && hit.control == control => {}
                _ => return Ok(true),
            }
            self.select(&id)?;
            if control == "previous" {
                self.step(-1)?;
            } else if control == "next" {
                self.step(1)?;
            } else {
                let playing = self.clip(&id)?.playing.get();
                self.play(!playing)?;
            }
            return Ok(true);
        }
        if action == MOTIONEVENT_ACTION_CANCEL {
            self.pressed_id.set(None);
            self.pressed_control.set(None);
            return Ok(true);
        }
        Ok(true)
    }
    fn paint_style(&self, style: Style) -> AppResult<()> {
        jcall!(self.control_paint, "android/graphics/Paint", "setStyle", "(Landroid/graphics/Paint$Style;)V", &crate::android_canvas::style_obj(style)?)?;
        Ok(())
    }
    /// `private void centered(Canvas canvas, String value, float left, float right, float baseline)`.
    pub fn centered(&self, canvas: &Obj, value: &str, left: f32, right: f32, baseline: f32) -> AppResult<()> {
        self.paint_style(Style::Fill)?;
        jcall!(self.control_paint, "android/graphics/Paint", "setColor", "(I)V", COLOR_BLACK)?;
        jcall!(self.control_paint, "android/graphics/Paint", "setTextSize", "(F)V", 17.0f32)?;
        let measured = jcall!(self.control_paint, "android/graphics/Paint", "measureText", "(Ljava/lang/String;)F", value)?.float();
        jcall!(canvas, "android/graphics/Canvas", "drawText", "(Ljava/lang/String;FFLandroid/graphics/Paint;)V", value, (left + right - measured) / 2.0, baseline, &self.control_paint)?;
        Ok(())
    }
    /// `void drawControls(Canvas canvas)`.
    pub fn draw_controls(&self, canvas: &Obj) -> AppResult<()> {
        let app = self.app();
        let view = app.canvas();
        let objects: Vec<Arc<Map>> = app.model().objects.values().cloned().collect();
        let paint = &self.control_paint;
        for o in objects {
            if !self.available(Some(&o)) || !view.visible(&json::java_string(o.get("layer"))) {
                continue;
            }
            let id = json::java_string(o.get("object"));
            let Some(clip) = self.clips.with(|c| c.get(&id).cloned()) else { continue };
            let bar = self.controls(&o);
            let unit = bar.width();
            let bar_obj = rect_to_java(&bar)?;
            self.paint_style(Style::Fill)?;
            jcall!(paint, "android/graphics/Paint", "setColor", "(I)V", COLOR_WHITE)?;
            jcall!(canvas, "android/graphics/Canvas", "drawRect", "(Landroid/graphics/RectF;Landroid/graphics/Paint;)V", &bar_obj, paint)?;
            self.paint_style(Style::Stroke)?;
            jcall!(paint, "android/graphics/Paint", "setStrokeWidth", "(F)V", 1.5f32)?;
            jcall!(paint, "android/graphics/Paint", "setColor", "(I)V", COLOR_BLACK)?;
            jcall!(canvas, "android/graphics/Canvas", "drawRect", "(Landroid/graphics/RectF;Landroid/graphics/Paint;)V", &bar_obj, paint)?;
            jcall!(canvas, "android/graphics/Canvas", "drawLine", "(FFFFLandroid/graphics/Paint;)V", bar.left, bar.top + ROW_HEIGHT, bar.right, bar.top + ROW_HEIGHT, paint)?;
            for fraction in [0.24f32, 0.58, 0.82] {
                jcall!(canvas, "android/graphics/Canvas", "drawLine", "(FFFFLandroid/graphics/Paint;)V", bar.left + unit * fraction, bar.top, bar.left + unit * fraction, bar.top + ROW_HEIGHT, paint)?;
            }
            let frames = clip.frames();
            self.centered(canvas, "Prev", bar.left, bar.left + unit * 0.24, bar.top + 26.0)?;
            self.centered(canvas, if clip.playing.get() { "Pause" } else { "Play" }, bar.left + unit * 0.24, bar.left + unit * 0.58, bar.top + 26.0)?;
            self.centered(canvas, "Next", bar.left + unit * 0.58, bar.left + unit * 0.82, bar.top + 26.0)?;
            self.centered(canvas, &format!("{}/{}", clip.frame.get() + 1, frames), bar.left + unit * 0.82, bar.right, bar.top + 26.0)?;
            let track_left = bar.left + 12.0;
            let track_right = bar.right - 12.0;
            let track_y = bar.top + 58.0;
            self.paint_style(Style::Stroke)?;
            jcall!(paint, "android/graphics/Paint", "setStrokeWidth", "(F)V", 2.0f32)?;
            jcall!(canvas, "android/graphics/Canvas", "drawLine", "(FFFFLandroid/graphics/Paint;)V", track_left, track_y, track_right, track_y, paint)?;
            if frames <= 20 {
                for i in 0..frames {
                    let tx = track_left + (track_right - track_left) * i as f32 / (frames - 1).max(1) as f32;
                    jcall!(canvas, "android/graphics/Canvas", "drawLine", "(FFFFLandroid/graphics/Paint;)V", tx, track_y - 5.0, tx, track_y + 5.0, paint)?;
                }
            }
            let knob = track_left + (track_right - track_left) * clip.frame.get() as f32 / (frames - 1).max(1) as f32;
            self.paint_style(Style::Fill)?;
            jcall!(canvas, "android/graphics/Canvas", "drawCircle", "(FFFLandroid/graphics/Paint;)V", knob, track_y, 6.0f32, paint)?;
        }
        Ok(())
    }
    /// `void show()` (seek listener `AnimationPlayer$2`).
    pub fn show(self: &Arc<Self>) -> AppResult<()> {
        let app = self.app();
        let mut ids: Vec<String> = Vec::new();
        let mut names: Vec<String> = Vec::new();
        let objects: Vec<Arc<Map>> = app.model().objects.values().cloned().collect();
        for o in objects {
            if self.available(Some(&o)) {
                ids.push(json::as_string_opt(o.get("object"))?.unwrap_or("null").to_string());
                let name = match o.get("text") {
                    None => "",
                    v => json::as_string_opt(v)?.unwrap_or(""),
                };
                names.push(format!("{} · {}", if name.is_empty() { "GIF" } else { name }, ids.last().unwrap()));
            }
        }
        if ids.is_empty() {
            ui::toast(&app.this, "No GIF animations on this page", TOAST_LENGTH_SHORT)?;
            return Ok(());
        }
        let selected = self.selected.get();
        if selected.is_none() || !ids.contains(selected.as_ref().unwrap()) {
            self.selected.set(Some(ids[0].clone()));
        }
        let choices: Vec<&str> = names.iter().map(String::as_str).collect();
        let choices = jx::new_string_array(&choices)?;
        let weak = Arc::downgrade(self);
        let listener = jx::callback(move |_, args| {
            let which = jx::unbox_int(&args[1]);
            if let Some(player) = weak.upgrade() {
                player.show_controls(&ids[which as usize]).map_err(jx::JavaError::from)?;
            }
            Ok(Reply::Void)
        });
        let builder = jnew!("android/app/AlertDialog$Builder", "(Landroid/content/Context;)V", &app.this)?;
        let title = jx::jstr("Choose animation");
        jcall!(builder, "android/app/AlertDialog$Builder", "setTitle", "(Ljava/lang/CharSequence;)Landroid/app/AlertDialog$Builder;", &title)?;
        jcall!(builder, "android/app/AlertDialog$Builder", "setItems", "([Ljava/lang/CharSequence;Landroid/content/DialogInterface$OnClickListener;)Landroid/app/AlertDialog$Builder;", &choices, &listener)?;
        let close = jx::jstr("Close");
        jcall!(builder, "android/app/AlertDialog$Builder", "setNegativeButton", "(Ljava/lang/CharSequence;Landroid/content/DialogInterface$OnClickListener;)Landroid/app/AlertDialog$Builder;", &close, jx::NULL)?;
        jcall!(builder, "android/app/AlertDialog$Builder", "show", "()Landroid/app/AlertDialog;")?;
        Ok(())
    }
    /// `private void showControls(String id)`.
    pub fn show_controls(self: &Arc<Self>, id: &str) -> AppResult<()> {
        self.select(id)?;
        let context = self.app().this.clone();
        let boxv = ui::column(&context)?;
        ui::set_padding(&boxv, 24, 12, 24, 12)?;
        let label = ui::text_view(&context)?;
        self.label.set(Some(label.clone()));
        ui::add_view(&boxv, &label)?;
        let timeline = jnew!("android/widget/SeekBar", "(Landroid/content/Context;)V", &context)?;
        self.timeline.set(Some(timeline.clone()));
        jcall!(timeline, "android/widget/AbsSeekBar", "setMax", "(I)V", self.clip(id)?.frames() - 1)?;
        let weak = Arc::downgrade(self);
        let seek_listener = jx::callback(move |method, args| {
            if method == "onProgressChanged(SeekBar,int,boolean)" && jx::unbox_bool(&args[2]) {
                if let Some(player) = weak.upgrade() {
                    player.seek(jx::unbox_int(&args[1])).map_err(jx::JavaError::from)?;
                }
            }
            Ok(Reply::Void)
        });
        jcall!(timeline, "android/widget/SeekBar", "setOnSeekBarChangeListener", "(Landroid/widget/SeekBar$OnSeekBarChangeListener;)V", &seek_listener)?;
        ui::add_view(&boxv, &timeline)?;
        let row = jnew!("android/widget/LinearLayout", "(Landroid/content/Context;)V", &context)?;
        let previous = jnew!("android/widget/Button", "(Landroid/content/Context;)V", &context)?;
        ui::set_text(&previous, "◀ Frame")?;
        let weak = Arc::downgrade(self);
        ui::on_click(&previous, move |_| match weak.upgrade() {
            Some(p) => p.step(-1).map_err(Into::into),
            None => Ok(()),
        })?;
        ui::add_view(&row, &previous)?;
        let toggle = jnew!("android/widget/Button", "(Landroid/content/Context;)V", &context)?;
        self.toggle.set(Some(toggle.clone()));
        let weak = Arc::downgrade(self);
        ui::on_click(&toggle, move |_| match weak.upgrade() {
            Some(p) => (|| -> AppResult<()> {
                let playing = p.clip(&p.require_selected()?)?.playing.get();
                p.play(!playing)
            })()
            .map_err(Into::into),
            None => Ok(()),
        })?;
        ui::add_view(&row, &toggle)?;
        let next = jnew!("android/widget/Button", "(Landroid/content/Context;)V", &context)?;
        ui::set_text(&next, "Frame ▶")?;
        let weak = Arc::downgrade(self);
        ui::on_click(&next, move |_| match weak.upgrade() {
            Some(p) => p.step(1).map_err(Into::into),
            None => Ok(()),
        })?;
        ui::add_view(&row, &next)?;
        ui::add_view(&boxv, &row)?;
        let builder = jnew!("android/app/AlertDialog$Builder", "(Landroid/content/Context;)V", &context)?;
        let title = jx::jstr("GIF player");
        jcall!(builder, "android/app/AlertDialog$Builder", "setTitle", "(Ljava/lang/CharSequence;)Landroid/app/AlertDialog$Builder;", &title)?;
        jcall!(builder, "android/app/AlertDialog$Builder", "setView", "(Landroid/view/View;)Landroid/app/AlertDialog$Builder;", &boxv)?;
        let close = jx::jstr("Close");
        jcall!(builder, "android/app/AlertDialog$Builder", "setNegativeButton", "(Ljava/lang/CharSequence;Landroid/content/DialogInterface$OnClickListener;)Landroid/app/AlertDialog$Builder;", &close, jx::NULL)?;
        let dialog = jcall!(builder, "android/app/AlertDialog$Builder", "create", "()Landroid/app/AlertDialog;")?.some();
        self.dialog.set(Some(dialog.clone()));
        let weak = Arc::downgrade(self);
        let dismissed = jx::callback(move |_, _| {
            if let Some(p) = weak.upgrade() {
                p.dialog.set(None);
                p.label.set(None);
                p.timeline.set(None);
                p.toggle.set(None);
            }
            Ok(Reply::Void)
        });
        jcall!(dialog, "android/app/Dialog", "setOnDismissListener", "(Landroid/content/DialogInterface$OnDismissListener;)V", &dismissed)?;
        self.refresh()?;
        jcall!(dialog, "android/app/Dialog", "show", "()V")?;
        Ok(())
    }
    /// `private void refresh()`.
    pub fn refresh(&self) -> AppResult<()> {
        let (Some(label), Some(selected)) = (self.label.get(), self.selected.get()) else { return Ok(()) };
        let Some(c) = self.clips.with(|c| c.get(&selected).cloned()) else { return Ok(()) };
        let playing = c.playing.get();
        ui::set_text(&label, &format!("Frame {} / {} · {}", c.frame.get() + 1, c.frames(), if playing { "Playing" } else { "Paused" }))?;
        if let Some(timeline) = self.timeline.get() {
            jcall!(timeline, "android/widget/ProgressBar", "setProgress", "(I)V", c.frame.get())?;
        }
        if let Some(toggle) = self.toggle.get() {
            ui::set_text(&toggle, if playing { "Pause" } else { "Play" })?;
        }
        Ok(())
    }
    /// `void pauseAll()`.
    pub fn pause_all(&self) -> AppResult<()> {
        ui::remove_callbacks(&self.handler, &self.tick)?;
        for (_, c) in self.clip_list() {
            c.playing.set(false);
        }
        self.refresh()
    }
    /// `void stop()`.
    pub fn stop(&self) -> AppResult<()> {
        self.pause_all()?;
        if let Some(dialog) = self.dialog.get() {
            jcall!(dialog, "android/content/DialogInterface", "dismiss", "()V")?;
        }
        Ok(())
    }
    /// `void reset()`.
    pub fn reset(&self) -> AppResult<()> {
        self.stop()?;
        self.clips.with_mut(|c| c.clear());
        self.selected.set(None);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controls_hit_zones() {
        let bar = RectF::new(0.0, 0.0, 100.0, BAR_HEIGHT);
        assert_eq!(AnimationPlayer::control_at(&bar, 10.0, 10.0).as_deref(), Some("previous"));
        assert_eq!(AnimationPlayer::control_at(&bar, 40.0, 10.0).as_deref(), Some("toggle"));
        assert_eq!(AnimationPlayer::control_at(&bar, 70.0, 10.0).as_deref(), Some("next"));
        assert_eq!(AnimationPlayer::control_at(&bar, 90.0, 10.0), None);
        assert_eq!(AnimationPlayer::control_at(&bar, 90.0, 50.0).as_deref(), Some("timeline"));
        assert_eq!(AnimationPlayer::control_at(&bar, 101.0, 10.0), None);
    }
}
