//! Port of `UiActions.java`: semantic controls for a live Activity, served to
//! the CLI through `LocalProxy.UiSession`. Register every new editor control
//! here.
//!
//! `request` is called on proxy threads; actions run on the main thread
//! (posted as a `FutureTask` with a 30 s timeout, or directly when already
//! on the main looper).

use crate::android_consts::COLOR_TRANSPARENT;
use crate::error::{require, AppError, AppResult};
use crate::jx::{self, Obj};
use crate::main_activity::MainActivity;
use crate::ui;
use indexmap::IndexMap;
use notemaker_cli::local_proxy::{UiError, UiSession};
use notemaker_core::json::{self, Map, Value};
use notemaker_core::object;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Weak};
use std::time::Duration;

/// `private interface Action { void run(Map<String,Object> input) throws Exception; }`.
/// The Java lambdas captured `app`; here the activity is passed in so the
/// registry holds no strong reference to it.
pub type Action = Box<dyn Fn(&Arc<MainActivity>, &Map) -> AppResult<()> + Send + Sync>;

/// `final class UiActions implements LocalProxy.UiSession`.
pub struct UiActions {
    pub app: Weak<MainActivity>,
    /// `new Handler(Looper.getMainLooper())`.
    pub main: Obj,
    pub actions: IndexMap<String, Action>,
    pub descriptions: IndexMap<String, String>,
    /// This instance, for tasks posted to `main`.
    this: Weak<UiActions>,
}

/// `Json.string(a, key)`.
fn string(a: &Map, key: &str) -> AppResult<String> {
    Ok(json::string(a, key)?.to_string())
}
/// Java `String.trim()`.
fn trim(s: &str) -> &str {
    s.trim_matches(|c: char| c <= ' ')
}
fn browser_showing(app: &MainActivity) -> AppResult<bool> {
    Ok(match app.browser.get() {
        Some(b) => b.showing()?,
        None => false,
    })
}
fn require_browser(app: &MainActivity) -> AppResult<Arc<crate::document_browser::DocumentBrowser>> {
    require(browser_showing(app)?, "Files browser is closed")?;
    Ok(app.browser.get().unwrap())
}
fn agent(app: &MainActivity) -> Arc<crate::agent_ui::AgentUi> {
    app.agent.get().expect("agent")
}
fn put_pref(app: &MainActivity, f: impl FnOnce(&Obj) -> jx::JResult<()>) -> AppResult<()> {
    let editor = ui::edit(&app.this)?;
    f(&editor)?;
    ui::apply(&editor)?;
    Ok(())
}

const BRUSHES: [&str; 11] = ["pen", "pencil", "marker", "brush", "oily", "fountain", "charcoal", "neoBrush", "latinCalligraphy", "asiaCalligraphy", "square"];

impl UiActions {
    /// `UiActions(MainActivity app)`: registers every action in Java order.
    pub fn new(app: &Arc<MainActivity>) -> AppResult<Arc<UiActions>> {
        let main = ui::main_handler()?;
        let mut actions: IndexMap<String, Action> = IndexMap::new();
        let mut descriptions: IndexMap<String, String> = IndexMap::new();
        let (ac, de) = (&mut actions, &mut descriptions);
        UiActions::add(ac, de, "panel.open", "{name: document|pen|eraser|template|layers|shapes|fill|select|text|insert|move|page|agent|more|history|conflicts|network|preferences|presets|brushes}", Box::new(|app, a| app.editor().open_panel(&string(a, "name")?)));
        UiActions::add(ac, de, "panel.close", "{}", Box::new(|app, _| app.editor().dismiss_popup()));
        UiActions::add(ac, de, "files.open", "{}", Box::new(|app, _| app.editor().show_files()));
        UiActions::add(ac, de, "files.close", "{}", Box::new(|app, _| {
            if let Some(b) = app.browser.get() {
                b.dismiss()?;
            }
            Ok(())
        }));
        UiActions::add(ac, de, "files.search", "{query: string}", Box::new(|app, a| {
            let b = require_browser(app)?;
            b.search(&string(a, "query")?)
        }));
        UiActions::add(ac, de, "files.mode", "{mode: grid|list}", Box::new(|app, a| {
            let b = require_browser(app)?;
            b.mode(&string(a, "mode")?)
        }));
        UiActions::add(ac, de, "files.refresh", "{}", Box::new(|app, _| {
            let b = require_browser(app)?;
            b.refresh_now()
        }));
        UiActions::add(ac, de, "document.open", "{name: string}", Box::new(|app, a| {
            let raw = string(a, "name")?;
            let name = trim(&raw);
            require(!name.is_empty() && json::jlen(name) <= 200, "Document name must be 1–200 characters")?;
            if let Some(b) = app.browser.get() {
                b.dismiss()?;
            }
            app.editor().dismiss_popup()?;
            app.open_document(name, "fixed")
        }));
        UiActions::add(ac, de, "page.open", "{page: integer, zero based}", Box::new(|app, a| {
            require(app.type_.get() == "pages", "Document has no pages")?;
            let page = UiActions::integer(a, "page", 0, 10000)?;
            app.deselect()?;
            app.reset_drawing_gesture()?;
            app.page.set(page);
            app.save();
            app.update()
        }));
        UiActions::add(ac, de, "page.next", "{}", Box::new(|app, _| {
            require(app.type_.get() == "pages" && app.page.get() < 10000, "No next page")?;
            app.deselect()?;
            app.reset_drawing_gesture()?;
            app.page.set(app.page.get() + 1);
            app.save();
            app.update()
        }));
        UiActions::add(ac, de, "page.previous", "{}", Box::new(|app, _| {
            require(app.type_.get() == "pages" && app.page.get() > 0, "No previous page")?;
            app.deselect()?;
            app.reset_drawing_gesture()?;
            app.page.set(app.page.get() - 1);
            app.save();
            app.update()
        }));
        UiActions::add(ac, de, "layer.select", "{id: layer ID}", Box::new(|app, a| {
            let id = string(a, "id")?;
            let known = app.layers.with(|l| l.contains_key(&id));
            require(known && app.model().layer_active.get(&id) != Some(&false), "Layer is unavailable")?;
            app.select_layer(&id)
        }));
        UiActions::add(ac, de, "layer.visibility", "{id: layer ID, visible: boolean} — device view only", Box::new(|app, a| {
            let id = string(a, "id")?;
            require(app.layers.with(|l| l.contains_key(&id)), "Unknown layer")?;
            let visible = UiActions::bool_(a, "visible")?;
            if visible {
                app.hidden.with_mut(|h| h.remove(&id));
            } else {
                app.hidden.with_mut(|h| h.insert(id.clone()));
            }
            app.save();
            app.update()
        }));
        UiActions::add(ac, de, "tool.select", "{name: pen|eraser|move|shapes|text|fill|select|interact}", Box::new(|app, a| {
            let name = string(a, "name")?;
            require(["pen", "eraser", "move", "shapes", "text", "fill", "select", "interact"].contains(&name.as_str()), "Unknown drawing tool")?;
            app.editor().open_panel(&name)
        }));
        UiActions::add(ac, de, "shape.select", "{id: shape ID}", Box::new(|app, a| {
            let id = string(a, "id")?;
            require(notemaker_core::shape_catalog::ids().contains(&id.as_str()), "Unknown shape")?;
            app.editor().dismiss_popup()?;
            app.set_drawing_tool(Some(&id))
        }));
        UiActions::add(ac, de, "pen.configure", "{brush?, color?, width?, material?}", Box::new(|app, a| {
            if a.contains_key("brush") {
                let brush = string(a, "brush")?;
                require(BRUSHES.contains(&brush.as_str()), "Unknown brush")?;
                app.brush.set(brush);
            }
            if a.contains_key("color") {
                app.color.set(UiActions::integer(a, "color", i32::MIN, i32::MAX)?);
            }
            if a.contains_key("width") {
                app.width.set(UiActions::integer(a, "width", 1, 30)? as f32);
            }
            if a.contains_key("material") {
                app.material.set(UiActions::integer(a, "material", 0, 100)?);
                let material = app.material.get();
                put_pref(app, |e| ui::put_int(e, "penMaterial", material))?;
            }
            app.update()
        }));
        UiActions::add(ac, de, "pen.preset.save", "{name: string}", Box::new(|app, a| app.editor().save_pen_preset_named(&string(a, "name")?)));
        UiActions::add(ac, de, "pen.preset.apply", "{name: string}", Box::new(|app, a| app.editor().apply_pen_preset(&string(a, "name")?)));
        UiActions::add(ac, de, "shape.fill", "{enabled: boolean}", Box::new(|app, a| {
            let fill = if UiActions::bool_(a, "enabled")? { app.color.get() } else { COLOR_TRANSPARENT };
            app.object_fill_color.set(fill);
            app.update()
        }));
        UiActions::add(ac, de, "fill.tap", "{enabled: boolean}", Box::new(|app, a| {
            app.fill_tap_mode.set(UiActions::bool_(a, "enabled")?);
            if app.fill_tap_mode.get() {
                app.selecting.set(false);
                if app.object_fill_color.get() == COLOR_TRANSPARENT {
                    app.object_fill_color.set(app.color.get());
                }
            }
            app.editor().dismiss_popup()?;
            app.update()
        }));
        UiActions::add(ac, de, "eraser.configure", "{mode?, radius?, scribble?}", Box::new(|app, a| {
            if a.contains_key("mode") {
                let mode = string(a, "mode")?;
                require(["stroke", "partial", "area"].contains(&mode.as_str()), "Unknown eraser mode")?;
                app.eraser_mode.set(mode);
            }
            if a.contains_key("radius") {
                app.eraser_radius.set(UiActions::integer(a, "radius", 1, 80)? as f32);
            }
            if a.contains_key("scribble") {
                app.scribble_to_erase.set(UiActions::bool_(a, "scribble")?);
                let value = app.scribble_to_erase.get();
                put_pref(app, |e| ui::put_bool(e, "scribbleToErase", value))?;
            }
            app.update()
        }));
        UiActions::add(ac, de, "selection.lasso", "{enabled: boolean}", Box::new(|app, a| {
            app.freeform_lasso.set(UiActions::bool_(a, "enabled")?);
            let value = app.freeform_lasso.get();
            put_pref(app, |e| ui::put_bool(e, "freeformLasso", value))?;
            app.update()
        }));
        UiActions::add(ac, de, "selection.all", "{}", Box::new(|app, _| app.select_all_visible()));
        UiActions::add(ac, de, "selection.choose", "{entities: [{kind: object|stroke, id: string}]}", Box::new(|app, a| {
            let raw = json::list(a.get("entities").unwrap_or(&Value::Null))?;
            require(!raw.is_empty() && raw.len() <= 100, "Choose 1–100 entities")?;
            let mut refs: Vec<Map> = Vec::new();
            for value in raw {
                let r = json::map(value)?;
                let kind = string(r, "kind")?;
                let id = string(r, "id")?;
                require(kind == "object" || kind == "stroke", "Unknown entity kind")?;
                refs.push(object! { "kind" => kind, "id" => id, "page" => app.page.get() as i64 });
            }
            app.start_selection(refs)
        }));
        UiActions::add(ac, de, "selection.clear", "{}", Box::new(|app, _| app.deselect()));
        UiActions::add(ac, de, "view.zoom", "{factor: positive number}", Box::new(|app, a| {
            let factor = UiActions::number(a, "factor")?;
            require(factor.is_finite() && factor > 0.0, "Zoom factor must be positive")?;
            let canvas = app.canvas();
            let zoom = notemaker_core::jmath::max(0.2, notemaker_core::jmath::min(5.0, canvas.zoom.get() as f64 * factor)) as f32;
            canvas.zoom.set(zoom);
            canvas.invalidate()
        }));
        UiActions::add(ac, de, "view.pan", "{dx: number, dy: number} — device pixels", Box::new(|app, a| {
            let dx = UiActions::number(a, "dx")?;
            let dy = UiActions::number(a, "dy")?;
            require(dx.is_finite() && dy.is_finite(), "Pan offsets must be finite")?;
            let canvas = app.canvas();
            canvas.offset_x.set(canvas.offset_x.get() + dx as f32);
            canvas.offset_y.set(canvas.offset_y.get() + dy as f32);
            canvas.invalidate()
        }));
        UiActions::add(ac, de, "view.reset", "{}", Box::new(|app, _| {
            let canvas = app.canvas();
            canvas.zoom.set(1.0);
            canvas.offset_x.set(0.0);
            canvas.offset_y.set(0.0);
            canvas.invalidate()
        }));
        UiActions::add(ac, de, "toolbar.labels", "{visible: boolean}", Box::new(|app, a| app.editor().show_tool_labels(UiActions::bool_(a, "visible")?)));
        UiActions::add(ac, de, "network.configure", "{server: string, port: string, proxy: string, token: string}", Box::new(|app, a| {
            let server = string(a, "server")?;
            let port = string(a, "port")?;
            let proxy = string(a, "proxy")?;
            let token = string(a, "token")?;
            let host = app.host.get();
            MainActivity::compose_server_address(&server, &port, Some(&host))?;
            require(proxy.is_empty() || (proxy.starts_with("http://") || proxy.starts_with("https://")) && !token.is_empty(), "Proxy URL requires an HTTP(S) URL and token")?;
            app.save_network_settings(&server, &port, &proxy, &token)
        }));
        UiActions::add(ac, de, "history.undo", "{}", Box::new(|app, _| app.history(false)));
        UiActions::add(ac, de, "history.redo", "{}", Box::new(|app, _| app.history(true)));
        UiActions::add(ac, de, "template.insert", "{name: ruled|grid|dots|planner|blank}", Box::new(|app, a| {
            let name = string(a, "name")?;
            require(["ruled", "grid", "dots", "planner", "blank"].contains(&name.as_str()), "Unknown template")?;
            app.insert_template(&name)
        }));
        UiActions::add(ac, de, "agent.open", "{}", Box::new(|app, _| agent(app).show()));
        UiActions::add(ac, de, "agent.close", "{}", Box::new(|app, _| agent(app).dismiss()));
        UiActions::add(ac, de, "gif.player.open", "{}", Box::new(|app, _| app.animation_player().show()));
        UiActions::add(ac, de, "gif.import.open", "{}", Box::new(|app, _| app.choose_gif()));
        UiActions::add(ac, de, "gif.select", "{id: GIF object ID}", Box::new(|app, a| app.animation_player().select(&string(a, "id")?)));
        UiActions::add(ac, de, "gif.play", "{}", Box::new(|app, _| app.animation_player().play(true)));
        UiActions::add(ac, de, "gif.pause", "{}", Box::new(|app, _| app.animation_player().play(false)));
        UiActions::add(ac, de, "gif.next", "{}", Box::new(|app, _| app.animation_player().step(1)));
        UiActions::add(ac, de, "gif.previous", "{}", Box::new(|app, _| app.animation_player().step(-1)));
        UiActions::add(ac, de, "gif.seek", "{frame: zero-based integer}", Box::new(|app, a| app.animation_player().seek(UiActions::integer(a, "frame", 0, 499)?)));
        Ok(Arc::new_cyclic(|this| UiActions { app: Arc::downgrade(app), main, actions, descriptions, this: this.clone() }))
    }
    pub fn app(&self) -> Arc<MainActivity> {
        self.app.upgrade().expect("MainActivity dropped")
    }
    /// `private void add(String name, String schema, Action action)` (constructor only).
    pub fn add(actions: &mut IndexMap<String, Action>, descriptions: &mut IndexMap<String, String>, name: &str, schema: &str, action: Action) {
        actions.insert(name.to_string(), action);
        descriptions.insert(name.to_string(), schema.to_string());
    }
    /// `private static int integer(Map a, String key, int min, int max)`.
    pub fn integer(a: &Map, key: &str, min: i32, max: i32) -> AppResult<i32> {
        let value = a.get(key);
        let integral = match value {
            Some(Value::Number(n)) => json::number_double(n) == json::number_int(n) as f64,
            _ => false,
        };
        require(integral, &format!("{key} must be an integer"))?;
        let n = json::as_int(value)?;
        require(n >= min && n <= max, &format!("{key} is out of range"))?;
        Ok(n)
    }
    /// `private static double number(Map a, String key)`.
    pub fn number(a: &Map, key: &str) -> AppResult<f64> {
        let value = a.get(key);
        require(json::is_number(value), &format!("{key} must be a number"))?;
        Ok(json::as_double(value)?)
    }
    /// `private static boolean bool(Map a, String key)`.
    pub fn bool_(a: &Map, key: &str) -> AppResult<bool> {
        require(matches!(a.get(key), Some(Value::Bool(_))), &format!("{key} must be boolean"))?;
        Ok(json::as_bool(a.get(key))?)
    }
    /// `private Map<String,Object> state()`.
    pub fn state(&self) -> AppResult<Map> {
        let app = self.app();
        let manufacturer = jsget!("android/os/Build", "MANUFACTURER", "Ljava/lang/String;")?.string().unwrap_or_else(|| "null".into());
        let model = jsget!("android/os/Build", "MODEL", "Ljava/lang/String;")?.string().unwrap_or_else(|| "null".into());
        let files_open = browser_showing(&app)?;
        let files = if files_open { Value::Object(app.browser.get().unwrap().state()?) } else { Value::Null };
        let editor = app.editor();
        let canvas = app.canvas();
        let selected = app.selected.get();
        Ok(object! {
            "gifPlayer" => app.animation_player().state()?,
            "session" => app.app_session_id.get(),
            "deviceId" => app.device_id.get(),
            "deviceName" => format!("{manufacturer} {model}"),
            "platform" => "Android",
            "startedAt" => app.app_session_started_at.get(),
            "document" => app.document.get(),
            "documentType" => app.type_.get(),
            "page" => app.page.get() as i64,
            "layer" => selected,
            "tool" => editor.selected_tool(),
            "filesOpen" => files_open,
            "files" => files,
            "panelOpen" => editor.panel_showing()?,
            "agentOpen" => agent(&app).showing()?,
            "foreground" => app.foreground.get(),
            "zoom" => json::from_float(canvas.zoom.get()),
            "offsetX" => json::from_float(canvas.offset_x.get()),
            "offsetY" => json::from_float(canvas.offset_y.get()),
            "color" => app.color.get() as i64,
            "brush" => app.brush.get(),
            "width" => json::from_float(app.width.get()),
            "material" => app.material.get() as i64,
            "eraserMode" => app.eraser_mode.get(),
            "eraserRadius" => json::from_float(app.eraser_radius.get()),
            "selectedCount" => app.selected_entities.with(|s| s.len()) as i64,
        })
    }
    /// The `Callable<Object> task` of `request` (main thread).
    fn task(&self, verb: &str, action: &str, input: &Map) -> AppResult<Value> {
        let app = self.app();
        require(app.running.get(), "App session is closed")?;
        if verb == "state" {
            return Ok(Value::Object(self.state()?));
        }
        if verb == "actions" {
            let descriptions: Map = self.descriptions.iter().map(|(k, v)| (k.clone(), Value::from(v.as_str()))).collect();
            return Ok(Value::Object(object! { "session" => app.app_session_id.get(), "actions" => descriptions }));
        }
        require(verb == "action", "Unknown UI request")?;
        let selected = self.actions.get(action);
        require(selected.is_some(), &format!("Unknown UI action: {action}"))?;
        selected.unwrap()(&app, input)?;
        Ok(Value::Object(object! { "action" => action, "state" => self.state()? }))
    }
    /// `public Object request(String verb, String action, Map input) throws Exception`.
    pub fn request_impl(&self, verb: &str, action: &str, input: Map) -> AppResult<Value> {
        if ui::on_main_thread()? {
            return self.task(verb, action, &input);
        }
        let (sender, receiver) = mpsc::sync_channel::<AppResult<Value>>(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let this = self.this.clone();
        let (verb_owned, action_owned) = (verb.to_string(), action.to_string());
        let flag = cancelled.clone();
        let pending = jx::once_runnable(move || {
            // FutureTask.cancel(false) before the task starts skips it.
            if flag.load(Ordering::SeqCst) {
                return;
            }
            let Some(me) = this.upgrade() else { return };
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| me.task(&verb_owned, &action_owned, &input)))
                .unwrap_or_else(|p| {
                    let msg = p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_else(|| "panic".into());
                    Err(AppError::runtime(msg))
                });
            let _ = sender.send(result);
        });
        ui::post(&self.main, &pending)?;
        match receiver.recv_timeout(Duration::from_secs(30)) {
            Ok(result) => result,
            Err(_) => {
                cancelled.store(true, Ordering::SeqCst);
                Err(AppError::illegal_state("UI session did not respond within 30 seconds"))
            }
        }
    }
}

impl UiSession for UiActions {
    fn request(&self, verb: &str, action: &str, input: Map) -> Result<Value, UiError> {
        self.request_impl(verb, action, input).map_err(|e| Box::new(e) as UiError)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(text: &str) -> Map {
        json::parse(text).unwrap().as_object().unwrap().clone()
    }

    #[test]
    fn validators() {
        let a = map(r#"{"i":3,"d":3.0,"f":3.5,"big":4294967296,"s":"x","b":true}"#);
        assert_eq!(UiActions::integer(&a, "i", 0, 5).unwrap(), 3);
        assert_eq!(UiActions::integer(&a, "d", 0, 5).unwrap(), 3);
        assert_eq!(UiActions::integer(&a, "f", 0, 5).unwrap_err().message().unwrap(), "f must be an integer");
        assert_eq!(UiActions::integer(&a, "big", i32::MIN, i32::MAX).unwrap_err().message().unwrap(), "big must be an integer");
        assert_eq!(UiActions::integer(&a, "i", 4, 5).unwrap_err().message().unwrap(), "i is out of range");
        assert_eq!(UiActions::integer(&a, "s", 0, 5).unwrap_err().message().unwrap(), "s must be an integer");
        assert_eq!(UiActions::number(&a, "f").unwrap(), 3.5);
        assert_eq!(UiActions::number(&a, "s").unwrap_err().message().unwrap(), "s must be a number");
        assert!(UiActions::bool_(&a, "b").unwrap());
        assert_eq!(UiActions::bool_(&a, "i").unwrap_err().message().unwrap(), "i must be boolean");
    }
}
